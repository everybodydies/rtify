//! Right-hand "now playing" panel: large cover, track details, device and the queue.

use gtk::{gdk, prelude::*};

use super::widgets::*;
use crate::app::App;
use crate::services::playback::Playback;
use crate::spotify::model::{Track, pick_image};

#[derive(Clone)]
pub struct NowPlayingPanel {
    pub root: gtk::Overlay,
    cover: gtk::Picture,
    title: gtk::Label,
    artist: gtk::Label,
    album: gtk::Label,
    device: gtk::Label,
    queue: gtk::ListBox,
    app: App,
}

impl NowPlayingPanel {
    pub fn new(app: &App) -> Self {
        let cover = gtk::Picture::new();
        cover.set_content_fit(gtk::ContentFit::Cover);
        cover.add_css_class("rt-cover");
        cover.set_can_shrink(true);
        cover.set_size_request(300, 300);
        let frame = gtk::AspectFrame::new(0.5, 0.0, 1.0, false);
        frame.set_child(Some(&cover));

        let title = label("nothing playing", &["rt-title"]);
        title.set_wrap(true);
        title.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        let artist = ellipsized("", &["rt-dim"]);
        let album = ellipsized("", &["rt-faint", "rt-small"]);
        let device = ellipsized("", &["rt-faint", "rt-small"]);

        let queue = gtk::ListBox::new();
        queue.add_css_class("rt-list");
        queue.set_selection_mode(gtk::SelectionMode::None);

        let body = vbox(6);
        body.append(&frame);
        body.append(&title);
        body.append(&artist);
        body.append(&album);
        body.append(&device);
        body.append(&label("[ NEXT UP ]", &["rt-section"]));
        body.append(&queue);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&body)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let root = panel("now playing", &scroll).root;
        root.set_size_request(340, -1);
        root.set_hexpand(false);
        Self {
            root,
            cover,
            title,
            artist,
            album,
            device,
            queue,
            app: app.clone(),
        }
    }

    pub fn update(&self, pb: &Playback) {
        match &pb.track {
            Some(t) => {
                self.title.set_text(&t.name);
                self.artist.set_text(&t.artist_names());
                self.album.set_text(t.album_name());
            }
            None => {
                self.title.set_text("nothing playing");
                self.artist.set_text("");
                self.album.set_text("");
            }
        }
        self.device.set_text(&match (&pb.device, pb.stale) {
            (_, true) => "last session".to_owned(),
            (Some(d), _) => format!("◉ {} · {}", d.name, d.kind.to_lowercase()),
            (None, _) => "no active device".to_owned(),
        });
    }

    pub fn set_cover(&self, texture: Option<&gdk::Texture>) {
        self.cover.set_paintable(texture);
    }

    pub fn set_queue(&self, tracks: &[Track]) {
        clear_list(&self.queue);
        if tracks.is_empty() {
            self.queue.append(&label("queue is empty", &["rt-faint", "rt-small"]));
            return;
        }
        for t in tracks.iter().take(12) {
            let row = hbox(8);
            let pic = cover_picture(32);
            if let Some(url) = pick_image(t.images(), 60).map(|i| i.url.clone()) {
                let p = pic.clone();
                self.app.load_image(&url, move |tex| p.set_paintable(tex.as_ref()));
            }
            row.append(&pic);
            let meta = vbox(0);
            meta.append(&ellipsized(&t.name, &["rt-small"]));
            meta.append(&ellipsized(&t.artist_names(), &["rt-faint", "rt-small"]));
            row.append(&meta);
            self.queue.append(&row);
        }
    }
}
