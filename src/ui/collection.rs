//! Main view for a playlist, album, artist, liked songs or search results: header with song
//! count and total length, play/shuffle, search inside the list, and a sortable track table with
//! multi-select (Ctrl/Shift-click), right-click menus, middle-click to queue, drag to playlists
//! and a hover "+" for the last-used playlist. Each visited list is cached with its scroll
//! position, so going back is instant and lands where you were.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::{gdk, glib, prelude::*};

use super::clamp::Clamp;
use super::library::{Collection, LIKED_URI, SEARCH_URI, cover_widget};
use super::menu::{Menu, on_right_click};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    /// The playlist's own order.
    #[default]
    Custom,
    Title,
    Album,
    Added,
    Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sort {
    pub key: SortKey,
    pub desc: bool,
}

impl Sort {
    pub fn parse(s: &str) -> Sort {
        let (k, d) = s.split_once(':').unwrap_or((s, "asc"));
        let key = match k {
            "title" => SortKey::Title,
            "album" => SortKey::Album,
            "added" => SortKey::Added,
            "duration" => SortKey::Duration,
            _ => SortKey::Custom,
        };
        Sort {
            key,
            desc: d == "desc" && key != SortKey::Custom,
        }
    }

    pub fn to_config(self) -> String {
        let k = match self.key {
            SortKey::Custom => return "custom".into(),
            SortKey::Title => "title",
            SortKey::Album => "album",
            SortKey::Added => "added",
            SortKey::Duration => "duration",
        };
        format!("{k}:{}", if self.desc { "desc" } else { "asc" })
    }

    /// Clicking a header: ascending → descending → back to the custom order.
    pub fn click(self, key: SortKey) -> Sort {
        match (self.key == key, self.desc) {
            _ if key == SortKey::Custom => Sort::default(),
            (false, _) => Sort { key, desc: false },
            (true, false) => Sort { key, desc: true },
            (true, true) => Sort::default(),
        }
    }
}

/// Display order (indices into `rows`) for a sort. Stable, so equal keys keep playlist order.
pub fn sorted_order(rows: &[TrackRow], sort: Sort) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..rows.len()).collect();
    let lower = |s: &str| s.to_lowercase();
    match sort.key {
        SortKey::Custom => {}
        SortKey::Title => idx.sort_by_key(|&i| lower(&rows[i].track.name)),
        SortKey::Album => idx.sort_by_key(|&i| lower(rows[i].track.album_name())),
        SortKey::Added => idx.sort_by(|&a, &b| rows[a].added_at.cmp(&rows[b].added_at)),
        SortKey::Duration => idx.sort_by_key(|&i| rows[i].track.duration_ms),
    }
    if sort.desc {
        idx.reverse();
    }
    idx
}

pub fn matches_filter(r: &TrackRow, q: &str) -> bool {
    if q.is_empty() {
        return true;
    }
    let t = &r.track;
    [t.name.as_str(), &t.artist_names(), t.album_name()]
        .iter()
        .any(|s| s.to_lowercase().contains(q))
}

/// `2026-10-01T00:00:00Z` → `2026-10-01`.
pub fn short_date(s: &str) -> &str {
    s.get(..10).unwrap_or(s)
}

/// `1 h 23 min`, `23 min 5 s`.
pub fn fmt_total(ms: u64) -> String {
    let s = ms / 1000;
    match (s / 3600, s / 60 % 60, s % 60) {
        (0, 0, sec) => format!("{sec} s"),
        (0, m, sec) => format!("{m} min {sec} s"),
        (h, m, _) => format!("{h} h {m} min"),
    }
}

struct Cached {
    collection: Collection,
    rows: Vec<TrackRow>,
    next: Option<u32>,
    scroll: f64,
    extra: Vec<Collection>,
    at: Instant,
}

/// Cached lists older than this are reloaded when revisited.
const CACHE_TTL: Duration = Duration::from_secs(10 * 60);
/// Long playlists load completely (for counts, search and sorting) up to this many songs.
pub const MAX_ROWS: usize = 20_000;
/// Only the rows around what is shown are built, about this many at a time: thousands of rows
/// would freeze the window for seconds and take hundreds of MB. Empty space above and below
/// stands in for the rest, so the scrollbar still covers the whole list.
const CHUNK: usize = 200;
/// Rows kept built ahead of what is shown, in each direction.
const MARGIN: usize = 60;
/// At most this many rows stay built; the ones furthest from the view go first.
const MAX_BUILT: usize = 3 * CHUNK;
/// Height of a row until one has been measured.
const ROW_HEIGHT_GUESS: f64 = 46.0;

#[derive(Default)]
pub struct ViewState {
    pub collection: Option<Collection>,
    pub rows: Vec<TrackRow>,
    /// Sort and in-list search applied: position in the list → index into `rows`. Only
    /// `display[first..first + rendered]` have a row in the table.
    display: Vec<usize>,
    first: usize,
    rendered: usize,
    /// Measured height of one table row (0: not yet).
    row_height: f64,
    pub next_offset: Option<u32>,
    pub loading: bool,
    pub editable: bool,
    playing_uri: Option<String>,
    sort: Sort,
    filter: String,
    /// Scroll position to restore once a reload has finished (after an edit).
    pending_scroll: Option<f64>,
    /// The built rows are about to follow the view (from an idle callback, never during layout).
    sync_queued: bool,
    /// Playlists/albums shown as cards (search results, artist albums).
    extra: Vec<Collection>,
    cache: HashMap<String, Cached>,
}

