//! Settings, as a view in the main window. Every control writes the config immediately
//! (debounced to disk) and applies live; nothing needs a restart except the preferred global
//! shortcut triggers.
//!
//! Settings that only matter while another one is on are hidden until it is, and drawn indented
//! under it ("└"). The filter field narrows the rows by label and description; a section whose
//! rows are all filtered out disappears. A section shows "reset" while any of its values differs
//! from the default. Invalid input gets a red frame and the rule in place of the description, and
//! is not saved.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::{glib, prelude::*};

use super::widgets::*;
use crate::app::App;
use crate::config::{Config, LyricAnimation, Notifications, PaletteSource, PollingMode};
use crate::desktop::{lifecycle, shortcuts};
use crate::keymap::{self, Action};

/// Input rules for text fields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Rule {
    /// `#rrggbb`.
    Hex,
    /// `#rrggbb` or empty.
    HexOrEmpty,
    /// A whole number in the range.
    Int(i64, i64),
    /// A number in the range.
    Float(f64, f64),
    /// `en`, `de`, `pt-BR`, `zh-Hans`.
    Language,
    /// 32 hexadecimal characters, or empty (not signed in yet).
    ClientId,
    Any,
}

impl Rule {
    pub fn check(self, s: &str) -> bool {
        let s = s.trim();
        let hex = |s: &str| s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit());
        match self {
            Rule::Hex => hex(s),
            Rule::HexOrEmpty => s.is_empty() || hex(s),
            Rule::Int(lo, hi) => s.parse::<i64>().is_ok_and(|v| (lo..=hi).contains(&v)),
            Rule::Float(lo, hi) => s.parse::<f64>().is_ok_and(|v| v.is_finite() && (lo..=hi).contains(&v)),
            Rule::Language => {
                let mut parts = s.split('-');
                let lang = parts.next().unwrap_or_default();
                let region = parts.next();
                (2..=3).contains(&lang.len())
                    && lang.chars().all(|c| c.is_ascii_lowercase())
                    && region.is_none_or(|r| (2..=4).contains(&r.len()) && r.chars().all(|c| c.is_ascii_alphanumeric()))
                    && parts.next().is_none()
            }
            Rule::ClientId => s.is_empty() || (s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit())),
            Rule::Any => true,
        }
    }

    /// What the field expects, shown in place of the description while the input is invalid.
    pub fn explain(self) -> String {
        match self {
            Rule::Hex => "a hex color like #9bd4c9".into(),
            Rule::HexOrEmpty => "a hex color like #000000, or empty".into(),
            Rule::Int(lo, hi) => format!("a whole number from {lo} to {hi}"),
            Rule::Float(lo, hi) => format!("a number from {lo:.1} to {hi:.1}"),
            Rule::Language => "a language code like en, de or pt-BR".into(),
            Rule::ClientId => "32 hexadecimal characters from the Spotify dashboard".into(),
            Rule::Any => String::new(),
        }
    }
}

/// Whether a setting row matches the filter text.
pub fn row_matches(filter: &str, title: &str, desc: &str) -> bool {
    let f = filter.trim().to_lowercase();
    f.is_empty() || title.to_lowercase().contains(&f) || desc.to_lowercase().contains(&f)
}

type Pred = fn(&Config) -> bool;

struct RowHandle {
    widget: gtk::Box,
    title: String,
    desc: String,
    section: usize,
    visible: Pred,
}

struct SectionHandle {
    root: gtk::Box,
    reset: gtk::Button,
    differs: Pred,
    visible: Pred,
}

/// A row of Settings → shortcuts, updated in place while keys are recorded.
struct KeyRow {
    action: Action,
    key: gtk::Button,
    note: gtk::Label,
    reset: gtk::Button,
    /// The portal trigger, for actions that have one; a blank of the same width otherwise.
    global: Option<gtk::Entry>,
    blank: gtk::Box,
}

#[derive(Default)]
struct State {
    rows: Vec<RowHandle>,
    sections: Vec<SectionHandle>,
    keys: Vec<KeyRow>,
    global_head: Option<gtk::Label>,
}

/// Width of the key and global trigger columns.
const KEY_COLUMN: i32 = 150;

#[derive(Clone)]
pub struct SettingsView {
    pub root: gtk::Overlay,
    content: gtk::Box,
    filter: gtk::Entry,
    state: Rc<RefCell<State>>,
    app: App,
}

/// Push the current config to every live consumer.
fn after_change(app: &App) {
    let appearance = app.config.borrow().config.appearance.clone();
    app.theme.set_appearance(&appearance);
    app.apply_lyrics_options();
    app.win().settings.refresh();
}

fn always(_: &Config) -> bool {
    true
}

