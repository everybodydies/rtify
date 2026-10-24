//! Search: one large field ("/ what do you want to hear?"), recent searches while it is empty,
//! and results while typing: a top result, the best songs, then artists, albums and playlists.
//! Spotify's browse categories are closed to apps in development mode, so there is no genre grid.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk::{glib, prelude::*};

use super::library::{Collection, cover_widget};
use super::widgets::*;
use crate::app::App;
use crate::spotify::model::{Track, pick_image};

/// How many searches "recent searches" keeps.
pub const RECENT_MAX: usize = 8;

/// Put `query` first in the recent searches (case-insensitively once) and keep `RECENT_MAX`.
pub fn remember(recent: &mut Vec<String>, query: &str) {
    let q = query.trim();
    if q.is_empty() {
        return;
    }
    recent.retain(|r| !r.eq_ignore_ascii_case(q));
    recent.insert(0, q.to_owned());
    recent.truncate(RECENT_MAX);
}

/// What a search found, in the order the view shows it.
#[derive(Debug, Clone, Default)]
pub struct Results {
    pub tracks: Vec<Track>,
    pub artists: Vec<Collection>,
    pub albums: Vec<Collection>,
    pub playlists: Vec<Collection>,
}

#[derive(Clone)]
pub struct SearchView {
    pub root: gtk::Overlay,
    panel: Panel,
    pub entry: gtk::Entry,
    clear: gtk::Button,
    recent_box: gtk::Box,
    recent: gtk::FlowBox,
    results: gtk::Box,
    status: gtk::Label,
    tracks: Rc<RefCell<Vec<Track>>>,
    timer: Rc<RefCell<Option<glib::SourceId>>>,
    /// Set while the entry is filled from history, so it doesn't search again.
    quiet: Rc<std::cell::Cell<bool>>,
    app: App,
}

fn cards() -> gtk::FlowBox {
    let f = gtk::FlowBox::new();
    f.set_selection_mode(gtk::SelectionMode::None);
    f.set_homogeneous(false);
    f.set_min_children_per_line(1);
    f.set_max_children_per_line(12);
    f.set_column_spacing(16);
    f.set_row_spacing(16);
    f.add_css_class("rt-menu-host");
    f
}