#[derive(Clone)]
pub struct CollectionView {
    pub root: gtk::Overlay,
    panel: Panel,
    cover: gtk::Box,
    title: gtk::Label,
    subtitle: gtk::Label,
    description: gtk::Label,
    filter: gtk::SearchEntry,
    cards_title: gtk::Label,
    cards: gtk::FlowBox,
    songs_title: gtk::Label,
    headers: Rc<Vec<(SortKey, gtk::Button, &'static str)>>,
    table: gtk::ListBox,
    /// Stand-ins for the rows above and below the built ones.
    gap_above: gtk::Box,
    gap_below: gtk::Box,
    content: gtk::Box,
    status: gtk::Label,
    scroll: gtk::ScrolledWindow,
    pub state: Rc<RefCell<ViewState>>,
    app: App,
}

/// Column header with the same character width as the cells below it.
/// Natural width of the title column in pixels (it takes any extra room on wide windows).
const TITLE_NATURAL: i32 = 320;
/// Album column width in characters: narrowest, widest.
const ALBUM_CHARS: (i32, i32) = (14, 26);

/// Width of `n` average characters in the window font. Header and rows must agree exactly, so
/// this never depends on a particular widget's (smaller or larger) text style.
fn chars_px(n: i32) -> i32 {
    let m = gtk::Label::new(None).pango_context().metrics(None, None);
    n * m.approximate_char_width() / gtk::pango::SCALE
}

/// `width_chars..max_chars`: the column's width range (equal for fixed columns). Rows use the
/// same range, so header and cells shrink alike on narrow windows.
fn header_button(text: &str, width_chars: i32, max_chars: i32, xalign: f32) -> gtk::Button {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(xalign);
    if width_chars > 0 {
        l.set_width_chars(width_chars);
        l.set_max_width_chars(max_chars);
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
    }
    let b = gtk::Button::new();
    b.set_child(Some(&l));
    b.add_css_class("rt-th");
    b
}

impl CollectionView {
    pub fn new(app: &App) -> Self {
        let cover = hbox(0);
        let title = label("", &["rt-hero"]);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let subtitle = label("", &["rt-dim"]);
        subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let description = label("", &["rt-dim"]);
        description.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let meta = vbox(4);
        meta.set_valign(gtk::Align::End);
        meta.set_hexpand(true);
        for w in [&title, &description, &subtitle] {
            meta.append(w);
        }
        let header = hbox(22);
        header.append(&cover);
        header.append(&meta);

        let play = button("▶  play", "Play from the top");
        play.add_css_class("rt-primary");
        let shuffle = button("⇄ shuffle", "Shuffle play");
        shuffle.add_css_class("rt-chip");
        let filter = gtk::SearchEntry::new();
        filter.add_css_class("rt-filter");
        filter.set_placeholder_text(Some("search in list"));
        filter.set_tooltip_text(Some("Search in this list (Ctrl+F)"));
        filter.set_width_chars(12);
        filter.set_max_width_chars(26);
        let actions = hbox(10);
        actions.append(&play);
        actions.append(&shuffle);
        actions.append(&spacer());
        actions.append(&filter);

        let cards_title = label("", &["rt-section"]);
        let cards = gtk::FlowBox::new();
        cards.set_selection_mode(gtk::SelectionMode::None);
        cards.set_max_children_per_line(12);
        cards.set_column_spacing(14);
        cards.set_row_spacing(14);
        cards.set_homogeneous(true);
        cards.add_css_class("rt-menu-host");
        let songs_title = label("songs", &["rt-section"]);

        // Sortable column headers; widths match the row cells below.
        let head = hbox(12);
        head.add_css_class("rt-table-head");
        let num = header_button("#", 3, 3, 1.0);
        num.update_property(&[gtk::accessible::Property::Label("sort by playlist order")]);
        let thumb_gap = hbox(0);
        thumb_gap.set_size_request(36, -1);
        let t = header_button("title", 0, 0, 0.0);
        t.set_hexpand(true);
        let t_col = Clamp::natural(&t, TITLE_NATURAL);
        let album = header_button("album", ALBUM_CHARS.0, ALBUM_CHARS.1, 0.0);
        let album_col = Clamp::natural(&album, chars_px(ALBUM_CHARS.1));
        let added = header_button("added", 10, 10, 0.0);
        let time = header_button("time", 6, 6, 1.0);
        let plus_gap = hbox(0);
        plus_gap.set_size_request(26, -1);
        head.append(&num);
        head.append(&thumb_gap);
        head.append(&t_col);
        head.append(&album_col);
        head.append(&added);
        head.append(&time);
        head.append(&plus_gap);
        let headers = Rc::new(vec![
            (SortKey::Custom, num, "#"),
            (SortKey::Title, t, "title"),
            (SortKey::Album, album, "album"),
            (SortKey::Added, added, "added"),
            (SortKey::Duration, time, "time"),
        ]);

        let table = gtk::ListBox::new();
        table.add_css_class("rt-list");
        table.set_selection_mode(gtk::SelectionMode::Multiple);
        table.set_activate_on_single_click(false);
        table.add_css_class("rt-menu-host");
        tab_leaves(&table);
        let status = label("", &["rt-dim"]);
        status.set_margin_top(8);

        let content = vbox(14);
        content.set_margin_start(8);
        content.set_margin_end(8);
        content.set_margin_bottom(12);
        for w in [
            header.upcast_ref::<gtk::Widget>(),
            actions.upcast_ref(),
            cards_title.upcast_ref(),
            cards.upcast_ref(),
            songs_title.upcast_ref(),
        ] {
            content.append(w);
        }
        let gap_above = vbox(0);
        let gap_below = vbox(0);
        let tracks_box = vbox(0);
        tracks_box.append(&head);
        tracks_box.append(&gap_above);
        tracks_box.append(&table);
        tracks_box.append(&gap_below);
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
            title,
            subtitle,
            description,
            filter: filter.clone(),
            cards_title,
            cards,
            songs_title,
            headers,
            table: table.clone(),
            gap_above,
            gap_below,
            content: content.clone(),
            status,
            scroll: scroll.clone(),
            state: Rc::default(),
            app: app.clone(),
        };

