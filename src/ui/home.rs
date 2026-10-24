//! Home: a greeting, "jump back in" (the places you played from last, then your library) and
//! "recently played" songs. Spotify's personalised shelves ("made for you", mixes, genres) are not
//! open to apps in development mode, so home only shows what rtify can really fetch.

use gtk::prelude::*;

use super::library::{Collection, cover_widget};
use super::widgets::*;
use crate::app::App;
use crate::spotify::model::{Track, pick_image};

/// "good morning" from 5 to 12 h, "good afternoon" until 18 h, otherwise "good evening".
pub fn greeting(hour: i32) -> &'static str {
    match hour {
        5..=11 => "good morning",
        12..=17 => "good afternoon",
        _ => "good evening",
    }
}

/// Tiles under "jump back in": the contexts played from last (newest first, each once), then
/// the rest of the library, at most `max`.
pub fn jump_back_in(recent_contexts: &[String], library: &[Collection], max: usize) -> Vec<Collection> {
    let mut out: Vec<Collection> = Vec::new();
    let push = |c: &Collection, out: &mut Vec<Collection>| {
        if out.len() < max && !out.iter().any(|o| o.uri == c.uri) {
            out.push(c.clone());
        }
    };
    for uri in recent_contexts {
        if let Some(c) = library.iter().find(|c| &c.uri == uri) {
            push(c, &mut out);
        }
    }
    for c in library {
        push(c, &mut out);
    }
    out
}

/// Each song once, in the order first seen.
pub fn unique_tracks(tracks: Vec<Track>, max: usize) -> Vec<Track> {
    let mut out: Vec<Track> = Vec::new();
    for t in tracks {
        if out.len() >= max {
            break;
        }
        if !t.uri.is_empty() && !out.iter().any(|o| o.uri == t.uri) {
            out.push(t);
        }
    }
    out
}

#[derive(Clone)]
pub struct HomeView {
    pub root: gtk::Overlay,
    panel: Panel,
    date: gtk::Label,
    greeting: gtk::Label,
    jump: gtk::FlowBox,
    recent_heading: gtk::Box,
    recent: gtk::FlowBox,
    app: App,
}

fn flow(min_per_line: u32, max_per_line: u32) -> gtk::FlowBox {
    let f = gtk::FlowBox::new();
    f.set_selection_mode(gtk::SelectionMode::None);
    f.set_homogeneous(true);
    f.set_valign(gtk::Align::Start);
    f.set_min_children_per_line(min_per_line);
    f.set_max_children_per_line(max_per_line);
    f.set_column_spacing(8);
    f.set_row_spacing(8);
    f.add_css_class("rt-menu-host");
    f
}