impl SettingsView {
    pub fn new(app: &App) -> Self {
        let title = label("settings", &["rt-hero-xl"]);
        title.set_hexpand(true);
        let filter = gtk::Entry::new();
        filter.add_css_class("rt-field");
        filter.set_placeholder_text(Some("filter settings"));
        filter.update_property(&[gtk::accessible::Property::Label("Filter settings")]);
        filter.set_width_chars(22);
        filter.set_valign(gtk::Align::Center);
        let head = hbox(12);
        head.append(&title);
        head.append(&filter);
        head.set_margin_bottom(10);
        let content = vbox(26);
        let page = vbox(0);
        page.set_margin_start(16);
        page.set_margin_end(16);
        page.set_margin_top(14);
        page.set_margin_bottom(24);
        page.append(&head);
        page.append(&content);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&page)
            .vexpand(true)
            .hexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let panel = panel("settings", &scroll);
        panel.set_meta("saved automatically");
        let this = Self {
            root: panel.root.clone(),
            content,
            filter: filter.clone(),
            state: Rc::default(),
            app: app.clone(),
        };
        let v = this.clone();
        filter.connect_changed(move |_| v.refresh());
        this.build();
        this
    }

    pub fn focus_filter(&self) {
        self.filter.grab_focus();
    }

    /// Show or hide rows, sections and reset buttons for the current config and filter.
    pub fn refresh(&self) {
        let cfg = self.app.config.borrow().config.clone();
        let filter = self.filter.text().to_string();
        let st = self.state.borrow();
        let mut any = vec![false; st.sections.len()];
        for r in &st.rows {
            let show = (r.visible)(&cfg) && row_matches(&filter, &r.title, &r.desc);
            r.widget.set_visible(show);
            if show {
                any[r.section] = true;
            }
        }
        for (i, s) in st.sections.iter().enumerate() {
            s.root.set_visible(any[i] && (s.visible)(&cfg));
            s.reset.set_visible((s.differs)(&cfg));
        }
        drop(st);
        self.refresh_keys();
    }

    /// Key labels, clashes and the recording state of Settings → shortcuts.
    pub fn refresh_keys(&self) {
        let shortcuts = self.app.config.borrow().config.shortcuts.clone();
        let recording = self.app.recording();
        if let Some(w) = self.app.try_win() {
            w.set_key_hints(&shortcuts.keys);
        }
        let st = self.state.borrow();
        if let Some(h) = &st.global_head {
            h.set_visible(shortcuts.portal);
        }
        for r in &st.keys {
            let k = keymap::key_of(&shortcuts.keys, r.action);
            let rec = recording == Some(r.action);
            let clash: Vec<&str> = if rec {
                Vec::new()
            } else {
                keymap::clashes(&shortcuts.keys, r.action)
                    .into_iter()
                    .map(|a| a.description())
                    .collect()
            };
            r.key.set_label(&if rec { "press a key…".to_owned() } else { keymap::label(k) });
            if let Some(l) = r.key.child().and_downcast::<gtk::Label>() {
                l.set_xalign(0.0);
            }
            r.key.update_property(&[gtk::accessible::Property::Label(&format!(
                "{} key: {}",
                r.action.description(),
                if rec { "press a key".to_owned() } else { keymap::label(k) }
            ))]);
            // The label child would otherwise name the button.
            r.key.reset_relation(gtk::AccessibleRelation::LabelledBy);
            for (class, on) in [("rt-recording", rec), ("rt-invalid", !clash.is_empty())] {
                if on {
                    r.key.add_css_class(class);
                } else {
                    r.key.remove_css_class(class);
                }
            }
            let note = if rec {
                "esc cancels · backspace clears".to_owned()
            } else if !clash.is_empty() {
                format!("also used by {}", clash.join(", "))
            } else {
                String::new()
            };
            r.note.set_text(&note);
            r.note.set_visible(!note.is_empty());
            for (class, on) in [("rt-note-accent", rec), ("rt-note-red", !clash.is_empty())] {
                if on {
                    r.note.add_css_class(class);
                } else {
                    r.note.remove_css_class(class);
                }
            }
            r.reset.set_visible(shortcuts.keys.contains_key(r.action.id()));
            if let Some(g) = &r.global {
                g.set_visible(shortcuts.portal);
            }
            r.blank.set_visible(shortcuts.portal && r.global.is_none());
        }
    }

    /// Rows as the config is now (it also changes outside this view).
    pub fn rebuild(&self) {
        self.build();
        self.refresh();
    }

    /// (Re)create every row from the config, e.g. after a section was reset.
    fn build(&self) {
        while let Some(c) = self.content.first_child() {
            self.content.remove(&c);
        }
        *self.state.borrow_mut() = State::default();
        let app = &self.app;
        let c = app.config.borrow().config.clone();
        let mut b = Builder { view: self.clone() };

        // Appearance.
        b.section(
            "appearance",
            |c| c.appearance != Config::default().appearance,
            |c| c.appearance = Config::default().appearance,
            always,
        );
        let source = PaletteSource::ALL
            .iter()
            .position(|s| *s == c.appearance.palette_source)
            .unwrap_or(0);
        b.row(
            "colors",
            "fixed: your own accent · noctalia: the generated template (rtify --print-noctalia-template) · album art: accent from the playing cover",
            &b.choice(&["fixed", "noctalia", "album art"], source, |_, c, i| {
                c.appearance.palette_source = PaletteSource::ALL[i.min(2)]
            }),
            false,
            always,
        );
        b.field(
            "accent",
            "hex color",
            &c.appearance.fixed_accent,
            "#9bd4c9",
            Rule::Hex,
            true,
            fixed_palette,
            |c, t| c.appearance.fixed_accent = t.to_owned(),
        );
        b.field(
            "background",
            "optional hex; empty derives it from the accent (ignored while OLED black is on)",
            &c.appearance.fixed_background,
            "auto",
            Rule::HexOrEmpty,
            true,
            fixed_palette,
            |c, t| c.appearance.fixed_background = t.to_owned(),
        );
        if let Some(status) = app.theme.status.borrow().clone() {
            let l = label(&status, &["rt-error"]);
            l.set_wrap(true);
            b.row("palette", "", &l, true, always);
        }
        b.row(
            "OLED black",
            "true black, framed panels, dimmed text; only the accent hue follows the palette",
            &b.on_off(c.appearance.oled, |_, c, on| c.appearance.oled = on),
            false,
            always,
        );
        b.row(
            "light surfaces",
            "light panels for the fixed palette",
            &b.on_off(c.appearance.light, |_, c, on| c.appearance.light = on),
            true,
            |c| !c.appearance.oled,
        );
        b.field(
            "font",
            "monospace family used for the interface",
            &c.appearance.font_family,
            "DM Mono",
            Rule::Any,
            false,
            always,
            |c, t| c.appearance.font_family = t.to_owned(),
        );
        b.field(
            "font size",
            "points",
            &c.appearance.font_size.to_string(),
            "10",
            Rule::Int(6, 32),
            false,
            always,
            |c, t| c.appearance.font_size = t.parse().unwrap_or(10),
        );
        b.row(
            "animations",
            "list entrances, toasts, lyric scrolling and fades",
            &b.on_off(c.appearance.animations_enabled, |_, c, on| c.appearance.animations_enabled = on),
            false,
            always,
        );
        b.row(
            "reduced motion",
            "no scrolling, blur, scaling or color fades",
            &b.on_off(c.appearance.reduced_motion, |_, c, on| c.appearance.reduced_motion = on),
            true,
            |c| c.appearance.animations_enabled,
        );

        // Layout.
        b.section(
            "layout",
            |c| {
                let d = Config::default().window;
                (
                    c.window.now_playing_panel,
                    c.window.queue_panel,
                    c.window.sidebar_collapsed,
                    c.window.sidebar_width,
                    c.window.right_width,
                ) != (
                    d.now_playing_panel,
                    d.queue_panel,
                    d.sidebar_collapsed,
                    d.sidebar_width,
                    d.right_width,
                )
            },
            |c| {
                let d = Config::default().window;
                c.window.now_playing_panel = d.now_playing_panel;
                c.window.queue_panel = d.queue_panel;
                c.window.sidebar_collapsed = d.sidebar_collapsed;
                c.window.sidebar_width = d.sidebar_width;
                c.window.right_width = d.right_width;
            },
            always,
        );
        b.row(
            "sidebar",
            "the nav and library column, or icons only (B)",
            &b.choice(&["full", "icons"], c.window.sidebar_collapsed as usize, |a, _, i| {
                let a = a.clone();
                glib::idle_add_local_once(move || a.set_sidebar_collapsed(i == 1));
            }),
            false,
            always,
        );
        b.row(
            "now playing panel",
            "cover and details on the right; steps aside on windows under 1180px",
            &b.on_off(c.window.now_playing_panel, |a, _, on| {
                let a = a.clone();
                glib::idle_add_local_once(move || a.set_now_playing_panel(on));
            }),
            false,
            always,
        );
        b.row(
            "queue",
            "what plays next, under now playing",
            &b.on_off(c.window.queue_panel, |a, _, on| {
                let a = a.clone();
                glib::idle_add_local_once(move || a.set_queue_panel(on));
            }),
            true,
            |c| c.window.now_playing_panel,
        );

        // Playback.
        b.section(
            "playback",
            |c| c.playback != Config::default().playback,
            |c| c.playback = Config::default().playback,
            always,
        );
        b.row(
            "reapply shuffle/repeat on reconnect",
            "waits for an active device, then sets them one at a time; off: Spotify's current values win",
            &b.on_off(c.playback.restore_shuffle_repeat, |_, c, on| c.playback.restore_shuffle_repeat = on),
            false,
            always,
        );
        b.row(
            "resume last context on startup",
            "only when nothing is playing anywhere; off by default so starting rtify is silent",
            &b.on_off(c.playback.resume_on_startup, |_, c, on| c.playback.resume_on_startup = on),
            false,
            always,
        );

        // Built-in player (rtify as a Spotify Connect device).
        b.section(
            "built-in player",
            |c| c.player != Config::default().player,
            |c| c.player = Config::default().player,
            always,
        );
        if !crate::player::AVAILABLE {
            b.row(
                "not available",
                "not in this build (cargo feature builtin-player); rtify controls other Spotify devices only",
                &hbox(0),
                false,
                always,
            );
        } else {
            b.row(
                "play on this computer",
                "rtify becomes a Spotify Connect device (librespot) with sound through PipeWire/PulseAudio; needs Premium",
                &b.on_off(c.player.enabled, |a, c, on| {
                    c.player.enabled = on;
                    let a = a.clone();
                    // set_builtin_enabled writes the config itself and starts or stops the device.
                    glib::idle_add_local_once(move || a.set_builtin_enabled(on));
                }),
                false,
                always,
            );
            let builtin = |c: &Config| c.player.enabled;
            let status = label(&app.builtin_status_text(), &["rt-faint", "rt-small"]);
            status.set_wrap(true);
            status.set_xalign(1.0);
            status.set_max_width_chars(36);
            *app.builtin_label.borrow_mut() = Some(status.downgrade());
            let sign_in = button("sign in", "Sign in once in the browser so this computer can play");
            let a = app.clone();
            sign_in.connect_clicked(move |_| a.builtin_sign_in());
            let status_box = hbox(8);
            status_box.append(&status);
            status_box.append(&sign_in);
            b.row("status", "the login is kept in the Secret Service", &status_box, true, builtin);
            let name = b.entry(&c.player.device_name, &crate::player::default_name(), Rule::Any, |c, t| {
                c.player.device_name = t.to_owned()
            });
            name.update_property(&[gtk::accessible::Property::Label("device name")]);
            let a = app.clone();
            name.connect_changed(move |_| a.builtin_settings_changed());
            b.row(
                "device name",
                "how this computer shows up in Spotify's device lists",
                &name,
                true,
                builtin,
            );
            let bitrate = crate::config::Player::BITRATES
                .iter()
                .position(|r| *r == c.player.bitrate)
                .unwrap_or(2);
            b.row(
                "quality",
                "kbit/s; 320 is Spotify's \"very high\"",
                &b.choice(&["96", "160", "320"], bitrate, |a, c, i| {
                    c.player.bitrate = crate::config::Player::BITRATES[i.min(2)];
                    a.builtin_settings_changed();
                }),
                true,
                builtin,
            );
            b.row(
                "volume normalisation",
                "same loudness for every song, like the official apps",
                &b.on_off(c.player.normalisation, |a, c, on| {
                    c.player.normalisation = on;
                    a.builtin_settings_changed();
                }),
                true,
                builtin,
            );
        }

        // Lyrics.
        b.section(
            "lyrics",
            |c| {
                let (l, d) = (&c.lyrics, Config::default().lyrics);
                (
                    l.follow_playback,
                    l.romanization,
                    l.animation,
                    l.font_scale,
                    l.offset_ms,
                    l.translation_enabled,
                ) != (
                    d.follow_playback,
                    d.romanization,
                    d.animation,
                    d.font_scale,
                    d.offset_ms,
                    d.translation_enabled,
                )
            },
            |c| {
                let d = Config::default().lyrics;
                c.lyrics.follow_playback = d.follow_playback;
                c.lyrics.romanization = d.romanization;
                c.lyrics.animation = d.animation;
                c.lyrics.font_scale = d.font_scale;
                c.lyrics.offset_ms = d.offset_ms;
                c.lyrics.translation_enabled = d.translation_enabled;
            },
            always,
        );
        b.row(
            "follow Spotify playback",
            "show and sync lyrics for the playing song from LRCLIB",
            &b.on_off(c.lyrics.follow_playback, |a, c, on| {
                c.lyrics.follow_playback = on;
                let a = a.clone();
                glib::idle_add_local_once(move || a.lyrics_follow_changed());
            }),
            false,
            always,
        );
        let follows = |c: &Config| c.lyrics.follow_playback;
        b.row(
            "romanization",
            "offline: Japanese (incl. kanji), Korean, Chinese, Cyrillic (romaji toggle)",
            &b.on_off(c.lyrics.romanization, |_, c, on| c.lyrics.romanization = on),
            true,
            follows,
        );
        let anim = match c.lyrics.animation {
            LyricAnimation::Off => 0,
            LyricAnimation::Line => 1,
            LyricAnimation::Smooth => 2,
        };
        b.row(
            "animation",
            "smooth: word fill, blur and scale on lines with word timing",
            &b.choice(&["off", "line", "smooth"], anim, |_, c, i| {
                c.lyrics.animation = [LyricAnimation::Off, LyricAnimation::Line, LyricAnimation::Smooth][i.min(2)]
            }),
            true,
            follows,
        );
        b.field(
            "text size",
            "relative to the interface",
            &format!("{:.1}", c.lyrics.font_scale),
            "1.0",
            Rule::Float(0.5, 3.0),
            true,
            follows,
            |c, t| c.lyrics.font_scale = t.parse().unwrap_or(1.0),
        );
        b.field(
            "timing offset",
            "milliseconds; positive shows lines earlier",
            &c.lyrics.offset_ms.to_string(),
            "0",
            Rule::Int(-60_000, 60_000),
            true,
            follows,
            |c, t| c.lyrics.offset_ms = t.parse().unwrap_or(0),
        );
        b.row(
            "translation",
            "sends the shown lyrics to the provider under translation (translate toggle)",
            &b.on_off(c.lyrics.translation_enabled, |a, c, on| {
                c.lyrics.translation_enabled = on;
                if on {
                    let a = a.clone();
                    glib::idle_add_local_once(move || a.translate_current_lyrics());
                }
            }),
            true,
            follows,
        );

        // Translation.
        b.section(
            "translation",
            |c| {
                let (l, d) = (&c.lyrics, Config::default().lyrics);
                (&l.translation_language, l.translation_only, &l.translation_provider)
                    != (&d.translation_language, d.translation_only, &d.translation_provider)
            },
            |c| {
                let d = Config::default().lyrics;
                c.lyrics.translation_language = d.translation_language;
                c.lyrics.translation_only = d.translation_only;
                c.lyrics.translation_provider = d.translation_provider;
            },
            |c| c.lyrics.follow_playback && c.lyrics.translation_enabled,
        );
        b.field(
            "target language",
            "e.g. en, de, ja, pt-BR",
            &c.lyrics.translation_language,
            "en",
            Rule::Language,
            false,
            always,
            |c, t| c.lyrics.translation_language = t.to_owned(),
        );
        b.row(
            "translation only",
            "show the translation instead of the original line",
            &b.on_off(c.lyrics.translation_only, |_, c, on| c.lyrics.translation_only = on),
            false,
            always,
        );
        b.row(
            "provider",
            "Google needs no key (like qtify); DeepL uses your API key",
            &b.choice(
                &["google", "deepl"],
                (c.lyrics.translation_provider == "deepl") as usize,
                |_, c, i| c.lyrics.translation_provider = if i == 1 { "deepl" } else { "google" }.into(),
            ),
            false,
            always,
        );
        let key = gtk::PasswordEntry::new();
        key.set_show_peek_icon(true);
        key.set_width_chars(16);
        key.set_placeholder_text(Some("stored in the Secret Service"));
        key.update_property(&[gtk::accessible::Property::Label("DeepL API key")]);
        let save_key = button("save", "Store the key in the Secret Service");
        let key_box = hbox(6);
        key_box.append(&key);
        key_box.append(&save_key);
        let (a, k) = (app.clone(), key.clone());
        save_key.connect_clicked(move |_| {
            a.set_deepl_key(&k.text());
            k.set_text("");
            a.translate_current_lyrics();
        });
        b.row(
            "DeepL API key",
            "kept in the Secret Service, never in the config file",
            &key_box,
            true,
            |c| c.lyrics.translation_provider == "deepl",
        );

        // Window & system.
        b.section(
            "window & system",
            |c| {
                let (d, w, bg) = (Config::default(), &c.window, &c.background);
                (w.minimize_to_tray, w.start_hidden, w.remember_geometry)
                    != (d.window.minimize_to_tray, d.window.start_hidden, d.window.remember_geometry)
                    || *bg != d.background
            },
            |c| {
                let d = Config::default();
                c.window.minimize_to_tray = d.window.minimize_to_tray;
                c.window.start_hidden = d.window.start_hidden;
                c.window.remember_geometry = d.window.remember_geometry;
                c.background = d.background;
            },
            always,
        );
        b.row(
            "tray icon",
            "StatusNotifierItem (Noctalia, Waybar, KDE …)",
            &b.on_off(c.background.tray, |a, c, on| {
                c.background.tray = on;
                a.set_tray_enabled(on);
            }),
            false,
            always,
        );
        let tray = |c: &Config| c.background.tray;
        b.row(
            "minimize to tray",
            "closing the window keeps rtify running; quit from the tray or the quit key",
            &b.on_off(c.window.minimize_to_tray, |_, c, on| c.window.minimize_to_tray = on),
            true,
            tray,
        );
        b.row(
            "start hidden",
            "open to the tray instead of a window",
            &b.on_off(c.window.start_hidden, |_, c, on| c.window.start_hidden = on),
            true,
            tray,
        );
        b.row(
            "launch at login",
            "XDG autostart entry, starts hidden; never starts playback by itself",
            &b.on_off(lifecycle::autostart_enabled(), |a, c, on| {
                c.background.autostart = on;
                if let Err(e) = lifecycle::set_autostart(on) {
                    a.notice(&format!("autostart: {e}"), true);
                }
            }),
            false,
            always,
        );
        b.row(
            "remember window size",
            "",
            &b.on_off(c.window.remember_geometry, |_, c, on| c.window.remember_geometry = on),
            false,
            always,
        );
        let notify = match c.background.notifications {
            Notifications::Off => 0,
            Notifications::Background => 1,
            Notifications::Always => 2,
        };
        b.row(
            "track notifications",
            "",
            &b.choice(&["off", "when hidden", "always"], notify, |_, c, i| {
                c.background.notifications = [Notifications::Off, Notifications::Background, Notifications::Always][i.min(2)]
            }),
            false,
            always,
        );
        b.row(
            "polling",
            "adaptive polls less while hidden or paused",
            &b.choice(
                &["adaptive", "fixed"],
                (c.background.polling_mode == PollingMode::Fixed) as usize,
                |_, c, i| c.background.polling_mode = if i == 1 { PollingMode::Fixed } else { PollingMode::Adaptive },
            ),
            false,
            always,
        );
        b.field(
            "poll interval",
            "milliseconds",
            &c.background.polling_interval_ms.to_string(),
            "3000",
            Rule::Int(500, 60_000),
            true,
            |c| c.background.polling_mode == PollingMode::Fixed,
            |c, t| c.background.polling_interval_ms = t.parse().unwrap_or(3000),
        );

        // Spotify account.
        b.section(
            "spotify account",
            |c| c.spotify.redirect_port != Config::default().spotify.redirect_port,
            |c| c.spotify.redirect_port = Config::default().spotify.redirect_port,
            always,
        );
        b.field(
            "client ID",
            "from developer.spotify.com/dashboard",
            &c.spotify.client_id,
            "client ID",
            Rule::ClientId,
            false,
            always,
            |c, t| c.spotify.client_id = t.to_owned(),
        );
        b.field(
            "redirect port",
            "the redirect URI is http://127.0.0.1:<port>/callback",
            &c.spotify.redirect_port.to_string(),
            "8898",
            Rule::Int(1024, 65_535),
            false,
            always,
            |c, t| c.spotify.redirect_port = t.parse().unwrap_or(8898),
        );
        let logout = button("sign out", "Forget the stored login");
        let a = app.clone();
        logout.connect_clicked(move |_| a.logout());
        b.row("session", "", &logout, false, always);

        // Global shortcuts.
        // Keys in the window, with the portal trigger next to them.
        b.section("shortcuts", |c| !c.shortcuts.keys.is_empty(), |c| c.shortcuts.keys.clear(), always);
        let head = hbox(4);
        let in_app = label("in app", &["rt-faint", "rt-small"]);
        in_app.set_size_request(KEY_COLUMN, -1);
        let global = label("global", &["rt-faint", "rt-small"]);
        global.set_size_request(KEY_COLUMN, -1);
        for l in [&in_app, &global] {
            l.set_xalign(0.0);
            head.append(l);
        }
        b.row("", "", &head, false, always);
        self.state.borrow_mut().global_head = Some(global);
        for action in Action::ALL {
            b.key_row(action, &c);
        }

        b.section(
            "global shortcuts",
            |c| {
                let d = Config::default().shortcuts;
                (c.shortcuts.portal, &c.shortcuts.bindings) != (d.portal, &d.bindings)
            },
            |c| {
                let d = Config::default().shortcuts;
                c.shortcuts.portal = d.portal;
                c.shortcuts.bindings = d.bindings;
            },
            always,
        );
        b.row(
            "use the GlobalShortcuts portal",
            "applies on next start",
            &b.on_off(c.shortcuts.portal, |_, c, on| c.shortcuts.portal = on),
            false,
            always,
        );
        let portal = |c: &Config| c.shortcuts.portal;
        let status = label(&app.shortcut_status.borrow(), &["rt-faint", "rt-small"]);
        status.set_wrap(true);
        status.set_selectable(true);
        status.set_max_width_chars(48);
        b.row("status", "", &status, true, portal);
        let copy_lua = button("copy lua", "Copy the binds for Hyprland 0.55+ (Lua)");
        let copy_conf = button("copy hyprlang", "Copy the binds for older Hyprland (hyprlang)");
        for (btn, lua) in [(&copy_lua, true), (&copy_conf, false)] {
            let a = app.clone();
            btn.connect_clicked(move |btn| {
                let text = shortcuts::hyprland_snippet(&a.config.borrow().config.shortcuts.bindings, lua);
                btn.clipboard().set_text(&text);
                a.notice("Hyprland bindings copied to the clipboard", false);
            });
        }
        let copy_row = hbox(6);
        copy_row.append(&copy_lua);
        copy_row.append(&copy_conf);
        b.row("Hyprland binds", "for compositors without the portal", &copy_row, true, portal);

        drop(b);
        self.refresh();
    }
}