        let v = this.clone();
        table.connect_row_activated(move |_, row| v.play_from_display(v.position(row), false));
        let v = this.clone();
        play.connect_clicked(move |_| v.play_from_display(0, false));
        let v = this.clone();
        shuffle.connect_clicked(move |_| v.play_from_display(0, true));
        let v = this.clone();
        filter.connect_search_changed(move |e| {
            v.state.borrow_mut().filter = e.text().trim().to_lowercase();
            v.render_rows();
        });
        // The built rows follow the view.
        let v = this.clone();
        let adj = scroll.vadjustment();
        let follow = move |_: &gtk::Adjustment| v.schedule_sync();
        adj.connect_value_changed(follow.clone());
        adj.connect_page_size_notify(follow);
        // Home and End go to the first and last song, not to the first and last row built.
        let v = this.clone();
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(move |_, key, _, _| {
            let to_end = match key {
                gdk::Key::End | gdk::Key::KP_End => true,
                gdk::Key::Home | gdk::Key::KP_Home => false,
                _ => return glib::Propagation::Proceed,
            };
            let built = {
                let st = v.state.borrow();
                if to_end {
                    st.first + st.rendered >= st.display.len()
                } else {
                    st.first == 0
                }
            };
            if built {
                return glib::Propagation::Proceed;
            }
            v.jump(to_end);
            glib::Propagation::Stop
        });
        table.add_controller(keys);
        for (key, b, _) in this.headers.iter() {
            let (v, key) = (this.clone(), *key);
            b.connect_clicked(move |_| {
                let sort = v.state.borrow().sort.click(key);
                v.set_sort(sort, true);
            });
        }

        // Right-click: song menu for the selection (the clicked row joins/replaces it).
        let v = this.clone();
        on_right_click(&table, move |_, y| {
            let row = v.table.row_at_y(y as i32)?;
            if !row.is_selected() {
                v.table.unselect_all();
                v.table.select_row(Some(&row));
            }
            Some(v.track_menu())
        });
        // Middle-click queues the song under the pointer.
        let middle = gtk::GestureClick::new();
        middle.set_button(2);
        let v = this.clone();
        middle.connect_pressed(move |_, _, _, y| {
            if let Some(t) = v.table.row_at_y(y as i32).and_then(|r| v.track_at(v.position(&r))) {
                v.app.queue_uris(vec![t.uri]);
            }
        });
        table.add_controller(middle);
        // Delete removes the selection from an editable playlist.
        let keys = gtk::EventControllerKey::new();
        let v = this.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Delete && v.state.borrow().editable {
                v.remove_selected();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        table.add_controller(keys);
        // Drag the selection (or the row under the pointer) onto a playlist in the sidebar.
        let drag = gtk::DragSource::new();
        drag.set_actions(gdk::DragAction::COPY);
        let v = this.clone();
        drag.connect_prepare(move |src, _, y| {
            let row = v.table.row_at_y(y as i32)?;
            if !row.is_selected() {
                v.table.unselect_all();
                v.table.select_row(Some(&row));
            }
            let uris: Vec<String> = v.selected_tracks().into_iter().map(|t| t.uri).collect();
            if uris.is_empty() {
                return None;
            }
            let n = uris.len();
            let tag = gtk::Label::new(Some(&if n == 1 { "1 song".to_owned() } else { format!("{n} songs") }));
            tag.add_css_class("rt-drag-tag");
            src.set_icon(Some(&gtk::WidgetPaintable::new(Some(&tag))), 0, 0);
            Some(gdk::ContentProvider::for_value(&uris.join("\n").to_value()))
        });
        table.add_controller(drag);

        let a = app.clone();
        scroll.connect_edge_reached(move |_, pos| {
            if pos == gtk::PositionType::Bottom {
                a.load_more_tracks();
            }
        });
        this.set_cards(None, Vec::new());
        this
    }

    /// Position in the list (index into `display`) of a built row.
    fn position(&self, row: &gtk::ListBoxRow) -> usize {
        self.state.borrow().first + row.index().max(0) as usize
    }

    fn track_at(&self, display_index: usize) -> Option<Track> {
        let st = self.state.borrow();
        st.display.get(display_index).and_then(|&i| st.rows.get(i)).map(|r| r.track.clone())
    }

    /// Selected tracks in display order.
    pub fn selected_tracks(&self) -> Vec<Track> {
        let mut idx: Vec<usize> = self.table.selected_rows().iter().map(|r| self.position(r)).collect();
        idx.sort_unstable();
        idx.into_iter().filter_map(|i| self.track_at(i)).collect()
    }

    /// Visible tracks in display order (sort and in-list search applied).
    fn visible_tracks(&self) -> Vec<Track> {
        let st = self.state.borrow();
        st.display.iter().filter_map(|&i| st.rows.get(i)).map(|r| r.track.clone()).collect()
    }

    fn play_from_display(&self, display_index: usize, shuffle: bool) {
        let (uri, custom, filtered) = {
            let st = self.state.borrow();
            let Some(c) = &st.collection else { return };
            (c.uri.clone(), st.sort.key == SortKey::Custom, !st.filter.is_empty())
        };
        let start = self.track_at(display_index);
        let visible = self.visible_tracks();
        let index = start
            .as_ref()
            .and_then(|s| visible.iter().position(|t| t.uri == s.uri))
            .unwrap_or(0);
        if shuffle {
            self.app.set_shuffle(true);
        }
        // Sorted or filtered views play exactly what is shown; the playlist order plays as context.
        self.app.play_from(&uri, &visible, index, custom && !filtered);
    }

