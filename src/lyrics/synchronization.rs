//! Turning "position X at instant T" snapshots into a lyric position for every frame.

use std::time::{Duration, Instant};

use super::model::{LyricDocument, LyricLine, TimingMode};

/// An authoritative position taken at a known monotonic instant. Between snapshots the position is
/// extrapolated; new snapshots replace it on seeks, pauses, track changes and device transfers.
#[derive(Debug, Clone, Copy)]
pub struct PlaybackSnapshot {
    pub position_ms: u64,
    pub playing: bool,
    pub taken_at: Instant,
    pub duration_ms: Option<u64>,
}

impl PlaybackSnapshot {
    pub fn new(position_ms: u64, playing: bool, duration_ms: Option<u64>) -> Self {
        Self {
            position_ms,
            playing,
            taken_at: Instant::now(),
            duration_ms,
        }
    }

    pub fn position_at(&self, now: Instant) -> u64 {
        let mut pos = self.position_ms;
        if self.playing {
            pos += now.saturating_duration_since(self.taken_at).as_millis() as u64;
        }
        match self.duration_ms {
            Some(d) if d > 0 => pos.min(d),
            _ => pos,
        }
    }
}

/// A clock the renderer can read every frame. The local variant drives demo/local material
/// without any connection to Spotify playback.
#[derive(Debug, Clone, Copy)]
pub struct LyricClock {
    snapshot: PlaybackSnapshot,
}

impl LyricClock {
    pub fn new(duration_ms: Option<u64>) -> Self {
        Self {
            snapshot: PlaybackSnapshot::new(0, false, duration_ms),
        }
    }

    pub fn from_snapshot(snapshot: PlaybackSnapshot) -> Self {
        Self { snapshot }
    }

    pub fn now_ms(&self) -> u64 {
        self.snapshot.position_at(Instant::now())
    }

    pub fn playing(&self) -> bool {
        self.snapshot.playing
    }

    pub fn set_playing(&mut self, playing: bool) {
        self.snapshot = PlaybackSnapshot::new(self.now_ms(), playing, self.snapshot.duration_ms);
    }

    pub fn seek(&mut self, position_ms: u64) {
        self.snapshot = PlaybackSnapshot::new(position_ms, self.snapshot.playing, self.snapshot.duration_ms);
    }

    /// True if the clock ran to the end of its duration.
    pub fn finished(&self) -> bool {
        self.snapshot.duration_ms.is_some_and(|d| self.now_ms() >= d)
    }
}

/// Seek detection: a jump larger than this between frames is rendered as a cut, not a scroll.
pub const DISCONTINUITY: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cursor {
    /// Index of the active line, or the line about to start when in a gap.
    pub line: Option<usize>,
    /// True while the active line is actually being sung (not a gap before it).
    pub active: bool,
    /// 0..1 progress through the active line.
    pub line_progress: f64,
}

pub fn line_progress(line: &LyricLine, pos: u64) -> f64 {
    match (line.start_ms, line.end_ms) {
        (Some(s), Some(e)) if e > s => ((pos.saturating_sub(s)) as f64 / (e - s) as f64).clamp(0.0, 1.0),
        _ => 0.0,
    }
}

/// 0..1 fill for each word of `line` at `pos`.
pub fn word_progress(line: &LyricLine, pos: u64) -> impl Iterator<Item = f64> + '_ {
    line.words.iter().map(move |w| {
        if pos <= w.start_ms {
            0.0
        } else if pos >= w.end_ms || w.end_ms <= w.start_ms {
            1.0
        } else {
            (pos - w.start_ms) as f64 / (w.end_ms - w.start_ms) as f64
        }
    })
}

pub fn cursor(doc: &LyricDocument, pos: u64) -> Cursor {
    if doc.timing == TimingMode::Static || doc.lines.is_empty() {
        return Cursor {
            line: None,
            active: false,
            line_progress: 0.0,
        };
    }
    // Last line that has started.
    let started = doc.lines.partition_point(|l| l.start_ms.unwrap_or(0) <= pos);
    if started == 0 {
        return Cursor {
            line: Some(0),
            active: false,
            line_progress: 0.0,
        };
    }
    let idx = started - 1;
    let line = &doc.lines[idx];
    let active = line.end_ms.is_none_or(|e| pos < e);
    if !active && idx + 1 < doc.lines.len() {
        // Instrumental gap: point at the upcoming line so the view can show a waiting indicator.
        return Cursor {
            line: Some(idx + 1),
            active: false,
            line_progress: 0.0,
        };
    }
    Cursor {
        line: Some(idx),
        active,
        line_progress: line_progress(line, pos),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::lrc;

    #[test]
    fn snapshot_extrapolates_and_clamps() {
        let t0 = Instant::now();
        let s = PlaybackSnapshot {
            position_ms: 1000,
            playing: true,
            taken_at: t0,
            duration_ms: Some(1500),
        };
        assert_eq!(s.position_at(t0 + Duration::from_millis(200)), 1200);
        assert_eq!(s.position_at(t0 + Duration::from_secs(5)), 1500);
        let paused = PlaybackSnapshot { playing: false, ..s };
        assert_eq!(paused.position_at(t0 + Duration::from_secs(5)), 1000);
    }

    #[test]
    fn cursor_walks_lines_and_gaps() {
        let doc = lrc::parse("[00:02.00]a\n[00:04.00]b\n[00:05.00]\n[00:09.00]c\n");
        assert_eq!(
            cursor(&doc, 0),
            Cursor {
                line: Some(0),
                active: false,
                line_progress: 0.0
            }
        );
        let c = cursor(&doc, 3000);
        assert_eq!((c.line, c.active), (Some(0), true));
        assert!((c.line_progress - 0.5).abs() < 1e-9);
        assert_eq!(
            cursor(&doc, 6000),
            Cursor {
                line: Some(2),
                active: false,
                line_progress: 0.0
            }
        );
        let c = cursor(&doc, 9500);
        assert_eq!((c.line, c.active), (Some(2), true));
    }

    #[test]
    fn words_fill_in_order() {
        let doc = lrc::parse("[00:01.00]<00:01.00>a <00:02.00>b <00:03.00>c<00:04.00>\n");
        let p: Vec<f64> = word_progress(&doc.lines[0], 2500).collect();
        assert_eq!(p, [1.0, 0.5, 0.0]);
    }

    #[test]
    fn local_clock_pause_and_seek() {
        let mut c = LyricClock::new(Some(10_000));
        assert!(!c.playing());
        c.seek(4000);
        assert_eq!(c.now_ms(), 4000);
        c.set_playing(true);
        assert!(c.now_ms() >= 4000);
        c.seek(20_000);
        assert!(c.finished());
    }
}
