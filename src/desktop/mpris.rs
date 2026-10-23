//! MPRIS (`org.mpris.MediaPlayer2.rtify`): media keys, `playerctl`, Noctalia/Waybar media widgets
//! and lock screens see rtify as a player. Calls become the same commands the GUI uses; the app
//! pushes state with `MprisHandle::update`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_channel::Sender;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};
use zbus::{connection, interface};

use crate::commands::Command;
use crate::paths::APP_ID;

pub const BUS_NAME: &str = "org.mpris.MediaPlayer2.rtify";
const PATH: &str = "/org/mpris/MediaPlayer2";
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";

#[derive(Debug, Clone, PartialEq)]
pub enum MprisEvent {
    Command(Command),
    Play,
    Pause,
    SeekTo(u64),
    SeekBy(i64),
    SetVolume(u32),
    SetShuffle(bool),
    /// "None", "Track" or "Playlist".
    SetLoop(String),
    OpenUri(String),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MprisState {
    pub playing: bool,
    pub has_track: bool,
    /// Spotify base62 id; becomes the MPRIS track object path.
    pub track_id: String,
    pub uri: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub art_url: String,
    pub length_ms: u64,
    pub position_ms: u64,
    pub volume: Option<u32>,
    pub shuffle: bool,
    /// "None", "Track" or "Playlist".
    pub loop_status: String,
}

#[derive(Default)]
struct Shared {
    state: MprisState,
    /// When `state.position_ms` was taken, to extrapolate `Position` while playing.
    at: Option<Instant>,
}

impl Shared {
    fn position_us(&self) -> i64 {
        let mut ms = self.state.position_ms;
        if self.state.playing
            && let Some(at) = self.at
        {
            ms += at.elapsed().as_millis() as u64;
        }
        if self.state.length_ms > 0 {
            ms = ms.min(self.state.length_ms);
        }
        ms as i64 * 1000
    }
}

pub fn track_path(id: &str) -> OwnedObjectPath {
    let clean: String = id.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let p = if clean.is_empty() {
        NO_TRACK.to_owned()
    } else {
        format!("/io/github/everybodydies/rtify/track/{clean}")
    };
    OwnedObjectPath::try_from(p).unwrap_or_else(|_| OwnedObjectPath::try_from(NO_TRACK).unwrap())
}

pub fn metadata(s: &MprisState) -> HashMap<String, OwnedValue> {
    let mut m: HashMap<String, OwnedValue> = HashMap::new();
    let mut put = |k: &str, v: Value<'_>| {
        if let Ok(v) = OwnedValue::try_from(v) {
            m.insert(k.to_owned(), v);
        }
    };
    put("mpris:trackid", Value::from(track_path(if s.has_track { &s.track_id } else { "" })));
    if !s.has_track {
        return m;
    }
    put("mpris:length", Value::from(s.length_ms as i64 * 1000));
    put("xesam:title", Value::from(s.title.clone()));
    put("xesam:artist", Value::from(s.artists.clone()));
    put("xesam:album", Value::from(s.album.clone()));
    if !s.art_url.is_empty() {
        put("mpris:artUrl", Value::from(s.art_url.clone()));
    }
    if !s.uri.is_empty() {
        put(
            "xesam:url",
            Value::from(format!(
                "https://open.spotify.com/{}",
                s.uri.trim_start_matches("spotify:").replace(':', "/")
            )),
        );
    }
    m
}

struct Root {
    tx: Sender<MprisEvent>,
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        let _ = self.tx.try_send(MprisEvent::Command(Command::ShowWindow));
    }
    fn quit(&self) {
        let _ = self.tx.try_send(MprisEvent::Command(Command::Quit));
    }
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn identity(&self) -> String {
        "rtify".into()
    }
    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        APP_ID.into()
    }
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        vec!["spotify".into()]
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        vec![]
    }
}

struct Player {
    tx: Sender<MprisEvent>,
    shared: Arc<Mutex<Shared>>,
}

impl Player {
    fn send(&self, e: MprisEvent) {
        let _ = self.tx.try_send(e);
    }
    fn state(&self) -> MprisState {
        self.shared.lock().unwrap().state.clone()
    }
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn next(&self) {
        self.send(MprisEvent::Command(Command::Next));
    }
    fn previous(&self) {
        self.send(MprisEvent::Command(Command::Previous));
    }
    fn pause(&self) {
        self.send(MprisEvent::Pause);
    }
    fn play_pause(&self) {
        self.send(MprisEvent::Command(Command::PlayPause));
    }
    fn stop(&self) {
        self.send(MprisEvent::Pause);
    }
    fn play(&self) {
        self.send(MprisEvent::Play);
    }
    fn seek(&self, offset: i64) {
        self.send(MprisEvent::SeekBy(offset / 1000));
    }
    fn set_position(&self, track_id: ObjectPath<'_>, position: i64) {
        // Per spec: ignore stale requests for another track and out-of-range positions.
        let s = self.state();
        if track_id.as_str() != track_path(&s.track_id).as_str() || position < 0 || position as u64 / 1000 > s.length_ms {
            return;
        }
        self.send(MprisEvent::SeekTo(position as u64 / 1000));
    }
    fn open_uri(&self, uri: String) {
        self.send(MprisEvent::OpenUri(uri));
    }

