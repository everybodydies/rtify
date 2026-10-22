//! The now-playing panel: cover, metadata, transport, progress and volume.

use std::cell::Cell;
use std::rc::Rc;

use gtk::{gdk, glib, prelude::*};

use super::widgets::*;
use crate::app::{App, PlayerOp};
use crate::commands::Command;
use crate::services::playback::{Playback, Repeat};

#[derive(Clone)]
pub struct PlayerBar {
    pub root: gtk::Overlay,
    cover: gtk::Picture,
    title: gtk::Label,
    artist: gtk::Label,
    detail: gtk::Label,
    play: gtk::Button,
    shuffle: gtk::ToggleButton,
    repeat: gtk::Button,
    like: gtk::ToggleButton,
    progress: gtk::Scale,
    elapsed: gtk::Label,
    duration: gtk::Label,
    volume: gtk::Scale,
    /// Set while we update widgets from state so their signals don't echo commands back.
    updating: Rc<Cell<bool>>,
    /// The user is dragging the progress bar; don't move it under their pointer.
    seeking: Rc<Cell<bool>>,
}

impl PlayerBar {
    pub fn new(app: &App) -> Self {
        let cover = gtk::Picture::new();
        cover.set_size_request(72, 72);
        cover.set_content_fit(gtk::ContentFit::Cover);
        cover.add_css_class("rt-cover");
        cover.set_can_shrink(true);

        let title = ellipsized("nothing playing", &["rt-title"]);
        let artist = ellipsized("", &["rt-subtitle"]);
        let detail = ellipsized("", &["rt-faint"]);
        let meta = vbox(2);
        meta.set_hexpand(true);
        meta.set_valign(gtk::Align::Center);
        meta.append(&title);
        meta.append(&artist);
        meta.append(&detail);

        let shuffle = toggle("shuf", "Shuffle (Ctrl+S)");
        let prev = button("◀◀", "Previous (Ctrl+←)");
        let play = button("PLAY", "Play/Pause (Ctrl+Space)");
        play.add_css_class("rt-accent");
        play.set_size_request(80, -1);
        let next = button("▶▶", "Next (Ctrl+→)");
        let repeat = button("rep:off", "Repeat (Ctrl+R)");
        let like = toggle("♡", "Save to your library");

        let controls = hbox(6);
        controls.set_halign(gtk::Align::Center);
        for w in [
            shuffle.upcast_ref::<gtk::Widget>(),
            prev.upcast_ref(),
            play.upcast_ref(),
            next.upcast_ref(),
            repeat.upcast_ref(),
            like.upcast_ref(),
        ] {
            controls.append(w);
        }

        let elapsed = label("0:00", &["rt-dim", "rt-mono-num"]);
        let duration = label("0:00", &["rt-dim", "rt-mono-num"]);
        let progress = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 1000.0);
        progress.set_draw_value(false);
        progress.set_hexpand(true);
        let volume = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 5.0);
        volume.set_draw_value(false);
        volume.set_size_request(100, -1);
        volume.set_tooltip_text(Some("Volume (Ctrl+↑/↓)"));
        let bar = hbox(8);
        bar.append(&elapsed);
        bar.append(&progress);
        bar.append(&duration);
        bar.append(&label("vol", &["rt-faint"]));
        bar.append(&volume);

        let right = vbox(4);
        right.set_hexpand(true);
        right.append(&controls);
        right.append(&bar);

        let body = hbox(12);
        body.append(&cover);
        body.append(&meta);
        body.append(&right);
        meta.set_size_request(220, -1);
        let root = panel("now playing", &body).root;

        let this = Self {
            root,
            cover,
            title,
            artist,
            detail,
            play: play.clone(),
            shuffle: shuffle.clone(),
            repeat: repeat.clone(),
            like: like.clone(),
            progress: progress.clone(),
            elapsed,
            duration,
            volume: volume.clone(),
            updating: Rc::new(Cell::new(false)),
            seeking: Rc::new(Cell::new(false)),
        };

        let a = app.clone();
        play.connect_clicked(move |_| a.command(Command::PlayPause));
        let a = app.clone();
        prev.connect_clicked(move |_| a.command(Command::Previous));
        let a = app.clone();
        next.connect_clicked(move |_| a.command(Command::Next));
        let a = app.clone();
        let upd = this.updating.clone();
        shuffle.connect_toggled(move |_| {
            if !upd.get() {
                a.command(Command::ToggleShuffle);
            }
        });
        let a = app.clone();
        repeat.connect_clicked(move |_| a.command(Command::CycleRepeat));
        let a = app.clone();
        let upd = this.updating.clone();
        like.connect_toggled(move |b| {
            if !upd.get() {
                a.player(PlayerOp::Like(b.is_active()));
            }
        });

        // change-value fires only for user input. Seek once the user stops moving the slider;
        // until then only the elapsed label follows and polling doesn't move the knob.
        let a = app.clone();
        let seeking = this.seeking.clone();
        let el = this.elapsed.clone();
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

        let a = app.clone();
        let upd = this.updating.clone();
        let pending: Rc<Cell<Option<glib::SourceId>>> = Rc::default();
        volume.connect_value_changed(move |s| {
            if upd.get() {
                return;
            }
            // Debounce: the API is slow, scrolling produces many values.
            if let Some(id) = pending.take() {
                id.remove();
            }
            let a = a.clone();
            let v = s.value() as u32;
            let pending2 = pending.clone();
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
                let device = pb.device.as_ref().map(|d| format!(" · {}", d.name)).unwrap_or_default();
                let stale = if pb.stale { " · last session" } else { "" };
                self.detail.set_text(&format!("{}{device}{stale}", t.album_name()));
            }
            None => {
                self.title.set_text("nothing playing");
                self.artist.set_text("");
                self.detail
                    .set_text(pb.device.as_ref().map(|d| d.name.as_str()).unwrap_or("no active device"));
            }
        }
        self.play.set_label(if pb.playing { "PAUSE" } else { "PLAY" });
        self.shuffle.set_active(pb.shuffle);
        self.repeat.set_label(match pb.repeat {
            Repeat::Off => "rep:off",
            Repeat::Context => "rep:all",
            Repeat::Track => "rep:one",
        });
        if pb.repeat == Repeat::Off {
            self.repeat.remove_css_class("rt-accent");
        } else {
            self.repeat.add_css_class("rt-accent");
        }
        if let Some(s) = saved {
            self.like.set_active(s);
            self.like.set_label(if s { "♥" } else { "♡" });
        }
        self.like.set_sensitive(pb.track.is_some() && !pb.stale);
        if let Some(v) = pb.device.as_ref().and_then(|d| d.volume_percent) {
            self.volume.set_value(v as f64);
        }
        self.volume
            .set_sensitive(pb.device.as_ref().is_some_and(|d| d.supports_volume != Some(false)));
        let dur = pb.duration_ms().max(1);
        self.progress.set_range(0.0, dur as f64);
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

    pub fn set_compact(&self, compact: bool) {
        self.cover
            .set_size_request(if compact { 48 } else { 72 }, if compact { 48 } else { 72 });
        self.detail.set_visible(!compact);
    }
}
