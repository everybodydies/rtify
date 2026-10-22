//! Library browser: collections on the left (playlists, albums, liked songs, search results),
//! tracks of the selected collection on the right. Tab, filter, sort and selection persist.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::{glib, prelude::*};

use super::widgets::*;
use crate::app::App;
use crate::spotify::model::{Album, Playlist, Track};

pub const TABS: [(&str, &str); 4] = [
    ("playlists", "playlists"),
    ("albums", "albums"),
    ("liked", "liked"),
    ("search", "search"),
];
pub const SORTS: [&str; 2] = ["recent", "a-z"];
pub const LIKED_URI: &str = "rtify:liked";
pub const SEARCH_URI: &str = "rtify:search";

#[derive(Debug, Clone, PartialEq)]
pub struct Collection {
    pub uri: String,
    pub name: String,
    pub subtitle: String,
}

impl Collection {
    pub fn from_playlist(p: &Playlist) -> Self {
        let owner = p.owner.display_name.clone().unwrap_or_else(|| p.owner.id.clone());
        Self {
            uri: p.uri.clone(),
            name: p.name.clone(),
            subtitle: format!("{} · {} tracks", owner, p.count()),
        }
    }

    pub fn from_album(a: &Album) -> Self {
        let artists = a.artists.iter().map(|x| x.name.as_str()).collect::<Vec<_>>().join(", ");
        Self {
            uri: a.uri.clone(),
            name: a.name.clone(),
            subtitle: artists,
        }
    }
}

/// Filter by case-insensitive substring and sort; "recent" keeps Spotify's order.
pub fn visible(items: &[Collection], filter: &str, sort: &str) -> Vec<Collection> {
    let f = filter.trim().to_lowercase();
    let mut out: Vec<Collection> = items
        .iter()
        .filter(|c| f.is_empty() || c.name.to_lowercase().contains(&f) || c.subtitle.to_lowercase().contains(&f))
        .cloned()
        .collect();
    if sort == "a-z" {
        out.sort_by_key(|c| c.name.to_lowercase());
    }
    out
}

#[derive(Default)]
pub struct LibState {
    pub tab: String,
    pub playlists: Vec<Collection>,
    pub albums: Vec<Collection>,
    pub search_collections: Vec<Collection>,
    pub search_tracks: Vec<Track>,
    pub shown: Vec<Collection>,
    /// Selected collection URI (playlist/album URI, `LIKED_URI` or `SEARCH_URI`).
    pub selection: Option<String>,
    pub tracks: Vec<Track>,
    pub next_offset: Option<u32>,
    pub loading: bool,
    pub playing_uri: Option<String>,
}

#[derive(Clone)]
pub struct LibraryPane {
    pub root: gtk::Paned,
    tabs: Vec<gtk::ToggleButton>,
    pub search: gtk::SearchEntry,
    filter: gtk::SearchEntry,
    sort: gtk::DropDown,
    list: gtk::ListBox,
    tracks_frame: Panel,
    tracks: gtk::ListBox,
    tracks_status: gtk::Label,
    pub state: Rc<RefCell<LibState>>,
}

impl LibraryPane {
    pub fn new(app: &App) -> Self {
        let cfg = app.config.borrow().config.library.clone();
        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("search spotify  (Ctrl+L)"));

        let tab_box = hbox(2);
        let mut tabs = Vec::new();
        for (id, title) in TABS {
            let b = toggle(title, "");
            b.add_css_class("rt-tab");
            if let Some(first) = tabs.first() {
                b.set_group(Some(first));
            }
            b.set_widget_name(id);
            tab_box.append(&b);
            tabs.push(b);
        }

        let filter = gtk::SearchEntry::new();
        filter.set_placeholder_text(Some("filter"));
        filter.set_hexpand(true);
        filter.set_text(&cfg.filter);
        let sort = gtk::DropDown::from_strings(&SORTS);
        sort.set_selected(SORTS.iter().position(|s| *s == cfg.sort).unwrap_or(0) as u32);
        let filter_row = hbox(6);
        filter_row.append(&filter);
        filter_row.append(&sort);

        let list = gtk::ListBox::new();
        list.add_css_class("rt-list");
        list.set_selection_mode(gtk::SelectionMode::Single);
        let list_scroll = gtk::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();

        let left = vbox(6);
        left.append(&search);
        left.append(&tab_box);
        left.append(&filter_row);
        left.append(&list_scroll);
        let left_panel = panel("library", &left);
        left_panel.root.set_size_request(260, -1);

        let tracks = gtk::ListBox::new();
        tracks.add_css_class("rt-list");
        tracks.set_activate_on_single_click(false);
        let tracks_status = label("", &["rt-faint"]);
        let tracks_box = vbox(4);
        tracks_box.append(&tracks);
        tracks_box.append(&tracks_status);
        let tracks_scroll = gtk::ScrolledWindow::builder()
            .child(&tracks_box)
            .vexpand(true)
            .hexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let tracks_frame = panel("tracks", &tracks_scroll);

