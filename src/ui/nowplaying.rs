//! Right-hand column: the "now playing" panel (large cover, title, artist, album, like) and the
//! "queue" panel below it (what plays next and from where).

use std::cell::Cell;
use std::rc::Rc;

use gtk::{gdk, prelude::*};

use super::widgets::*;
use crate::app::{App, PlayerOp};
use crate::services::playback::Playback;
use crate::spotify::model::{Track, pick_image};

#[derive(Clone)]
pub struct NowPlayingPanel {
    pub root: gtk::Box,
    now: Panel,
    cover: gtk::Picture,
    title: gtk::Label,
    artist: gtk::Label,
    album: gtk::Label,
    device: gtk::Label,
    like: gtk::ToggleButton,
    pub queue_root: gtk::Overlay,
    queue_panel: Panel,
    queue_from: gtk::Label,
    queue: gtk::ListBox,
    updating: Rc<Cell<bool>>,
    app: App,
}

impl NowPlayingPanel {
    pub fn new(app: &App) -> Self {
        let cover = gtk::Picture::new();
        cover.set_content_fit(gtk::ContentFit::Cover);
        cover.add_css_class("rt-cover");
        cover.set_can_shrink(true);
        cover.set_size_request(160, 160);
        let frame = gtk::AspectFrame::new(0.5, 0.0, 1.0, false);
        frame.set_child(Some(&cover));

        // One line: a wrapping label has no baseline until it knows its width, which makes the
        // row's height depend on the width it is measured for.
        let title = ellipsized("nothing playing", &["rt-title"]);
        title.set_hexpand(true);
        let like = toggle("♡", "Save to your library");
        like.add_css_class("rt-square");
        like.set_valign(gtk::Align::Start);
        let title_row = hbox(8);
        title_row.append(&title);
        title_row.append(&like);
        let artist = ellipsized("", &["rt-dim"]);
        let album = ellipsized("", &["rt-faint", "rt-small"]);
        let device = ellipsized("", &["rt-faint", "rt-small"]);

        let body = vbox(6);
        body.append(&frame);
        let text = vbox(4);
        text.set_margin_top(8);
        text.append(&title_row);
        text.append(&artist);
        text.append(&album);
        text.append(&device);
        body.append(&text);
        let now = panel("now playing", &body);

        let queue_from = ellipsized("", &["rt-faint", "rt-small"]);
        let queue = gtk::ListBox::new();
        queue.add_css_class("rt-list");
        queue.set_selection_mode(gtk::SelectionMode::None);
        tab_leaves(&queue);
        let qbody = vbox(6);
        qbody.append(&queue_from);
        qbody.append(&queue);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&qbody)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let queue_panel = panel("queue", &scroll);
        queue_panel.root.set_vexpand(true);

        let root = vbox(12);
        root.append(&now.root);
        root.append(&queue_panel.root);
        root.set_hexpand(false);
        let this = Self {
            root,
            now,
            cover,
            title,
            artist,
            album,
            device,
            like: like.clone(),
            queue_root: queue_panel.root.clone(),
            queue_panel,
            queue_from,
            queue,
            updating: Rc::new(Cell::new(false)),
            app: app.clone(),
        };
        let (a, updating) = (app.clone(), this.updating.clone());
        like.connect_toggled(move |b| {
            if !updating.get() {
                a.player(PlayerOp::Like(b.is_active()));
            }
        });
        // Songs dragged from a list onto the queue panel go into the queue. (Spotify's queue
        // cannot be reordered from outside, so it only takes additions.)
        let drop = gtk::DropTarget::new(gtk::glib::Type::STRING, gdk::DragAction::COPY);
        let host = this.queue_root.clone();
        drop.connect_enter(move |_, _, _| {
            host.add_css_class("rt-drop-into");
            gdk::DragAction::COPY
        });
        let host = this.queue_root.clone();
        drop.connect_leave(move |_| host.remove_css_class("rt-drop-into"));
        let (a, host) = (app.clone(), this.queue_root.clone());
        drop.connect_drop(move |_, value, _, _| {
            host.remove_css_class("rt-drop-into");
            let Ok(text) = value.get::<String>() else { return false };
            let uris: Vec<String> = text
                .lines()
                .filter(|l| l.starts_with("spotify:track:") || l.starts_with("spotify:episode:"))
                .map(String::from)
                .collect();
            if uris.is_empty() {
                return false;
            }
            a.queue_uris(uris);
            true
        });
        this.queue_root.add_controller(drop);
        this
    }

    pub fn update(&self, pb: &Playback, saved: Option<bool>) {
        self.updating.set(true);
        match &pb.track {
            Some(t) => {
                self.title.set_text(&t.name);
                self.title.set_tooltip_text(Some(&t.name));
                self.artist.set_text(&t.artist_names());
                self.album.set_text(t.album_name());
            }
            None => {
                self.title.set_text("nothing playing");
                self.title.set_tooltip_text(None);
                self.artist.set_text("");
                self.album.set_text("");
            }
        }
        self.like.set_visible(pb.track.as_ref().is_some_and(|t| !t.is_local));
        self.like.set_active(saved == Some(true));
        self.like.set_label(if saved == Some(true) { "♥" } else { "♡" });
        self.device.set_text(&match (&pb.device, pb.stale) {
            (_, true) => "last session".to_owned(),
            (Some(d), _) => format!("on {} · {}", d.name, d.kind.to_lowercase()),
            (None, _) => "no active device".to_owned(),
        });
        self.now.set_meta(match (&pb.track, pb.playing) {
            (None, _) => "",
            (Some(_), true) => "playing",
            (Some(_), false) => "paused",
        });
        self.updating.set(false);
    }

    pub fn set_cover(&self, texture: Option<&gdk::Texture>) {
        self.cover.set_paintable(texture);
    }

    /// `from`: the name of the playlist/album the queue continues, when known.
    pub fn set_queue(&self, tracks: &[Track], from: Option<&str>) {
        clear_list(&self.queue);
        self.queue_panel.set_meta(&match tracks.len() {
            0 => String::new(),
            n => format!("{n} queued"),
        });
        self.queue_from
            .set_text(&from.map(|f| format!("next from {f}")).unwrap_or_default());
        self.queue_from.set_visible(from.is_some() && !tracks.is_empty());
        if tracks.is_empty() {
            self.queue.append(&label("queue is empty", &["rt-faint", "rt-small"]));
            return;
        }
        for (i, t) in tracks.iter().take(20).enumerate() {
            let row = hbox(10);
            let num = label(&(i + 1).to_string(), &["rt-faint", "rt-small", "rt-mono-num"]);
            num.set_width_chars(2);
            num.set_xalign(1.0);
            row.append(&num);
            let pic = cover_picture(32);
            if let Some(url) = pick_image(t.images(), 60).map(|i| i.url.clone()) {
                let p = pic.clone();
                self.app.load_image(&url, move |tex| p.set_paintable(tex.as_ref()));
            }
            row.append(&pic);
            let meta = vbox(0);
            meta.set_hexpand(true);
            meta.append(&ellipsized(&t.name, &["rt-row-title"]));
            meta.append(&ellipsized(&t.artist_names(), &["rt-faint", "rt-small"]));
            row.append(&meta);
            row.append(&label(&fmt_time(t.duration_ms), &["rt-faint", "rt-small", "rt-mono-num"]));
            let item = gtk::ListBoxRow::builder().child(&row).build();
            item.update_property(&[gtk::accessible::Property::Label(&format!("{}, {}", t.name, t.artist_names()))]);
            self.queue.append(&item);
        }
    }
}
