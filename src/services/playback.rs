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
    /// True until the first live state arrives, and again while Spotify has nothing active:
    /// `track` then holds the remembered track, paused where it was.
    pub stale: bool,
    /// The remembered volume and device, shown while no device is active.
    pub volume_hint: Option<u32>,
    pub device_hint: Option<String>,
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
            volume_hint: None,
            device_hint: None,
        }
    }

    /// What to show before Spotify answered, or while it has nothing active: the last-known
    /// track, paused where it was, never playing.
    pub fn from_session(s: &AccountSession) -> Playback {
        let track = s.last_track.as_ref().map(track_of);
        Playback {
            progress_ms: s.last_track.as_ref().map_or(0, |t| t.position_ms.min(t.duration_ms)),
            fetched_at: Some(Instant::now()),
            track,
            shuffle: s.shuffle.unwrap_or(false),
            repeat: s.repeat.as_deref().map(Repeat::parse).unwrap_or_default(),
            context_uri: s.last_context_uri.clone(),
            stale: true,
            volume_hint: s.volume,
            device_hint: s.device_name.clone(),
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
            s.last_track = Some(last_track(t, self.position_now()));
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
            if d.volume_percent.is_some() {
                s.volume = d.volume_percent;
            }
        }
        s.updated_at = crate::spotify::auth::unix_now();
    }
}

/// What rtify remembers of a track: enough to show it and to start it again.
pub fn last_track(t: &Track, position_ms: u64) -> LastTrack {
    LastTrack {
        uri: t.uri.clone(),
        name: t.name.clone(),
        artists: t.artist_names(),
        album: t.album_name().to_owned(),
        image_url: pick_image(t.images(), 200).map(|i| i.url.clone()).unwrap_or_default(),
        duration_ms: t.duration_ms,
        position_ms,
    }
}

/// A remembered track as the UI shows tracks.
pub fn track_of(t: &LastTrack) -> Track {
    Track {
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
        kind: if t.uri.starts_with("spotify:episode:") {
            "episode"
        } else {
            "track"
        }
        .into(),
        ..Default::default()
    }
}

/// The songs queued from rtify that are still ahead, given Spotify's queue. Spotify plays its
/// user queue before the context, so they can only be among the first `queued.len()` entries;
/// a song that left that window has played (or was skipped).
pub fn still_queued(queued: &[LastTrack], upcoming: &[Track]) -> Vec<LastTrack> {
    upcoming
        .iter()
        .take(queued.len())
        .filter(|t| queued.iter().any(|q| q.uri == t.uri))
        .map(|t| last_track(t, 0))
        .collect()
}

/// One deliberate step taken after reconnecting. Applied in order, each awaited, because
/// Spotify does not guarantee the ordering of concurrent playback commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconnectStep {
    SetShuffle(bool),
    SetRepeat(Repeat),
    /// Start `track` at `position_ms` on `device`, inside `context` when there was one.
    Resume {
        context: Option<String>,
        track: String,
        position_ms: u64,
        device: String,
    },
    SetVolume(u32),
    /// Queue these songs again, in this order.
    Queue(Vec<String>),
}

/// The device to wake for a restore: the one that played last if it is still there, else the
/// first one that takes commands.
pub fn pick_device(saved: &AccountSession, available_devices: &[Device]) -> Option<String> {
    saved
        .device_id
        .clone()
        .filter(|id| available_devices.iter().any(|d| d.id.as_deref() == Some(id.as_str())))
        .or_else(|| available_devices.iter().find(|d| !d.is_restricted).and_then(|d| d.id.clone()))
}

