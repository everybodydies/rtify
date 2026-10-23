//! The lyrics page: the renderer, two toggles and a `⋯` menu for everything rarely needed.

pub mod animations;
pub mod layout;
pub mod view;

use std::cell::Cell;
use std::rc::Rc;

use gtk::{glib, prelude::*};

use super::widgets::*;
use crate::app::App;
use crate::lyrics::LyricDocument;
use crate::lyrics::provider::demo;
use view::{Clock, LyricsView, ViewOptions};

#[derive(Clone)]
pub struct LyricsPage {
    pub root: gtk::Overlay,
    pub view: LyricsView,
    panel: Panel,
    footer: gtk::Label,
    status: gtk::Label,
    local_controls: gtk::Box,
    play: gtk::Button,
    romanized: gtk::ToggleButton,
    translated: gtk::ToggleButton,
    follow: gtk::CheckButton,
    offset: gtk::Label,
    demo_index: Rc<Cell<usize>>,
    updating: Rc<Cell<bool>>,
}

impl LyricsPage {
    pub fn new(app: &App) -> Self {
        let view = LyricsView::default();
        let romanized = toggle("romaji", "Romanization (Japanese, Korean, Chinese, Cyrillic)");
        let translated = toggle("translate", "Translation under each line");

        // Local clock controls only appear for demo songs (not while following Spotify).
        let play = button("▶", "Play/pause");
        let restart = button("⟲", "From the start");
        let local_controls = hbox(2);
        local_controls.append(&play);
        local_controls.append(&restart);

        // ⋯ menu.
        let follow = gtk::CheckButton::with_label("follow Spotify playback");
        let demo_btn = button("play a demo song", "");
        let minus = button("−", "Show lines later");
        let plus = button("+", "Show lines earlier");
        let offset = label("", &["rt-mono-num"]);
        let offset_row = hbox(6);
        offset_row.append(&label("timing", &["rt-dim"]));
        offset_row.append(&spacer());
        offset_row.append(&minus);
        offset_row.append(&offset);
        offset_row.append(&plus);
        let menu_box = vbox(6);
        menu_box.set_size_request(240, -1);
        for w in [follow.upcast_ref::<gtk::Widget>(), offset_row.upcast_ref(), demo_btn.upcast_ref()] {
            menu_box.append(w);
        }
        let popover = gtk::Popover::new();
        popover.set_child(Some(&menu_box));
        let more = gtk::MenuButton::builder()
            .child(&gtk::Label::new(Some("⋯")))
            .popover(&popover)
            .tooltip_text("Lyrics options")
            .build();

        let bar = hbox(4);
        bar.add_css_class("rt-toolbar");
        bar.set_halign(gtk::Align::End);
        bar.append(&local_controls);
        bar.append(&romanized);
        bar.append(&translated);
        bar.append(&more);

        let footer = ellipsized("", &["rt-faint", "rt-small"]);
        let status = ellipsized("", &["rt-faint", "rt-small"]);
        status.set_halign(gtk::Align::End);
        let foot = hbox(8);
        footer.set_hexpand(true);
        foot.append(&footer);
        foot.append(&status);

        let body = vbox(4);
        body.append(&bar);
        body.append(&view);
        body.append(&foot);
        let panel = panel("lyrics", &body);

        let this = Self {
            root: panel.root.clone(),
            view: view.clone(),
            panel,
            footer,
            status,
            local_controls,
            play: play.clone(),
            romanized: romanized.clone(),
            translated: translated.clone(),
            follow: follow.clone(),
            offset,
            demo_index: Rc::new(Cell::new(0)),
            updating: Rc::new(Cell::new(false)),
        };

        let (page, a, pop) = (this.clone(), app.clone(), popover.clone());
        demo_btn.connect_clicked(move |_| {
            pop.popdown();
            let i = page.demo_index.get();
            page.demo_index.set((i + 1) % demo::SONGS.len());
            a.open_demo_lyrics(i, true);
        });
        for (b, delta) in [(&minus, -100i64), (&plus, 100)] {
            let a = app.clone();
            b.connect_clicked(move |_| {
                a.update_config(|c| c.lyrics.offset_ms = (c.lyrics.offset_ms + delta).clamp(-5000, 5000));
                a.apply_lyrics_options();
            });
        }
        let (page, a) = (this.clone(), app.clone());
        follow.connect_toggled(move |b| {
            if page.updating.get() {
                return;
            }
            let on = b.is_active();
            a.update_config(|c| c.lyrics.follow_playback = on);
            a.lyrics_follow_changed();
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
        restart.connect_clicked(move |_| page.view.with_local_clock(|c| c.seek(0)));
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

        // Keep the play button honest while a local clock runs out.
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
                self.local_controls.set_visible(self.view.document().is_some());
                self.play.set_label(if c.playing() && !c.finished() { "❚❚" } else { "▶" });
            }
            Clock::External(_) => self.local_controls.set_visible(false),
        }
    }

    pub fn set_options(&self, opts: ViewOptions, follow: bool) {
        self.updating.set(true);
        self.romanized.set_active(opts.romanized);
        self.translated.set_active(opts.translation);
        self.follow.set_active(follow);
        self.offset.set_text(&format!("{:+.1}s", opts.offset_ms as f64 / 1000.0));
        self.updating.set(false);
        self.view.set_options(opts);
    }

    pub fn show_document(&self, doc: Option<&LyricDocument>, title: &str, clock: Clock) {
        self.view.set_document(doc.cloned());
        self.view.set_clock(clock);
        self.panel.set_title(if title.is_empty() { "lyrics" } else { title });
        self.status.set_text("");
        self.set_attribution(doc);
        self.refresh_play_button();
    }

    pub fn set_attribution(&self, doc: Option<&LyricDocument>) {
        let Some(doc) = doc else {
            self.footer.set_text("");
            return;
        };
        let a = &doc.attribution;
        let mut parts = vec![a.source.clone()];
        if !a.songwriters.is_empty() {
            parts.push(format!("written by {}", a.songwriters.join(", ")));
        }
        if let Some(t) = &doc.translation {
            parts.push(format!("translation: {} ({})", t.provenance, t.language));
        }
        self.footer.set_text(&parts.join(" · "));
        self.footer.set_tooltip_text(Some(&a.source_id));
    }

    pub fn set_status(&self, text: &str) {
        self.status.set_text(text);
    }
}
