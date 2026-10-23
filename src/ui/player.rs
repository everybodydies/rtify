//! Full-width player bar, laid out like the official client: track on the left, transport and
//! progress centered, lyrics / panel / devices / volume on the right.

use std::cell::Cell;
use std::rc::Rc;

use gtk::{gdk, glib, prelude::*};

use super::widgets::*;
use crate::app::{App, PlayerOp};
use crate::commands::Command;
use crate::services::playback::{Playback, Repeat};
use crate::spotify::model::Device;

#[derive(Clone)]
pub struct PlayerBar {
    pub root: gtk::CenterBox,
    cover: gtk::Picture,
    title: gtk::Label,
    artist: gtk::Label,
    play: gtk::Button,
    shuffle: gtk::ToggleButton,
    repeat: gtk::Button,
    like: gtk::ToggleButton,
    progress: gtk::Scale,
    elapsed: gtk::Label,
    duration: gtk::Label,
    volume: gtk::Scale,
    pub lyrics: gtk::ToggleButton,
    pub panel_toggle: gtk::ToggleButton,
    devices: gtk::ListBox,
    devices_button: gtk::MenuButton,
    device_label: gtk::Label,
    /// Set while widgets are updated from state so their signals don't echo commands back.
    updating: Rc<Cell<bool>>,
    /// The user is dragging the progress bar; don't move it under their pointer.
    seeking: Rc<Cell<bool>>,
    /// Last volume change by the user; polls don't overwrite it for a moment.
    volume_touched: Rc<Cell<Option<std::time::Instant>>>,
}

fn icon(text: &str, tooltip: &str) -> gtk::Button {
    let b = button(text, tooltip);
    b.add_css_class("rt-icon");
    b
}

fn icon_toggle(text: &str, tooltip: &str) -> gtk::ToggleButton {
    let b = toggle(text, tooltip);
    b.add_css_class("rt-icon");
    b
}

impl PlayerBar {
    pub fn new(app: &App) -> Self {
        // Left: cover, title/artist, like.
        let cover = cover_picture(56);
        let title = ellipsized("nothing playing", &["rt-row-title"]);
        let artist = ellipsized("", &["rt-dim", "rt-small"]);
        let meta = vbox(2);
        meta.set_valign(gtk::Align::Center);
        meta.append(&title);
        meta.append(&artist);
        let like = icon_toggle("♡", "Save to your library");
        like.set_valign(gtk::Align::Center);
        let left = hbox(12);
        left.append(&cover);
        left.append(&meta);
        left.append(&like);
        left.set_size_request(320, -1);

        // Center: transport + progress.
        let shuffle = icon_toggle("⇄", "Shuffle (Ctrl+S)");
        let prev = icon("◀◀", "Previous (Ctrl+←)");
        let play = button("▶", "Play/Pause (Space)");
        play.add_css_class("rt-play");
        let next = icon("▶▶", "Next (Ctrl+→)");
        let repeat = icon("↻", "Repeat (Ctrl+R)");
        let controls = hbox(10);
        controls.set_halign(gtk::Align::Center);
        for w in [
            shuffle.upcast_ref::<gtk::Widget>(),
            prev.upcast_ref(),
            play.upcast_ref(),
            next.upcast_ref(),
            repeat.upcast_ref(),
        ] {
            controls.append(w);
        }
        let elapsed = label("0:00", &["rt-faint", "rt-mono-num", "rt-small"]);
        elapsed.set_width_chars(5);
        elapsed.set_xalign(1.0);
        let duration = label("0:00", &["rt-faint", "rt-mono-num", "rt-small"]);
        duration.set_width_chars(5);
        let progress = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 1000.0);
        progress.set_draw_value(false);
        progress.set_hexpand(true);
        let bar = hbox(8);
        bar.append(&elapsed);
        bar.append(&progress);
        bar.append(&duration);
        let center = vbox(2);
        // Fixed-width transport like the official client, however wide the window gets.
        center.set_size_request(680, -1);
        center.set_hexpand(false);
        center.set_valign(gtk::Align::Center);
        center.append(&controls);
        center.append(&bar);