        let root = gtk::Paned::new(gtk::Orientation::Horizontal);
        root.set_start_child(Some(&left_panel.root));
        root.set_end_child(Some(&tracks_frame.root));
        root.set_resize_start_child(false);
        root.set_shrink_start_child(false);
        root.set_position(300);
        root.set_wide_handle(false);

        let state = Rc::new(RefCell::new(LibState {
            tab: cfg.last_tab.clone(),
            ..Default::default()
        }));
        let this = Self {
            root,
            tabs,
            search,
            filter,
            sort,
            list,
            tracks_frame,
            tracks,
            tracks_status,
            state,
        };

        for b in &this.tabs {
            let (pane, a) = (this.clone(), app.clone());
            b.connect_toggled(move |b| {
                if b.is_active() {
                    let tab = b.widget_name().to_string();
                    pane.state.borrow_mut().tab = tab.clone();
                    pane.rebuild_collections();
                    a.update_config(|c| c.library.last_tab = tab.clone());
                    if tab == "liked" {
                        pane.select(&a, LIKED_URI, "liked songs");
                    } else if tab == "search" {
                        pane.select(&a, SEARCH_URI, "search");
                    }
                }
            });
        }
        let (pane, a) = (this.clone(), app.clone());
        this.filter.connect_search_changed(move |f| {
            pane.rebuild_collections();
            let text = f.text().to_string();
            a.update_config(|c| c.library.filter = text.clone());
        });
        let (pane, a) = (this.clone(), app.clone());
        this.sort.connect_selected_notify(move |d| {
            pane.rebuild_collections();
            let sort = SORTS.get(d.selected() as usize).unwrap_or(&"recent").to_string();
            a.update_config(|c| c.library.sort = sort.clone());
        });
        let (pane, a) = (this.clone(), app.clone());
        this.list.connect_row_selected(move |_, row| {
            let Some(row) = row else { return };
            let c = pane.state.borrow().shown.get(row.index() as usize).cloned();
            if let Some(c) = c
                && pane.state.borrow().selection.as_deref() != Some(c.uri.as_str())
            {
                pane.select(&a, &c.uri, &c.name);
            }
        });
        let (pane, a) = (this.clone(), app.clone());
        this.tracks.connect_row_activated(move |_, row| {
            let idx = row.index() as usize;
            let (selection, tracks) = {
                let st = pane.state.borrow();
                (st.selection.clone(), st.tracks.clone())
            };
            if let Some(sel) = selection {
                a.play_from(&sel, &tracks, idx);
            }
        });
        let (pane, a) = (this.clone(), app.clone());
        this.search.connect_activate(move |e| {
            let q = e.text().trim().to_string();
            if !q.is_empty() {
                a.search(&q);
                pane.set_tab("search");
            }
        });
        let (pane, a) = (this.clone(), app.clone());
        tracks_scroll.connect_edge_reached(move |_, pos| {
            if pos == gtk::PositionType::Bottom {
                a.load_more_tracks(&pane);
            }
        });

