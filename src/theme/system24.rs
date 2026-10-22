//! The System24 look recreated in GTK CSS: flat rectangular panels with the title set into a
//! thin border, monospace type, one accent. Colors are resolved in Rust and written into the
//! stylesheet, so this works on every GTK 4 release (no reliance on CSS custom properties).

use super::contrast::{Rgb, contrast, ensure_contrast, on_color};

/// The few colors a palette source has to provide.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BaseColors {
    pub background: Rgb,
    pub foreground: Rgb,
    pub accent: Rgb,
    pub border: Option<Rgb>,
    pub error: Option<Rgb>,
}

impl BaseColors {
    /// System24 defaults: near-black surfaces tinted with the accent hue.
    pub fn from_accent(accent: Rgb, background: Option<Rgb>, light: bool) -> Self {
        let (h, s, _) = accent.to_hsl();
        let tint = (s * 0.18).min(0.14);
        let background = background.unwrap_or_else(|| {
            if light {
                Rgb::from_hsl(h, tint, 0.95)
            } else {
                Rgb::from_hsl(h, tint, 0.10)
            }
        });
        let foreground = if background.is_light() {
            Rgb::from_hsl(h, tint, 0.16)
        } else {
            Rgb::from_hsl(h, tint, 0.84)
        };
        Self {
            background,
            foreground,
            accent,
            border: None,
            error: None,
        }
    }

    pub fn lerp(self, other: BaseColors, t: f64) -> BaseColors {
        let opt = |a: Option<Rgb>, b: Option<Rgb>| match (a, b) {
            (Some(a), Some(b)) => Some(a.mix(b, t)),
            (_, b) => b,
        };
        BaseColors {
            background: self.background.mix(other.background, t),
            foreground: self.foreground.mix(other.foreground, t),
            accent: self.accent.mix(other.accent, t),
            border: opt(self.border, other.border),
            error: opt(self.error, other.error),
        }
    }
}

/// Every color the stylesheet and the lyrics renderer use.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub bg0: Rgb,
    pub bg1: Rgb,
    pub bg2: Rgb,
    pub bg3: Rgb,
    pub border: Rgb,
    pub border_hover: Rgb,
    pub text: Rgb,
    pub text_dim: Rgb,
    pub text_faint: Rgb,
    pub accent: Rgb,
    pub on_accent: Rgb,
    pub red: Rgb,
    pub light: bool,
}

impl Palette {
    pub fn derive(base: BaseColors) -> Self {
        let bg = base.background;
        let fg = ensure_contrast(base.foreground, bg, 7.0);
        let light = bg.is_light();
        let darker = |amount: f64| {
            if light {
                bg.mix(Rgb::new(0.0, 0.0, 0.0), amount)
            } else {
                bg.mix(Rgb::new(0.0, 0.0, 0.0), amount * 2.5)
            }
        };
        let accent = ensure_contrast(base.accent, bg, 4.5);
        let border = base.border.filter(|b| contrast(*b, bg) >= 1.3).unwrap_or_else(|| bg.mix(fg, 0.16));
        Palette {
            bg0: darker(0.06),
            bg1: bg,
            bg2: bg.mix(fg, 0.06),
            bg3: bg.mix(fg, 0.11),
            border,
            border_hover: bg.mix(fg, 0.34),
            text: fg,
            text_dim: ensure_contrast(fg.mix(bg, 0.38), bg, 4.5),
            text_faint: fg.mix(bg, 0.6),
            accent,
            on_accent: on_color(accent),
            red: ensure_contrast(base.error.unwrap_or(Rgb::parse("#e06c75").unwrap()), bg, 4.5),
            light,
        }
    }
}

fn rgba(c: Rgb, a: f64) -> String {
    let v = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("rgba({},{},{},{a:.2})", v(c.r), v(c.g), v(c.b))
}

