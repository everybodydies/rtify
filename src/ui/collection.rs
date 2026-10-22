//! Main view for a playlist, album, liked songs or search results: a large header like the
//! official client, play/shuffle actions and a track table (#, title, album, added, time).

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use super::library::{Collection, SEARCH_URI, cover_widget};
use super::widgets::*;
use crate::app::App;
use crate::spotify::model::{Track, pick_image};

#[derive(Debug, Clone)]
pub struct TrackRow {
    pub track: Track,
    pub added_at: String,
}

impl TrackRow {
    pub fn new(track: Track) -> Self {
        Self {
            track,
            added_at: String::new(),
        }
    }
}

#[derive(Default)]
pub struct ViewState {
    pub collection: Option<Collection>,
    pub rows: Vec<TrackRow>,
    pub next_offset: Option<u32>,
    pub loading: bool,
    playing_uri: Option<String>,
}

#[derive(Clone)]
pub struct CollectionView {
    pub root: gtk::Overlay,
    panel: Panel,
    cover: gtk::Box,
    kind: gtk::Label,
    title: gtk::Label,
    subtitle: gtk::Label,
    description: gtk::Label,
    results_title: gtk::Label,
    results: gtk::FlowBox,
    songs_title: gtk::Label,
    table: gtk::ListBox,
    status: gtk::Label,
    scroll: gtk::ScrolledWindow,
    pub state: Rc<RefCell<ViewState>>,
    app: App,
}

/// `2026-10-01T00:00:00Z` → `2026-10-01`.
pub fn short_date(s: &str) -> &str {
    s.get(..10).unwrap_or(s)
}

impl CollectionView {
    pub fn new(app: &App) -> Self {
        let cover = hbox(0);
        let kind = label("", &["rt-kind"]);
        let title = label("", &["rt-hero"]);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let subtitle = label("", &["rt-dim"]);
        let description = label("", &["rt-faint"]);
        description.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let meta = vbox(4);
        meta.set_valign(gtk::Align::End);
        meta.set_hexpand(true);
        for w in [&kind, &title, &description, &subtitle] {
            meta.append(w);
        }
        let header = hbox(22);
        header.append(&cover);
        header.append(&meta);

        let play = button("▶  PLAY", "Play from the top");
        play.add_css_class("rt-primary");
        let shuffle = button("⇄ shuffle", "Shuffle play");
        shuffle.add_css_class("rt-chip");
        let actions = hbox(10);
        actions.append(&play);
        actions.append(&shuffle);

        let results_title = label("[ PLAYLISTS & ALBUMS ]", &["rt-section"]);
        let results = gtk::FlowBox::new();
        results.set_selection_mode(gtk::SelectionMode::None);
        results.set_max_children_per_line(12);
        results.set_column_spacing(14);
        results.set_row_spacing(14);
        results.set_homogeneous(true);
        let songs_title = label("[ SONGS ]", &["rt-section"]);

        let head = hbox(12);
        head.add_css_class("rt-table-head");
        let num = label("#", &[]);
        num.set_width_chars(3);
        num.set_xalign(1.0);
        let t = label("TITLE", &[]);
        t.set_hexpand(true);
        let album = label("ALBUM", &[]);
        album.set_width_chars(26);
        let added = label("ADDED", &[]);
        added.set_width_chars(11);
        let time = label("TIME", &[]);
        time.set_width_chars(6);
        time.set_xalign(1.0);
        head.append(&num);
        let thumb_gap = hbox(0);
        thumb_gap.set_size_request(36, -1);
        head.append(&thumb_gap);
        head.append(&t);
        head.append(&album);
        head.append(&added);
        head.append(&time);

        let table = gtk::ListBox::new();
        table.add_css_class("rt-list");
        table.set_activate_on_single_click(false);
        let status = label("", &["rt-faint"]);
        status.set_margin_top(8);

        let content = vbox(14);
        content.set_margin_start(8);
        content.set_margin_end(8);
        content.set_margin_bottom(12);
        for w in [
            header.upcast_ref::<gtk::Widget>(),
            actions.upcast_ref(),
            results_title.upcast_ref(),
            results.upcast_ref(),
            songs_title.upcast_ref(),
        ] {
            content.append(w);
        }
        let tracks_box = vbox(0);
        tracks_box.append(&head);
        tracks_box.append(&table);
        tracks_box.append(&status);
        content.append(&tracks_box);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&content)
            .vexpand(true)
            .hexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let panel = panel("library", &scroll);