fn fixed_palette(c: &Config) -> bool {
    c.appearance.palette_source == PaletteSource::Fixed
}

struct Builder {
    view: SettingsView,
}

impl Builder {
    fn section(&mut self, title: &str, differs: Pred, reset: fn(&mut Config), visible: Pred) {
        let root = vbox(0);
        let head = hbox(10);
        head.add_css_class("rt-heading");
        head.append(&label(title, &[]));
        let rule = hbox(0);
        rule.add_css_class("rt-rule");
        rule.set_hexpand(true);
        rule.set_valign(gtk::Align::Center);
        head.append(&rule);
        let reset_btn = button("reset", &format!("Reset {title} to the defaults"));
        reset_btn.add_css_class("rt-seg");
        head.append(&reset_btn);
        root.append(&head);
        let v = self.view.clone();
        reset_btn.connect_clicked(move |_| {
            let (followed, player) = {
                let c = &v.app.config.borrow().config;
                (c.lyrics.follow_playback, c.player.clone())
            };
            v.app.update_config(reset);
            let a = v.app.clone();
            // Live consumers first, then rebuild the rows with the restored values.
            let appearance = a.config.borrow().config.appearance.clone();
            a.theme.set_appearance(&appearance);
            a.apply_lyrics_options();
            if a.config.borrow().config.lyrics.follow_playback != followed {
                a.lyrics_follow_changed();
            }
            let now = a.config.borrow().config.player.clone();
            if now.enabled != player.enabled {
                a.set_builtin_enabled(now.enabled);
            } else if now != player {
                a.builtin_settings_changed();
            }
            a.apply_layout();
            let v = v.clone();
            glib::idle_add_local_once(move || v.build());
        });
        self.view.content.append(&root);
        self.view.state.borrow_mut().sections.push(SectionHandle {
            root,
            reset: reset_btn,
            differs,
            visible,
        });
    }

