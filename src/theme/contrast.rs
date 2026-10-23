//! sRGB colors, WCAG contrast and lightness adjustment.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}

impl Rgb {
    pub const fn new(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b }
    }

    pub fn from_u8(r: u8, g: u8, b: u8) -> Self {
        Self::new(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0)
    }

    /// `#rgb`, `#rrggbb` or `#rrggbbaa` (alpha ignored).
    pub fn parse(s: &str) -> Option<Self> {
        let h = s.trim().strip_prefix('#')?;
        let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        match h.len() {
            3 => {
                let mut c = h.chars().map(|c| c.to_digit(16).map(|d| (d * 17) as u8));
                Some(Self::from_u8(c.next()??, c.next()??, c.next()??))
            }
            6 | 8 => Some(Self::from_u8(byte(0)?, byte(2)?, byte(4)?)),
            _ => None,
        }
    }

    pub fn to_hex(self) -> String {
        let c = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!("#{:02x}{:02x}{:02x}", c(self.r), c(self.g), c(self.b))
    }

    pub fn mix(self, other: Rgb, t: f64) -> Rgb {
        Rgb::new(
            self.r + (other.r - self.r) * t,
            self.g + (other.g - self.g) * t,
            self.b + (other.b - self.b) * t,
        )
    }

    pub fn luminance(self) -> f64 {
        let lin = |c: f64| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }

    pub fn is_light(self) -> bool {
        self.luminance() > 0.4
    }

    /// OKLCH (lightness 0–1, chroma, hue in degrees), clamped into sRGB.
    pub fn from_oklch(l: f64, c: f64, h: f64) -> Rgb {
        let (a, b) = (c * h.to_radians().cos(), c * h.to_radians().sin());
        let l_ = (l + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
        let m_ = (l - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
        let s_ = (l - 0.089_484_177_5 * a - 1.291_485_548 * b).powi(3);
        let r = 4.076_741_662_1 * l_ - 3.307_711_591_3 * m_ + 0.230_969_929_2 * s_;
        let g = -1.268_438_004_6 * l_ + 2.609_757_401_1 * m_ - 0.341_319_396_5 * s_;
        let bl = -0.004_196_086_3 * l_ - 0.703_418_614_7 * m_ + 1.707_614_701 * s_;
        let enc = |x: f64| {
            let x = x.clamp(0.0, 1.0);
            if x <= 0.003_130_8 {
                12.92 * x
            } else {
                1.055 * x.powf(1.0 / 2.4) - 0.055
            }
        };
        Rgb::new(enc(r), enc(g), enc(bl))
    }

    pub fn to_oklch(self) -> (f64, f64, f64) {
        let lin = |c: f64| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        let (r, g, b) = (lin(self.r), lin(self.g), lin(self.b));
        let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
        let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
        let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
        let ll = 0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s;
        let a = 1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s;
        let bb = 0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s;
        (ll, a.hypot(bb), bb.atan2(a).to_degrees().rem_euclid(360.0))
    }

    pub fn to_hsl(self) -> (f64, f64, f64) {
        let max = self.r.max(self.g).max(self.b);
        let min = self.r.min(self.g).min(self.b);
        let l = (max + min) / 2.0;
        if (max - min).abs() < 1e-9 {
            return (0.0, 0.0, l);
        }
        let d = max - min;
        let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        let h = if max == self.r {
            (self.g - self.b) / d + if self.g < self.b { 6.0 } else { 0.0 }
        } else if max == self.g {
            (self.b - self.r) / d + 2.0
        } else {
            (self.r - self.g) / d + 4.0
        };
        (h * 60.0, s, l)
    }

    pub fn from_hsl(h: f64, s: f64, l: f64) -> Rgb {
        let h = h.rem_euclid(360.0) / 360.0;
        if s <= 0.0 {
            return Rgb::new(l, l, l);
        }
        let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let p = 2.0 * l - q;
        let channel = |mut t: f64| {
            t = t.rem_euclid(1.0);
            if t < 1.0 / 6.0 {
                p + (q - p) * 6.0 * t
            } else if t < 0.5 {
                q
            } else if t < 2.0 / 3.0 {
                p + (q - p) * (2.0 / 3.0 - t) * 6.0
            } else {
                p
            }
        };
        Rgb::new(channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0))
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (a.luminance(), b.luminance());
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Move `fg`'s lightness away from `bg` (keeping hue and saturation) until the contrast ratio
/// reaches `min`. Falls back to black or white if the hue cannot get there.
pub fn ensure_contrast(fg: Rgb, bg: Rgb, min: f64) -> Rgb {
    if contrast(fg, bg) >= min {
        return fg;
    }
    let (h, s, l) = fg.to_hsl();
    let lighter = !bg.is_light();
    let mut lo = l;
    let mut hi = if lighter { 1.0 } else { 0.0 };
    if contrast(Rgb::from_hsl(h, s, hi), bg) < min {
        return if lighter {
            Rgb::new(1.0, 1.0, 1.0)
        } else {
            Rgb::new(0.0, 0.0, 0.0)
        };
    }
    // Binary search for the smallest lightness change that satisfies the ratio.
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if contrast(Rgb::from_hsl(h, s, mid), bg) >= min {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Rgb::from_hsl(h, s, hi)
}

/// Text color to put on top of `bg`.
pub fn on_color(bg: Rgb) -> Rgb {
    let black = Rgb::new(0.07, 0.07, 0.08);
    let white = Rgb::new(1.0, 1.0, 1.0);
    if contrast(black, bg) >= contrast(white, bg) { black } else { white }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        assert_eq!(Rgb::parse("#b99aff").unwrap().to_hex(), "#b99aff");
        assert_eq!(Rgb::parse("#fff").unwrap().to_hex(), "#ffffff");
        assert_eq!(Rgb::parse("#11223344").unwrap().to_hex(), "#112233");
        assert!(Rgb::parse("b99aff").is_none());
        assert!(Rgb::parse("#zzz").is_none());
    }

    #[test]
    fn oklch_matches_design_tokens() {
        // Hex values from DESIGN.md.
        for (l, c, h, hex) in [
            (0.90, 0.0, 0.0, "#dedede"),
            (0.58, 0.0, 0.0, "#7a7a7a"),
            (0.70, 0.12, 310.0, "#b589d6"),
            (0.65, 0.12, 310.0, "#a679c6"),
            (0.70, 0.13, 0.0, "#df7a9b"),
        ] {
            assert_eq!(Rgb::from_oklch(l, c, h).to_hex(), hex);
        }
        let (l, c, h) = Rgb::parse("#b589d6").unwrap().to_oklch();
        assert!((l - 0.70).abs() < 0.005 && (c - 0.12).abs() < 0.005 && (h - 310.0).abs() < 1.5);
    }

    #[test]
    fn hsl_roundtrip() {
        let c = Rgb::parse("#b99aff").unwrap();
        let (h, s, l) = c.to_hsl();
        assert_eq!(Rgb::from_hsl(h, s, l).to_hex(), "#b99aff");
    }

    #[test]
    fn contrast_ratio_bounds() {
        let black = Rgb::new(0.0, 0.0, 0.0);
        let white = Rgb::new(1.0, 1.0, 1.0);
        assert!((contrast(black, white) - 21.0).abs() < 0.01);
        assert!((contrast(white, white) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn ensure_contrast_lifts_dark_accent_on_dark_bg() {
        let bg = Rgb::parse("#16181d").unwrap();
        let accent = Rgb::parse("#3a2a66").unwrap();
        let fixed = ensure_contrast(accent, bg, 4.5);
        assert!(contrast(fixed, bg) >= 4.5);
        assert!(fixed.to_hsl().2 > accent.to_hsl().2);
        let light_bg = Rgb::parse("#f4f4f4").unwrap();
        let fixed = ensure_contrast(Rgb::parse("#ffd966").unwrap(), light_bg, 3.0);
        assert!(contrast(fixed, light_bg) >= 3.0);
    }
}
