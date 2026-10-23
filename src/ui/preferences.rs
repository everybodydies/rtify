//! Preferences. Every control writes the config immediately (debounced to disk) and applies
//! live; nothing needs a restart except the preferred shortcut triggers.

use gtk::{glib, prelude::*};

use super::widgets::*;
use crate::app::App;
use crate::commands::Command;
use crate::config::{Config, LyricAnimation, Notifications, PaletteSource, PollingMode};
use crate::desktop::{lifecycle, shortcuts};

fn row(title: &str, hint: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let r = hbox(12);
    let text = vbox(0);
    text.set_hexpand(true);
    text.append(&label(title, &[]));
    if !hint.is_empty() {
        let h = label(hint, &["rt-dim"]);
        h.set_wrap(true);
        text.append(&h);
    }
    r.append(&text);
    control.set_valign(gtk::Align::Center);
    r.append(control);
    r
}

fn switch(app: &App, value: bool, apply: impl Fn(&App, &mut Config, bool) + 'static) -> gtk::Switch {
    let s = gtk::Switch::new();
    s.set_active(value);
    let a = app.clone();
    s.connect_active_notify(move |s| {
        let on = s.is_active();
        let mut cfg = a.config.borrow().config.clone();
        apply(&a, &mut cfg, on);
        a.update_config(|c| *c = cfg);
        after_change(&a);
    });
    s
}

fn dropdown(app: &App, options: &[&str], selected: usize, apply: impl Fn(&mut Config, usize) + 'static) -> gtk::DropDown {
    let d = gtk::DropDown::from_strings(options);
    d.set_selected(selected as u32);
    let a = app.clone();
    d.connect_selected_notify(move |d| {
        let i = d.selected() as usize;
        a.update_config(|c| apply(c, i));
        after_change(&a);
    });
    d
}

fn entry(app: &App, value: &str, placeholder: &str, apply: impl Fn(&mut Config, &str) + 'static) -> gtk::Entry {
    let e = gtk::Entry::new();
    e.set_text(value);
    e.set_placeholder_text(Some(placeholder));
    e.set_width_chars(18);
    let a = app.clone();
    e.connect_changed(move |e| {
        let text = e.text().to_string();
        a.update_config(|c| apply(c, &text));
        after_change(&a);
    });
    e
}

fn spin(app: &App, value: f64, min: f64, max: f64, step: f64, digits: u32, apply: impl Fn(&mut Config, f64) + 'static) -> gtk::SpinButton {
    let s = gtk::SpinButton::with_range(min, max, step);
    s.set_digits(digits);
    s.set_value(value);
    let a = app.clone();
    s.connect_value_changed(move |s| {
        let v = s.value();
        a.update_config(|c| apply(c, v));
        after_change(&a);
    });
    s
}

/// Push the current config to every live consumer.
fn after_change(app: &App) {
    let appearance = app.config.borrow().config.appearance.clone();
    app.theme.set_appearance(&appearance);
    app.apply_lyrics_options();
}

fn section(title: &str, rows: Vec<gtk::Box>) -> gtk::Overlay {
    let b = vbox(10);
    for r in rows {
        b.append(&r);
    }
    panel(title, &b).root
}

