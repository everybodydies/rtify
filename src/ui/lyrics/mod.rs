//! The lyrics page: toolbar, renderer and attribution line.

pub mod animations;
pub mod layout;
pub mod view;

use std::cell::Cell;
use std::rc::Rc;

use gtk::{gio, glib, prelude::*};

use super::widgets::*;
use crate::app::App;
use crate::lyrics::LyricDocument;
use crate::lyrics::provider::demo;
use view::{Clock, LyricsView, ViewOptions};

#[derive(Clone)]
pub struct LyricsPage {
    pub root: gtk::Overlay,
    pub view: LyricsView,
    source: gtk::Label,
    sync: gtk::Label,
    attribution: gtk::Label,
    play: gtk::Button,
    romanized: gtk::ToggleButton,
    translated: gtk::ToggleButton,
    demo_index: Rc<Cell<usize>>,
    updating: Rc<Cell<bool>>,
}

impl LyricsPage {
    pub fn new(app: &App) -> Self {
        let view = LyricsView::default();
        let source = ellipsized("no lyrics loaded", &["rt-dim"]);
        source.set_hexpand(true);
        let sync = label("", &["rt-faint"]);
        let demo_btn = button("demo", "Cycle through the bundled demo songs");
        let open = button("open…", "Open a .lrc or .ttml file");
        let play = button("▶", "Play/pause the local lyric clock");
        let restart = button("⟲", "Restart from the beginning");
        let romanized = toggle("roma", "Show romanization");
        let translated = toggle("trans", "Show translation");

        let bar = hbox(6);
        bar.add_css_class("rt-toolbar");
        for w in [
            source.upcast_ref::<gtk::Widget>(),
            sync.upcast_ref(),
            demo_btn.upcast_ref(),
            open.upcast_ref(),
            play.upcast_ref(),
            restart.upcast_ref(),
            romanized.upcast_ref(),
            translated.upcast_ref(),
        ] {
            bar.append(w);
        }
        let attribution = ellipsized("", &["rt-faint"]);
        let body = vbox(6);
        body.append(&bar);
        body.append(&view);
        body.append(&attribution);
        let root = panel("lyrics", &body).root;

        let this = Self {
            root,
            view: view.clone(),
            source,
            sync,
            attribution,
            play: play.clone(),
            romanized: romanized.clone(),
            translated: translated.clone(),
            demo_index: Rc::new(Cell::new(0)),
            updating: Rc::new(Cell::new(false)),
        };

        let (page, a) = (this.clone(), app.clone());
        demo_btn.connect_clicked(move |_| {
            let i = page.demo_index.get();
            page.demo_index.set((i + 1) % demo::SONGS.len());
            a.open_demo_lyrics(i, true);
        });
        let a = app.clone();
        open.connect_clicked(move |b| {
            let dialog = gtk::FileDialog::builder().title("Open lyrics").modal(true).build();
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("Lyrics (.lrc, .ttml, .txt)"));
            for p in ["*.lrc", "*.LRC", "*.ttml", "*.TTML", "*.txt"] {
                filter.add_pattern(p);
            }
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            dialog.set_filters(Some(&filters));
            let window = b.root().and_downcast::<gtk::Window>();
            let a = a.clone();
            dialog.open(window.as_ref(), gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res
                    && let Some(path) = file.path()
                {
                    a.open_lyrics_file(&path);
                }
            });
        });
        let page = this.clone();
        play.connect_clicked(move |_| {
            page.view.with_local_clock(|c| {
                if c.finished() {
                    c.seek(0);
                }
                let playing = c.playing();
                c.set_playing(!playing);
            });
            page.refresh_play_button();
        });
        let page = this.clone();
        restart.connect_clicked(move |_| {
            page.view.with_local_clock(|c| c.seek(0));
        });
        let (page, a) = (this.clone(), app.clone());
        romanized.connect_toggled(move |b| {
            if page.updating.get() {
                return;
            }
            let on = b.is_active();
            a.update_config(|c| c.lyrics.romanization = on);
            a.apply_lyrics_options();
        });
        let (page, a) = (this.clone(), app.clone());
        translated.connect_toggled(move |b| {
            if page.updating.get() {
                return;
            }
            let on = b.is_active();
            a.update_config(|c| c.lyrics.translation_enabled = on);
            a.apply_lyrics_options();
            a.translate_current_lyrics();
        });

        // Keep the play button label honest while the clock runs out.
        let page = this.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
            if page.root.is_mapped() {
                page.refresh_play_button();
            }
            glib::ControlFlow::Continue
        });
        this
    }

    pub fn refresh_play_button(&self) {
        match self.view.clock() {
            Clock::Local(c) => {
                self.play.set_sensitive(true);
                self.play.set_label(if c.playing() && !c.finished() { "❚❚" } else { "▶" });
            }
            Clock::External(_) => {
                self.play.set_sensitive(false);
                self.play.set_label("▶");
            }
        }
    }

    pub fn set_options(&self, opts: ViewOptions) {
        self.updating.set(true);
        self.romanized.set_active(opts.romanized);
        self.translated.set_active(opts.translation);
        self.updating.set(false);
        self.view.set_options(opts);
    }

    pub fn show_document(&self, doc: Option<&LyricDocument>, source: &str, clock: Clock) {
        self.view.set_document(doc.cloned());
        self.view.set_clock(clock);
        self.source.set_text(source);
        self.set_attribution(doc);
        self.refresh_play_button();
    }

    pub fn set_attribution(&self, doc: Option<&LyricDocument>) {
        let Some(doc) = doc else {
            self.attribution.set_text("");
            return;
        };
        let a = &doc.attribution;
        let mut parts = vec![format!("lyrics: {}", a.source)];
        if !a.rights.is_empty() {
            parts.push(a.rights.clone());
        }
        if !a.songwriters.is_empty() {
            parts.push(format!("written by {}", a.songwriters.join(", ")));
        }
        if let Some(t) = &doc.translation {
            parts.push(format!("translation: {} ({})", t.provenance, t.language));
        }
        self.attribution.set_text(&parts.join(" · "));
        self.attribution.set_tooltip_text(Some(&a.source_id));
    }

    pub fn set_sync_status(&self, text: &str) {
        self.sync.set_text(text);
    }
}