    #[zbus(signal)]
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> String {
        let s = self.state();
        if !s.has_track {
            "Stopped"
        } else if s.playing {
            "Playing"
        } else {
            "Paused"
        }
        .into()
    }
    #[zbus(property)]
    fn loop_status(&self) -> String {
        let l = self.state().loop_status;
        if l.is_empty() { "None".into() } else { l }
    }
    #[zbus(property)]
    fn set_loop_status(&mut self, value: String) {
        self.send(MprisEvent::SetLoop(value));
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn set_rate(&mut self, _value: f64) {}
    #[zbus(property)]
    fn shuffle(&self) -> bool {
        self.state().shuffle
    }
    #[zbus(property)]
    fn set_shuffle(&mut self, value: bool) {
        self.send(MprisEvent::SetShuffle(value));
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        metadata(&self.state())
    }
    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.state().volume.unwrap_or(0) as f64 / 100.0
    }
    #[zbus(property)]
    fn set_volume(&mut self, value: f64) {
        self.send(MprisEvent::SetVolume((value.clamp(0.0, 1.0) * 100.0).round() as u32));
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        self.shared.lock().unwrap().position_us()
    }
    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.state().has_track
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.state().has_track
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.state().has_track
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

#[derive(Clone)]
pub struct MprisHandle {
    conn: connection::Connection,
    shared: Arc<Mutex<Shared>>,
}

/// Must run on the Tokio runtime.
pub async fn start(tx: Sender<MprisEvent>) -> zbus::Result<MprisHandle> {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let conn = connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(PATH, Root { tx: tx.clone() })?
        .serve_at(
            PATH,
            Player {
                tx,
                shared: shared.clone(),
            },
        )?
        .build()
        .await?;
    Ok(MprisHandle { conn, shared })
}

/// Seek-like jumps bigger than this (beyond normal progress) emit `Seeked`.
const SEEK_JUMP_MS: i64 = 1500;

impl MprisHandle {
    /// Store the new state and emit PropertiesChanged for what changed (and Seeked on jumps).
    pub async fn update(&self, new: MprisState) -> zbus::Result<()> {
        let (old, expected) = {
            let mut sh = self.shared.lock().unwrap();
            let expected = sh.position_us() / 1000;
            let old = std::mem::replace(&mut sh.state, new.clone());
            sh.at = Some(Instant::now());
            (old, expected)
        };
        let iface = self.conn.object_server().interface::<_, Player>(PATH).await?;
        let p = iface.get().await;
        let e = iface.signal_emitter();
        if old.playing != new.playing || old.has_track != new.has_track {
            p.playback_status_changed(e).await?;
        }
        if metadata(&old) != metadata(&new) {
            p.metadata_changed(e).await?;
            p.can_go_next_changed(e).await?;
            p.can_go_previous_changed(e).await?;
            p.can_seek_changed(e).await?;
        }
        if old.volume != new.volume {
            p.volume_changed(e).await?;
        }
        if old.shuffle != new.shuffle {
            p.shuffle_changed(e).await?;
        }
        if old.loop_status != new.loop_status {
            p.loop_status_changed(e).await?;
        }
        let same_track = old.track_id == new.track_id;
        if same_track && (new.position_ms as i64 - expected).abs() > SEEK_JUMP_MS {
            Player::seeked(e, new.position_ms as i64 * 1000).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_paths_are_valid_object_paths() {
        assert_eq!(
            track_path("4uLU6hMCjMI75M1A2tKUQC").as_str(),
            "/io/github/everybodydies/rtify/track/4uLU6hMCjMI75M1A2tKUQC"
        );
        assert_eq!(track_path("").as_str(), NO_TRACK);
        assert_eq!(track_path("a-b:c").as_str(), "/io/github/everybodydies/rtify/track/abc");
    }

    #[test]
    fn metadata_has_required_keys() {
        let s = MprisState {
            has_track: true,
            track_id: "abc".into(),
            uri: "spotify:track:abc".into(),
            title: "T".into(),
            artists: vec!["A".into()],
            length_ms: 1000,
            ..Default::default()
        };
        let m = metadata(&s);
        for k in ["mpris:trackid", "mpris:length", "xesam:title", "xesam:artist", "xesam:url"] {
            assert!(m.contains_key(k), "{k}");
        }
        assert_eq!(metadata(&MprisState::default()).len(), 1);
    }

    #[test]
    fn position_extrapolates_while_playing() {
        let sh = Shared {
            state: MprisState {
                playing: true,
                position_ms: 1000,
                length_ms: 1500,
                ..Default::default()
            },
            at: Some(Instant::now() - std::time::Duration::from_secs(5)),
        };
        assert_eq!(sh.position_us(), 1_500_000);
    }
}
