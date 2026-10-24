//! Local preferences (`~/.config/rtify/config.toml`). Secrets never live here: OAuth tokens and
//! translation API keys go to the Secret Service (see `spotify::tokens`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const CONFIG_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub appearance: Appearance,
    pub window: Window,
    pub background: Background,
    pub lyrics: Lyrics,
    pub library: Library,
    pub playback: Playback,
    pub spotify: Spotify,
    pub shortcuts: Shortcuts,
    pub player: Player,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            appearance: Appearance::default(),
            window: Window::default(),
            background: Background::default(),
            lyrics: Lyrics::default(),
            library: Library::default(),
            playback: Playback::default(),
            spotify: Spotify::default(),
            shortcuts: Shortcuts::default(),
            player: Player::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PaletteSource {
    #[default]
    Fixed,
    Noctalia,
    AlbumArt,
}

impl PaletteSource {
    pub const ALL: [PaletteSource; 3] = [Self::Fixed, Self::Noctalia, Self::AlbumArt];

    pub fn label(self) -> &'static str {
        match self {
            Self::Fixed => "fixed custom palette",
            Self::Noctalia => "follow noctalia",
            Self::AlbumArt => "album-art accent",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub style: String,
    pub palette_source: PaletteSource,
    pub fixed_accent: String,
    /// Empty: derive from the accent hue (system24 default surfaces).
    pub fixed_background: String,
    pub light: bool,
    pub font_family: String,
    pub font_size: u32,
    pub animations_enabled: bool,
    pub reduced_motion: bool,
    /// True-black surfaces and outlined controls for OLED panels.
    pub oled: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            style: "system24".into(),
            palette_source: PaletteSource::Fixed,
            fixed_accent: crate::theme::system24::DEFAULT_ACCENT.into(),
            fixed_background: String::new(),
            light: false,
            font_family: "DM Mono, JetBrains Mono, monospace".into(),
            font_size: 10,
            animations_enabled: true,
            reduced_motion: false,
            oled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Window {
    pub minimize_to_tray: bool,
    pub start_hidden: bool,
    pub remember_geometry: bool,
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
    pub compact: bool,
    /// Right-hand column: "now playing" (cover, title) and the queue below it.
    pub now_playing_panel: bool,
    /// The queue panel under "now playing".
    pub queue_panel: bool,
    pub sidebar_width: i32,
    /// Width of the right-hand column.
    pub right_width: i32,
    /// The left column shrinks to an icon rail.
    pub sidebar_collapsed: bool,
}

impl Default for Window {
    fn default() -> Self {
        Self {
            minimize_to_tray: true,
            start_hidden: false,
            remember_geometry: true,
            width: 1600,
            height: 900,
            maximized: false,
            compact: false,
            now_playing_panel: true,
            queue_panel: true,
            sidebar_width: 270,
            right_width: 300,
            sidebar_collapsed: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PollingMode {
    #[default]
    Adaptive,
    Fixed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Background {
    pub autostart: bool,
    pub polling_mode: PollingMode,
    /// Used when `polling_mode = "fixed"`.
    pub polling_interval_ms: u64,
    pub tray: bool,
    pub notifications: Notifications,
}

impl Default for Background {
    fn default() -> Self {
        Self {
            autostart: false,
            polling_mode: PollingMode::Adaptive,
            polling_interval_ms: 3000,
            tray: true,
            notifications: Notifications::Background,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Notifications {
    Off,
    #[default]
    Background,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LyricAnimation {
    /// Static text, current line marked only.
    Off,
    /// Line highlight with smooth scrolling.
    Line,
    /// Word/syllable fill when the document has word timing, line highlight otherwise.
    #[default]
    Smooth,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lyrics {
    pub enabled: bool,
    pub romanization: bool,
    pub translation_enabled: bool,
    pub translation_language: String,
    pub translation_only: bool,
    pub animation: LyricAnimation,
    pub font_scale: f64,
    pub offset_ms: i64,
    /// Show and sync lyrics (from LRCLIB) for whatever Spotify is playing.
    pub follow_playback: bool,
    /// "google" (no key, like qtify) or "deepl" (API key in the Secret Service).
    pub translation_provider: String,
}

impl Default for Lyrics {
    fn default() -> Self {
        Self {
            enabled: true,
            romanization: true,
            translation_enabled: false,
            translation_language: "en".into(),
            translation_only: false,
            animation: LyricAnimation::Smooth,
            font_scale: 1.0,
            offset_ms: 0,
            follow_playback: true,
            translation_provider: "google".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    /// Type chip: "all", "playlists", "albums" or "artists".
    pub last_tab: String,
    pub filter: String,
    /// "recent", "added", "a-z", "creator" or "custom".
    pub sort: String,
    pub sort_reversed: bool,
    /// Library URIs in the order dragged into ("custom" sort).
    pub custom_order: Vec<String>,
    /// Song order per playlist/album URI ("title:asc", "added:desc" …); absent = custom order.
    pub track_sort: BTreeMap<String, String>,
    /// New releases: "all", "albums" or "singles".
    pub releases_filter: String,
    /// Last searches, newest first.
    pub recent_searches: Vec<String>,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            last_tab: "playlists".into(),
            track_sort: BTreeMap::new(),
            filter: String::new(),
            sort: "recent".into(),
            sort_reversed: false,
            custom_order: Vec::new(),
            releases_filter: "all".into(),
            recent_searches: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Playback {
    /// Re-apply the last-known shuffle/repeat once a device is available after reconnecting.
    pub restore_shuffle_repeat: bool,
    /// Continue the last session (track, position, queue) when nothing is playing at startup.
    pub resume_on_startup: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Spotify {
    /// Client ID of your own app from developer.spotify.com (PKCE, no secret needed).
    pub client_id: String,
    pub redirect_port: u16,
}

impl Default for Spotify {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            redirect_port: 8898,
        }
    }
}

/// Keys in the window and the preferred triggers handed to the GlobalShortcuts portal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shortcuts {
    pub portal: bool,
    /// Portal triggers by action name from `commands::Command::shortcut_actions`. The
    /// compositor has the final say.
    pub bindings: BTreeMap<String, String>,
    /// In-window keys that differ from the defaults in `keymap` ("" = unassigned).
    pub keys: BTreeMap<String, String>,
}

impl Shortcuts {
    /// Global triggers added after the first release; the config migration fills them in.
    pub const ADDED_BINDINGS: [(&'static str, &'static str); 3] = [
        ("volume-up", "XF86AudioRaiseVolume"),
        ("volume-down", "XF86AudioLowerVolume"),
        ("mute", "XF86AudioMute"),
    ];
}

impl Default for Shortcuts {
    fn default() -> Self {
        let bindings = [
            ("toggle-window", "SUPER+ALT+m"),
            ("toggle-lyrics", "SUPER+ALT+l"),
            ("play-pause", "XF86AudioPlay"),
            ("next", "XF86AudioNext"),
            ("previous", "XF86AudioPrev"),
            ("focus-search", "SUPER+ALT+s"),
            ("toggle-compact", "SUPER+ALT+c"),
        ];
        Self {
            portal: true,
            bindings: bindings
                .into_iter()
                .chain(Self::ADDED_BINDINGS)
                .map(|(a, b)| (a.to_owned(), b.to_owned()))
                .collect(),
            keys: BTreeMap::new(),
        }
    }
}

/// The built-in Spotify Connect device (`player`, librespot). Its login lives in the Secret
/// Service like the Web API one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Player {
    pub enabled: bool,
    /// Name in Spotify's device lists; empty: "rtify on <hostname>".
    pub device_name: String,
    /// Stream quality in kbit/s: 96, 160 or 320.
    pub bitrate: u32,
    /// Spotify's loudness normalisation (like the official clients' default).
    pub normalisation: bool,
}

impl Player {
    pub const BITRATES: [u32; 3] = [96, 160, 320];
}

impl Default for Player {
    fn default() -> Self {
        Self {
            enabled: true,
            device_name: String::new(),
            bitrate: 320,
            normalisation: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roundtrips_through_toml() {
        let c = Config::default();
        let s = toml::to_string_pretty(&c).unwrap();
        assert_eq!(toml::from_str::<Config>(&s).unwrap(), c);
        assert!(s.contains("palette_source = \"fixed\""));
    }

    #[test]
    fn partial_file_keeps_defaults() {
        let c: Config = toml::from_str("version = 2\n[lyrics]\nromanization = false\n").unwrap();
        assert!(!c.lyrics.romanization);
        assert!(c.lyrics.enabled);
        assert!(c.lyrics.follow_playback);
        assert_eq!(c.window, Window::default());
    }
}