impl SearchView {
    pub fn new(app: &App) -> Self {
        let slash = label("/", &["rt-slash-xl"]);
        let entry = gtk::Entry::new();
        entry.add_css_class("rt-bare");
        entry.add_css_class("rt-search-xl");
        entry.set_placeholder_text(Some("what do you want to hear?"));
        entry.update_property(&[gtk::accessible::Property::Label("Search Spotify")]);
        entry.set_hexpand(true);
        let clear = button("clear", "Clear the search");
        clear.set_valign(gtk::Align::Center);
        let field = hbox(14);
        field.append(&slash);
        field.append(&entry);
        field.append(&clear);
        let rule = hbox(0);
        rule.add_css_class("rt-rule");

        let recent = cards();
        recent.set_homogeneous(false);
        recent.set_max_children_per_line(12);
        recent.set_column_spacing(6);
        recent.set_row_spacing(6);
        let recent_box = vbox(12);
        recent_box.append(&heading("recent searches"));
        recent_box.append(&recent);
        recent_box.set_margin_top(14);

        let status = label("", &["rt-faint"]);
        let results = vbox(28);
        results.set_margin_top(14);

        let content = vbox(10);
        content.set_margin_start(16);
        content.set_margin_end(16);
        content.set_margin_top(10);
        content.set_margin_bottom(16);
        content.append(&field);
        content.append(&rule);
        content.append(&recent_box);
        content.append(&status);
        content.append(&results);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&content)
            .vexpand(true)
            .hexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let panel = panel("search", &scroll);
        panel.set_meta("ctrl+k");
        let this = Self {
            root: panel.root.clone(),
            panel,
            entry: entry.clone(),
            clear: clear.clone(),
            recent_box,
            recent,
            results,
            status,
            tracks: Rc::default(),
            timer: Rc::default(),
            quiet: Rc::default(),
            app: app.clone(),
        };
        let v = this.clone();
        entry.connect_changed(move |e| {
            let q = e.text().trim().to_owned();
            v.clear.set_visible(!q.is_empty());
            if q.is_empty() {
                v.show_recent();
            }
            if v.quiet.get() {
                return;
            }
            // Search while typing, once the typing pauses.
            if let Some(id) = v.timer.take() {
                id.remove();
            }
            if q.is_empty() {
                return;
            }
            let (a, timer) = (v.app.clone(), v.timer.clone());
            let id = glib::timeout_add_local_once(Duration::from_millis(350), move || {
                timer.take();
                a.search(&q, true);
            });
            v.timer.replace(Some(id));
        });
        let v = this.clone();
        entry.connect_activate(move |e| {
            if let Some(id) = v.timer.take() {
                id.remove();
            }
            let q = e.text().trim().to_owned();
            if !q.is_empty() {
                v.app.search(&q, true);
            }
        });
        let e = entry.clone();
        clear.connect_clicked(move |_| {
            e.set_text("");
            e.grab_focus();
        });
        this.clear.set_visible(false);
        this.show_recent();
        this
    }

    /// Fill the field without searching again (back/forward, or text typed in the top bar).
    pub fn set_query(&self, q: &str) {
        self.quiet.set(true);
        self.entry.set_text(q);
        self.entry.set_position(-1);
        self.quiet.set(false);
    }

    pub fn query(&self) -> String {
        self.entry.text().trim().to_owned()
    }

    pub fn focus(&self) {
        self.entry.grab_focus();
        self.entry.set_position(-1);
    }

    fn show_recent(&self) {
        while let Some(c) = self.recent.child_at_index(0) {
            self.recent.remove(&c);
        }
        let recent = self.app.config.borrow().config.library.recent_searches.clone();
        for q in &recent {
            let b = button(q, "");
            b.add_css_class("rt-seg");
            let (e, q) = (self.entry.clone(), q.clone());
            b.connect_clicked(move |_| {
                e.set_text(&q);
                e.grab_focus();
                e.set_position(-1);
            });
            self.recent.insert(&b, -1);
        }
        self.recent_box.set_visible(!recent.is_empty());
        self.results.set_visible(false);
        self.status.set_visible(false);
        self.panel.set_meta("ctrl+k");
    }

    pub fn set_error(&self, text: &str) {
        self.status.set_text(text);
        self.status.set_visible(true);
    }

    pub fn set_results(&self, query: &str, r: Results) {
        if self.query() != query.trim() {
            // A newer query is on its way.
            return;
        }
        self.recent_box.set_visible(false);
        self.status.set_visible(false);
        while let Some(c) = self.results.first_child() {
            self.results.remove(&c);
        }
        self.results.set_visible(true);
        let count = |n: usize, one: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") };
        self.panel.set_meta(&format!(
            "{} · {} · {}",
            count(r.tracks.len(), "track"),
            count(r.artists.len(), "artist"),
            count(r.albums.len(), "album")
        ));
        *self.tracks.borrow_mut() = r.tracks.clone();

        if r.tracks.is_empty() && r.artists.is_empty() && r.albums.is_empty() && r.playlists.is_empty() {
            self.set_error(&format!("nothing found for “{}”", query.trim()));
            return;
        }

        // Top result next to the best songs.
        let top_row = hbox(24);
        top_row.set_homogeneous(true);
        if let Some(top) = self.top_result(&r) {
            let col = vbox(12);
            col.append(&heading("top result"));
            col.append(&top);
            top_row.append(&col);
        }
        if !r.tracks.is_empty() {
            let col = vbox(12);
            col.append(&heading("tracks"));
            col.append(&self.track_list(&r.tracks));
            top_row.append(&col);
        }
        self.results.append(&top_row);

        for (title, items) in [("artists", &r.artists), ("albums", &r.albums), ("playlists", &r.playlists)] {
            if items.is_empty() {
                continue;
            }
            let section = vbox(12);
            section.append(&heading(title));
            let flow = cards();
            for c in items.iter().take(8) {
                flow.insert(&self.card(c), -1);
            }
            section.append(&flow);
            self.results.append(&section);
        }
    }

    fn top_result(&self, r: &Results) -> Option<gtk::Button> {
        let (name, sub, cover): (String, String, gtk::Widget) = if let Some(t) = r.tracks.first() {
            let pic = cover_picture(96);
            if let Some(url) = pick_image(t.images(), 200).map(|i| i.url.clone()) {
                let p = pic.clone();
                self.app.load_image(&url, move |tex| p.set_paintable(tex.as_ref()));
            }
            (t.name.clone(), format!("track · {}", t.artist_names()), square(&pic, 96).upcast())
        } else {
            let c = r.artists.first().or(r.albums.first()).or(r.playlists.first())?;
            (
                c.name.clone(),
                format!("{} · {}", c.kind, c.subtitle),
                cover_widget(&self.app, c, 96),
            )
        };
        let body = vbox(10);
        body.append(&cover);
        let title = ellipsized(&name, &["rt-hero"]);
        title.set_margin_top(12);
        body.append(&title);
        body.append(&ellipsized(&sub, &["rt-dim"]));
        let b = gtk::Button::new();
        b.add_css_class("rt-tile");
        b.set_child(Some(&body));
        b.update_property(&[gtk::accessible::Property::Label(&format!("top result: {name}"))]);
        b.set_valign(gtk::Align::Start);
        let v = self.clone();
        let first_artist = r.artists.first().cloned();
        b.connect_clicked(move |_| {
            if !v.tracks.borrow().is_empty() {
                let tracks = v.tracks.borrow().clone();
                v.app.play_from("rtify:search", &tracks, 0, false);
            } else if let Some(c) = first_artist.clone() {
                v.app.open_collection(c, true);
            }
        });
        Some(b)
    }

    fn track_list(&self, tracks: &[Track]) -> gtk::ListBox {
        let list = gtk::ListBox::new();
        list.add_css_class("rt-list");
        list.set_activate_on_single_click(false);
        tab_leaves(&list);
        for t in tracks.iter().take(5) {
            let row = hbox(12);
            let pic = cover_picture(36);
            if let Some(url) = pick_image(t.images(), 60).map(|i| i.url.clone()) {
                let p = pic.clone();
                self.app.load_image(&url, move |tex| p.set_paintable(tex.as_ref()));
            }
            row.append(&pic);
            let text = vbox(2);
            text.set_hexpand(true);
            text.set_valign(gtk::Align::Center);
            text.append(&ellipsized(&t.name, &["rt-row-title"]));
            text.append(&ellipsized(
                &format!("{} · {}", t.artist_names(), t.album_name()),
                &["rt-faint", "rt-small"],
            ));
            row.append(&text);
            row.append(&label(&fmt_time(t.duration_ms), &["rt-faint", "rt-mono-num"]));
            let item = gtk::ListBoxRow::builder().child(&row).build();
            item.update_property(&[gtk::accessible::Property::Label(&format!("{}, {}", t.name, t.artist_names()))]);
            list.append(&item);
        }
        let v = self.clone();
        list.connect_row_activated(move |_, row| {
            let tracks = v.tracks.borrow().clone();
            v.app.play_from("rtify:search", &tracks, row.index() as usize, false);
        });
        list
    }

    fn card(&self, c: &Collection) -> gtk::Button {
        let card = vbox(4);
        card.append(&cover_widget(&self.app, c, 150));
        let name = ellipsized(&c.name, &["rt-row-title"]);
        name.set_max_width_chars(16);
        name.set_margin_top(4);
        card.append(&name);
        let sub = ellipsized(&c.subtitle, &["rt-faint", "rt-small"]);
        sub.set_max_width_chars(16);
        card.append(&sub);
        let b = gtk::Button::new();
        b.add_css_class("rt-card");
        b.set_child(Some(&card));
        b.update_property(&[gtk::accessible::Property::Label(&c.name)]);
        let (a, col) = (self.app.clone(), c.clone());
        b.connect_clicked(move |_| a.open_collection(col.clone(), true));
        let (a, col) = (self.app.clone(), c.clone());
        super::menu::on_right_click(&b, move |_, _| Some(a.collection_menu(&col)));
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_searches_move_to_the_front_once() {
        let mut r = vec!["neon".to_owned(), "lumen".to_owned()];
        remember(&mut r, " Lumen ");
        assert_eq!(r, ["Lumen", "neon"]);
        remember(&mut r, "   ");
        assert_eq!(r.len(), 2);
        for i in 0..20 {
            remember(&mut r, &format!("q{i}"));
        }
        assert_eq!(r.len(), RECENT_MAX);
        assert_eq!(r[0], "q19");
    }
}
