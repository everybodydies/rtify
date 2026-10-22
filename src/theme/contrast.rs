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
