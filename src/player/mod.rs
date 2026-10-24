//! The built-in player: rtify as a Spotify Connect device of its own (librespot), so it can play
//! without the official client. The rest of rtify keeps driving playback through the Web API;
//! this device simply shows up in the device list next to phones and speakers.
//!
//! It signs in with Spotify's own desktop client ID (the streaming servers take no other) and
//! stores the reusable credentials that login returns in the Secret Service. Audio goes to
//! PulseAudio, which PipeWire provides on current desktops.

use sha2::{Digest, Sha256};

use crate::config;

#[cfg(feature = "builtin-player")]
mod librespot;

/// Built with the `builtin-player` feature.
pub const AVAILABLE: bool = cfg!(feature = "builtin-player");

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub name: String,
    pub bitrate: u32,
    pub normalisation: bool,
}

impl Settings {
    pub fn from_config(c: &config::Player) -> Self {
        Self {
            name: device_name(c),
            bitrate: c.bitrate,
            normalisation: c.normalisation,
        }
    }
}

/// The name the device shows in Spotify.
pub fn device_name(c: &config::Player) -> String {
    match c.device_name.trim() {
        "" => default_name(),
        name => name.to_owned(),
    }
}

pub fn default_name() -> String {
    let host = gtk::glib::host_name();
    if host.is_empty() || host == "localhost" {
        "rtify".to_owned()
    } else {
        format!("rtify on {host}")
    }
}

/// The same ID on every start, so Spotify keeps one entry for this computer (and the stored
/// login, which is bound to it, stays valid).
pub fn device_id() -> String {
    static ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ID.get_or_init(|| {
        let machine = std::fs::read_to_string("/etc/machine-id")
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| gtk::glib::host_name().to_string());
        device_id_for(&machine)
    })
    .clone()
}

fn device_id_for(machine: &str) -> String {
    let digest = Sha256::digest(format!("rtify-builtin-player:{machine}").as_bytes());
    digest[..20].iter().map(|b| format!("{b:02x}")).collect()
}

/// How the player signs in.
#[derive(Clone)]
pub enum Login {
    /// A fresh access token from the browser sign-in (scope `streaming`).
    Token(String),
    /// Reusable credentials from an earlier session, as stored ([`Event::Login`]).
    Stored(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Off,
    Connecting,
    /// Visible to Spotify as a Connect device, signed in as `username`.
    Ready {
        username: String,
    },
    /// The login is missing or no longer accepted.
    SignInNeeded(String),
    /// Spotify cannot be reached or refused for another reason; the player retries by itself.
    Failed(String),
}

impl Status {
    pub fn describe(&self, name: &str) -> String {
        match self {
            Status::Off => "off".to_owned(),
            Status::Connecting => "connecting…".to_owned(),
            Status::Ready { .. } => format!("ready · shows up as \"{name}\""),
            Status::SignInNeeded(why) => format!("sign in needed · {why}"),
            Status::Failed(why) => why.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Status(Status),
    /// New reusable credentials (JSON) to keep in the Secret Service.
    Login(String),
    /// Playback on this device changed (play, pause, seek, track, volume): worth polling now.
    Changed,
    /// The sound server refused or dropped the stream; playback paused.
    Audio(String),
}

/// A running player. Dropping it without [`Handle::shutdown`] stops it too, just less politely.
pub struct Handle {
    stop: tokio::sync::watch::Sender<bool>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Handle {
    /// Start the device on the shared runtime once `previous` (a restart) has disconnected;
    /// progress arrives on `events`.
    pub fn start(settings: Settings, login: Login, events: async_channel::Sender<Event>, previous: Option<Handle>) -> Handle {
        let (stop, stopped) = tokio::sync::watch::channel(false);
        #[cfg(feature = "builtin-player")]
        let task = Some(crate::runtime::rt().spawn(async move {
            if let Some(p) = previous {
                p.shutdown().await;
            }
            let volume_dir = crate::paths::state_dir().join("player");
            librespot::run(settings, login, Some(volume_dir), events, stopped).await
        }));
        #[cfg(not(feature = "builtin-player"))]
        let task = {
            let _ = (settings, login, stopped, previous);
            let _ = events.try_send(Event::Status(Status::Failed("this build has no built-in player".into())));
            None
        };
        Handle { stop, task }
    }

    /// Disconnect from Spotify (the device leaves the lists right away) and wait for it, briefly.
    pub async fn shutdown(mut self) {
        let _ = self.stop.send(true);
        if let Some(task) = self.task.take() {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), task).await;
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}

/// Seconds to wait before reconnecting after `failures` attempts in a row: 2, 4, 8 … up to 60.
pub fn backoff_secs(failures: u32) -> u64 {
    (2u64 << failures.saturating_sub(1).min(5)).min(60)
}

/// Pick rustls' process-wide crypto backend (see [`init`]); a no-op once one is installed.
fn install_crypto_provider() {
    #[cfg(feature = "builtin-player")]
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

/// Process-wide setup, first thing in `main` while no other thread exists:
/// - librespot's TLS stack pulls in both rustls crypto backends, so rustls cannot pick one by
///   itself and would panic on the first connection; choose the one reqwest uses.
/// - name the PulseAudio/PipeWire stream "rtify" with the app icon (libpulse reads these
///   environment variables).
pub fn init() {
    install_crypto_provider();
    if !AVAILABLE {
        return;
    }
    for (key, value) in [
        ("PULSE_PROP_application.name", "rtify"),
        ("PULSE_PROP_application.icon_name", crate::paths::APP_ID),
        ("PULSE_PROP_stream.description", "Spotify"),
        ("PULSE_PROP_media.role", "music"),
    ] {
        if std::env::var_os(key).is_none() {
            // SAFETY: called first thing in main(), while the process is still single-threaded.
            unsafe { std::env::set_var(key, value) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_id_is_stable_and_spotify_shaped() {
        let a = device_id_for("0123456789abcdef");
        assert_eq!(a, device_id_for("0123456789abcdef"));
        assert_ne!(a, device_id_for("fedcba9876543210"));
        assert_eq!(a.len(), 40);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn custom_name_wins_over_the_default() {
        let mut c = config::Player::default();
        assert_eq!(device_name(&c), default_name());
        c.device_name = "  desk  ".into();
        assert_eq!(device_name(&c), "desk");
    }

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff_secs(0), 2);
        assert_eq!(backoff_secs(1), 2);
        assert_eq!(backoff_secs(2), 4);
        assert_eq!(backoff_secs(3), 8);
        assert_eq!(backoff_secs(20), 60);
    }
}