    fn remove_selected(&self) {
        let Some(c) = self.state.borrow().collection.clone() else { return };
        let sel: Vec<String> = self.selected_tracks().into_iter().map(|t| t.uri).collect();
        if !sel.is_empty() {
            self.app.remove_from_playlist(&c, sel);
        }
    }

    fn track_menu(&self) -> Menu {
        let tracks = self.selected_tracks();
        let uris: Vec<String> = tracks.iter().map(|t| t.uri.clone()).collect();
        let (collection, editable) = {
            let st = self.state.borrow();
            (st.collection.clone(), st.editable)
        };
        let menu = Menu::new();
        let a = self.app.clone();
        let n = uris.len();
        if n == 1 {
            let v = self.clone();
            let i = self.table.selected_rows().first().map(|r| self.position(r)).unwrap_or(0);
            menu.item("play", move || v.play_from_display(i, false));
        }
        let (a2, u) = (a.clone(), uris.clone());
        menu.item(if n == 1 { "add to queue" } else { "add to queue (all)" }, move || {
            a2.queue_uris(u.clone())
        });
        if let Some(last) = a.last_playlist() {
            let (a2, u, l) = (a.clone(), uris.clone(), last.clone());
            menu.item(&format!("add to “{}”", last.name), move || a2.add_to_playlist(&l, u.clone()));
        }
        let entries = a
            .editable_playlists()
            .into_iter()
            .map(|p| {
                let (a2, u) = (a.clone(), uris.clone());
                let name = p.name.clone();
                (name, Rc::new(move || a2.add_to_playlist(&p, u.clone())) as Rc<dyn Fn()>)
            })
            .collect();
        menu.submenu("add to playlist", entries);
        if editable && let Some(c) = collection.clone() {
            let (a2, u) = (a.clone(), uris.clone());
            menu.item("remove from this playlist", move || a2.remove_from_playlist(&c, u.clone()));
        }
        let (a2, u) = (a.clone(), uris.clone());
        if collection.as_ref().is_some_and(|c| c.uri == LIKED_URI) {
            menu.item("remove from liked songs", move || a2.set_saved_many(u.clone(), false));
        } else {
            menu.item("save to liked songs", move || a2.set_saved_many(u.clone(), true));
        }
        menu.separator();
        if let [t] = tracks.as_slice() {
            if let Some(album) = t.album.clone().filter(|al| !al.uri.is_empty()) {
                let a2 = a.clone();
                menu.item("go to album", move || a2.open_uri_view(&album.uri, &album.name));
            }
            for artist in t.artists.iter().filter(|ar| !ar.uri.is_empty()).take(3) {
                let (a2, ar) = (a.clone(), artist.clone());
                menu.item(&format!("go to {}", artist.name), move || a2.open_uri_view(&ar.uri, &ar.name));
            }
        }
        let (a2, u) = (a.clone(), uris.clone());
        menu.item(if n == 1 { "copy link" } else { "copy links" }, move || a2.copy_links(&u));
        menu
    }

    fn set_cards(&self, title: Option<&str>, items: Vec<Collection>) {
        self.cards_title.set_visible(title.is_some());
        self.cards.set_visible(title.is_some());
        self.songs_title.set_visible(title.is_some());
        if let Some(t) = title {
            self.cards_title.set_text(&panel_title(t));
        }
        while let Some(w) = self.cards.child_at_index(0) {
            self.cards.remove(&w);
        }
        for col in &items {
            let card = vbox(6);
            card.append(&cover_widget(&self.app, col, 132));
            let name = ellipsized(&col.name, &["rt-row-title"]);
            name.set_max_width_chars(16);
            card.append(&name);
            let sub = ellipsized(&col.subtitle, &["rt-dim", "rt-small"]);
            sub.set_max_width_chars(16);
            card.append(&sub);
            let b = gtk::Button::new();
            b.add_css_class("flat");
            b.set_child(Some(&card));
            let (a, c) = (self.app.clone(), col.clone());
            b.connect_clicked(move |_| a.open_collection(c.clone(), true));
            let (a, c) = (self.app.clone(), col.clone());
            on_right_click(&b, move |_, _| Some(a.collection_menu(&c)));
            self.cards.insert(&b, -1);
        }
        self.state.borrow_mut().extra = items;
    }

    /// Remember the current list and its scroll position before switching away.
    fn stash(&self) {
        let scroll = self.scroll.vadjustment().value();
        let mut st = self.state.borrow_mut();
        if let Some(c) = st.collection.clone()
            && c.uri != SEARCH_URI
            && !st.rows.is_empty()
        {
            let cached = Cached {
                collection: c.clone(),
                rows: st.rows.clone(),
                next: st.next_offset,
                scroll,
                extra: st.extra.clone(),
                at: Instant::now(),
            };
            st.cache.insert(c.uri, cached);
        }
    }

    pub fn scroll_value(&self) -> f64 {
        self.scroll.vadjustment().value()
    }

    /// After the reload that follows an edit, put the list back where it was.
    pub fn set_scroll_after_load(&self, y: f64) {
        self.state.borrow_mut().pending_scroll = Some(y);
    }

    pub fn invalidate(&self, uri: &str) {
        self.state.borrow_mut().cache.remove(uri);
    }

