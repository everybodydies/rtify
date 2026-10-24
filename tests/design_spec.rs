//! Checks the generated stylesheet against the hard rules of DESIGN.md (system24 OLED).
//!
//! The rules are applied to what the app really loads: the default appearance in OLED mode, and
//! a sweep of accents an album cover or a Noctalia template could produce. Every test collects
//! all violations first, so one run lists everything that needs fixing.

use std::fmt::Write as _;

use rtify::config::Appearance;
use rtify::theme::contrast::{Rgb, contrast};
use rtify::theme::manager::fixed_colors;
use rtify::theme::system24::{BaseColors, Palette, css};

// ---------------------------------------------------------------------------------------------
// Colors

#[derive(Debug, Clone, Copy, PartialEq)]
struct Color {
    r: f64,
    g: f64,
    b: f64,
    a: f64,
}

impl Color {
    fn rgb(&self) -> Rgb {
        Rgb::new(self.r, self.g, self.b)
    }

    /// The color as it lands on a black panel (alpha composited over `#000`).
    fn on_black(&self) -> Color {
        Color {
            r: self.r * self.a,
            g: self.g * self.a,
            b: self.b * self.a,
            a: 1.0,
        }
    }

    /// OKLCH lightness (0..1) and chroma.
    fn oklch(&self) -> (f64, f64) {
        let lin = |c: f64| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        let (r, g, b) = (lin(self.r), lin(self.g), lin(self.b));
        let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
        let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
        let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
        let big_l = 0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s;
        let a = 1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s;
        let bb = 0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s;
        (big_l, (a * a + bb * bb).sqrt())
    }

