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

/// Every color the stylesheet and the lyrics renderer use. In OLED mode these are the DESIGN.md
/// tokens (named in the comments); the other modes derive the same roles from the palette source.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// Window background and gaps (`bg-3`).
    pub bg0: Rgb,
    /// Panel surface (`bg-4`).
    pub bg1: Rgb,
    /// Hovered rows (`hover`).
    pub bg2: Rgb,
    /// Selected rows (`active`).
    pub bg3: Rgb,
    /// Neutral button and its hover (`bg-2`, `bg-1`).
    pub button: Rgb,
    pub button_hover: Rgb,
    /// Panel frames, lines inside panels, button and input outlines.
    pub border: Rgb,
    pub border_light: Rgb,
    pub button_border: Rgb,
    /// Hovered or focused panel frame (`border-hover`).
    pub border_hover: Rgb,
    /// Brightest text: selection (`text-1`).
    pub text_bright: Rgb,
    /// Headings, names, important labels (`text-2`).
    pub text: Rgb,
    /// Running text (`text-3`).
    pub text_body: Rgb,
    /// List entries, icons, panel labels (`text-4`).
    pub text_dim: Rgb,
    /// Timestamps and muted entries only (`text-5`).
    pub text_faint: Rgb,
    /// Signal color: focus, hover frames, markers (`accent-2`).
    pub accent: Rgb,
    /// Accent text and links (`accent-1`).
    pub accent_text: Rgb,
    /// The one filled primary button and its hover (`accent-3`, `accent-4`).
    pub accent_fill: Rgb,
    pub accent_fill_hover: Rgb,
    pub on_accent: Rgb,
    /// Errors, destructive actions (`accent-new`).
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
            return Self::oled(base.accent);
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
            button: bg.mix(fg, 0.06),
            button_hover: bg.mix(fg, 0.11),
            border,
            border_light: bg.mix(fg, 0.09),
            button_border: border,
            border_hover: bg.mix(fg, 0.34),
            text_bright: fg,
            text: fg,
            text_body: ensure_contrast(fg.mix(bg, 0.15), bg, 7.0),
            text_dim: ensure_contrast(fg.mix(bg, 0.38), bg, 4.5),
            text_faint: fg.mix(bg, 0.6),
            accent,
            accent_text: accent,
            accent_fill: accent,
            accent_fill_hover: accent.mix(bg, 0.15),
            on_accent: on_color(accent),
            red: ensure_contrast(base.error.unwrap_or(Rgb::parse("#e06c75").unwrap()), bg, 4.5),
            light,
            oled: false,
        }
    }
}

/// The default accent (DESIGN.md `accent`): a pastel mint. Album art, Noctalia or a custom
/// accent only change its hue.
pub const DEFAULT_ACCENT: &str = "#9bd4c9";

impl Palette {
    /// DESIGN.md "system24 OLED": four achromatic greys on true black and one pastel accent.
    /// Only the accent's hue is taken from the palette source (lightness and chroma stay at the
    /// token values, so contrast never shifts); a grey source keeps the default hue.
    fn oled(accent: Rgb) -> Palette {
        let default = Rgb::parse(DEFAULT_ACCENT).unwrap_or(accent);
        let (l0, c0, h0) = default.to_oklch();
        let (_, chroma, hue) = accent.to_oklch();
        let accent = if chroma < 0.03 || (hue - h0).abs() < 0.5 {
            default
        } else {
            Rgb::from_oklch(l0, c0, hue)
        };
        let hex = |s: &str| Rgb::parse(s).unwrap_or(BLACK);
        Palette {
            bg0: BLACK,
            bg1: BLACK,
            bg2: hex("#141414"),
            bg3: hex("#141414"),
            button: hex("#141414"),
            button_hover: hex("#1f1f1f"),
            border: hex("#2a2a2a"),
            border_light: hex("#141414"),
            button_border: hex("#2a2a2a"),
            border_hover: hex("#3a3a3a"),
            text_bright: hex("#f0f0f0"),
            text: hex("#e6e6e6"),
            text_body: hex("#a0a0a0"),
            text_dim: hex("#8a8a8a"),
            text_faint: hex("#8a8a8a"),
            accent,
            accent_text: accent,
            accent_fill: accent,
            accent_fill_hover: accent.mix(hex("#f0f0f0"), 0.25),
            on_accent: BLACK,
            red: hex("#e8a0a0"),
            light: false,
            oled: true,
        }
    }
}