impl HomeView {
    pub fn new(app: &App) -> Self {
        let date = label("", &["rt-kicker"]);
        let greeting = label("", &["rt-hero-xl"]);
        let jump = flow(1, 4);
        let recent = flow(2, 12);
        recent.set_homogeneous(false);
        recent.set_column_spacing(16);
        let recent_heading = heading("recently played");

        let content = vbox(16);
        content.set_margin_start(16);
        content.set_margin_end(16);
        content.set_margin_top(14);
        content.set_margin_bottom(16);
        let top = vbox(4);
        top.append(&date);
        top.append(&greeting);
        top.set_margin_bottom(14);
        content.append(&top);
        content.append(&heading("jump back in"));
        content.append(&jump);
        recent_heading.set_margin_top(18);
        content.append(&recent_heading);
        content.append(&recent);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&content)
            .vexpand(true)
            .hexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let panel = panel("home", &scroll);
        let this = Self {
            root: panel.root.clone(),
            panel,
            date,
            greeting,
            jump,
            recent_heading,
            recent,
            app: app.clone(),
        };
        this.refresh_greeting();
        this
    }

    /// Date and greeting for the current local time.
    pub fn refresh_greeting(&self) {
        if let Some(now) = crate::app::now_local() {
            let date = now.format("%A %-d %B").map(|s| s.to_lowercase()).unwrap_or_default();
            self.date.set_text(&date);
            self.greeting.set_text(greeting(now.hour()));
        }
    }

    pub fn set_library_size(&self, n: usize) {
        self.panel.set_meta(&format!("{n} in library"));
    }

    pub fn set_jump(&self, items: &[Collection]) {
        while let Some(c) = self.jump.child_at_index(0) {
            self.jump.remove(&c);
        }
        for c in items {
            let row = hbox(12);
            row.append(&cover_widget(&self.app, c, 48));
            let text = vbox(2);
            text.set_valign(gtk::Align::Center);
            text.append(&ellipsized(&c.name, &["rt-row-title"]));
            text.append(&ellipsized(&c.subtitle, &["rt-faint", "rt-small"]));
            row.append(&text);
            let b = gtk::Button::new();
            b.add_css_class("rt-tile");
            b.set_child(Some(&row));
            b.set_tooltip_text(Some(&c.name));
            b.update_property(&[gtk::accessible::Property::Label(&c.name)]);
            let (a, col) = (self.app.clone(), c.clone());
            b.connect_clicked(move |_| a.open_collection(col.clone(), true));
            let (a, col) = (self.app.clone(), c.clone());
            super::menu::on_right_click(&b, move |_, _| Some(a.collection_menu(&col)));
            self.jump.insert(&b, -1);
        }
    }

    /// Recently played songs as cards: a click selects, a double click plays from that song.
    pub fn set_recent(&self, tracks: Vec<Track>) {
        while let Some(c) = self.recent.child_at_index(0) {
            self.recent.remove(&c);
        }
        self.recent_heading.set_visible(!tracks.is_empty());
        let mut group: Option<gtk::ToggleButton> = None;
        for (i, t) in tracks.iter().enumerate() {
            let card = vbox(6);
            let pic = cover_picture(132);
            if let Some(url) = pick_image(t.images(), 300).map(|i| i.url.clone()) {
                let p = pic.clone();
                self.app.load_image(&url, move |tex| p.set_paintable(tex.as_ref()));
            }
            card.append(&square(&pic, 132));
            let name = ellipsized(&t.name, &["rt-row-title"]);
            name.set_max_width_chars(14);
            card.append(&name);
            let artist = ellipsized(&t.artist_names(), &["rt-faint", "rt-small"]);
            artist.set_max_width_chars(14);
            card.append(&artist);
            let b = gtk::ToggleButton::new();
            b.add_css_class("rt-card");
            b.set_child(Some(&card));
            b.update_property(&[gtk::accessible::Property::Label(&format!("{}, {}", t.name, t.artist_names()))]);
            match &group {
                Some(g) => b.set_group(Some(g)),
                None => group = Some(b.clone()),
            }
            let double = gtk::GestureClick::new();
            double.set_propagation_phase(gtk::PropagationPhase::Capture);
            let (a, list) = (self.app.clone(), tracks.clone());
            double.connect_pressed(move |_, n, _, _| {
                if n == 2 {
                    a.play_from("rtify:recent", &list, i, false);
                }
            });
            b.add_controller(double);
            self.recent.insert(&b, -1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(uri: &str) -> Collection {
        Collection {
            uri: uri.into(),
            name: uri.into(),
            ..Default::default()
        }
    }

    #[test]
    fn greetings_follow_the_clock() {
        assert_eq!(greeting(4), "good evening");
        assert_eq!(greeting(5), "good morning");
        assert_eq!(greeting(12), "good afternoon");
        assert_eq!(greeting(18), "good evening");
    }

    #[test]
    fn jump_back_in_puts_recent_contexts_first() {
        let lib = vec![c("a"), c("b"), c("c"), c("d")];
        let recent = vec!["c".to_owned(), "x".to_owned(), "c".to_owned(), "a".to_owned()];
        let uris: Vec<_> = jump_back_in(&recent, &lib, 3).into_iter().map(|c| c.uri).collect();
        assert_eq!(uris, ["c", "a", "b"]);
        assert!(jump_back_in(&[], &[], 6).is_empty());
    }

    #[test]
    fn recent_tracks_are_unique() {
        let t = |u: &str| Track {
            uri: u.into(),
            ..Default::default()
        };
        let out = unique_tracks(vec![t("1"), t("2"), t("1"), t(""), t("3")], 2);
        assert_eq!(out.iter().map(|t| t.uri.as_str()).collect::<Vec<_>>(), ["1", "2"]);
    }
}