    fn set_header(&self, c: &Collection, editable: bool) {
        let sort = Sort::parse(
            self.app
                .config
                .borrow()
                .config
                .library
                .track_sort
                .get(&c.uri)
                .map(String::as_str)
                .unwrap_or("custom"),
        );
        {
            let mut st = self.state.borrow_mut();
            st.collection = Some(c.clone());
            st.rows.clear();
            st.display.clear();
            st.first = 0;
            st.rendered = 0;
            st.next_offset = None;
            st.loading = true;
            st.editable = editable;
            st.sort = sort;
        }
        self.panel.set_title(match c.kind.as_str() {
            "liked" => "playlist",
            k => k,
        });
        self.title.set_text(&c.name);
        self.subtitle.set_text(&c.subtitle);
        self.description.set_text(&c.description);
        self.description.set_visible(!c.description.is_empty());
        while let Some(w) = self.cover.first_child() {
            self.cover.remove(&w);
        }
        self.cover.append(&cover_widget(&self.app, c, 168));
        self.filter.set_text("");
        self.state.borrow_mut().filter.clear();
        self.update_headers();
        clear_list(&self.table);
        self.update_gaps();
    }

    /// Switch to `c` and clear the table; tracks arrive through `append`.
    pub fn show(&self, c: &Collection, editable: bool) {
        self.stash();
        self.set_header(c, editable);
        self.set_cards(None, Vec::new());
        self.status.set_text("loading…");
        self.scroll.vadjustment().set_value(0.0);
    }

    /// Show `c` from the cache (rows and scroll position) if it is fresh. Returns false otherwise.
    pub fn restore(&self, c: &Collection, editable: bool) -> bool {
        self.stash();
        let cached = {
            let mut st = self.state.borrow_mut();
            match st.cache.remove(&c.uri) {
                Some(x) if x.at.elapsed() < CACHE_TTL => Some(x),
                _ => None,
            }
        };
        let Some(cached) = cached else { return false };
        self.set_header(&cached.collection, editable);
        let extra = cached.extra.clone();
        if !extra.is_empty() {
            self.set_cards(Some("albums"), extra);
        } else {
            self.set_cards(None, Vec::new());
        }
        self.append(&c.uri, cached.rows, cached.next);
        self.scroll_to(cached.scroll);
        true
    }

    pub fn show_search(&self, query: &str, collections: Vec<Collection>, tracks: Vec<Track>) {
        let c = Collection {
            uri: SEARCH_URI.into(),
            name: format!("“{query}”"),
            subtitle: String::new(),
            kind: "search".into(),
            ..Default::default()
        };
        self.show(&c, false);
        self.set_cards(Some("playlists & albums"), collections);
        self.append(SEARCH_URI, tracks.into_iter().map(TrackRow::new).collect(), None);
    }

    /// Artist page: albums as cards, songs below.
    pub fn show_artist_albums(&self, uri: &str, albums: Vec<Collection>) {
        if self.uri().as_deref() == Some(uri) {
            self.set_cards(Some("albums"), albums);
        }
    }

    pub fn uri(&self) -> Option<String> {
        self.state.borrow().collection.as_ref().map(|c| c.uri.clone())
    }

    fn update_headers(&self) {
        let sort = self.state.borrow().sort;
        for (key, b, text) in self.headers.iter() {
            let arrow = if *key == sort.key && sort.key != SortKey::Custom {
                if sort.desc { " ▼" } else { " ▲" }
            } else {
                ""
            };
            if let Some(l) = b.child().and_downcast::<gtk::Label>() {
                l.set_text(&format!("{text}{arrow}"));
            }
            if *key == sort.key {
                b.add_css_class("rt-on");
            } else {
                b.remove_css_class("rt-on");
            }
        }
    }

    fn set_sort(&self, sort: Sort, persist: bool) {
        let uri = {
            let mut st = self.state.borrow_mut();
            st.sort = sort;
            st.collection.as_ref().map(|c| c.uri.clone())
        };
        if persist && let Some(uri) = uri.filter(|u| u != SEARCH_URI) {
            let value = sort.to_config();
            self.app.update_config(|c| {
                if value == "custom" {
                    c.library.track_sort.remove(&uri);
                } else {
                    c.library.track_sort.insert(uri.clone(), value.clone());
                }
            });
        }
        self.update_headers();
        self.render_rows();
    }

    fn make_row(&self, number: usize, r: &TrackRow, show_added: bool, playing: Option<&str>) -> gtk::ListBoxRow {
        let t = &r.track;
        let row = gtk::ListBoxRow::new();
        // One sentence for screen readers instead of an unnamed row.
        row.update_property(&[gtk::accessible::Property::Label(&format!(
            "{}, {}, {}",
            t.name,
            t.artist_names(),
            fmt_time(t.duration_ms)
        ))]);
        let b = hbox(12);
        let num = label(&number.to_string(), &["rt-faint", "rt-mono-num", "rt-row-num", "rt-small"]);
        num.set_width_chars(3);
        num.set_xalign(1.0);
        b.append(&num);
        let pic = cover_picture(36);
        if let Some(url) = pick_image(t.images(), 60).map(|im| im.url.clone()) {
            let p = pic.clone();
            self.app.load_image(&url, move |tex| p.set_paintable(tex.as_ref()));
        }
        b.append(&pic);
        let meta = vbox(0);
        meta.set_hexpand(true);
        meta.set_valign(gtk::Align::Center);
        meta.append(&ellipsized(&t.name, &["rt-row-title"]));
        meta.append(&ellipsized(&t.artist_names(), &["rt-dim", "rt-small"]));
        // Same natural width as the header's title column, so every row shrinks alike.
        b.append(&Clamp::natural(&meta, TITLE_NATURAL));
        let album = ellipsized(t.album_name(), &["rt-dim", "rt-small"]);
        album.set_width_chars(ALBUM_CHARS.0);
        album.set_max_width_chars(ALBUM_CHARS.1);
        b.append(&Clamp::natural(&album, chars_px(ALBUM_CHARS.1)));
        let added = label(
            if show_added { short_date(&r.added_at) } else { "" },
            &["rt-faint", "rt-small", "rt-mono-num"],
        );
        added.set_width_chars(10);
        b.append(&added);
        let time = label(&fmt_time(t.duration_ms), &["rt-faint", "rt-mono-num", "rt-small"]);
        time.set_width_chars(6);
        time.set_xalign(1.0);
        b.append(&time);
        // One-click add to the last-used playlist (shown on hover).
        let plus = gtk::Button::with_label("+");
        plus.add_css_class("rt-hover");
        plus.add_css_class("rt-icon");
        plus.set_valign(gtk::Align::Center);
        // Same width as the header's spacer so the columns line up.
        plus.set_size_request(26, -1);
        let plus_tip = match self.app.last_playlist() {
            Some(p) => format!("Add to “{}”", p.name),
            None => "Add to playlist".to_owned(),
        };
        plus.set_tooltip_text(Some(&plus_tip));
        name_symbol_button(&plus, "+", &plus_tip);
        let (a, uri, v, rw) = (self.app.clone(), t.uri.clone(), self.clone(), row.clone());
        plus.connect_clicked(move |btn| match a.last_playlist() {
            Some(p) => a.add_to_playlist(&p, vec![uri.clone()]),
            None => {
                v.table.unselect_all();
                v.table.select_row(Some(&rw));
                let (x, y) = (btn.width() as f64 / 2.0, btn.height() as f64);
                v.track_menu().popup_at(btn, x, y);
            }
        });
        b.append(&plus);
        row.set_child(Some(&b));
        if playing == Some(t.uri.as_str()) {
            row.add_css_class("rt-playing");
        }
        row
    }

