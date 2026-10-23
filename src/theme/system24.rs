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
    /// Pure black surfaces, hairline borders, no large lit areas.
    pub oled: bool,
}

const BLACK: Rgb = Rgb::new(0.0, 0.0, 0.0);

impl Palette {
    pub fn derive(base: BaseColors) -> Self {
        Self::derive_with(base, false)
    }

    /// `oled` replaces every surface with true black (pixels off) whatever the palette source
    /// says; foreground and accent are re-checked against black.
    pub fn derive_with(base: BaseColors, oled: bool) -> Self {
        let bg = if oled { BLACK } else { base.background };
        // Slightly below full white on OLED: less glare, less wear on static text.
        let fg = ensure_contrast(base.foreground, bg, 7.0);
        let fg = if oled { fg.mix(BLACK, 0.08) } else { fg };
        let light = bg.is_light();
        let accent = ensure_contrast(base.accent, bg, 4.5);
        if oled {
            return Palette {
                bg0: BLACK,
                bg1: BLACK,
                bg2: BLACK.mix(fg, 0.05),
                bg3: BLACK.mix(fg, 0.09),
                border: BLACK.mix(fg, 0.14),
                border_hover: BLACK.mix(fg, 0.3),
                text: fg,
                text_dim: ensure_contrast(fg.mix(BLACK, 0.4), BLACK, 4.5),
                text_faint: fg.mix(BLACK, 0.62),
                accent,
                on_accent: on_color(accent),
                red: ensure_contrast(base.error.unwrap_or(Rgb::parse("#e06c75").unwrap()), BLACK, 4.5),
                light: false,
                oled: true,
            };
        }
        let darker = |amount: f64| {
            if light {
                bg.mix(BLACK, amount)
            } else {
                bg.mix(BLACK, amount * 2.5)
            }
        };
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
            oled: false,
        }
    }
}