    fn from_oklch(l: f64, c: f64, h_deg: f64) -> Color {
        let (a, b) = (c * h_deg.to_radians().cos(), c * h_deg.to_radians().sin());
        let l_ = (l + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
        let m_ = (l - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
        let s_ = (l - 0.089_484_177_5 * a - 1.291_485_548 * b).powi(3);
        let r = 4.076_741_662_1 * l_ - 3.307_711_591_3 * m_ + 0.230_969_929_2 * s_;
        let g = -1.268_438_004_6 * l_ + 2.609_757_401_1 * m_ - 0.341_319_396_5 * s_;
        let bl = -0.004_196_086_3 * l_ - 0.703_418_614_7 * m_ + 1.707_614_701 * s_;
        let enc = |c: f64| {
            let c = c.clamp(0.0, 1.0);
            if c <= 0.003_130_8 {
                12.92 * c
            } else {
                1.055 * c.powf(1.0 / 2.4) - 0.055
            }
        };
        Color {
            r: enc(r),
            g: enc(g),
            b: enc(bl),
            a: 1.0,
        }
    }

    fn hex(&self) -> String {
        let h = |c: f64| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        if self.a < 1.0 {
            format!("#{:02x}{:02x}{:02x} @ {:.0}%", h(self.r), h(self.g), h(self.b), self.a * 100.0)
        } else {
            format!("#{:02x}{:02x}{:02x}", h(self.r), h(self.g), h(self.b))
        }
    }
}

fn parse_hex(s: &str) -> Option<Color> {
    let digits: Vec<u8> = s.chars().map(|c| c.to_digit(16).map(|d| d as u8)).collect::<Option<_>>()?;
    let (r, g, b, a) = match digits.len() {
        3 | 4 => {
            let d = |i: usize| f64::from(digits[i] * 17) / 255.0;
            (d(0), d(1), d(2), if digits.len() == 4 { d(3) } else { 1.0 })
        }
        6 | 8 => {
            let d = |i: usize| f64::from(digits[i] * 16 + digits[i + 1]) / 255.0;
            (d(0), d(2), d(4), if digits.len() == 8 { d(6) } else { 1.0 })
        }
        _ => return None,
    };
    Some(Color { r, g, b, a })
}

fn number(s: &str, percent_scale: f64) -> Option<f64> {
    let s = s.trim();
    match s.strip_suffix('%') {
        Some(p) => p.trim().parse::<f64>().ok().map(|v| v / 100.0 * percent_scale),
        None => s.parse().ok(),
    }
}

/// Split on top-level separators (commas, slashes, whitespace) of a function's arguments.
fn args(inner: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in inner.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            ',' | '/' | ' ' | '\t' | '\n' if depth == 0 => {
                if !cur.trim().is_empty() {
                    out.push(cur.trim().to_owned());
                }
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_owned());
    }
    out
}

/// A color found in a declaration value, or the function name we could not evaluate.
#[derive(Debug)]
enum Found {
    Color(Color),
    Unknown(String),
}

/// Every color in a CSS value: hex, rgb()/rgba(), oklch(), GTK's alpha(), and named colors.
fn colors(value: &str) -> Vec<Found> {
    let mut out = Vec::new();
    let bytes: Vec<char> = value.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == '#' {
            let hex: String = bytes[i + 1..].iter().take_while(|c| c.is_ascii_hexdigit()).collect();
            if let Some(col) = parse_hex(&hex) {
                out.push(Found::Color(col));
            }
            i += 1 + hex.len();
            continue;
        }
        if c.is_ascii_alphabetic() || c == '-' {
            let word: String = bytes[i..].iter().take_while(|c| c.is_ascii_alphanumeric() || **c == '-').collect();
            let after = i + word.len();
            let is_call = bytes.get(after) == Some(&'(');
            if is_call {
                let mut depth = 0;
                let mut end = bytes.len();
                for (j, ch) in bytes.iter().enumerate().skip(after) {
                    if *ch == '(' {
                        depth += 1;
                    } else if *ch == ')' {
                        depth -= 1;
                        if depth == 0 {
                            end = j;
                            break;
                        }
                    }
                }
                if end == bytes.len() {
                    // Unbalanced (a regex or a format string in source code): nothing to read.
                    i = after + 1;
                    continue;
                }
                let inner: String = bytes[after + 1..end].iter().collect();
                match word.to_ascii_lowercase().as_str() {
                    "rgb" | "rgba" => {
                        let a = args(&inner);
                        let ch = |k: usize| a.get(k).and_then(|s| number(s, 255.0)).map(|v| v / 255.0);
                        let alpha = a.get(3).and_then(|s| number(s, 1.0)).unwrap_or(1.0);
                        match (ch(0), ch(1), ch(2)) {
                            (Some(r), Some(g), Some(b)) => out.push(Found::Color(Color { r, g, b, a: alpha })),
                            _ => out.push(Found::Unknown(format!("{word}({inner})"))),
                        }
                    }
                    "oklch" => {
                        let a = args(&inner);
                        let l = a.first().and_then(|s| number(s, 1.0));
                        let ch = a.get(1).and_then(|s| number(s, 0.4));
                        let h = a.get(2).and_then(|s| s.trim_end_matches("deg").parse::<f64>().ok());
                        let alpha = a.get(3).and_then(|s| number(s, 1.0)).unwrap_or(1.0);
                        match (l, ch, h) {
                            (Some(l), Some(c), Some(h)) => out.push(Found::Color(Color {
                                a: alpha,
                                ..Color::from_oklch(l, c, h)
                            })),
                            _ => out.push(Found::Unknown(format!("{word}({inner})"))),
                        }
                    }
                    "alpha" => {
                        let a = args(&inner);
                        let factor = a.last().and_then(|s| number(s, 1.0));
                        let base: Vec<_> = colors(&a[..a.len().saturating_sub(1)].join(" "));
                        match (base.as_slice(), factor) {
                            ([Found::Color(c)], Some(f)) => out.push(Found::Color(Color { a: c.a * f, ..*c })),
                            _ => out.push(Found::Unknown(format!("{word}({inner})"))),
                        }
                    }
                    "linear-gradient" | "radial-gradient" | "conic-gradient" | "repeating-linear-gradient" => {
                        out.extend(colors(&inner));
                    }
                    "mix" | "shade" | "lighter" | "darker" | "color-mix" | "color" | "hsl" | "hsla" => {
                        out.push(Found::Unknown(format!("{word}({inner})")));
                    }
                    _ => {}
                }
                i = end + 1;
                continue;
            }
            match word.to_ascii_lowercase().as_str() {
                "white" => out.push(Found::Color(Color {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                })),
                "black" => out.push(Found::Color(Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                })),
                "transparent" => out.push(Found::Color(Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.0,
                })),
                _ => {}
            }
            i = after.max(i + 1);
            continue;
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------------------------
// A tiny CSS reader: rules of `selector { prop: value; ... }`, comments stripped.

#[derive(Debug, Clone)]
struct Decl {
    selector: String,
    prop: String,
    value: String,
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = rest[start + 2..].split_once("*/").map(|(_, r)| r).unwrap_or("");
    }
    out.push_str(rest);
    out
}