    /// Rebuild the table from `rows`: sort, in-list search, then the rows around the view.
    fn render_rows(&self) {
        let selected: Vec<String> = self.selected_tracks().into_iter().map(|t| t.uri).collect();
        {
            let mut st = self.state.borrow_mut();
            let order = sorted_order(&st.rows, st.sort);
            st.display = order.into_iter().filter(|&i| matches_filter(&st.rows[i], &st.filter)).collect();
            st.first = 0;
            st.rendered = 0;
        }
        clear_list(&self.table);
        self.sync_window();
        if !selected.is_empty() {
            let st = self.state.borrow();
            for (n, &i) in st.display[st.first..st.first + st.rendered].iter().enumerate() {
                if selected.contains(&st.rows[i].track.uri)
                    && let Some(row) = self.table.row_at_index(n as i32)
                {
                    self.table.select_row(Some(&row));
                }
            }
        }
        self.update_status();
    }

    /// Rows all have the same height: measured once, from two laid-out rows.
    fn row_height(&self) -> f64 {
        let known = self.state.borrow().row_height;
        if known > 0.0 {
            return known;
        }
        let (Some(a), Some(b)) = (self.table.row_at_index(0), self.table.row_at_index(1)) else {
            return ROW_HEIGHT_GUESS;
        };
        let origin = gtk::graphene::Point::new(0.0, 0.0);
        let pitch = match (a.compute_point(&self.table, &origin), b.compute_point(&self.table, &origin)) {
            (Some(pa), Some(pb)) if a.height() > 0 => (pb.y() - pa.y()) as f64,
            _ => 0.0,
        };
        if pitch < 10.0 {
            return ROW_HEIGHT_GUESS;
        }
        self.state.borrow_mut().row_height = pitch;
        pitch
    }

    /// Size the stand-ins for the rows that are not built.
    fn update_gaps(&self) {
        let h = self.row_height();
        let (above, below) = {
            let st = self.state.borrow();
            (st.first, st.display.len() - st.first - st.rendered)
        };
        self.gap_above.set_size_request(-1, (above as f64 * h).round() as i32);
        self.gap_below.set_size_request(-1, (below as f64 * h).round() as i32);
    }

    /// Positions in the list that are on screen now.
    fn shown_range(&self) -> (usize, usize) {
        let h = self.row_height();
        let adj = self.scroll.vadjustment();
        let top = self
            .gap_above
            .compute_point(&self.content, &gtk::graphene::Point::new(0.0, 0.0))
            .map_or(0.0, |p| p.y() as f64);
        let from = ((adj.value() - top) / h).floor().max(0.0) as usize;
        let to = ((adj.value() + adj.page_size() - top) / h).ceil().max(0.0) as usize;
        (from, to.max(from + 1))
    }

    fn build(&self, from: usize, to: usize) -> Vec<gtk::ListBoxRow> {
        let (rows, show_added, playing) = {
            let st = self.state.borrow();
            let rows: Vec<TrackRow> = st.display[from..to].iter().map(|&i| st.rows[i].clone()).collect();
            (rows, st.rows.iter().any(|r| !r.added_at.is_empty()), st.playing_uri.clone())
        };
        rows.iter()
            .enumerate()
            .map(|(k, r)| self.make_row(from + k + 1, r, show_added, playing.as_deref()))
            .collect()
    }

