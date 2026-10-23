//! Live playback state as the UI sees it, and the rules for reconciling it with what rtify
//! remembered from the last run. Spotify's state always wins; saved preferences are only
//! re-applied when the user asked for that.

use std::time::Instant;

use crate::lyrics::synchronization::PlaybackSnapshot;
use crate::persistence::{AccountSession, LastTrack};
use crate::spotify::model::{Device, PlayerState, Track, pick_image};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    Off,
    Context,
    Track,
}

impl Repeat {
    pub fn parse(s: &str) -> Repeat {
        match s {
            "context" => Repeat::Context,
            "track" => Repeat::Track,
            _ => Repeat::Off,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Repeat::Off => "off",
            Repeat::Context => "context",
            Repeat::Track => "track",
        }
    }

    pub fn next(self) -> Repeat {
        match self {
            Repeat::Off => Repeat::Context,
            Repeat::Context => Repeat::Track,
            Repeat::Track => Repeat::Off,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Playback {
    pub track: Option<Track>,
    pub playing: bool,
    pub progress_ms: u64,
    pub fetched_at: Option<Instant>,
    pub device: Option<Device>,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub context_uri: Option<String>,
    /// True until the first live state arrives; `track` then holds the remembered track.
    pub stale: bool,
}

impl Playback {
    pub fn from_remote(state: Option<PlayerState>) -> Playback {
        let Some(s) = state else {
            return Playback {
                fetched_at: Some(Instant::now()),
                ..Default::default()
            };
        };
        Playback {
            track: s.item,
            playing: s.is_playing,
            progress_ms: s.progress_ms.unwrap_or(0),
            fetched_at: Some(Instant::now()),
            device: s.device,
            shuffle: s.shuffle_state,
            repeat: Repeat::parse(&s.repeat_state),
            context_uri: s.context.map(|c| c.uri).filter(|u| !u.is_empty()),
            stale: false,
        }
    }

    /// What to show before Spotify answered: the last-known track, paused, never playing.
    pub fn from_session(s: &AccountSession) -> Playback {
        let track = s.last_track.as_ref().map(|t| Track {
            uri: t.uri.clone(),
            name: t.name.clone(),
            duration_ms: t.duration_ms,
            artists: vec![crate::spotify::model::ArtistRef {
                name: t.artists.clone(),
                ..Default::default()
            }],
            album: Some(crate::spotify::model::AlbumRef {
                name: t.album.clone(),
                images: if t.image_url.is_empty() {
                    vec![]
                } else {
                    vec![crate::spotify::model::Image {
                        url: t.image_url.clone(),
                        width: Some(300),
                        height: Some(300),
                    }]
                },
                ..Default::default()
            }),
            kind: "track".into(),
            ..Default::default()
        });
        Playback {
            track,
            shuffle: s.shuffle.unwrap_or(false),
            repeat: s.repeat.as_deref().map(Repeat::parse).unwrap_or_default(),
            context_uri: s.last_context_uri.clone(),
            stale: true,
            ..Default::default()
        }
    }

    pub fn duration_ms(&self) -> u64 {
        self.track.as_ref().map(|t| t.duration_ms).unwrap_or(0)
    }

    pub fn snapshot(&self) -> PlaybackSnapshot {
        PlaybackSnapshot {
            position_ms: self.progress_ms,
            playing: self.playing && !self.stale,
            taken_at: self.fetched_at.unwrap_or_else(Instant::now),
            duration_ms: Some(self.duration_ms()).filter(|d| *d > 0),
        }
    }

    pub fn position_now(&self) -> u64 {
        self.snapshot().position_at(Instant::now())
    }

    pub fn remaining_ms(&self) -> Option<u64> {
        let d = self.duration_ms();
        (d > 0).then(|| d.saturating_sub(self.position_now()))
    }

    pub fn track_uri(&self) -> Option<&str> {
        self.track.as_ref().map(|t| t.uri.as_str())
    }

    /// Record what is worth remembering for the next start.
    pub fn remember(&self, s: &mut AccountSession) {
        if self.stale {
            return;
        }
        if let Some(t) = &self.track {
            s.last_track = Some(LastTrack {
                uri: t.uri.clone(),
                name: t.name.clone(),
                artists: t.artist_names(),
                album: t.album_name().to_owned(),
                image_url: pick_image(t.images(), 200).map(|i| i.url.clone()).unwrap_or_default(),
                duration_ms: t.duration_ms,
            });
        }
        if self.context_uri.is_some() {
            s.last_context_uri = self.context_uri.clone();
        }
        if self.track.is_some() {
            s.shuffle = Some(self.shuffle);
            s.repeat = Some(self.repeat.as_str().into());
        }
        if let Some(d) = &self.device {
            s.device_id = d.id.clone();
            s.device_name = Some(d.name.clone());
        }
        s.updated_at = crate::spotify::auth::unix_now();
    }
}

/// One deliberate step taken after reconnecting. Applied in order, each awaited, because
/// Spotify does not guarantee the ordering of concurrent playback commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconnectStep {
    SetShuffle(bool),
    SetRepeat(Repeat),
    /// Start `context` (optionally at `track`) on `device`. Only with `resume_on_startup`.
    Resume {
        context: String,
        track: Option<String>,
        device: Option<String>,
    },
}

pub fn reconnect_plan(
    restore_shuffle_repeat: bool,
    resume_on_startup: bool,
    saved: Option<&AccountSession>,
    live: &Playback,
    available_devices: &[Device],
) -> Vec<ReconnectStep> {
    let Some(saved) = saved else { return vec![] };
    let mut steps = Vec::new();
    let has_active = live.device.is_some();
    if resume_on_startup && !live.playing && live.track.is_none() {
        let device = saved
            .device_id
            .clone()
            .filter(|id| available_devices.iter().any(|d| d.id.as_deref() == Some(id.as_str())))
            .or_else(|| available_devices.iter().find(|d| !d.is_restricted).and_then(|d| d.id.clone()));
        if let (Some(context), Some(device)) = (saved.last_context_uri.clone(), device) {
            steps.push(ReconnectStep::Resume {
                context,
                track: saved.last_track.as_ref().map(|t| t.uri.clone()),
                device: Some(device),
            });
        }
    }
    // Shuffle/repeat need a device that is (or is about to be) active.
    if restore_shuffle_repeat && (has_active || !steps.is_empty()) {
        if let Some(sh) = saved.shuffle.filter(|s| *s != live.shuffle) {
            steps.push(ReconnectStep::SetShuffle(sh));
        }
        if let Some(r) = saved.repeat.as_deref().map(Repeat::parse).filter(|r| *r != live.repeat) {
            steps.push(ReconnectStep::SetRepeat(r));
        }
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved() -> AccountSession {
        AccountSession {
            last_track: Some(LastTrack {
                uri: "spotify:track:1".into(),
                ..Default::default()
            }),
            last_context_uri: Some("spotify:playlist:p".into()),
            shuffle: Some(true),
            repeat: Some("track".into()),
            device_id: Some("d1".into()),
            ..Default::default()
        }
    }

    fn device(id: &str) -> Device {
        Device {
            id: Some(id.into()),
            name: id.into(),
            ..Default::default()
        }
    }

    #[test]
    fn default_changes_nothing() {
        let live = Playback {
            device: Some(device("d1")),
            ..Default::default()
        };
        assert!(reconnect_plan(false, false, Some(&saved()), &live, &[device("d1")]).is_empty());
    }

    #[test]
    fn restores_only_differences_on_active_device() {
        let live = Playback {
            device: Some(device("d1")),
            shuffle: true,
            ..Default::default()
        };
        assert_eq!(
            reconnect_plan(true, false, Some(&saved()), &live, &[]),
            vec![ReconnectStep::SetRepeat(Repeat::Track)]
        );
        let no_device = Playback::default();
        assert!(
            reconnect_plan(true, false, Some(&saved()), &no_device, &[]).is_empty(),
            "waits for a device"
        );
    }

    #[test]
    fn resume_targets_known_device_and_never_interrupts() {
        let idle = Playback::default();
        let plan = reconnect_plan(true, true, Some(&saved()), &idle, &[device("x"), device("d1")]);
        assert_eq!(
            plan[0],
            ReconnectStep::Resume {
                context: "spotify:playlist:p".into(),
                track: Some("spotify:track:1".into()),
                device: Some("d1".into())
            }
        );
        assert_eq!(plan.len(), 3);
        let busy = Playback {
            playing: true,
            track: Some(Track::default()),
            device: Some(device("phone")),
            ..Default::default()
        };
        assert!(
            !reconnect_plan(false, true, Some(&saved()), &busy, &[])
                .iter()
                .any(|s| matches!(s, ReconnectStep::Resume { .. }))
        );
    }

    #[test]
    fn remember_skips_stale_state() {
        let mut s = AccountSession::default();
        Playback::from_session(&saved()).remember(&mut s);
        assert!(s.last_track.is_none());
        let live = Playback {
            track: Some(Track {
                uri: "u".into(),
                ..Default::default()
            }),
            repeat: Repeat::Context,
            ..Default::default()
        };
        live.remember(&mut s);
        assert_eq!(s.repeat.as_deref(), Some("context"));
        assert_eq!(s.last_track.unwrap().uri, "u");
    }
}