/// The application stylesheet for a palette and typography settings.
///
/// Layout follows the official client (top bar, nav and library column, main view, now playing
/// and queue column, player bar); the look follows DESIGN.md (system24 OLED): monospace, square
/// 1px frames with a lowercase accent label set into the top line, four greys on black and one
/// pastel accent for hover, focus, selection and the primary action.
pub fn css(p: &Palette, font_family: &str, font_size: u32) -> String {
    let font = font_family.replace(['{', '}', ';'], "");
    format!(
        r#"
window.rt-window, window.rt-window.background {{ background: {bg0}; color: {text}; }}
window.rt-window, window.rt-window * {{ font-family: {font}; letter-spacing: -0.2px; }}
window.rt-window {{ font-size: {size}pt; }}
.rt-root {{ background: {bg0}; }}
.rt-shell {{ padding: 0 12px; }}

/* panels: 1px frame with a lowercase accent label set into the top line, an optional grey
   meta text and small framed buttons at the right end of the same line */
.rt-panel {{ background: {bg1}; border: 1px solid {border}; transition: border-color 200ms ease; }}
label.rt-panel-title {{ color: {accent_text}; font-size: 0.85em; padding: 0 6px; min-height: 18px;
                        background-image: linear-gradient(to bottom, {bg0} 50%, {bg1} 50%); }}
label.rt-panel-meta {{ color: {text_faint}; font-size: 0.85em; padding: 0 6px; min-height: 18px;
                       background-image: linear-gradient(to bottom, {bg0} 50%, {bg1} 50%); }}
overlay.rt-panel-host:hover > .rt-panel, overlay.rt-panel-host:focus-within > .rt-panel {{ border-color: {border_hover}; }}
.rt-panel-body {{ padding: 14px 8px 8px 8px; }}
.rt-frame-end {{ background: {bg0}; padding: 0 4px; }}
button.rt-frame {{ background: {bg0}; color: {text_body}; font-size: 0.85em; min-height: 16px; padding: 0 6px; }}

/* top bar */
.rt-topbar {{ padding: 8px 14px 4px 14px; min-height: 30px; }}
button.rt-square {{ min-width: 22px; min-height: 22px; padding: 0; }}
.rt-searchbox {{ border: 1px solid {button_border}; padding: 0 10px; min-height: 26px; }}
.rt-searchbox:focus-within {{ border-color: {accent}; }}
.rt-searchbox entry, entry.rt-bare {{ border: none; background: transparent; min-height: 24px; padding: 0; }}
.rt-searchbox entry:focus-within, entry.rt-bare:focus-within {{ border: none; }}
label.rt-slash {{ color: {accent_text}; }}
entry.rt-search-xl {{ font-size: 2.4em; font-weight: 500; min-height: 52px; }}
label.rt-slash-xl {{ color: {accent_text}; font-size: 2.4em; font-weight: 500; }}

/* typography: hierarchy through brightness, a few sizes */
label.rt-hero-xl {{ font-size: 3.2em; font-weight: 500; color: {text_bright}; }}
label.rt-hero {{ font-size: 2.6em; font-weight: 500; color: {text_bright}; }}
label.rt-kind, label.rt-kicker {{ font-size: 0.85em; color: {text_faint}; }}
label.rt-title {{ font-size: 1.3em; font-weight: 500; color: {text_bright}; }}
label.rt-subtitle, label.rt-dim, .rt-dim {{ color: {text_body}; }}
label.rt-faint, .rt-faint {{ color: {text_faint}; }}
label.rt-small {{ font-size: 0.85em; }}
label.rt-accent {{ color: {accent_text}; }}
label.rt-error {{ color: {red}; }}
label.rt-status-online {{ color: {text_faint}; }}
label.rt-status-offline {{ color: {red}; }}
label.rt-mono-num {{ font-feature-settings: "tnum"; }}
label.rt-section {{ color: {accent_text}; font-size: 0.85em; margin: 10px 0 4px 0; }}
label.rt-key {{ color: {text_faint}; }}
label.rt-indent {{ color: {text_faint}; }}
.rt-rule {{ background: {border_light}; min-height: 1px; }}
.rt-heading label {{ color: {accent_text}; font-size: 0.85em; }}

/* buttons: 1px outlines; hover takes the accent frame and bright text; the primary action is
   the only filled surface */
button {{ background: transparent; background-image: none; color: {text}; border: 1px solid {button_border};
          border-radius: 0; box-shadow: none; padding: 2px 10px; min-height: 22px; text-shadow: none;
          transition: border-color 150ms ease, color 150ms ease; }}
button:hover {{ border-color: {accent}; color: {text_bright}; }}
button:active {{ background: {button}; }}
button:disabled {{ color: {text_faint}; border-color: {border_light}; }}
button:focus-visible, entry:focus-visible, dropdown:focus-visible > button {{ outline: 2px solid {accent}; outline-offset: -2px; }}
button.flat, button.rt-icon {{ border-color: transparent; color: {text_body}; }}
button.flat:hover, button.rt-icon:hover {{ color: {text_bright}; background: transparent; border-color: transparent; }}
button.rt-icon {{ padding: 2px 6px; min-width: 22px; }}
button.rt-icon:checked, button.rt-on {{ color: {accent_text}; background: transparent; border-color: transparent; }}

button.rt-primary {{ background: {accent_fill}; color: {on_accent}; border-color: {accent_fill}; font-weight: 500; padding: 4px 14px; }}
button.rt-primary:hover {{ background: {accent_fill_hover}; border-color: {accent_fill_hover}; color: {on_accent}; }}
button.rt-primary:active {{ background: {accent_fill_hover}; box-shadow: inset 0 0 0 2px {on_accent}; }}
button.rt-primary label {{ color: {on_accent}; }}
button.rt-chip, button.rt-seg {{ color: {text_body}; padding: 1px 8px; min-height: 22px; font-size: 0.85em; }}
button.rt-chip:hover, button.rt-seg:hover {{ color: {text_bright}; }}
button.rt-chip:checked, button.rt-seg:checked, button:checked {{ color: {accent_text}; border-color: {accent}; background: transparent; }}
button.rt-current {{ color: {accent_text}; border-color: {accent}; }}
button.rt-danger {{ background: {red}; color: {on_accent}; border-color: {red}; }}

entry, spinbutton, dropdown > button, combobox > button {{ background: {bg1}; color: {text}; border: 1px solid {button_border}; border-radius: 0; box-shadow: none; caret-color: {accent}; min-height: 24px; }}
entry:focus-within {{ border-color: {accent}; outline: none; }}
entry.rt-invalid, entry.rt-invalid:focus-within {{ border-color: {red}; }}
entry > text > placeholder {{ color: {text_faint}; }}
entry image {{ color: {text_faint}; }}
entry.rt-filter {{ border-width: 0 0 1px 0; background: transparent; min-height: 24px; }}
entry.rt-field {{ font-size: 0.9em; }}
entry > text > selection, label > selection, text > selection {{ background: {accent}; color: {on_accent}; }}

/* lists: hover and selection are one opaque grey; the selected or playing title takes the accent */
list, listview, list.rt-list {{ background: transparent; color: {text}; }}
list > row {{ padding: 4px 8px; border-radius: 0; transition: background-color 150ms ease; }}
list > row:hover {{ background: {bg2}; }}
list > row:selected {{ background: {bg3}; color: {text_bright}; }}
list > row:selected label.rt-row-title {{ color: {accent_text}; }}
list > row.rt-playing label.rt-row-title, list > row.rt-playing label.rt-row-num {{ color: {accent_text}; }}
list > row:focus-visible {{ outline: 2px solid {accent}; outline-offset: -2px; }}
list.rt-nav > row {{ padding: 5px 8px; color: {text_body}; }}
list.rt-nav > row:selected {{ color: {text_bright}; }}
list.rt-nav > row:selected label {{ color: {text_bright}; }}
list.rt-nav > row:selected label.rt-key {{ color: {text_faint}; }}
.rt-table-head {{ border-bottom: 1px solid {border_light}; padding: 0 10px 6px 10px; margin: 0 0 4px 0; }}
.rt-table-head label {{ color: {text_faint}; font-size: 0.85em; }}

/* covers: framed, the brightest content on screen */
.rt-cover {{ border: 1px solid {border}; background: {bg2}; }}
.rt-cover-placeholder {{ border: 1px solid {border}; color: {accent}; font-size: 2em; }}

/* cards and tiles: a faint frame that takes the accent on hover */
button.rt-card {{ border: 1px solid transparent; padding: 0; background: transparent; color: {text}; }}
button.rt-card:hover {{ border-color: transparent; color: {text_bright}; }}
button.rt-card:hover .rt-cover {{ border-color: {accent}; }}
button.rt-tile {{ border: 1px solid {border_light}; padding: 6px; background: transparent; color: {text}; }}
button.rt-tile:hover {{ border-color: {accent}; color: {text_bright}; }}
button.rt-tile:checked, button.rt-card:checked {{ border-color: {accent}; }}
label.rt-badge {{ background: {bg0}; color: {text_body}; border: 1px solid {button_border}; font-size: 0.8em; padding: 0 5px; margin: 6px; }}

/* settings: rows divided by a faint line, dependent rows indented under their parent */
.rt-setting {{ border-bottom: 1px solid {border_light}; padding: 10px 0; }}
.rt-setting label.rt-setting-desc {{ color: {text_faint}; font-size: 0.85em; }}
.rt-setting.rt-invalid label.rt-setting-desc {{ color: {red}; }}
.rt-setting label.rt-note-accent {{ color: {accent_text}; }}
.rt-setting label.rt-note-red {{ color: {red}; }}
button.rt-keycap {{ font-size: 0.85em; min-height: 22px; padding: 0 8px; color: {text}; }}
button.rt-keycap.rt-recording {{ border-color: {accent}; color: {accent_text}; }}
button.rt-keycap.rt-invalid {{ border-color: {red}; color: {red}; }}

/* draggable column dividers: a grey line appears on hover */
.rt-divider {{ min-width: 12px; }}
.rt-divider-line {{ min-width: 2px; background: transparent; }}
.rt-divider:hover > .rt-divider-line {{ background: {text_faint}; }}

/* player bar: every control framed and 32px high; the accent marks what is on */
.rt-playerbar {{ padding: 6px 14px; min-height: 64px; background: {bg1}; }}
button.rt-pbtn {{ min-height: 30px; padding: 0 12px; color: {text_body}; }}
button.rt-pbtn-square {{ min-width: 30px; padding: 0; }}
button.rt-pbtn:hover {{ border-color: {accent}; color: {text_bright}; }}
button.rt-pbtn:checked, button.rt-pbtn.rt-on {{ border-color: {accent}; color: {accent_text}; background: transparent; }}
button.rt-pbtn label.rt-pbtn-value {{ color: {text}; }}
button.rt-pbtn.rt-play {{ background: {accent_fill}; color: {on_accent}; border-color: {accent_fill}; }}
button.rt-pbtn.rt-play:hover {{ background: {accent_fill_hover}; border-color: {accent_fill_hover}; color: {on_accent}; }}
button.rt-pbtn.rt-play:focus-visible {{ outline: 2px solid {on_accent}; outline-offset: -4px; }}
.rt-volume {{ min-height: 30px; padding: 0 12px; color: {text_body}; border: 1px solid {button_border}; }}
.rt-volume:hover {{ color: {text_bright}; }}
.rt-volume-lit {{ color: {accent}; }}

scale {{ padding: 6px 0; }}
scale > trough {{ background: {border}; min-height: 2px; border-radius: 0; border: none; }}
scale > trough > highlight {{ background: {accent}; border-radius: 0; border: none; min-height: 2px; margin: 0; }}
scale > trough > slider {{ background: transparent; min-width: 9px; min-height: 9px; margin: -4px; border-radius: 0; border: none; box-shadow: none; }}
scale:hover > trough > slider {{ background: {text}; }}

checkbutton {{ color: {text}; }}
checkbutton check, checkbutton radio {{ background: {bg1}; border: 1px solid {button_border}; border-radius: 0; color: {on_accent}; }}
checkbutton check:checked, checkbutton radio:checked {{ background: {accent}; border-color: {accent}; }}
switch {{ background: {bg1}; border: 1px solid {button_border}; border-radius: 0; }}
switch:checked {{ background: {bg1}; border-color: {accent}; }}
switch > slider {{ background: {text_faint}; border-radius: 0; box-shadow: none; margin: 2px; }}
switch:checked > slider {{ background: {accent}; }}

/* floating: black, framed, no shadow */
popover > contents, popover > arrow, .rt-popover {{ background: {bg0}; color: {text}; border: 1px solid {border_hover}; border-radius: 0; box-shadow: none; }}
popover modelbutton:hover, popover row:hover {{ background: {bg2}; }}
tooltip, tooltip.background {{ background: {bg0}; color: {text}; border: 1px solid {border_hover}; border-radius: 0; box-shadow: none; }}
separator {{ background: {border_light}; min-width: 1px; min-height: 1px; }}
paned > separator {{ background: transparent; background-image: none; min-width: 12px; }}
scrollbar {{ background: transparent; border: none; }}
scrollbar slider {{ background: {border}; border-radius: 0; min-width: 4px; min-height: 4px; }}
scrollbar slider:hover {{ background: {border_hover}; }}
scrolledwindow undershoot, scrolledwindow overshoot {{ background: none; }}

/* context menus: entries behave like list rows */
popover.rt-menu > contents {{ padding: 4px 0; min-width: 220px; }}
button.rt-menu-item {{ border: none; padding: 3px 14px; min-height: 24px; color: {text}; }}
button.rt-menu-item:hover {{ background: {bg2}; color: {accent_text}; }}
button.rt-menu-back {{ color: {text_faint}; }}

/* sortable table headers */
button.rt-th {{ border: none; padding: 0; min-height: 0; background: transparent; color: {text_faint}; }}
button.rt-th label {{ font-size: 0.85em; }}
button.rt-th:hover {{ color: {text}; background: transparent; }}
button.rt-th.rt-on {{ color: {accent_text}; }}

/* one-click add: only visible on the hovered/selected row */
button.rt-hover {{ opacity: 0; min-width: 0; padding: 0; }}
list > row:hover button.rt-hover, list > row:selected button.rt-hover {{ opacity: 1; }}

.rt-toast {{ background: {bg0}; border: 1px solid {accent}; min-height: 30px; }}
.rt-toast-content {{ padding: 0 8px 0 14px; }}
.rt-toast label {{ color: {text_body}; }}
.rt-toast-line {{ color: {accent_line}; }}
@keyframes rt-toast-in {{ from {{ opacity: 0; transform: translateY(14px) scale(0.96); }} to {{ opacity: 1; transform: none; }} }}
@keyframes rt-toast-out {{ from {{ opacity: 1; transform: none; }} to {{ opacity: 0; transform: translateY(8px) scale(0.98); }} }}
@keyframes rt-toast-bump-a {{ 0% {{ transform: scale(1); }} 40% {{ transform: scale(1.04); }} 100% {{ transform: scale(1); }} }}
@keyframes rt-toast-bump-b {{ 0% {{ transform: scale(1); }} 40% {{ transform: scale(1.04); }} 100% {{ transform: scale(1); }} }}
.rt-toast-in {{ animation: rt-toast-in 260ms cubic-bezier(0.2, 0.9, 0.25, 1.15) both; }}
.rt-toast-out {{ animation: rt-toast-out 200ms ease-in both; }}
.rt-toast.rt-toast-bump-a {{ animation: rt-toast-bump-a 220ms ease-out; }}
.rt-toast.rt-toast-bump-b {{ animation: rt-toast-bump-b 220ms ease-out; }}
popover.rt-tip > contents {{ padding: 2px 6px; }}
label.rt-drag-tag {{ background: {bg0}; color: {accent_text}; border: 1px solid {accent}; padding: 3px 8px; }}
dropdown.rt-compact > button {{ border-color: transparent; background: transparent; padding: 0 4px; color: {text_body}; }}
list > row.rt-drop {{ box-shadow: inset 2px 0 0 {accent}; }}
list > row:drop(active) {{ box-shadow: inset 2px 0 0 {accent}; background: {bg2}; }}

.rt-banner {{ border: 1px solid {border}; padding: 4px 10px; margin: 0 12px 6px 12px; }}
.rt-banner label {{ color: {text}; }}
.rt-banner.rt-warning {{ border-color: {red}; }}
.rt-lyrics {{ background: {bg1}; }}
.rt-toolbar {{ padding: 2px 0; }}
.rt-toolbar button {{ border-color: transparent; color: {text_body}; }}
.rt-toolbar button:hover {{ color: {text_bright}; background: transparent; }}
.rt-toolbar button:checked {{ color: {accent_text}; }}
"#,
        bg0 = p.bg0,
        bg1 = p.bg1,
        bg2 = p.bg2,
        bg3 = p.bg3,
        button = p.button,
        border = p.border,
        border_light = p.border_light,
        button_border = p.button_border,
        border_hover = p.border_hover,
        text_bright = p.text_bright,
        text = p.text,
        text_body = p.text_body,
        text_faint = p.text_faint,
        accent = p.accent,
        accent_text = p.accent_text,
        // The prototype's 50% accent line, mixed onto the toast's black (no translucency).
        accent_line = p.accent.mix(p.bg0, 0.5),
        accent_fill = p.accent_fill,
        accent_fill_hover = p.accent_fill_hover,
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

    #[test]
    fn oled_palette_follows_design_md() {
        // Album art, Noctalia or a custom accent: only the hue may differ from the tokens.
        for accent in ["#b589d6", "#b99aff", "#ff0000", "#1a1a1a", "#00ff88"] {
            let p = Palette::derive_with(BaseColors::from_accent(Rgb::parse(accent).unwrap(), None, false), true);
            let surfaces = [
                p.bg0,
                p.bg1,
                p.bg2,
                p.bg3,
                p.button,
                p.button_hover,
                p.border,
                p.border_light,
                p.button_border,
            ];
            for c in surfaces {
                let (l, _, _) = c.to_oklch();
                assert!(!(0.005..=0.175).contains(&l), "{accent}: {} is in the smearing range", c.to_hex());
            }
            for c in [p.text_bright, p.text, p.text_body, p.text_dim, p.text_faint] {
                let (l, chroma, _) = c.to_oklch();
                assert!(l <= 0.965 && chroma < 0.01, "{accent}: {} too bright or tinted", c.to_hex());
            }
            assert!(contrast(p.text_dim, BLACK) >= 4.5);
            assert!(contrast(p.accent, BLACK) >= 7.0);
            assert!(contrast(p.on_accent, p.accent_fill) >= 6.0);
            let (l, c, _) = p.accent.to_oklch();
            assert!((l - 0.83).abs() < 0.01 && (c - 0.06).abs() < 0.01, "{accent}");
        }
        // Only the hue follows the source: a violet accent keeps the token lightness and chroma.
        let p = Palette::derive_with(BaseColors::from_accent(Rgb::parse("#b589d6").unwrap(), None, false), true);
        let (_, _, hue) = Rgb::parse("#b589d6").unwrap().to_oklch();
        assert!((p.accent.to_oklch().2 - hue).abs() < 0.5);
        assert_eq!(p.text_bright.to_hex(), "#f0f0f0");
        assert_eq!(p.red.to_hex(), "#e8a0a0");
        let p = Palette::derive_with(BaseColors::from_accent(Rgb::parse("#1a1a1a").unwrap(), None, false), true);
        assert_eq!(p.accent.to_hex(), DEFAULT_ACCENT, "a grey source keeps the default accent");
    }
}