    /// Build the rows around the view and drop the ones far from it.
    fn sync_window(&self) {
        self.state.borrow_mut().sync_queued = false;
        let len = self.state.borrow().display.len();
        let (first, rendered) = {
            let st = self.state.borrow();
            (st.first, st.rendered)
        };
        if len == 0 {
            self.update_gaps();
            return;
        }
        let (shown_from, shown_to) = self.shown_range();
        let shown_from = shown_from.min(len - 1);
        let want_from = shown_from.saturating_sub(MARGIN);
        let want_to = (shown_to + MARGIN).min(len);
        let end = first + rendered;
        if rendered > 0 && want_from >= first && want_to <= end {
            return;
        }
        if rendered == 0 || want_from >= end || want_to <= first {
            // Nothing built near the view (a jump, or a new list): start over there.
            let had_focus = self.table.focus_child().is_some();
            clear_list(&self.table);
            let from = want_from.min(len.saturating_sub(CHUNK));
            let to = (from + CHUNK.max(want_to - from)).min(len);
            for row in self.build(from, to) {
                self.table.append(&row);
            }
            {
                let mut st = self.state.borrow_mut();
                st.first = from;
                st.rendered = to - from;
            }
            // The focused row is gone: keys would go nowhere until the next click.
            if had_focus {
                self.focus_row_in_place(shown_from.clamp(from, to - 1) - from);
            }
        } else {
            if want_to > end {
                let to = (end + CHUNK).max(want_to).min(len);
                for row in self.build(end, to) {
                    self.table.append(&row);
                }
                self.state.borrow_mut().rendered += to - end;
            }
            if want_from < first {
                let from = first.saturating_sub(CHUNK).min(want_from);
                for row in self.build(from, first).into_iter().rev() {
                    self.table.prepend(&row);
                }
                let mut st = self.state.borrow_mut();
                st.first = from;
                st.rendered += first - from;
            }
            // Drop rows far from the view, the side away from it first.
            let (first, rendered) = {
                let st = self.state.borrow();
                (st.first, st.rendered)
            };
            if rendered > MAX_BUILT {
                let keep_from = want_from.saturating_sub(CHUNK / 2).max(first);
                let keep_to = (want_to + CHUNK / 2).min(first + rendered);
                for i in (keep_to - first..rendered).rev() {
                    if let Some(row) = self.table.row_at_index(i as i32) {
                        self.table.remove(&row);
                    }
                }
                for _ in first..keep_from {
                    if let Some(row) = self.table.row_at_index(0) {
                        self.table.remove(&row);
                    }
                }
                let mut st = self.state.borrow_mut();
                st.first = keep_from;
                st.rendered = keep_to - keep_from;
            }
        }
        self.update_gaps();
    }

    fn schedule_sync(&self) {
        {
            let mut st = self.state.borrow_mut();
            if st.sync_queued {
                return;
            }
            st.sync_queued = true;
        }
        let v = self.clone();
        glib::idle_add_local_once(move || v.sync_window());
    }

    /// Home/End: scroll to the top or bottom of the whole list and focus its first or last song.
    fn jump(&self, to_end: bool) {
        let adj = self.scroll.vadjustment();
        adj.set_value(if to_end { adj.upper() } else { 0.0 });
        self.sync_window();
        let (first, rendered, len) = {
            let st = self.state.borrow();
            (st.first, st.rendered, st.display.len())
        };
        let index = if to_end {
            (len - 1).checked_sub(first)
        } else {
            0usize.checked_sub(first)
        };
        if let Some(index) = index.filter(|&i| i < rendered) {
            self.focus_row_in_place(index);
        }
        // Once the new rows have their size, the bottom is where it will stay.
        let v = self.clone();
        let frames = std::cell::Cell::new(0);
        self.table.add_tick_callback(move |_, _| {
            frames.set(frames.get() + 1);
            if frames.get() < 2 {
                return glib::ControlFlow::Continue;
            }
            let adj = v.scroll.vadjustment();
            adj.set_value(if to_end { adj.upper() } else { 0.0 });
            glib::ControlFlow::Break
        });
    }

    /// Focus a built row without scrolling to it: a row that was just built has no size yet and
    /// counts as being at the top of the list.
    fn focus_row_in_place(&self, index: usize) {
        let Some(row) = self.table.row_at_index(index as i32) else { return };
        let viewport = self.scroll.child().and_downcast::<gtk::Viewport>();
        if let Some(vp) = &viewport {
            vp.set_scroll_to_focus(false);
        }
        row.grab_focus();
        if let Some(vp) = &viewport {
            vp.set_scroll_to_focus(true);
        }
    }

    /// Scroll to `y` once the list is laid out (rows built there by `sync_window`).
    fn scroll_to(&self, y: f64) {
        self.update_gaps();
        let adj = self.scroll.vadjustment();
        glib::idle_add_local_once(move || adj.set_value(y));
    }

    /// Append a page of rows for `uri` (ignored when the user already moved on).
    pub fn append(&self, uri: &str, rows: Vec<TrackRow>, next: Option<u32>) {
        if self.uri().as_deref() != Some(uri) {
            return;
        }
        let custom = self.state.borrow().sort.key == SortKey::Custom;
        let start = {
            let mut st = self.state.borrow_mut();
            let start = st.rows.len();
            st.rows.extend(rows);
            st.next_offset = next.filter(|_| st.rows.len() < MAX_ROWS);
            st.loading = false;
            start
        };
        let pending = {
            let mut st = self.state.borrow_mut();
            if st.next_offset.is_none() { st.pending_scroll.take() } else { None }
        };
        let done = self.state.borrow().next_offset.is_none();
        if custom && start > 0 {
            // Playlist order: the new page goes below, built when it scrolls into view.
            {
                let mut st = self.state.borrow_mut();
                let fresh: Vec<usize> = (start..st.rows.len())
                    .filter(|&i| matches_filter(&st.rows[i], &st.filter))
                    .collect();
                st.display.extend(fresh);
            }
            self.update_gaps();
            self.schedule_sync();
            self.update_status();
        } else if start == 0 || done {
            // Sorted lists are rebuilt on the first page and once everything is there.
            self.render_rows();
        } else {
            self.update_status();
        }
        if let Some(y) = pending {
            self.scroll_to(y);
        }
    }