    fn row(&mut self, title: &str, desc: &str, control: &impl IsA<gtk::Widget>, indent: bool, visible: Pred) -> gtk::Label {
        let r = hbox(12);
        r.add_css_class("rt-setting");
        if indent {
            let mark = label("└", &["rt-indent"]);
            mark.set_valign(gtk::Align::Start);
            mark.set_margin_start(4);
            r.append(&mark);
        }
        let text = vbox(2);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        text.append(&label(title, &[]));
        let d = label(desc, &["rt-setting-desc"]);
        d.set_wrap(true);
        d.set_visible(!desc.is_empty());
        text.append(&d);
        r.append(&text);
        control.set_valign(gtk::Align::Center);
        r.append(control);
        self.view.content.last_child().and_downcast::<gtk::Box>().unwrap().append(&r);
        let mut st = self.view.state.borrow_mut();
        let section = st.sections.len().saturating_sub(1);
        st.rows.push(RowHandle {
            widget: r,
            title: title.to_owned(),
            desc: desc.to_owned(),
            section,
            visible,
        });
        d
    }

    /// One action of Settings → shortcuts: reset, the key (click, then press a key) and the
    /// portal trigger.
    fn key_row(&mut self, action: Action, c: &Config) {
        let controls = hbox(4);
        let reset = button("reset", &format!("Reset the {} key", action.description()));
        reset.add_css_class("rt-seg");
        let key = gtk::Button::with_label("");
        key.add_css_class("rt-keycap");
        key.set_size_request(KEY_COLUMN, -1);
        key.set_tooltip_text(Some("click, then press a key"));
        controls.append(&reset);
        controls.append(&key);
        let global = action.global().map(|cmd| {
            let current = c.shortcuts.bindings.get(cmd.name()).cloned().unwrap_or_default();
            let name = cmd.name();
            let e = self.entry(&current, "unassigned", Rule::Any, move |c, t| {
                c.shortcuts.bindings.insert(name.to_owned(), t.to_owned());
            });
            e.set_width_chars(1);
            e.set_size_request(KEY_COLUMN, -1);
            e.update_property(&[gtk::accessible::Property::Label(&format!("{} global key", action.description()))]);
            e.set_tooltip_text(Some(&format!("rtify --{}", cmd.name())));
            controls.append(&e);
            e
        });
        let blank = hbox(0);
        blank.set_size_request(KEY_COLUMN, -1);
        controls.append(&blank);
        let note = self.row(action.description(), "", &controls, false, always);
        let a = self.view.app.clone();
        key.connect_clicked(move |_| {
            let again = a.recording() == Some(action);
            a.set_recording(if again { None } else { Some(action) });
        });
        let a = self.view.app.clone();
        reset.connect_clicked(move |_| {
            a.update_config(|c| {
                c.shortcuts.keys.remove(action.id());
            });
            a.win().settings.refresh();
        });
        self.view.state.borrow_mut().keys.push(KeyRow {
            action,
            key,
            note,
            reset,
            global,
            blank,
        });
    }

