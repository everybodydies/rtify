//! Settings → built-in player (rtify as a Spotify Connect device), right after playback.

use gtk::prelude::*;

use super::widgets::*;
use crate::app::App;
use crate::config::{self, Config};
use crate::player;

// Own small controls (not the ones in preferences.rs) so the settings view can be rebuilt
// without touching these rows.

fn row(title: &str, hint: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let r = hbox(12);
    let text = vbox(0);
    text.set_hexpand(true);
    text.append(&label(title, &[]));
    let h = label(hint, &["rt-dim"]);
    h.set_wrap(true);
    text.append(&h);
    r.append(&text);
    control.set_valign(gtk::Align::Center);
    r.append(control);
    r
}

/// Writes the config on every change; `then` runs afterwards (restart the device …).
fn switch(app: &App, value: bool, apply: fn(&mut Config, bool), then: fn(&App, bool)) -> gtk::Switch {
    let s = gtk::Switch::new();
    s.set_active(value);
    let a = app.clone();
    s.connect_active_notify(move |s| {
        let on = s.is_active();
        a.update_config(|c| apply(c, on));
        then(&a, on);
    });
    s
}

/// A setting that only matters while its parent is on: indented under it with "└".
fn sub_row(title: &str, hint: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let r = row(&format!("└ {title}"), hint, control);
    r.set_margin_start(12);
    r
}

pub fn section(app: &App, c: &Config) -> gtk::Overlay {
    let b = vbox(10);
    for r in rows(app, c) {
        b.append(&r);
    }
    panel("built-in player", &b).root
}

fn rows(app: &App, c: &Config) -> Vec<gtk::Box> {
    if !player::AVAILABLE {
        return vec![row(
            "not available",
            "not in this build (cargo feature builtin-player); rtify controls other Spotify devices only",
            &gtk::Box::new(gtk::Orientation::Horizontal, 0),
        )];
    }
    let status = label(&app.builtin_status_text(), &["rt-dim"]);
    status.set_wrap(true);
    status.set_xalign(1.0);
    *app.builtin_label.borrow_mut() = Some(status.downgrade());
    let sign_in = button("sign in", "Sign in once in the browser so this computer can play");
    let a = app.clone();
    sign_in.connect_clicked(move |_| a.builtin_sign_in());
    let status_box = hbox(8);
    status_box.append(&status);
    status_box.append(&sign_in);

    let bitrates: Vec<String> = config::Player::BITRATES.iter().map(|b| format!("{b} kbit/s")).collect();
    let bitrate_opts: Vec<&str> = bitrates.iter().map(String::as_str).collect();
    let bitrate_idx = config::Player::BITRATES.iter().position(|b| *b == c.player.bitrate).unwrap_or(2);
    let name = gtk::Entry::new();
    name.set_text(&c.player.device_name);
    name.set_placeholder_text(Some(&player::default_name()));
    name.set_width_chars(18);
    let a = app.clone();
    name.connect_changed(move |e| {
        let text = e.text().trim().to_owned();
        a.update_config(|c| c.player.device_name = text);
        a.builtin_settings_changed();
    });
    let quality = gtk::DropDown::from_strings(&bitrate_opts);
    quality.set_selected(bitrate_idx as u32);
    let a = app.clone();
    quality.connect_selected_notify(move |d| {
        let kbps = config::Player::BITRATES[(d.selected() as usize).min(2)];
        a.update_config(|c| c.player.bitrate = kbps);
        a.builtin_settings_changed();
    });
    let normalise = switch(
        app,
        c.player.normalisation,
        |c, on| c.player.normalisation = on,
        |a, _| a.builtin_settings_changed(),
    );
    let subs = vec![
        sub_row("status", "the login is kept in the Secret Service", &status_box),
        sub_row("device name", "how this computer shows up in Spotify's device lists", &name),
        sub_row("quality", "320 kbit/s is Spotify's \"very high\"", &quality),
        sub_row(
            "volume normalisation",
            "same loudness for every song, like the official apps",
            &normalise,
        ),
    ];
    for r in &subs {
        r.set_visible(c.player.enabled);
    }
    let enabled = gtk::Switch::new();
    enabled.set_active(c.player.enabled);
    let a = app.clone();
    let dependents = subs.clone();
    // set_builtin_enabled writes the config itself.
    enabled.connect_active_notify(move |s| {
        let on = s.is_active();
        a.set_builtin_enabled(on);
        for r in &dependents {
            r.set_visible(on);
        }
    });
    let mut rows = vec![row(
        "play on this computer",
        "rtify becomes a Spotify Connect device (librespot) with sound through PipeWire/PulseAudio; needs Premium",
        &enabled,
    )];
    rows.extend(subs);
    rows
}
