//! Frame-rate independent easing for the lyrics view.

pub fn ease_out_cubic(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub fn ease_in_out_sine(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    -((std::f64::consts::PI * t).cos() - 1.0) / 2.0
}

/// Critically damped follow of a scroll target. `stiffness` ≈ 1/seconds to settle most of the way.
#[derive(Debug, Clone, Copy, Default)]
pub struct Scroller {
    pub value: f64,
    pub target: f64,
}

impl Scroller {
    pub fn snap(&mut self, target: f64) {
        self.value = target;
        self.target = target;
    }

    /// Advance by `dt` seconds. Returns whether the value is still moving.
    pub fn step(&mut self, dt: f64, stiffness: f64) -> bool {
        let k = 1.0 - (-dt * stiffness).exp();
        self.value += (self.target - self.value) * k;
        if (self.target - self.value).abs() < 0.25 {
            self.value = self.target;
            return false;
        }
        true
    }
}

/// Character-level fill for a word: with `n` characters and word progress `p`, how lit is char `i`.
pub fn char_fill(p: f64, i: usize, n: usize) -> f64 {
    if n == 0 {
        return 0.0;
    }
    (p * n as f64 - i as f64).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroller_converges_independent_of_frame_rate() {
        let run = |fps: f64| {
            let mut s = Scroller { value: 0.0, target: 100.0 };
            let mut t = 0.0;
            while t < 0.2 {
                s.step(1.0 / fps, 10.0);
                t += 1.0 / fps;
            }
            s.value
        };
        let (a, b) = (run(60.0), run(144.0));
        assert!((a - b).abs() < 2.0, "{a} vs {b}");
        let mut s = Scroller { value: 0.0, target: 100.0 };
        for _ in 0..200 {
            s.step(1.0 / 60.0, 10.0);
        }
        assert_eq!(s.value, 100.0);
        assert!(!s.step(0.016, 10.0));
    }

    #[test]
    fn chars_fill_left_to_right() {
        assert_eq!(char_fill(0.5, 0, 4), 1.0);
        assert_eq!(char_fill(0.5, 1, 4), 1.0);
        assert_eq!(char_fill(0.5, 2, 4), 0.0);
        assert!((char_fill(0.6, 2, 4) - 0.4).abs() < 1e-9);
    }
}
