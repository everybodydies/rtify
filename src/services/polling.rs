//! How often to ask Spotify for playback state. Between polls the UI extrapolates the position
//! locally, so slow polling costs accuracy only on remote changes (phone, other device).

use std::time::Duration;

use crate::config::PollingMode;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PollInputs {
    pub window_visible: bool,
    pub playing: bool,
    /// Milliseconds left in the current track, if known.
    pub remaining_ms: Option<u64>,
    /// Consecutive failed polls (network errors).
    pub failures: u32,
    /// Server asked us to wait (HTTP 429 Retry-After).
    pub retry_after: Option<Duration>,
}

pub fn next_poll(mode: PollingMode, fixed_ms: u64, i: PollInputs) -> Duration {
    if let Some(wait) = i.retry_after {
        return wait.max(Duration::from_secs(1));
    }
    let base = match mode {
        PollingMode::Fixed => Duration::from_millis(fixed_ms.clamp(500, 120_000)),
        PollingMode::Adaptive => match (i.window_visible, i.playing) {
            (true, true) => Duration::from_secs(3),
            (true, false) => Duration::from_secs(6),
            (false, true) => Duration::from_secs(15),
            (false, false) => Duration::from_secs(45),
        },
    };
    if i.failures > 0 {
        // Exponential backoff up to two minutes while offline.
        let factor = 2u32.saturating_pow(i.failures.min(6));
        return (base * factor).min(Duration::from_secs(120));
    }
    // Catch the track change right after it happens instead of up to one interval later.
    match i.remaining_ms {
        Some(rem) if i.playing && rem + 400 < base.as_millis() as u64 => Duration::from_millis(rem + 400),
        _ => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> PollInputs {
        PollInputs {
            window_visible: true,
            playing: true,
            remaining_ms: None,
            failures: 0,
            retry_after: None,
        }
    }

    #[test]
    fn hidden_and_paused_polls_rarely() {
        let visible = next_poll(PollingMode::Adaptive, 0, inputs());
        let hidden = next_poll(
            PollingMode::Adaptive,
            0,
            PollInputs {
                window_visible: false,
                playing: false,
                ..inputs()
            },
        );
        assert!(hidden >= visible * 10);
    }

    #[test]
    fn track_end_is_caught_early() {
        let d = next_poll(
            PollingMode::Adaptive,
            0,
            PollInputs {
                remaining_ms: Some(1000),
                ..inputs()
            },
        );
        assert_eq!(d, Duration::from_millis(1400));
    }

    #[test]
    fn backoff_and_rate_limit() {
        let d = next_poll(PollingMode::Adaptive, 0, PollInputs { failures: 10, ..inputs() });
        assert_eq!(d, Duration::from_secs(120));
        let d = next_poll(
            PollingMode::Fixed,
            1000,
            PollInputs {
                retry_after: Some(Duration::from_secs(7)),
                ..inputs()
            },
        );
        assert_eq!(d, Duration::from_secs(7));
        assert_eq!(next_poll(PollingMode::Fixed, 1, inputs()), Duration::from_millis(500));
    }
}