        // Right: lyrics, now-playing panel, devices, volume.
        let lyrics = icon_toggle("lyrics", "Lyrics (Ctrl+Y)");
        let panel_toggle = icon_toggle("▤", "Now playing panel");
        let devices = gtk::ListBox::new();
        devices.add_css_class("rt-list");
        let dev_box = vbox(6);
        dev_box.set_size_request(280, -1);
        dev_box.append(&label("connect to a device", &["rt-section"]));
        dev_box.append(&devices);
        let popover = gtk::Popover::new();
        popover.set_child(Some(&dev_box));
        // A child (not `label`) keeps GTK from adding a dropdown arrow.
        let device_label = gtk::Label::new(Some("⌁"));
        device_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        device_label.set_max_width_chars(14);
        let devices_button = gtk::MenuButton::builder()
            .child(&device_label)
            .popover(&popover)
            .tooltip_text("Spotify Connect devices")
            .build();
        devices_button.add_css_class("rt-icon");
        let volume = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 5.0);
        volume.set_draw_value(false);
        volume.set_size_request(110, -1);
        volume.set_tooltip_text(Some("Volume (Ctrl+↑/↓, scroll)"));
        // Wheel over the volume bar: exactly 5 % per notch (GtkRange's own scroll step is far
        // larger on many mice and touchpads).
        let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::DISCRETE);
        wheel.set_propagation_phase(gtk::PropagationPhase::Capture);
        let v = volume.clone();
        wheel.connect_scroll(move |_, _, dy| {
            let step = if dy < 0.0 {
                5.0
            } else if dy > 0.0 {
                -5.0
            } else {
                0.0
            };
            v.set_value((v.value() + step).clamp(0.0, 100.0));
            glib::Propagation::Stop
        });
        volume.add_controller(wheel);
        let right = hbox(6);
        right.set_halign(gtk::Align::End);
        right.set_valign(gtk::Align::Center);
        right.append(&lyrics);
        right.append(&panel_toggle);
        right.append(&devices_button);
        right.append(&volume);
        right.set_size_request(320, -1);

        let root = gtk::CenterBox::new();
        root.add_css_class("rt-playerbar");
        root.set_start_widget(Some(&left));
        root.set_center_widget(Some(&center));
        root.set_end_widget(Some(&right));

        let this = Self {
            root,
            cover,
            title,
            artist,
            play: play.clone(),
            shuffle: shuffle.clone(),
            repeat: repeat.clone(),
            like: like.clone(),
            progress: progress.clone(),
            elapsed,
            duration,
            volume: volume.clone(),
            lyrics: lyrics.clone(),
            panel_toggle: panel_toggle.clone(),
            devices: devices.clone(),
            devices_button,
            device_label,
            updating: Rc::new(Cell::new(false)),
            seeking: Rc::new(Cell::new(false)),
            volume_touched: Rc::default(),
        };

        let a = app.clone();
        play.connect_clicked(move |_| a.command(Command::PlayPause));
        let a = app.clone();
        prev.connect_clicked(move |_| a.command(Command::Previous));
        let a = app.clone();
        next.connect_clicked(move |_| a.command(Command::Next));
        let (a, upd) = (app.clone(), this.updating.clone());
        shuffle.connect_toggled(move |_| {
            if !upd.get() {
                a.command(Command::ToggleShuffle);
            }
        });
        let a = app.clone();
        repeat.connect_clicked(move |_| a.command(Command::CycleRepeat));
        let (a, upd) = (app.clone(), this.updating.clone());
        like.connect_toggled(move |b| {
            if !upd.get() {
                a.player(PlayerOp::Like(b.is_active()));
            }
        });
        let (a, upd) = (app.clone(), this.updating.clone());
        lyrics.connect_toggled(move |_| {
            if !upd.get() {
                a.command(Command::ToggleLyrics);
            }
        });
        let (a, upd) = (app.clone(), this.updating.clone());
        panel_toggle.connect_toggled(move |b| {
            if !upd.get() {
                a.set_now_playing_panel(b.is_active());
            }
        });
        let a = app.clone();
        popover.connect_show(move |_| a.refresh_devices());
        let (a, pop) = (app.clone(), popover.clone());
        devices.connect_row_activated(move |_, row| {
            a.transfer_to_row(row.index() as usize);
            pop.popdown();
        });

        // change-value fires only for user input. Seek once the user stops moving the slider;
        // until then only the elapsed label follows and polling doesn't move the knob.
        let (a, seeking, el) = (app.clone(), this.seeking.clone(), this.elapsed.clone());
        let pending_seek: Rc<Cell<Option<glib::SourceId>>> = Rc::default();
        progress.connect_change_value(move |_, _, value| {
            seeking.set(true);
            el.set_text(&fmt_time(value.max(0.0) as u64));
            if let Some(id) = pending_seek.take() {
                id.remove();
            }
            let (a, seeking, pending) = (a.clone(), seeking.clone(), pending_seek.clone());
            pending_seek.set(Some(glib::timeout_add_local_once(
                std::time::Duration::from_millis(300),
                move || {
                    pending.take();
                    seeking.set(false);
                    a.player(PlayerOp::Seek(value.max(0.0) as u64));
                },
            )));
            glib::Propagation::Proceed
        });

        // Hover preview: the time under the pointer, shown above the bar.
        let preview_label = label("", &["rt-mono-num", "rt-small"]);
        let preview = gtk::Popover::builder()
            .child(&preview_label)
            .has_arrow(false)
            .autohide(false)
            .can_focus(false)
            .position(gtk::PositionType::Top)
            .build();
        preview.add_css_class("rt-tip");
        preview.set_parent(&progress);
        let motion = gtk::EventControllerMotion::new();
        let (p, pv, pl) = (progress.clone(), preview.clone(), preview_label.clone());
        motion.connect_motion(move |_, x, _| {
            let (lo, hi) = (p.adjustment().lower(), p.adjustment().upper());
            // The trough is inset by the slider margins; match GTK's mapping closely enough.
            let w = (p.width() as f64 - 10.0).max(1.0);
            let frac = ((x - 5.0) / w).clamp(0.0, 1.0);
            if hi - lo <= 1.0 {
                return;
            }
            pl.set_text(&fmt_time((lo + frac * (hi - lo)) as u64));
            pv.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, 0, 1, 1)));
            if !pv.is_visible() {
                pv.popup();
            }
        });
        let pv = preview.clone();
        motion.connect_leave(move |_| pv.popdown());
        progress.add_controller(motion);

        let (a, upd, touched) = (app.clone(), this.updating.clone(), this.volume_touched.clone());
        let pending: Rc<Cell<Option<glib::SourceId>>> = Rc::default();
        volume.connect_value_changed(move |s| {
            if upd.get() {
                return;
            }
            touched.set(Some(std::time::Instant::now()));
            // Debounce: the API is slow, scrolling produces many values.
            if let Some(id) = pending.take() {
                id.remove();
            }
            let (a, v, pending2) = (a.clone(), s.value() as u32, pending.clone());
            pending.set(Some(glib::timeout_add_local_once(
                std::time::Duration::from_millis(250),
                move || {
                    pending2.take();
                    a.player(PlayerOp::SetVolume(v));
                },
            )));
        });
        this
    }

    pub fn update(&self, pb: &Playback, saved: Option<bool>) {
        self.updating.set(true);
        match &pb.track {
            Some(t) => {
                self.title.set_text(&t.name);
                self.artist.set_text(&t.artist_names());
            }
            None => {
                self.title.set_text("nothing playing");
                self.artist
                    .set_text(pb.device.as_ref().map(|d| d.name.as_str()).unwrap_or("no active device"));
            }
        }
        self.play.set_label(if pb.playing { "❚❚" } else { "▶" });
        self.shuffle.set_active(pb.shuffle);
        self.repeat.set_label(if pb.repeat == Repeat::Track { "↻1" } else { "↻" });
        if pb.repeat == Repeat::Off {
            self.repeat.remove_css_class("rt-on");
        } else {
            self.repeat.add_css_class("rt-on");
        }
        if let Some(s) = saved {
            self.like.set_active(s);
            self.like.set_label(if s { "♥" } else { "♡" });
        }
        self.like.set_sensitive(pb.track.is_some() && !pb.stale);
        let recently_touched = self
            .volume_touched
            .get()
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(2));
        if let Some(v) = pb.device.as_ref().and_then(|d| d.volume_percent)
            && !recently_touched
        {
            self.volume.set_value(v as f64);
        }
        self.volume
            .set_sensitive(pb.device.as_ref().is_some_and(|d| d.supports_volume != Some(false)));
        self.device_label.set_text(&match &pb.device {
            Some(d) => format!("⌁ {}", d.name),
            None => "⌁".to_owned(),
        });
        self.devices_button.set_tooltip_text(Some(&match &pb.device {
            Some(d) => format!("Playing on {} · Spotify Connect devices", d.name),
            None => "Spotify Connect devices".to_owned(),
        }));
        self.progress.set_range(0.0, pb.duration_ms().max(1) as f64);
        self.duration.set_text(&fmt_time(pb.duration_ms()));
        self.updating.set(false);
        self.tick(pb);
    }

    /// Called a few times per second while visible.
    pub fn tick(&self, pb: &Playback) {
        if self.seeking.get() {
            return;
        }
        self.updating.set(true);
        let pos = pb.position_now();
        self.progress.set_value(pos as f64);
        self.elapsed.set_text(&fmt_time(pos));
        self.updating.set(false);
    }

    pub fn set_cover(&self, texture: Option<&gdk::Texture>) {
        self.cover.set_paintable(texture);
    }

    /// Reflect page/panel state in the toggles without triggering them.
    pub fn set_toggles(&self, lyrics: bool, panel: bool) {
        self.updating.set(true);
        self.lyrics.set_active(lyrics);
        self.panel_toggle.set_active(panel);
        self.updating.set(false);
    }

    pub fn set_devices(&self, devices: &[Device]) {
        clear_list(&self.devices);
        if devices.is_empty() {
            self.devices
                .append(&label("no devices — open Spotify on a phone, speaker or computer", &["rt-dim"]));
            return;
        }
        for d in devices {
            let row = hbox(8);
            row.append(&label(
                if d.is_active { "◉" } else { "○" },
                &[if d.is_active { "rt-accent" } else { "rt-dim" }],
            ));
            let meta = vbox(0);
            meta.set_hexpand(true);
            meta.append(&ellipsized(&d.name, &[if d.is_active { "rt-accent" } else { "rt-row-title" }]));
            let mut sub = d.kind.to_lowercase();
            if let Some(v) = d.volume_percent {
                sub.push_str(&format!(" · vol {v}%"));
            }
            if d.is_restricted {
                sub.push_str(" · restricted");
            }
            meta.append(&label(&sub, &["rt-dim", "rt-small"]));
            row.append(&meta);
            self.devices.append(&row);
        }
    }
}