pub fn open(app: &App) {
    if let Some(w) = app.prefs_window() {
        w.present();
        return;
    }
    let c = app.config.borrow().config.clone();
    let window = gtk::Window::builder()
        .title("rtify · settings")
        .default_width(640)
        .default_height(760)
        .build();
    window.add_css_class("rt-window");
    if let Some(parent) = app.gtk_app.active_window() {
        window.set_transient_for(Some(&parent));
    }

    // Appearance.
    let sources: Vec<&str> = PaletteSource::ALL.iter().map(|s| s.label()).collect();
    let source_idx = PaletteSource::ALL
        .iter()
        .position(|s| *s == c.appearance.palette_source)
        .unwrap_or(0);
    let palette_status = label(app.theme.status.borrow().as_deref().unwrap_or(""), &["rt-error"]);
    palette_status.set_wrap(true);
    palette_status.set_visible(app.theme.status.borrow().is_some());
    let appearance = section(
        "appearance",
        vec![
            row(
                "colors",
                "fixed: your accent below · noctalia: the generated template (rtify --print-noctalia-template) · album art: accent from the playing cover",
                &dropdown(app, &sources, source_idx, |c, i| {
                    c.appearance.palette_source = PaletteSource::ALL[i.min(2)]
                }),
            ),
            row(
                "accent",
                "hex color for the fixed palette",
                &entry(app, &c.appearance.fixed_accent, "#b99aff", |c, t| {
                    c.appearance.fixed_accent = t.trim().to_owned()
                }),
            ),
            row(
                "background",
                "optional hex; empty derives it from the accent",
                &entry(app, &c.appearance.fixed_background, "auto", |c, t| {
                    c.appearance.fixed_background = t.trim().to_owned()
                }),
            ),
            row(
                "OLED black",
                "system24 OLED from DESIGN.md: true black, framed panels, dimmed text; only the accent hue follows the palette",
                &switch(app, c.appearance.oled, |_, c, on| c.appearance.oled = on),
            ),
            row(
                "light surfaces",
                "ignored while OLED black is on",
                &switch(app, c.appearance.light, |_, c, on| c.appearance.light = on),
            ),
            row(
                "font",
                "monospace family used for the interface",
                &entry(app, &c.appearance.font_family, "JetBrains Mono", |c, t| {
                    c.appearance.font_family = t.to_owned()
                }),
            ),
            row(
                "font size",
                "points",
                &spin(app, c.appearance.font_size as f64, 7.0, 24.0, 1.0, 0, |c, v| {
                    c.appearance.font_size = v as u32
                }),
            ),
            row(
                "animations",
                "",
                &switch(app, c.appearance.animations_enabled, |_, c, on| {
                    c.appearance.animations_enabled = on
                }),
            ),
            row(
                "reduced motion",
                "no scrolling, blur, scaling or color fades",
                &switch(app, c.appearance.reduced_motion, |_, c, on| c.appearance.reduced_motion = on),
            ),
            {
                let r = hbox(0);
                r.append(&palette_status);
                r
            },
        ],
    );

    // Window & background.
    let notify_opts = ["off", "when hidden", "always"];
    let notify_idx = match c.background.notifications {
        Notifications::Off => 0,
        Notifications::Background => 1,
        Notifications::Always => 2,
    };
    let background = section(
        "window & background",
        vec![
            row(
                "minimize to tray",
                "closing the window keeps rtify running; quit from the tray or Ctrl+Q",
                &switch(app, c.window.minimize_to_tray, |_, c, on| c.window.minimize_to_tray = on),
            ),
            row(
                "start hidden",
                "",
                &switch(app, c.window.start_hidden, |_, c, on| c.window.start_hidden = on),
            ),
            row(
                "launch at login",
                "XDG autostart entry, starts hidden; never starts playback by itself",
                &switch(app, lifecycle::autostart_enabled(), |a, c, on| {
                    c.background.autostart = on;
                    if let Err(e) = lifecycle::set_autostart(on) {
                        a.notice(&format!("autostart: {e}"), true);
                    }
                }),
            ),
            row(
                "tray icon",
                "StatusNotifierItem (Noctalia, Waybar, KDE …)",
                &switch(app, c.background.tray, |a, c, on| {
                    c.background.tray = on;
                    a.set_tray_enabled(on);
                }),
            ),
            row(
                "remember window size",
                "",
                &switch(app, c.window.remember_geometry, |_, c, on| c.window.remember_geometry = on),
            ),
            row(
                "track notifications",
                "",
                &dropdown(app, &notify_opts, notify_idx, |c, i| {
                    c.background.notifications = [Notifications::Off, Notifications::Background, Notifications::Always][i.min(2)]
                }),
            ),
            row(
                "polling",
                "adaptive polls less while hidden or paused",
                &dropdown(
                    app,
                    &["adaptive", "fixed interval"],
                    (c.background.polling_mode == PollingMode::Fixed) as usize,
                    |c, i| c.background.polling_mode = if i == 1 { PollingMode::Fixed } else { PollingMode::Adaptive },
                ),
            ),
            row(
                "fixed interval",
                "milliseconds",
                &spin(app, c.background.polling_interval_ms as f64, 500.0, 60_000.0, 500.0, 0, |c, v| {
                    c.background.polling_interval_ms = v as u64
                }),
            ),
        ],
    );

    // Playback.
    let playback = section(
        "playback",
        vec![
            row(
                "reapply shuffle/repeat on reconnect",
                "waits for an active device, then sets them one at a time; off: Spotify's current values win",
                &switch(app, c.playback.restore_shuffle_repeat, |_, c, on| {
                    c.playback.restore_shuffle_repeat = on
                }),
            ),
            row(
                "resume last context on startup",
                "only when nothing is playing anywhere; off by default so starting rtify is silent",
                &switch(app, c.playback.resume_on_startup, |_, c, on| c.playback.resume_on_startup = on),
            ),
        ],
    );

    // Lyrics.
    let anim_opts = ["off", "line", "smooth (word fill, blur)"];
    let anim_idx = match c.lyrics.animation {
        LyricAnimation::Off => 0,
        LyricAnimation::Line => 1,
        LyricAnimation::Smooth => 2,
    };
    let key = gtk::PasswordEntry::new();
    key.set_show_peek_icon(true);
    key.set_placeholder_text(Some("stored in the Secret Service"));
    let save_key = button("save", "Store the key in the Secret Service");
    let key_box = hbox(6);
    key_box.append(&key);
    key_box.append(&save_key);
    let a = app.clone();
    let k = key.clone();
    save_key.connect_clicked(move |_| {
        a.set_deepl_key(&k.text());
        k.set_text("");
        a.translate_current_lyrics();
    });
    let lyrics = section(
        "lyrics",
        vec![
            row(
                "follow Spotify playback",
                "show and sync lyrics for the playing song from LRCLIB",
                &switch(app, c.lyrics.follow_playback, |a, c, on| {
                    c.lyrics.follow_playback = on;
                    let a = a.clone();
                    glib::idle_add_local_once(move || a.lyrics_follow_changed());
                }),
            ),
            row(
                "romanization",
                "offline: Japanese (incl. kanji), Korean, Chinese, Cyrillic",
                &switch(app, c.lyrics.romanization, |_, c, on| c.lyrics.romanization = on),
            ),
            row(
                "translation",
                "sends the shown lyrics to the provider below",
                &switch(app, c.lyrics.translation_enabled, |a, c, on| {
                    c.lyrics.translation_enabled = on;
                    if on {
                        let a = a.clone();
                        glib::idle_add_local_once(move || a.translate_current_lyrics());
                    }
                }),
            ),
            row(
                "target language",
                "e.g. en, de, ja, pt-BR",
                &entry(app, &c.lyrics.translation_language, "en", |c, t| {
                    c.lyrics.translation_language = t.trim().to_owned()
                }),
            ),
            row(
                "translation only",
                "",
                &switch(app, c.lyrics.translation_only, |_, c, on| c.lyrics.translation_only = on),
            ),
            row(
                "translation provider",
                "Google needs no key (like qtify); DeepL uses your API key",
                &dropdown(
                    app,
                    &["Google", "DeepL"],
                    (c.lyrics.translation_provider == "deepl") as usize,
                    |c, i| c.lyrics.translation_provider = if i == 1 { "deepl" } else { "google" }.into(),
                ),
            ),
            row("DeepL API key", "", &key_box),
            row(
                "animation",
                "",
                &dropdown(app, &anim_opts, anim_idx, |c, i| {
                    c.lyrics.animation = [LyricAnimation::Off, LyricAnimation::Line, LyricAnimation::Smooth][i.min(2)]
                }),
            ),
            row(
                "text size",
                "",
                &spin(app, c.lyrics.font_scale, 0.5, 3.0, 0.1, 1, |c, v| c.lyrics.font_scale = v),
            ),
            row(
                "timing offset",
                "milliseconds; positive shows lines earlier",
                &spin(app, c.lyrics.offset_ms as f64, -5000.0, 5000.0, 50.0, 0, |c, v| {
                    c.lyrics.offset_ms = v as i64
                }),
            ),
        ],
    );

    // Spotify account.
    let logout = button("sign out", "Forget the stored login");
    let a = app.clone();
    logout.connect_clicked(move |_| a.logout());
    let account = section(
        "spotify",
        vec![
            row(
                "client ID",
                "from developer.spotify.com/dashboard",
                &entry(app, &c.spotify.client_id, "client ID", |c, t| {
                    c.spotify.client_id = t.trim().to_owned()
                }),
            ),
            row(
                "redirect port",
                "redirect URI is http://127.0.0.1:<port>/callback",
                &spin(app, c.spotify.redirect_port as f64, 1024.0, 65535.0, 1.0, 0, |c, v| {
                    c.spotify.redirect_port = v as u16
                }),
            ),
            row("session", "", &logout),
        ],
    );

    // Shortcuts.
    let status = label(&app.shortcut_status.borrow(), &["rt-dim"]);
    status.set_wrap(true);
    status.set_selectable(true);
    let mut rows = vec![
        row(
            "use the GlobalShortcuts portal",
            "applies on next start",
            &switch(app, c.shortcuts.portal, |_, c, on| c.shortcuts.portal = on),
        ),
        {
            let r = hbox(0);
            r.append(&status);
            r
        },
    ];
    for cmd in Command::shortcut_actions() {
        let current = c.shortcuts.bindings.get(cmd.name()).cloned().unwrap_or_default();
        let name = cmd.name().to_owned();
        rows.push(row(
            cmd.description(),
            &format!("rtify --{}", cmd.name()),
            &entry(app, &current, "unassigned", move |c, t| {
                c.shortcuts.bindings.insert(name.clone(), t.trim().to_owned());
            }),
        ));
    }
    let copy_lua = button("copy Hyprland binds (Lua)", "Hyprland 0.55+");
    let copy_conf = button("copy Hyprland binds (hyprlang)", "older Hyprland");
    for (b, lua) in [(&copy_lua, true), (&copy_conf, false)] {
        let a = app.clone();
        b.connect_clicked(move |b| {
            let text = shortcuts::hyprland_snippet(&a.config.borrow().config.shortcuts.bindings, lua);
            b.clipboard().set_text(&text);
            a.notice("Hyprland bindings copied to the clipboard", false);
        });
    }
    let copy_row = hbox(6);
    copy_row.append(&copy_lua);
    copy_row.append(&copy_conf);
    rows.push(copy_row);
    let shortcuts_section = section("global shortcuts", rows);

    let content = vbox(12);
    content.add_css_class("rt-root");
    // DESIGN.md: the same 12px gap to the window edge as between panels.
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    for s in [appearance, background, playback, lyrics, account, shortcuts_section] {
        content.append(&s);
    }
    let scroller = gtk::ScrolledWindow::builder()
        .child(&content)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    window.set_child(Some(&scroller));
    let a = app.clone();
    window.connect_close_request(move |_| {
        a.set_prefs_window(None);
        glib::Propagation::Proceed
    });
    app.set_prefs_window(Some(window.clone()));
    window.present();
}