/// Bring back the remembered session on `device`: the track at its position (inside its
/// context), shuffle, repeat, volume and the songs that were still queued.
pub fn restore_plan(saved: &AccountSession, device: &str) -> Vec<ReconnectStep> {
    let Some(track) = &saved.last_track else { return vec![] };
    let mut steps = vec![ReconnectStep::Resume {
        context: saved.last_context_uri.clone(),
        track: track.uri.clone(),
        position_ms: track.position_ms.min(track.duration_ms),
        device: device.to_owned(),
    }];
    if let Some(on) = saved.shuffle {
        steps.push(ReconnectStep::SetShuffle(on));
    }
    if let Some(r) = saved.repeat.as_deref() {
        steps.push(ReconnectStep::SetRepeat(Repeat::parse(r)));
    }
    if let Some(v) = saved.volume {
        steps.push(ReconnectStep::SetVolume(v));
    }
    if !saved.queue.is_empty() {
        steps.push(ReconnectStep::Queue(saved.queue.iter().map(|t| t.uri.clone()).collect()));
    }
    steps
}

pub fn reconnect_plan(
    restore_shuffle_repeat: bool,
    resume_on_startup: bool,
    saved: Option<&AccountSession>,
    live: &Playback,
    available_devices: &[Device],
) -> Vec<ReconnectStep> {
    let Some(saved) = saved else { return vec![] };
    // A stale state is the remembered one: Spotify has nothing active.
    let idle = !live.playing && (live.track.is_none() || live.stale);
    if resume_on_startup
        && idle
        && let Some(device) = pick_device(saved, available_devices)
    {
        let mut steps = restore_plan(saved, &device);
        if !restore_shuffle_repeat {
            steps.retain(|s| !matches!(s, ReconnectStep::SetShuffle(_) | ReconnectStep::SetRepeat(_)));
        }
        if !steps.is_empty() {
            return steps;
        }
    }
    let mut steps = Vec::new();
    // Shuffle/repeat need a device that is active.
    if restore_shuffle_repeat && live.device.is_some() && !live.stale {
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
                context: Some("spotify:playlist:p".into()),
                track: "spotify:track:1".into(),
                position_ms: 0,
                device: "d1".into()
            }
        );
        assert_eq!(plan.len(), 3);
        // The remembered state shown while Spotify has nothing active counts as idle.
        let restored = Playback::from_session(&saved());
        assert!(matches!(
            reconnect_plan(false, true, Some(&saved()), &restored, &[device("d1")])[..],
            [ReconnectStep::Resume { .. }]
        ));
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
    fn restore_brings_back_position_queue_and_volume() {
        let mut s = saved();
        s.last_track.as_mut().unwrap().duration_ms = 200_000;
        s.last_track.as_mut().unwrap().position_ms = 91_000;
        s.volume = Some(40);
        s.queue = vec![LastTrack {
            uri: "spotify:track:q".into(),
            ..Default::default()
        }];
        let restored = Playback::from_session(&s);
        assert!(restored.stale && !restored.playing);
        assert_eq!(restored.position_now(), 91_000);
        assert_eq!(restored.volume_hint, Some(40));
        assert_eq!(
            restore_plan(&s, "d1"),
            vec![
                ReconnectStep::Resume {
                    context: Some("spotify:playlist:p".into()),
                    track: "spotify:track:1".into(),
                    position_ms: 91_000,
                    device: "d1".into()
                },
                ReconnectStep::SetShuffle(true),
                ReconnectStep::SetRepeat(Repeat::Track),
                ReconnectStep::SetVolume(40),
                ReconnectStep::Queue(vec!["spotify:track:q".into()]),
            ]
        );
        assert!(restore_plan(&AccountSession::default(), "d1").is_empty());
    }

    #[test]
    fn queued_songs_leave_once_played() {
        let t = |u: &str| Track {
            uri: u.into(),
            name: u.to_uppercase(),
            ..Default::default()
        };
        let lt = |u: &str| LastTrack {
            uri: u.into(),
            ..Default::default()
        };
        let queued = [lt("a"), lt("b")];
        let kept = still_queued(&queued, &[t("a"), t("b"), t("c")]);
        assert_eq!(kept.iter().map(|k| k.uri.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(kept[0].name, "A", "takes Spotify's details");
        // "a" played; "a" again later in the context does not keep it.
        let kept = still_queued(&queued, &[t("b"), t("c"), t("a")]);
        assert_eq!(kept.iter().map(|k| k.uri.as_str()).collect::<Vec<_>>(), ["b"]);
        assert!(still_queued(&[], &[t("a")]).is_empty());
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