        let tab = this.state.borrow().tab.clone();
        this.set_tab(&tab);
        this
    }

    pub fn set_tab(&self, tab: &str) {
        let tab = if TABS.iter().any(|(id, _)| *id == tab) { tab } else { "playlists" };
        for b in &self.tabs {
            if b.widget_name() == tab {
                b.set_active(true);
            }
        }
        self.state.borrow_mut().tab = tab.to_owned();
        self.rebuild_collections();
    }

    pub fn rebuild_collections(&self) {
        let filter = self.filter.text().to_string();
        let sort = SORTS.get(self.sort.selected() as usize).copied().unwrap_or("recent");
        let shown = {
            let st = self.state.borrow();
            let items: &[Collection] = match st.tab.as_str() {
                "albums" => &st.albums,
                "search" => &st.search_collections,
                "liked" => &[],
                _ => &st.playlists,
            };
            visible(items, &filter, sort)
        };
        clear_list(&self.list);
        let selection = self.state.borrow().selection.clone();
        for c in &shown {
            let row = gtk::ListBoxRow::new();
            let b = vbox(0);
            b.append(&ellipsized(&c.name, &["rt-row-title"]));
            b.append(&ellipsized(&c.subtitle, &["rt-faint"]));
            row.set_child(Some(&b));
            self.list.append(&row);
            if selection.as_deref() == Some(c.uri.as_str()) {
                self.list.select_row(Some(&row));
            }
        }
        self.state.borrow_mut().shown = shown;
    }

    pub fn set_playlists(&self, items: Vec<Collection>) {
        self.state.borrow_mut().playlists = items;
        self.rebuild_collections();
    }

    pub fn set_albums(&self, items: Vec<Collection>) {
        self.state.borrow_mut().albums = items;
        self.rebuild_collections();
    }

    pub fn set_search(&self, collections: Vec<Collection>, tracks: Vec<Track>) {
        {
            let mut st = self.state.borrow_mut();
            st.search_collections = collections;
            st.search_tracks = tracks.clone();
        }
        self.rebuild_collections();
        if self.state.borrow().selection.as_deref() == Some(SEARCH_URI) {
            self.set_tracks(SEARCH_URI, "search", tracks, None);
        }
    }

    /// Select a collection and load its tracks.
    pub fn select(&self, app: &App, uri: &str, name: &str) {
        {
            let mut st = self.state.borrow_mut();
            st.selection = Some(uri.to_owned());
            st.tracks.clear();
            st.next_offset = None;
        }
        self.tracks_frame.set_title(name);
        clear_list(&self.tracks);
        if uri == SEARCH_URI {
            let tracks = self.state.borrow().search_tracks.clone();
            self.set_tracks(uri, name, tracks, None);
        } else {
            self.tracks_status.set_text("loading…");
            app.load_tracks(self, uri, 0);
        }
        app.remember_selection(uri);
    }

    pub fn selection(&self) -> Option<String> {
        self.state.borrow().selection.clone()
    }

    /// Show `tracks` for `uri`, or append when `append_from` is the current length.
    pub fn set_tracks(&self, uri: &str, name: &str, tracks: Vec<Track>, next: Option<u32>) {
        if self.state.borrow().selection.as_deref() != Some(uri) {
            return; // the user moved on while this was loading
        }
        self.tracks_frame.set_title(name);
        let start = self.state.borrow().tracks.len();
        let playing = self.state.borrow().playing_uri.clone();
        for (i, t) in tracks.iter().enumerate() {
            let row = gtk::ListBoxRow::new();
            let b = hbox(10);
            b.append(&label(&format!("{:>3}", start + i + 1), &["rt-faint", "rt-mono-num"]));
            let meta = vbox(0);
            meta.set_hexpand(true);
            meta.append(&ellipsized(&t.name, &["rt-row-title"]));
            meta.append(&ellipsized(&t.artist_names(), &["rt-dim"]));
            b.append(&meta);
            let album = ellipsized(t.album_name(), &["rt-faint"]);
            album.set_width_chars(18);
            album.set_max_width_chars(28);
            b.append(&album);
            b.append(&label(&fmt_time(t.duration_ms), &["rt-faint", "rt-mono-num"]));
            row.set_child(Some(&b));
            if playing.as_deref() == Some(t.uri.as_str()) {
                row.add_css_class("rt-playing");
            }
            self.tracks.append(&row);
        }
        let mut st = self.state.borrow_mut();
        st.tracks.extend(tracks);
        st.next_offset = next;
        st.loading = false;
        self.tracks_status.set_text(match (st.tracks.is_empty(), next) {
            (true, _) => "no tracks",
            (false, Some(_)) => "scroll for more…",
            (false, None) => "",
        });
    }

    pub fn set_error(&self, msg: &str) {
        self.state.borrow_mut().loading = false;
        self.tracks_status.set_text(msg);
    }

    pub fn mark_playing(&self, uri: Option<&str>) {
        let uris: Vec<String> = {
            let mut st = self.state.borrow_mut();
            st.playing_uri = uri.map(String::from);
            st.tracks.iter().map(|t| t.uri.clone()).collect()
        };
        let mut row = self.tracks.first_child();
        let mut i = 0;
        while let Some(w) = row {
            if uris.get(i).map(String::as_str) == uri && uri.is_some() {
                w.add_css_class("rt-playing");
            } else {
                w.remove_css_class("rt-playing");
            }
            row = w.next_sibling();
            i += 1;
        }
    }

    pub fn focus_search(&self) {
        self.search.grab_focus();
        glib::idle_add_local_once({
            let s = self.search.clone();
            move || s.select_region(0, -1)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(name: &str) -> Collection {
        Collection {
            uri: format!("u:{name}"),
            name: name.into(),
            subtitle: "me".into(),
        }
    }

    #[test]
    fn filter_and_sort() {
        let items = vec![c("zeta"), c("Alpha"), c("beta mix")];
        let names = |v: Vec<Collection>| v.into_iter().map(|c| c.name).collect::<Vec<_>>();
        assert_eq!(names(visible(&items, "", "recent")), ["zeta", "Alpha", "beta mix"]);
        assert_eq!(names(visible(&items, "", "a-z")), ["Alpha", "beta mix", "zeta"]);
        assert_eq!(names(visible(&items, "MIX", "a-z")), ["beta mix"]);
    }
}