/// The application stylesheet for a palette and typography settings.
///
/// Layout follows the official client (top bar, library sidebar, main view, now-playing panel,
/// player bar); the look stays System24: monospace, 1px rectangles, `[ TITLE ]` labels, one
/// accent used for text and outlines rather than large filled areas.
pub fn css(p: &Palette, font_family: &str, font_size: u32) -> String {
    let font = font_family.replace(['{', '}', ';'], "");
    format!(
        r#"
window.rt-window, window.rt-window.background {{ background: {bg0}; color: {text}; }}
window.rt-window, window.rt-window * {{ font-family: {font}; }}
window.rt-window {{ font-size: {size}pt; }}
.rt-root {{ background: {bg0}; }}
.rt-shell {{ padding: 0 8px; }}

/* panels: 1px rectangle, title sits on the top border */
.rt-panel {{ background: {bg1}; border: 1px solid {border}; }}
label.rt-panel-title {{ color: {text_faint}; font-weight: bold; padding: 0 5px; min-height: 18px;
                        background-image: linear-gradient(to bottom, {bg0} 50%, {bg1} 50%); }}
.rt-panel-body {{ padding: 14px 10px 8px 10px; }}

/* top bar */
.rt-topbar {{ padding: 6px 10px; min-height: 36px; }}
.rt-brand {{ color: {accent}; font-weight: bold; }}
entry.rt-search {{ min-height: 30px; padding: 0 10px; border-color: {border}; background: {bg0}; }}
entry.rt-search:focus-within {{ border-color: {accent}; }}

/* typography */
label.rt-hero {{ font-size: 2.6em; font-weight: 800; color: {text}; }}
label.rt-kind {{ font-size: 0.85em; color: {text_faint}; letter-spacing: 2px; }}
label.rt-title {{ font-size: 1.3em; font-weight: bold; color: {text}; }}
label.rt-subtitle, label.rt-dim, .rt-dim {{ color: {text_dim}; }}
label.rt-faint, .rt-faint {{ color: {text_faint}; }}
label.rt-small {{ font-size: 0.9em; }}
label.rt-accent {{ color: {accent}; }}
label.rt-error {{ color: {red}; }}
label.rt-status-online {{ color: {text_dim}; }}
label.rt-status-online.rt-dot {{ color: {accent}; }}
label.rt-status-offline {{ color: {red}; }}
label.rt-mono-num {{ font-feature-settings: "tnum"; }}
label.rt-section {{ color: {text_faint}; font-weight: bold; margin: 10px 0 4px 0; }}

/* buttons: outlines, never big filled blocks */
button {{ background: transparent; background-image: none; color: {text}; border: 1px solid {border};
          border-radius: 0; box-shadow: none; padding: 2px 10px; min-height: 24px; text-shadow: none; }}
button:hover {{ background: {bg2}; border-color: {border_hover}; }}
button:active {{ background: {bg3}; }}
button:disabled {{ color: {text_faint}; border-color: {bg3}; }}
button:focus-visible, entry:focus-visible, dropdown:focus-visible > button {{ outline: 1px solid {accent}; outline-offset: -1px; }}
button.flat, button.rt-icon {{ border-color: transparent; color: {text_dim}; }}
button.flat:hover, button.rt-icon:hover {{ color: {text}; background: transparent; border-color: transparent; }}
button.rt-icon {{ padding: 2px 6px; min-width: 22px; }}
button.rt-icon:checked, button.rt-on {{ color: {accent}; background: transparent; border-color: transparent; }}
button.rt-play {{ color: {accent}; border: 1px solid {accent}; min-width: 34px; min-height: 30px; padding: 0 10px; font-weight: bold; }}
button.rt-play:hover {{ background: {accent}; color: {on_accent}; }}
button.rt-primary {{ color: {accent}; border-color: {accent}; font-weight: bold; padding: 4px 18px; }}
button.rt-primary:hover {{ background: {accent}; color: {on_accent}; }}
button.rt-chip {{ color: {text_dim}; padding: 1px 10px; min-height: 22px; }}
button.rt-chip:checked {{ color: {accent}; border-color: {accent}; background: transparent; }}
button:checked {{ color: {accent}; border-color: {accent}; background: transparent; }}

entry, spinbutton, dropdown > button, combobox > button {{ background: {bg0}; color: {text}; border: 1px solid {border}; border-radius: 0; box-shadow: none; }}
entry:focus-within {{ border-color: {accent}; outline: none; }}
entry.rt-filter {{ border-width: 0 0 1px 0; background: transparent; min-height: 26px; }}
entry > text > selection, label > selection, text > selection {{ background: {accent}; color: {on_accent}; }}

/* lists */
list, listview, list.rt-list {{ background: transparent; color: {text}; }}
list > row {{ padding: 4px 8px; border-left: 2px solid transparent; border-radius: 0; }}
list > row:hover {{ background: {bg2}; }}
list > row:selected {{ background: {bg3}; color: {text}; border-left-color: {accent}; }}
list > row.rt-playing label.rt-row-title, list > row.rt-playing label.rt-row-num {{ color: {accent}; }}
list > row:focus-visible {{ outline: 1px solid {accent}; outline-offset: -1px; }}
.rt-table-head {{ border-bottom: 1px solid {border}; padding: 0 10px 6px 10px; margin: 0 0 4px 0; }}
.rt-table-head label {{ color: {text_faint}; font-size: 0.85em; letter-spacing: 1px; }}

/* covers */
.rt-cover {{ border: 1px solid {border}; background: {bg2}; }}
.rt-cover-placeholder {{ border: 1px solid {border}; color: {accent}; font-size: 2em; }}

/* player bar */
.rt-playerbar {{ border-top: 1px solid {border}; padding: 8px 14px; min-height: 76px; background: {bg0}; }}

scale {{ padding: 6px 0; }}
scale > trough {{ background: {bg3}; min-height: 3px; border-radius: 0; border: none; }}
scale > trough > highlight {{ background: {text_dim}; border-radius: 0; border: none; min-height: 3px; margin: 0; }}
scale:hover > trough > highlight {{ background: {accent}; }}
scale > trough > slider {{ background: transparent; min-width: 9px; min-height: 9px; margin: -4px; border-radius: 0; border: none; box-shadow: none; }}
scale:hover > trough > slider {{ background: {text}; }}

checkbutton check, checkbutton radio {{ background: {bg0}; border: 1px solid {border}; border-radius: 0; color: {on_accent}; }}
checkbutton check:checked, checkbutton radio:checked {{ background: {accent}; border-color: {accent}; }}
switch {{ background: {bg0}; border: 1px solid {border}; border-radius: 0; }}
switch:checked {{ background: {bg0}; border-color: {accent}; }}
switch > slider {{ background: {text_faint}; border-radius: 0; box-shadow: none; margin: 2px; }}
switch:checked > slider {{ background: {accent}; }}

popover > contents, popover > arrow, .rt-popover {{ background: {bg0}; color: {text}; border: 1px solid {border}; border-radius: 0; box-shadow: none; }}
popover modelbutton:hover, popover row:hover {{ background: {bg2}; }}
tooltip, tooltip.background {{ background: {bg0}; color: {text}; border: 1px solid {border}; border-radius: 0; box-shadow: none; }}
separator {{ background: {border}; min-width: 1px; min-height: 1px; }}
paned > separator {{ background: transparent; background-image: none; min-width: 8px; }}
scrollbar {{ background: transparent; border: none; }}
scrollbar slider {{ background: {border}; border-radius: 0; min-width: 3px; min-height: 3px; }}
scrollbar slider:hover {{ background: {border_hover}; }}
scrolledwindow undershoot, scrolledwindow overshoot {{ background: none; }}

/* context menus */
popover.rt-menu > contents {{ padding: 4px 0; min-width: 220px; }}
button.rt-menu-item {{ border: none; border-left: 2px solid transparent; padding: 3px 14px; min-height: 24px; color: {text}; }}
button.rt-menu-item:hover {{ background: {bg2}; border-left-color: {accent}; }}
button.rt-menu-back {{ color: {text_dim}; }}

/* sortable table headers */
button.rt-th {{ border: none; padding: 0; min-height: 0; background: transparent; color: {text_faint}; }}
button.rt-th label {{ font-size: 0.9em; letter-spacing: 1px; }}
button.rt-th:hover {{ color: {text}; background: transparent; }}
button.rt-th.rt-on {{ color: {accent}; }}

/* one-click add: only visible on the hovered/selected row */
button.rt-hover {{ opacity: 0; min-width: 0; padding: 0; }}
list > row:hover button.rt-hover, list > row:selected button.rt-hover {{ opacity: 1; }}

.rt-toast {{ background: {bg0}; border: 1px solid {accent}; padding: 6px 8px 6px 14px; }}
.rt-toast label {{ color: {text}; }}
popover.rt-tip > contents {{ padding: 2px 6px; }}
label.rt-drag-tag {{ background: {bg0}; color: {accent}; border: 1px solid {accent}; padding: 3px 8px; }}
dropdown.rt-compact > button {{ border-color: transparent; background: transparent; padding: 0 4px; color: {text_dim}; }}
list > row.rt-drop {{ border-left-color: {accent}; }}
list > row:drop(active) {{ border-left: 2px solid {accent}; background: {bg2}; box-shadow: none; }}

.rt-banner {{ border: 1px solid {border}; padding: 4px 10px; margin: 0 8px 6px 8px; }}
.rt-banner label {{ color: {text}; }}
.rt-banner.rt-warning {{ border-color: {red}; }}
.rt-lyrics {{ background: {bg1}; }}
.rt-toolbar {{ padding: 2px 0; }}
.rt-toolbar button {{ border-color: transparent; color: {text_dim}; }}
.rt-toolbar button:hover {{ color: {text}; background: transparent; }}
.rt-toolbar button:checked {{ color: {accent}; }}
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

    #[test]
    fn oled_surfaces_are_black_and_readable() {
        for (accent, light) in [("#b99aff", false), ("#b99aff", true), ("#1a1a1a", false)] {
            let p = Palette::derive_with(BaseColors::from_accent(Rgb::parse(accent).unwrap(), None, light), true);
            assert_eq!(p.bg0.to_hex(), "#000000");
            assert_eq!(p.bg1.to_hex(), "#000000");
            assert!(contrast(p.text, p.bg1) >= 7.0 * 0.85, "{accent}");
            assert!(contrast(p.text_dim, p.bg1) >= 4.5);
            assert!(contrast(p.accent, p.bg1) >= 4.5);
            assert!(p.border.luminance() < 0.05, "hairline borders stay dark");
        }
    }
}
