//! The librespot side of the built-in player: one session, one Connect device, reconnecting with
//! backoff until it is stopped or the login is refused.

use std::sync::Arc;
use std::time::Duration;

use librespot_connect::{ConnectConfig, Spirc};
use librespot_core::authentication::Credentials;
use librespot_core::cache::Cache;
use librespot_core::config::{DeviceType, SessionConfig};
use librespot_core::error::ErrorKind;
use librespot_core::session::Session;
use librespot_playback::audio_backend::{self, Sink, SinkResult};
use librespot_playback::config::{AudioFormat, Bitrate, PlayerConfig};
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;
use librespot_playback::mixer::softmixer::SoftMixer;
use librespot_playback::mixer::{Mixer, MixerConfig};
use librespot_playback::player::{Player, PlayerEvent};
use librespot_protocol::authentication::AuthenticationType;
use tokio::sync::watch;

use super::{Event, Login, Settings, Status, backoff_secs};

type Events = async_channel::Sender<Event>;

fn send(events: &Events, ev: Event) {
    let _ = events.try_send(ev);
}

fn bitrate(kbps: u32) -> Bitrate {
    match kbps {
        0..=96 => Bitrate::Bitrate96,
        97..=160 => Bitrate::Bitrate160,
        _ => Bitrate::Bitrate320,
    }
}

/// librespot ends the whole process when a sink fails to stop (e.g. the sound server went
/// away while paused). Report such errors instead and carry on; the next start reconnects.
struct Guarded {
    inner: Box<dyn Sink>,
    events: Events,
}

impl Sink for Guarded {
    fn start(&mut self) -> SinkResult<()> {
        let r = self.inner.start();
        if let Err(e) = &r {
            send(&self.events, Event::Audio(format!("cannot open the sound output ({e})")));
        }
        r
    }

    fn stop(&mut self) -> SinkResult<()> {
        if let Err(e) = self.inner.stop() {
            log::warn!("built-in player: closing the sound output failed: {e}");
        }
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        let r = self.inner.write(packet, converter);
        if let Err(e) = &r {
            send(&self.events, Event::Audio(format!("the sound output stopped ({e})")));
        }
        r
    }
}

/// Events that change what the Web API reports for this device.
fn worth_polling(ev: &PlayerEvent) -> bool {
    matches!(
        ev,
        PlayerEvent::Playing { .. }
            | PlayerEvent::Paused { .. }
            | PlayerEvent::Stopped { .. }
            | PlayerEvent::Seeked { .. }
            | PlayerEvent::TrackChanged { .. }
            | PlayerEvent::VolumeChanged { .. }
            | PlayerEvent::Unavailable { .. }
    )
}

async fn stopped(stop: &mut watch::Receiver<bool>) {
    // Also true once the handle is gone.
    let _ = stop.wait_for(|s| *s).await;
}