        let this = Self {
            root: panel.root.clone(),
            panel,
            cover,
            kind,
            title,
            subtitle,
            description,
            results_title,
            results,
            songs_title,
            table,
            status,
            scroll: scroll.clone(),
            state: Rc::default(),
            app: app.clone(),
        };
        let v = this.clone();
        this.table
            .connect_row_activated(move |_, row| v.play_from(row.index() as usize, false));
        let v = this.clone();
        play.connect_clicked(move |_| v.play_from(0, false));
        let v = this.clone();
        shuffle.connect_clicked(move |_| v.play_from(0, true));
        let a = app.clone();
        scroll.connect_edge_reached(move |_, pos| {
            if pos == gtk::PositionType::Bottom {
                a.load_more_tracks();
            }
        });
        this.set_search_mode(false);
        this
    }

    fn play_from(&self, index: usize, shuffle: bool) {
        let (uri, tracks) = {
            let st = self.state.borrow();
            let Some(c) = &st.collection else { return };
            (c.uri.clone(), st.rows.iter().map(|r| r.track.clone()).collect::<Vec<_>>())
        };
        if shuffle {
            self.app.set_shuffle(true);
        }
        self.app.play_from(&uri, &tracks, index);
    }

    fn set_search_mode(&self, search: bool) {
        self.results_title.set_visible(search);
        self.results.set_visible(search);
        self.songs_title.set_visible(search);
    }

    /// Switch to `c` and clear the table; tracks arrive through `append`.
    pub fn show(&self, c: &Collection) {
        {
            let mut st = self.state.borrow_mut();
            st.collection = Some(c.clone());
            st.rows.clear();
            st.next_offset = None;
            st.loading = true;
        }
        self.panel.set_title(if c.kind == "liked" { "playlist" } else { &c.kind });
        self.kind.set_text(&match c.kind.as_str() {
            "liked" => "PLAYLIST".to_owned(),
            k => k.to_uppercase(),
        });
        self.title.set_text(&c.name);
        self.subtitle.set_text(&c.subtitle);
        self.description.set_text(&c.description);
        self.description.set_visible(!c.description.is_empty());
        while let Some(w) = self.cover.first_child() {
            self.cover.remove(&w);
        }
        self.cover.append(&cover_widget(&self.app, c, 168));
        self.set_search_mode(c.uri == SEARCH_URI);
        clear_list(&self.table);
        self.status.set_text("loading…");
        self.scroll.vadjustment().set_value(0.0);
    }

    pub fn show_search(&self, query: &str, collections: Vec<Collection>, tracks: Vec<Track>) {
        let c = Collection {
            uri: SEARCH_URI.into(),
            name: format!("“{query}”"),
            subtitle: format!("{} songs · {} playlists & albums", tracks.len(), collections.len()),
            kind: "search".into(),
            ..Default::default()
        };
        self.show(&c);
        while let Some(w) = self.results.first_child() {
            self.results.remove(&w);
        }
        for col in collections {
            let card = vbox(6);
            card.append(&cover_widget(&self.app, &col, 132));
            let name = ellipsized(&col.name, &["rt-row-title"]);
            name.set_max_width_chars(16);
            card.append(&name);
            let sub = ellipsized(&col.subtitle, &["rt-faint", "rt-small"]);
            sub.set_max_width_chars(16);
            card.append(&sub);
            let b = gtk::Button::new();
            b.add_css_class("flat");
            b.set_child(Some(&card));
            let a = self.app.clone();
            b.connect_clicked(move |_| a.open_collection(col.clone(), true));
            self.results.insert(&b, -1);
        }
        self.append(SEARCH_URI, tracks.into_iter().map(TrackRow::new).collect(), None);
    }

    pub fn uri(&self) -> Option<String> {
        self.state.borrow().collection.as_ref().map(|c| c.uri.clone())
    }

    /// Append a page of rows for `uri` (ignored when the user already moved on).
    pub fn append(&self, uri: &str, rows: Vec<TrackRow>, next: Option<u32>) {
        if self.uri().as_deref() != Some(uri) {
            return;
        }
        let start = self.state.borrow().rows.len();
        let playing = self.state.borrow().playing_uri.clone();
        let show_added = rows.iter().any(|r| !r.added_at.is_empty());
        for (i, r) in rows.iter().enumerate() {
            let t = &r.track;
            let row = gtk::ListBoxRow::new();
            let b = hbox(12);
            let num = label(&format!("{}", start + i + 1), &["rt-faint", "rt-mono-num", "rt-row-num"]);
            num.set_width_chars(3);
            num.set_xalign(1.0);
            b.append(&num);
            match pick_image(t.images(), 60).map(|im| im.url.clone()) {
                Some(url) => {
                    let pic = cover_picture(36);
                    let p = pic.clone();
                    self.app.load_image(&url, move |tex| p.set_paintable(tex.as_ref()));
                    b.append(&pic);
                }
                None => b.append(&hbox(0)),
            }
            b.last_child().unwrap().set_size_request(36, 36);
            let meta = vbox(0);
            meta.set_hexpand(true);
            meta.set_valign(gtk::Align::Center);
            meta.append(&ellipsized(&t.name, &["rt-row-title"]));
            meta.append(&ellipsized(&t.artist_names(), &["rt-dim", "rt-small"]));
            b.append(&meta);
            let album = ellipsized(t.album_name(), &["rt-dim", "rt-small"]);
            album.set_width_chars(26);
            album.set_max_width_chars(26);
            b.append(&album);
            let added = label(
                if show_added { short_date(&r.added_at) } else { "" },
                &["rt-faint", "rt-small", "rt-mono-num"],
            );
            added.set_width_chars(11);
            b.append(&added);
            let time = label(&fmt_time(t.duration_ms), &["rt-faint", "rt-mono-num"]);
            time.set_width_chars(6);
            time.set_xalign(1.0);
            b.append(&time);
            row.set_child(Some(&b));
            if playing.as_deref() == Some(t.uri.as_str()) {
                row.add_css_class("rt-playing");
            }
            self.table.append(&row);
        }
        let mut st = self.state.borrow_mut();
        st.rows.extend(rows);
        st.next_offset = next;
        st.loading = false;
        self.status.set_text(match (st.rows.is_empty(), next) {
            (true, _) => "no songs",
            (false, Some(_)) => "scroll for more…",
            (false, None) => "",
        });
    }

    pub fn set_error(&self, msg: &str) {
        self.state.borrow_mut().loading = false;
        self.status.set_text(msg);
    }

    pub fn mark_playing(&self, uri: Option<&str>) {
        let uris: Vec<String> = {
            let mut st = self.state.borrow_mut();
            st.playing_uri = uri.map(String::from);
            st.rows.iter().map(|r| r.track.uri.clone()).collect()
        };
        let mut i = 0;
        while let Some(row) = self.table.row_at_index(i) {
            if uri.is_some() && uris.get(i as usize).map(String::as_str) == uri {
                row.add_css_class("rt-playing");
            } else {
                row.remove_css_class("rt-playing");
            }
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates() {
        assert_eq!(super::short_date("2026-10-01T00:00:00Z"), "2026-10-01");
        assert_eq!(super::short_date("2026"), "2026");
    }
}