    /// "on" / "off".
    fn on_off(&self, value: bool, apply: fn(&App, &mut Config, bool)) -> gtk::Box {
        let a = self.view.app.clone();
        segmented(&["on", "off"], if value { 0 } else { 1 }, move |i| {
            let mut cfg = a.config.borrow().config.clone();
            apply(&a, &mut cfg, i == 0);
            a.update_config(|c| *c = cfg);
            after_change(&a);
        })
    }

    fn choice(&self, options: &[&str], selected: usize, apply: fn(&App, &mut Config, usize)) -> gtk::Box {
        let a = self.view.app.clone();
        segmented(options, selected, move |i| {
            let mut cfg = a.config.borrow().config.clone();
            apply(&a, &mut cfg, i);
            a.update_config(|c| *c = cfg);
            after_change(&a);
        })
    }

    /// A text field that saves only valid input.
    fn entry(&self, value: &str, placeholder: &str, rule: Rule, apply: impl Fn(&mut Config, &str) + 'static) -> gtk::Entry {
        let e = gtk::Entry::new();
        e.add_css_class("rt-field");
        e.set_text(value);
        e.set_placeholder_text(Some(placeholder));
        e.set_width_chars(20);
        let a = self.view.app.clone();
        e.connect_changed(move |e| {
            let text = e.text().trim().to_owned();
            if rule.check(&text) {
                a.update_config(|c| apply(c, &text));
                after_change(&a);
            }
        });
        e
    }