pub(super) async fn run(settings: Settings, login: Login, events: Events, mut stop: watch::Receiver<bool>) {
    let mut credentials = match login {
        Login::Token(token) => Credentials::with_access_token(token),
        Login::Stored(json) => match serde_json::from_str::<Credentials>(&json) {
            Ok(c) => c,
            Err(_) => {
                return send(
                    &events,
                    Event::Status(Status::SignInNeeded("the stored login is unreadable".into())),
                );
            }
        },
    };
    // Only the volume is cached on disk; the login goes to the Secret Service instead.
    let cache = Cache::new(None, Some(crate::paths::state_dir().join("player")), None, None)
        .map_err(|e| log::warn!("built-in player: no volume cache ({e})"))
        .ok();
    let session_config = SessionConfig {
        device_id: super::device_id(),
        ..SessionConfig::default()
    };
    let connect_config = ConnectConfig {
        name: settings.name.clone(),
        device_type: DeviceType::Computer,
        initial_volume: cache.as_ref().and_then(|c| c.volume()).unwrap_or(u16::MAX / 2),
        ..ConnectConfig::default()
    };
    let mixer: Arc<dyn Mixer> = match SoftMixer::open(MixerConfig::default()) {
        Ok(m) => Arc::new(m),
        Err(e) => return send(&events, Event::Status(Status::Failed(format!("volume control: {e}")))),
    };
    let Some(sink_builder) = audio_backend::find(Some("pulseaudio".into())) else {
        return send(&events, Event::Status(Status::Failed("no PulseAudio output in this build".into())));
    };
    let player_config = PlayerConfig {
        bitrate: bitrate(settings.bitrate),
        normalisation: settings.normalisation,
        ..PlayerConfig::default()
    };
    let mut session = Session::new(session_config.clone(), cache.clone());
    let sink_events = events.clone();
    let player = Player::new(player_config, session.clone(), mixer.get_soft_volume(), move || {
        Box::new(Guarded {
            inner: sink_builder(None, AudioFormat::default()),
            events: sink_events,
        }) as Box<dyn Sink>
    });
    let mut player_events = player.get_player_event_channel();
    let changed = events.clone();
    tokio::spawn(async move {
        while let Some(ev) = player_events.recv().await {
            if worth_polling(&ev) {
                send(&changed, Event::Changed);
            }
        }
    });

    let mut failures = 0u32;
    let mut last = Status::Off;
    loop {
        if session.is_invalid() {
            session = Session::new(session_config.clone(), cache.clone());
            player.set_session(session.clone());
        }
        send(&events, Event::Status(Status::Connecting));
        let connecting = Spirc::new(
            connect_config.clone(),
            session.clone(),
            credentials.clone(),
            player.clone(),
            mixer.clone(),
        );
        let result = tokio::select! {
            r = connecting => r,
            _ = stopped(&mut stop) => break,
        };
        match result {
            Ok((spirc, task)) => {
                let up_since = std::time::Instant::now();
                // Reconnects (and the next start) use the long-lived credentials this login
                // returned, never the short-lived access token.
                let stored = Credentials {
                    username: Some(session.username()),
                    auth_type: AuthenticationType::AUTHENTICATION_STORED_SPOTIFY_CREDENTIALS,
                    auth_data: session.auth_data(),
                };
                if !stored.auth_data.is_empty() && stored != credentials {
                    credentials = stored;
                    if let Ok(json) = serde_json::to_string(&credentials) {
                        send(&events, Event::Login(json));
                    }
                }
                send(
                    &events,
                    Event::Status(Status::Ready {
                        username: session.username(),
                    }),
                );
                let mut task = std::pin::pin!(task);
                tokio::select! {
                    _ = &mut task => {
                        log::warn!("built-in player: connection to Spotify lost, reconnecting");
                        // Back off only if it keeps dropping right after connecting.
                        failures = if up_since.elapsed() < Duration::from_secs(60) { failures + 1 } else { 1 };
                    }
                    _ = stopped(&mut stop) => {
                        let _ = spirc.shutdown();
                        let _ = tokio::time::timeout(Duration::from_secs(2), task).await;
                        break;
                    }
                }
            }
            Err(e) if matches!(e.kind, ErrorKind::PermissionDenied | ErrorKind::Unauthenticated) => {
                log::warn!("built-in player: login refused: {e}");
                last = Status::SignInNeeded("Spotify refused the login".into());
                break;
            }
            Err(e) => {
                failures += 1;
                let secs = backoff_secs(failures);
                log::warn!("built-in player: {e}");
                send(
                    &events,
                    Event::Status(Status::Failed(format!("cannot reach Spotify · retrying in {secs} s"))),
                );
            }
        }
        if !session.is_invalid() {
            session.shutdown();
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(backoff_secs(failures))) => {}
            _ = stopped(&mut stop) => break,
        }
    }
    if !session.is_invalid() {
        session.shutdown();
    }
    send(&events, Event::Status(last));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitrates_map_to_librespot_steps() {
        assert_eq!(bitrate(96), Bitrate::Bitrate96);
        assert_eq!(bitrate(160), Bitrate::Bitrate160);
        assert_eq!(bitrate(320), Bitrate::Bitrate320);
        assert_eq!(bitrate(1000), Bitrate::Bitrate320);
    }

    #[test]
    fn spotifys_desktop_client_id_matches_librespots() {
        assert_eq!(SessionConfig::default().client_id, crate::spotify::auth::PLAYER_CLIENT_ID);
    }
}