    fn update_status(&self) {
        let st = self.state.borrow();
        let Some(c) = &st.collection else { return };
        let total: u64 = st.rows.iter().map(|r| r.track.duration_ms).sum();
        let n = st.rows.len();
        let more = if st.next_offset.is_some() { "+" } else { "" };
        let counts = format!("{n}{more} songs · {}", fmt_total(total));
        let owner = c.meta.as_str();
        self.subtitle.set_text(&if owner.is_empty() {
            counts.clone()
        } else {
            format!("{owner} · {counts}")
        });
        let shown = if st.filter.is_empty() { n } else { st.display.len() };
        self.status.set_text(&match (n, shown, st.next_offset.is_some()) {
            (0, _, _) if !st.loading => "no songs".to_owned(),
            (_, 0, _) => "no matches".to_owned(),
            (_, _, true) => "loading more…".to_owned(),
            _ => String::new(),
        });
    }

    pub fn set_error(&self, msg: &str) {
        self.state.borrow_mut().loading = false;
        self.status.set_text(msg);
    }

    pub fn focus_filter(&self) {
        self.filter.grab_focus();
    }

    pub fn mark_playing(&self, uri: Option<&str>) {
        let uris: Vec<String> = {
            let mut st = self.state.borrow_mut();
            st.playing_uri = uri.map(String::from);
            st.display[st.first..st.first + st.rendered]
                .iter()
                .filter_map(|&i| st.rows.get(i))
                .map(|r| r.track.uri.clone())
                .collect()
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

    /// Drop rows from the shown list after a successful removal (and from its cache).
    pub fn remove_uris(&self, playlist_uri: &str, uris: &[String]) {
        self.invalidate(playlist_uri);
        if self.uri().as_deref() != Some(playlist_uri) {
            return;
        }
        self.state.borrow_mut().rows.retain(|r| !uris.contains(&r.track.uri));
        self.render_rows();
    }

    /// Positions (in the playlist's own order) of each uri, for undo.
    pub fn positions_of(&self, playlist_uri: &str, uris: &[String]) -> Vec<(String, u32)> {
        let st = self.state.borrow();
        let rows: &[TrackRow] = if st.collection.as_ref().is_some_and(|c| c.uri == playlist_uri) {
            &st.rows
        } else {
            match st.cache.get(playlist_uri) {
                Some(c) => &c.rows,
                None => return Vec::new(),
            }
        };
        rows.iter()
            .enumerate()
            .filter(|(_, r)| uris.contains(&r.track.uri))
            .map(|(i, r)| (r.track.uri.clone(), i as u32))
            .collect()
    }

    /// URIs already in a loaded playlist (to skip duplicates when adding).
    pub fn known_uris(&self, playlist_uri: &str) -> Option<Vec<String>> {
        let st = self.state.borrow();
        if st.collection.as_ref().is_some_and(|c| c.uri == playlist_uri) && st.next_offset.is_none() && !st.loading {
            return Some(st.rows.iter().map(|r| r.track.uri.clone()).collect());
        }
        st.cache
            .get(playlist_uri)
            .filter(|c| c.next.is_none())
            .map(|c| c.rows.iter().map(|r| r.track.uri.clone()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, album: &str, added: &str, ms: u64) -> TrackRow {
        let mut t = Track {
            name: name.into(),
            duration_ms: ms,
            ..Default::default()
        };
        t.album = Some(crate::spotify::model::AlbumRef {
            name: album.into(),
            ..Default::default()
        });
        TrackRow {
            track: t,
            added_at: added.into(),
        }
    }

    #[test]
    fn dates_and_totals() {
        assert_eq!(short_date("2026-10-01T00:00:00Z"), "2026-10-01");
        assert_eq!(fmt_total(83_000), "1 min 23 s");
        assert_eq!(fmt_total(5_000_000), "1 h 23 min");
        assert_eq!(fmt_total(9_000), "9 s");
    }

    #[test]
    fn sort_cycle_and_persistence() {
        let s = Sort::default().click(SortKey::Title);
        assert_eq!(
            s,
            Sort {
                key: SortKey::Title,
                desc: false
            }
        );
        let s = s.click(SortKey::Title);
        assert!(s.desc);
        assert_eq!(s.click(SortKey::Title), Sort::default());
        assert_eq!(Sort::parse(&s.to_config()), s);
        assert_eq!(Sort::parse("custom"), Sort::default());
        assert_eq!(Sort::parse("bogus:desc"), Sort::default());
    }

    #[test]
    fn sorting_is_stable() {
        let rows = vec![
            row("b", "z", "2026-02", 3),
            row("a", "z", "2026-03", 1),
            row("c", "y", "2026-01", 2),
        ];
        assert_eq!(
            sorted_order(
                &rows,
                Sort {
                    key: SortKey::Title,
                    desc: false
                }
            ),
            [1, 0, 2]
        );
        assert_eq!(
            sorted_order(
                &rows,
                Sort {
                    key: SortKey::Album,
                    desc: false
                }
            ),
            [2, 0, 1]
        );
        assert_eq!(
            sorted_order(
                &rows,
                Sort {
                    key: SortKey::Added,
                    desc: true
                }
            ),
            [1, 0, 2]
        );
        assert_eq!(
            sorted_order(
                &rows,
                Sort {
                    key: SortKey::Duration,
                    desc: false
                }
            ),
            [1, 2, 0]
        );
        assert_eq!(sorted_order(&rows, Sort::default()), [0, 1, 2]);
    }

    #[test]
    fn filter_matches_title_artist_album() {
        let r = row("Neon Rain", "Midnight Circuits", "", 1);
        assert!(matches_filter(&r, "neon"));
        assert!(matches_filter(&r, "circuit"));
        assert!(!matches_filter(&r, "xyz"));
        assert!(matches_filter(&r, ""));
    }
}