    /// A validated field row: invalid input turns the frame red and shows the rule instead of
    /// the description.
    #[allow(clippy::too_many_arguments)]
    fn field(
        &mut self,
        title: &str,
        desc: &str,
        value: &str,
        placeholder: &str,
        rule: Rule,
        indent: bool,
        visible: Pred,
        apply: fn(&mut Config, &str),
    ) {
        let e = self.entry(value, placeholder, rule, apply);
        e.update_property(&[gtk::accessible::Property::Label(title)]);
        let d = self.row(title, desc, &e, indent, visible);
        let row = e.parent().and_downcast::<gtk::Box>().unwrap();
        let (desc, explain) = (desc.to_owned(), rule.explain());
        let check = move |e: &gtk::Entry| {
            if rule.check(&e.text()) {
                e.remove_css_class("rt-invalid");
                row.remove_css_class("rt-invalid");
                d.set_text(&desc);
                d.set_visible(!desc.is_empty());
            } else {
                e.add_css_class("rt-invalid");
                row.add_css_class("rt-invalid");
                d.set_text(&explain);
                d.set_visible(true);
            }
        };
        check(&e);
        e.connect_changed(check);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules() {
        assert!(Rule::Hex.check("#9bd4c9") && Rule::Hex.check(" #ABCDEF "));
        assert!(!Rule::Hex.check("9bd4c9") && !Rule::Hex.check("#9bd4c") && !Rule::Hex.check("#9bd4cg") && !Rule::Hex.check(""));
        assert!(Rule::HexOrEmpty.check("") && Rule::HexOrEmpty.check("#000000") && !Rule::HexOrEmpty.check("#000"));
        assert!(Rule::Int(6, 32).check("6") && Rule::Int(6, 32).check("32") && !Rule::Int(6, 32).check("33"));
        assert!(!Rule::Int(6, 32).check("12.5") && !Rule::Int(6, 32).check(""));
        assert!(Rule::Int(-60_000, 60_000).check("-250"));
        assert!(Rule::Float(0.5, 3.0).check("0.5") && Rule::Float(0.5, 3.0).check("3") && !Rule::Float(0.5, 3.0).check("3.1"));
        assert!(!Rule::Float(0.5, 3.0).check("NaN"));
        for ok in ["en", "de", "pt-BR", "zh-Hans", "fil"] {
            assert!(Rule::Language.check(ok), "{ok}");
        }
        for bad in ["", "EN", "english", "pt_BR", "pt-", "en-US-x"] {
            assert!(!Rule::Language.check(bad), "{bad}");
        }
        assert!(Rule::ClientId.check("") && Rule::ClientId.check("0123456789abcdef0123456789ABCDEF"));
        assert!(!Rule::ClientId.check("0123456789abcdef") && !Rule::ClientId.check("0123456789abcdef0123456789abcdeg"));
        assert!(Rule::Int(1024, 65_535).explain().contains("1024"));
    }

    #[test]
    fn filter_matches_label_or_description() {
        assert!(row_matches("", "font", ""));
        assert!(row_matches("FONT", "font size", "points"));
        assert!(row_matches("points", "font size", "points"));
        assert!(!row_matches("tray", "font size", "points"));
    }
}