/// The application stylesheet for a palette and typography settings.
pub fn css(p: &Palette, font_family: &str, font_size: u32) -> String {
    let font = font_family.replace(['{', '}', ';'], "");
    format!(
        r#"
window.rt-window, window.rt-window.background {{ background: {bg0}; color: {text}; }}
window.rt-window, window.rt-window * {{ font-family: {font}; }}
window.rt-window {{ font-size: {size}pt; }}
.rt-root {{ background: {bg0}; padding: 8px; }}

.rt-panel {{ background: {bg1}; border: 1px solid {border}; }}
label.rt-panel-title {{ color: {text_dim}; font-weight: bold; padding: 0 5px; min-height: 18px;
                        background-image: linear-gradient(to bottom, {bg0} 50%, {bg1} 50%); }}
.rt-panel-body {{ padding: 14px 10px 8px 10px; }}

label.rt-title {{ font-size: 1.35em; font-weight: bold; color: {text}; }}
label.rt-subtitle {{ color: {text_dim}; }}
label.rt-dim, .rt-dim {{ color: {text_dim}; }}
label.rt-faint, .rt-faint {{ color: {text_faint}; }}
label.rt-accent {{ color: {accent}; }}
label.rt-error {{ color: {red}; }}
label.rt-status-online {{ color: {accent}; }}
label.rt-status-offline {{ color: {red}; }}
label.rt-mono-num {{ font-feature-settings: "tnum"; }}

button {{ background: transparent; background-image: none; color: {text}; border: 1px solid {border};
          border-radius: 0; box-shadow: none; padding: 2px 10px; min-height: 22px; text-shadow: none; }}
button:hover {{ background: {bg2}; border-color: {border_hover}; }}
button:active {{ background: {bg3}; }}
button:checked, button.rt-accent {{ background: {accent}; color: {on_accent}; border-color: {accent}; }}
button:checked:hover, button.rt-accent:hover {{ background: {accent_hover}; }}
button:disabled {{ color: {text_faint}; border-color: {bg3}; }}
button.flat {{ border-color: transparent; }}
button.flat:hover {{ border-color: {border}; }}
button.rt-tab {{ border-color: transparent; color: {text_dim}; padding: 2px 6px; }}
button.rt-tab:checked {{ background: transparent; color: {accent}; border-color: transparent; border-bottom: 1px solid {accent}; }}

entry, spinbutton, dropdown > button, combobox > button {{ background: {bg0}; color: {text}; border: 1px solid {border}; border-radius: 0; box-shadow: none; }}
entry:focus-within {{ border-color: {accent}; outline: none; }}
entry > text > selection, label > selection, text > selection {{ background: {accent}; color: {on_accent}; }}

list, listview, list.rt-list {{ background: transparent; color: {text}; }}
list > row {{ padding: 3px 6px; border-left: 2px solid transparent; border-radius: 0; }}
list > row:hover {{ background: {bg2}; }}
list > row:selected {{ background: {bg3}; color: {text}; border-left-color: {accent}; }}
list > row.rt-playing label.rt-row-title {{ color: {accent}; }}
list > row:focus-visible {{ outline: 1px solid {accent}; outline-offset: -1px; }}

scale {{ padding: 6px 0; }}
scale > trough {{ background: {bg3}; min-height: 4px; border-radius: 0; border: none; }}
scale > trough > highlight {{ background: {accent}; border-radius: 0; border: none; min-height: 4px; margin: 0; }}
scale > trough > slider {{ background: {text}; min-width: 10px; min-height: 10px; margin: -4px; border-radius: 0; border: none; box-shadow: none; }}
scale:hover > trough > slider {{ background: {accent}; }}

checkbutton check, checkbutton radio {{ background: {bg0}; border: 1px solid {border}; border-radius: 0; color: {on_accent}; }}
checkbutton check:checked, checkbutton radio:checked {{ background: {accent}; border-color: {accent}; }}
switch {{ background: {bg3}; border: 1px solid {border}; border-radius: 0; }}
switch:checked {{ background: {accent}; }}
switch > slider {{ background: {text}; border-radius: 0; box-shadow: none; }}

popover > contents, popover > arrow, .rt-popover {{ background: {bg1}; color: {text}; border: 1px solid {border}; border-radius: 0; box-shadow: none; }}
popover modelbutton:hover, popover row:hover {{ background: {bg2}; }}
tooltip, tooltip.background {{ background: {bg1}; color: {text}; border: 1px solid {border}; border-radius: 0; box-shadow: none; }}
separator {{ background: {border}; min-width: 1px; min-height: 1px; }}
paned > separator {{ background: transparent; background-image: none; min-width: 6px; }}
scrollbar {{ background: transparent; border: none; }}
scrollbar slider {{ background: {border}; border-radius: 0; min-width: 4px; min-height: 4px; }}
scrollbar slider:hover {{ background: {border_hover}; }}
scrolledwindow undershoot, scrolledwindow overshoot {{ background: none; }}

.rt-cover {{ border: 1px solid {border}; background: {bg2}; }}
.rt-banner {{ background: {bg2}; border: 1px solid {border}; padding: 4px 10px; }}
.rt-banner label {{ color: {text}; }}
.rt-banner.rt-warning {{ border-color: {red}; }}
.rt-lyrics {{ background: {bg0}; }}
.rt-toolbar {{ padding: 2px 0; }}
"#,
        bg0 = p.bg0,
        bg1 = p.bg1,
        bg2 = p.bg2,
        bg3 = p.bg3,
        border = p.border,
        border_hover = p.border_hover,
        text = p.text,
        text_dim = p.text_dim,
        text_faint = p.text_faint,
        accent = p.accent,
        accent_hover = rgba(p.accent, 0.85),
        on_accent = p.on_accent,
        red = p.red,
        size = font_size.clamp(7, 32),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_palette_is_readable() {
        for (accent, light) in [("#b99aff", false), ("#b99aff", true), ("#202020", false), ("#ffff00", true)] {
            let base = BaseColors::from_accent(Rgb::parse(accent).unwrap(), None, light);
            let p = Palette::derive(base);
            assert!(contrast(p.text, p.bg1) >= 7.0, "{accent} light={light}");
            assert!(contrast(p.text_dim, p.bg1) >= 4.5);
            assert!(contrast(p.accent, p.bg1) >= 4.5);
            assert!(contrast(p.on_accent, p.accent) >= 3.0);
            assert_eq!(p.light, light);
        }
    }

    #[test]
    fn css_mentions_palette() {
        let p = Palette::derive(BaseColors::from_accent(Rgb::parse("#b99aff").unwrap(), None, false));
        let css = css(&p, "JetBrains Mono", 11);
        assert!(css.contains(&p.accent.to_hex()));
        assert!(css.contains("font-size: 11pt"));
        assert!(!css.contains("var("));
    }
}