fn decls(css: &str) -> Vec<Decl> {
    let css = strip_comments(css);
    let mut out = Vec::new();
    let mut rest = css.as_str();
    while let Some(open) = rest.find('{') {
        let selector = rest[..open].split_whitespace().collect::<Vec<_>>().join(" ");
        let Some(close) = rest[open..].find('}') else { break };
        let body = &rest[open + 1..open + close];
        let mut depth = 0;
        let mut start = 0;
        let mut parts = Vec::new();
        for (i, c) in body.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                ';' if depth == 0 => {
                    parts.push(&body[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        parts.push(&body[start..]);
        for part in parts {
            if let Some((prop, value)) = part.split_once(':') {
                out.push(Decl {
                    selector: selector.clone(),
                    prop: prop.trim().to_ascii_lowercase(),
                    value: value.trim().to_owned(),
                });
            }
        }
        rest = &rest[open + close + 1..];
    }
    out
}

fn is_color_prop(prop: &str) -> bool {
    prop == "color"
        || prop.starts_with("background")
        || prop.starts_with("border")
        || prop.starts_with("outline")
        || prop.contains("shadow")
        || prop == "caret-color"
        || prop == "-gtk-secondary-caret-color"
}

fn is_background_prop(prop: &str) -> bool {
    prop == "background" || prop == "background-color"
}

// ---------------------------------------------------------------------------------------------
// The stylesheets under test

fn default_css() -> (Palette, String) {
    let appearance = Appearance::default();
    assert!(appearance.oled, "DESIGN.md is an OLED design; OLED mode must be the default");
    let palette = Palette::derive_with(fixed_colors(&appearance), true);
    let css = css(&palette, &appearance.font_family, appearance.font_size);
    (palette, css)
}

/// Accents an album cover or a Noctalia template can hand us: neon, dark, grey, warm, cold.
const ACCENT_SWEEP: [&str; 10] = [
    "#b99aff", "#00ff00", "#ff0000", "#ffff00", "#0000ff", "#000080", "#202020", "#808080", "#ff8800", "#00e5ff",
];

fn sweep_css() -> Vec<(String, String)> {
    let appearance = Appearance::default();
    ACCENT_SWEEP
        .iter()
        .flat_map(|accent| [(accent, false), (accent, true)])
        .map(|(accent, light)| {
            let base = BaseColors::from_accent(Rgb::parse(accent).unwrap(), None, light);
            let palette = Palette::derive_with(base, true);
            (
                format!("accent {accent}{}", if light { " (light source)" } else { "" }),
                css(&palette, &appearance.font_family, appearance.font_size),
            )
        })
        .collect()
}

/// Fails with every violation listed once; repeats of the same color/value are folded into a count.
fn report(rule: &str, violations: &[String]) {
    if violations.is_empty() {
        return;
    }
    let mut folded: Vec<(String, usize)> = Vec::new();
    for v in violations {
        let key = v.split(" @@ ").next().unwrap_or(v);
        match folded.iter_mut().find(|(k, _)| k.split(" @@ ").next() == Some(key)) {
            Some((_, n)) => *n += 1,
            None => folded.push((v.clone(), 1)),
        }
    }
    let violations: Vec<String> = folded
        .into_iter()
        .map(|(v, n)| {
            let (key, example) = v.split_once(" @@ ").unwrap_or((&v, ""));
            match (n, example) {
                (1, "") => key.to_owned(),
                (1, e) => format!("{key}: {e}"),
                (n, e) => format!("{key}: {e} (+{} more rules)", n - 1),
            }
        })
        .collect();
    let mut msg = format!("DESIGN.md: {rule}\n{} violation(s):\n", violations.len());
    for v in violations.iter().take(60) {
        let _ = writeln!(msg, "  - {v}");
    }
    if violations.len() > 60 {
        let _ = writeln!(msg, "  … and {} more", violations.len() - 60);
    }
    panic!("{msg}");
}

fn each_color(css: &str, mut f: impl FnMut(&Decl, Color)) {
    for d in decls(css).iter().filter(|d| is_color_prop(&d.prop)) {
        for found in colors(&d.value) {
            if let Found::Color(c) = found {
                f(d, c);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Rules

/// "Kein Reinweiß (#FFFFFF), weder für Text noch für Flächen."
#[test]
fn no_pure_white() {
    let mut v = Vec::new();
    for (name, css) in std::iter::once(("default".to_owned(), default_css().1)).chain(sweep_css()) {
        each_color(&css, |d, c| {
            let c = c.on_black();
            if c.r > 0.99 && c.g > 0.99 && c.b > 0.99 {
                v.push(format!("[{name}] {} @@ {} {{ {}: {} }}", c.hex(), d.selector, d.prop, d.value));
            }
        });
    }
    report("no pure white", &v);
}

/// "Keine fast-schwarzen Grautöne zwischen L 1 % und 17 %" and "Nicht-schwarze Flächen haben
/// L ≥ 18 %". Translucent colors are judged as they land on black.
#[test]
fn no_near_black_greys() {
    let mut v = Vec::new();
    for (name, css) in std::iter::once(("default".to_owned(), default_css().1)).chain(sweep_css()) {
        each_color(&css, |d, c| {
            if c.a == 0.0 {
                return;
            }
            let (l, _) = c.on_black().oklch();
            if l > 0.01 && l < 0.175 {
                v.push(format!(
                    "[{name}] {} is L {:.1} % @@ {} {{ {}: {} }}",
                    c.hex(),
                    l * 100.0,
                    d.selector,
                    d.prop,
                    d.value
                ));
            }
        });
    }
    report("no near-black greys (L 1–17 %)", &v);
}

/// "Zustände – deckend" and "keine transparenten Panels": backgrounds are opaque or fully
/// transparent, never a translucent overlay.
#[test]
fn backgrounds_are_opaque() {
    let (_, css) = default_css();
    let mut v = Vec::new();
    for d in decls(&css).iter().filter(|d| is_background_prop(&d.prop)) {
        for found in colors(&d.value) {
            if let Found::Color(c) = found
                && c.a > 0.0
                && c.a < 1.0
            {
                v.push(format!("{} {{ {}: {} }}", d.selector, d.prop, d.value));
            }
        }
    }
    report("opaque state colors", &v);
}

/// "Anti-Blooming: Text maximal L 96 % und nie Reinweiß."
#[test]
fn text_is_at_most_l96() {
    let mut v = Vec::new();
    for (name, css) in std::iter::once(("default".to_owned(), default_css().1)).chain(sweep_css()) {
        for d in decls(&css).iter().filter(|d| d.prop == "color") {
            for found in colors(&d.value) {
                if let Found::Color(c) = found {
                    let (l, _) = c.on_black().oklch();
                    if l > 0.965 {
                        v.push(format!(
                            "[{name}] {} is L {:.1} % @@ {} {{ color: {} }}",
                            c.hex(),
                            l * 100.0,
                            d.selector,
                            d.value
                        ));
                    }
                }
            }
        }
    }
    report("text at most L 96 %", &v);
}

/// "Der Akzent bleibt pastellig (Chroma ≈ 0.12)", "keine gesättigten Neonfarben".
#[test]
fn colors_stay_pastel() {
    let mut v = Vec::new();
    for (name, css) in std::iter::once(("default".to_owned(), default_css().1)).chain(sweep_css()) {
        each_color(&css, |d, c| {
            if c.a == 0.0 {
                return;
            }
            let (_, chroma) = c.oklch();
            if chroma > 0.145 {
                v.push(format!(
                    "[{name}] {} has chroma {chroma:.3} @@ {} {{ {}: {} }}",
                    c.hex(),
                    d.selector,
                    d.prop,
                    d.value
                ));
            }
        });
    }
    report("pastel accents (chroma ≤ ~0.13)", &v);
}

/// "Radius 0 – überall."
#[test]
fn no_rounded_corners() {
    let (_, css) = default_css();
    let v: Vec<String> = decls(&css)
        .iter()
        .filter(|d| d.prop.contains("radius"))
        .filter(|d| {
            d.value
                .split_whitespace()
                .any(|t| number(t.trim_end_matches("px"), 1.0) != Some(0.0))
        })
        .map(|d| format!("{} {{ {}: {} }}", d.selector, d.prop, d.value))
        .collect();
    report("no rounded corners", &v);
}

/// "Keine Schatten, kein Glow", "Kein Blur, kein Glas". Inset shadows are allowed: DESIGN.md
/// uses them as a 2px accent bar for selected rows and pressed accent buttons.
#[test]
fn no_shadows_glow_or_blur() {
    let (_, css) = default_css();
    let mut v = Vec::new();
    for d in decls(&css) {
        let value = d.value.to_ascii_lowercase();
        let bad = match d.prop.as_str() {
            "box-shadow" => value != "none" && !value.split(',').all(|s| s.trim().starts_with("inset")),
            "text-shadow" | "-gtk-icon-shadow" => value != "none",
            "filter" | "-gtk-icon-filter" => value.contains("blur") || value.contains("drop-shadow"),
            "backdrop-filter" => true,
            _ => false,
        };
        if bad {
            v.push(format!("{} {{ {}: {} }}", d.selector, d.prop, d.value));
        }
    }
    report("no shadows, glow or blur", &v);
}

/// "Keine Verläufe als Flächenfüllung." A gradient between two identical colors (a trick to cut a
/// border behind a label) is not a fill and is allowed.
#[test]
fn no_gradient_fills() {
    let (_, css) = default_css();
    let mut v = Vec::new();
    for d in decls(&css).iter().filter(|d| d.value.contains("gradient(")) {
        let cols: Vec<Color> = colors(&d.value)
            .into_iter()
            .filter_map(|f| if let Found::Color(c) = f { Some(c) } else { None })
            .collect();
        if cols.windows(2).any(|w| w[0].hex() != w[1].hex()) {
            v.push(format!("{} {{ {}: {} }}", d.selector, d.prop, d.value));
        }
    }
    report("no gradient fills", &v);
}

/// Every color is something we can check: palette hex values, not GTK color math.
#[test]
fn every_color_is_checkable() {
    let (_, css) = default_css();
    let mut v = Vec::new();
    for d in decls(&css).iter().filter(|d| is_color_prop(&d.prop)) {
        for found in colors(&d.value) {
            if let Found::Unknown(f) = found {
                v.push(format!("{} {{ {}: {} }} uses {f}", d.selector, d.prop, d.value));
            }
        }
    }
    report("colors are plain values", &v);
}

/// "Linienstärken: 2px für Panel-Rahmen, 1px für Buttons, Inputs und Trennlinien, 4px für
/// Divider."
#[test]
fn border_widths_follow_the_scale() {
    let (_, css) = default_css();
    let allowed = [0.0, 1.0, 2.0, 4.0];
    let mut v = Vec::new();
    for d in decls(&css)
        .iter()
        .filter(|d| d.prop.starts_with("border") && !d.prop.contains("radius"))
    {
        for token in d.value.split_whitespace() {
            if let Some(px) = token.strip_suffix("px").and_then(|n| n.parse::<f64>().ok())
                && !allowed.contains(&px)
            {
                v.push(format!("{} {{ {}: {} }}", d.selector, d.prop, d.value));
            }
        }
    }
    report("border widths 0/1/2/4 px", &v);
}

/// "Grundgewicht 400 statt 300": nothing lighter than regular.
#[test]
fn no_light_font_weights() {
    let (_, css) = default_css();
    let v: Vec<String> = decls(&css)
        .iter()
        .filter(|d| d.prop == "font-weight")
        .filter(|d| match d.value.trim() {
            "lighter" => true,
            n => n.parse::<u32>().is_ok_and(|w| w < 400),
        })
        .map(|d| format!("{} {{ font-weight: {} }}", d.selector, d.value))
        .collect();
    report("font weight ≥ 400", &v);
}

/// "Eine Familie für alles: DM Mono", with a monospace fallback.
#[test]
fn font_is_dm_mono_with_monospace_fallback() {
    let family = Appearance::default().font_family;
    let families: Vec<String> = family
        .split(',')
        .map(|f| f.trim().trim_matches(['"', '\'']).to_ascii_lowercase())
        .collect();
    let mut v = Vec::new();
    if families.first().map(String::as_str) != Some("dm mono") {
        v.push(format!("default font family is {family:?}, DESIGN.md wants \"DM Mono\" first"));
    }
    if !families.iter().any(|f| f == "monospace" || f == "ui-monospace") {
        v.push(format!("default font family {family:?} has no monospace fallback"));
    }
    report("DM Mono with monospace fallback", &v);
}

/// "Fokus muss immer sichtbar sein (outline: 2px solid var(--accent-2))": buttons, entries and
/// list rows each have a :focus-visible outline of at least 2px in a visible color.
#[test]
fn focus_is_always_visible() {
    let (palette, css) = default_css();
    let all = decls(&css);
    let mut v = Vec::new();
    for widget in ["button", "entry", "row"] {
        let outline = all
            .iter()
            .find(|d| d.prop == "outline" && d.selector.split(',').any(|s| s.contains(widget) && s.contains(":focus-visible")));
        match outline {
            None => v.push(format!("no `{widget}:focus-visible {{ outline: … }}` rule")),
            Some(d) => {
                let width = d
                    .value
                    .split_whitespace()
                    .find_map(|t| t.strip_suffix("px").and_then(|n| n.parse::<f64>().ok()))
                    .unwrap_or(0.0);
                if width < 2.0 {
                    v.push(format!("{} {{ outline: {} }} is thinner than 2px", d.selector, d.value));
                }
                let visible = colors(&d.value)
                    .iter()
                    .any(|f| matches!(f, Found::Color(c) if c.a > 0.0 && contrast(c.on_black().rgb(), palette.bg1) >= 3.0));
                if !visible {
                    v.push(format!("{} {{ outline: {} }} is not visible on black", d.selector, d.value));
                }
            }
        }
    }
    report("visible focus", &v);
}

/// WCAG: wherever a rule sets both text and background, the text stays readable (4.5:1), and
/// every text color stays at least as bright as text-5 (2.5:1) on the black base.
#[test]
fn text_is_readable() {
    let mut v = Vec::new();
    for (name, css) in std::iter::once(("default".to_owned(), default_css().1)).chain(sweep_css()) {
        let all = decls(&css);
        let black = Rgb::new(0.0, 0.0, 0.0);
        // A check/radio `color` is the mark drawn on the checked (accent) fill, not on the box.
        let mark = |sel: &str| sel.split(',').all(|s| s.trim().ends_with(" check") || s.trim().ends_with(" radio"));
        for d in all.iter().filter(|d| d.prop == "color" && !mark(&d.selector)) {
            let Some(Found::Color(fg)) = colors(&d.value).into_iter().next() else {
                continue;
            };
            let fg = fg.on_black();
            // The background comes from the rule itself or, for `parent child`, from the parent.
            let bg = std::iter::successors(Some(d.selector.as_str()), |s| s.rsplit_once(' ').map(|(p, _)| p)).find_map(|sel| {
                all.iter()
                    .filter(|b| b.selector == sel && is_background_prop(&b.prop))
                    .find_map(|b| match colors(&b.value).into_iter().next() {
                        Some(Found::Color(c)) if c.a > 0.0 => Some(c.on_black()),
                        _ => None,
                    })
            });
            match bg {
                Some(bg) => {
                    let ratio = contrast(fg.rgb(), bg.rgb());
                    if ratio < 4.5 {
                        v.push(format!(
                            "[{name}] {} on {} is {ratio:.2}:1 (< 4.5) @@ {}",
                            fg.hex(),
                            bg.hex(),
                            d.selector
                        ));
                    }
                }
                None => {
                    let ratio = contrast(fg.rgb(), black);
                    if ratio < 2.4 {
                        v.push(format!(
                            "[{name}] {} on black is {ratio:.2}:1 (< text-5) @@ {}",
                            fg.hex(),
                            d.selector
                        ));
                    }
                }
            }
        }
    }
    report("readable text", &v);
}

/// Colors hardcoded outside the theme (an HTML page, a cairo fill) follow the same two hard
/// rules: no pure white and no near-black grey.
#[test]
fn hardcoded_colors_in_sources() {
    let mut v = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            // The theme module is where palette colors are supposed to live.
            if path.components().any(|c| c.as_os_str() == "theme") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            // Unit tests feed odd colors on purpose.
            let code = text.split("#[cfg(test)]").next().unwrap_or("");
            for (n, line) in code.lines().enumerate() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for found in colors(line) {
                    let Found::Color(c) = found else { continue };
                    if c.a == 0.0 {
                        continue;
                    }
                    let (l, _) = c.on_black().oklch();
                    let white = c.r > 0.99 && c.g > 0.99 && c.b > 0.99;
                    if white || (l > 0.01 && l < 0.175) {
                        let rel = path.strip_prefix(env!("CARGO_MANIFEST_DIR")).unwrap_or(&path);
                        v.push(format!("{}:{}: {} (L {:.1} %)", rel.display(), n + 1, c.hex(), l * 100.0));
                    }
                }
            }
        }
    }
    v.sort();
    report("hardcoded colors", &v);
}

// ---------------------------------------------------------------------------------------------
// The checker itself

#[test]
fn checker_reads_css_and_colors() {
    let d = decls("/* x */ a, b { color: #fff; background: rgba(255, 0, 0, 0.5) } c{border:1px solid oklch(70% 0.12 310)}");
    assert_eq!(d.len(), 3);
    assert_eq!(d[0].selector, "a, b");
    assert!(matches!(colors(&d[0].value)[..], [Found::Color(c)] if c.r == 1.0 && c.a == 1.0));
    assert!(matches!(colors(&d[1].value)[..], [Found::Color(c)] if c.r == 1.0 && c.a == 0.5));
    let Found::Color(accent) = &colors(&d[2].value)[0] else { panic!() };
    assert_eq!(accent.hex(), "#b589d6", "oklch() colors are parsed and converted");
    let (l, c) = accent.oklch();
    assert!((l - 0.70).abs() < 0.005 && (c - 0.12).abs() < 0.005);
    let (l_mint, c_mint) = parse_hex("9bd4c9").unwrap().oklch();
    assert!(
        (l_mint - 0.83).abs() < 0.01 && (c_mint - 0.06).abs() < 0.01,
        "DESIGN.md: accent-2 = oklch(83% 0.06 182) ≈ #9BD4C9"
    );
    let (l18, _) = parse_hex("141414").unwrap().oklch();
    assert!((l18 - 0.19).abs() < 0.01, "DESIGN.md: hover oklch(19%) ≈ #141414");
    assert!(matches!(colors("alpha(#ffffff, 0.1)")[..], [Found::Color(c)] if (c.a - 0.1).abs() < 1e-9));
    assert!(matches!(colors("mix(#000, #fff, 0.5)")[..], [Found::Unknown(_)]));
    assert!(colors("solid transparent").len() == 1);
}
