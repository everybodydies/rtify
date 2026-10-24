//! "Your library" sidebar: one row of type chips (all · playlists · albums · artists), filter,
//! sort (recent, added, a-z, creator, custom; either direction) in the frame line, and a list
//! with cover thumbnails. Liked Songs are pinned on top like in the official client. A click
//! opens, a double-click plays. Rows can be dragged into a custom order; songs dragged from the
//! track table can be dropped on a playlist (or Liked Songs). Everything here persists.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::{gdk, prelude::*};

use super::menu::on_right_click;
use super::widgets::*;
use crate::app::App;
use crate::spotify::model::{Album, Artist, Image, Playlist, pick_image};

pub const TABS: [&str; 4] = ["all", "playlists", "albums", "artists"];
pub const SORTS: [&str; 5] = ["recent", "added", "a-z", "creator", "custom"];
/// Drag payload prefix for reordering the library (songs travel as plain `spotify:` lines).
const REORDER: &str = "rtify-library:";
pub const LIKED_URI: &str = "rtify:liked";
pub const SEARCH_URI: &str = "rtify:search";

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Collection {
    pub uri: String,
    pub name: String,
    /// One line for lists: owner/artist plus count where known.
    pub subtitle: String,
    /// Owner (playlists) or artists/year (albums), shown before the song count in headers.
    pub meta: String,
    /// "playlist", "album", "artist", "liked" or "search".
    pub kind: String,
    pub images: Vec<Image>,
    pub description: String,
    pub owner_id: String,
    pub collaborative: bool,
    /// Artist ids of an album (for new releases).
    pub artist_ids: Vec<String>,
    /// When it was saved to the library (albums; Spotify gives no date for playlists).
    pub added_at: String,
    /// Owner (playlists), artists (albums) or the artist's own name, for the "creator" sort.
    pub creator: String,
}

impl Collection {
    pub fn from_playlist(p: &Playlist) -> Self {
        let owner = p.owner.display_name.clone().unwrap_or_else(|| p.owner.id.clone());
        Self {
            uri: p.uri.clone(),
            name: p.name.clone(),
            subtitle: format!("{} · {} songs", owner, p.count()),
            meta: owner.clone(),
            creator: owner,
            kind: "playlist".into(),
            images: p.images.clone().unwrap_or_default(),
            description: p.description.clone().unwrap_or_default(),
            owner_id: p.owner.id.clone(),
            collaborative: p.collaborative,
            ..Default::default()
        }
    }

    pub fn from_album(a: &Album) -> Self {
        let artists = a.artists.iter().map(|x| x.name.as_str()).collect::<Vec<_>>().join(", ");
        let year = a.release_date.get(..4).unwrap_or("");
        let meta = if year.is_empty() {
            artists.clone()
        } else {
            format!("{artists} · {year}")
        };
        Self {
            uri: a.uri.clone(),
            name: a.name.clone(),
            subtitle: meta.clone(),
            meta,
            creator: artists,
            kind: "album".into(),
            images: a.images.clone(),
            artist_ids: a.artists.iter().filter_map(|x| x.id.clone()).collect(),
            ..Default::default()
        }
    }

    pub fn from_artist(a: &Artist) -> Self {
        Self {
            uri: a.uri.clone(),
            name: a.name.clone(),
            subtitle: "artist".into(),
            meta: a.genres.iter().take(2).cloned().collect::<Vec<_>>().join(", "),
            kind: "artist".into(),
            images: a.images.clone(),
            creator: a.name.clone(),
            ..Default::default()
        }
    }

    pub fn liked() -> Self {
        Self {
            uri: LIKED_URI.into(),
            name: "Liked Songs".into(),
            subtitle: "your saved songs".into(),
            kind: "liked".into(),
            ..Default::default()
        }
    }

    pub fn thumb_url(&self, min: u32) -> Option<String> {
        pick_image(&self.images, min).map(|i| i.url.clone())
    }
}

/// Filter by case-insensitive substring and sort. "recent" keeps Spotify's order; "added" puts
/// the newest saves first (items without a date keep their place after them); "custom" follows
/// `custom` (URIs), with anything not in it after, in Spotify's order. `reversed` flips the result.
pub fn visible(items: &[Collection], filter: &str, sort: &str, reversed: bool, custom: &[String]) -> Vec<Collection> {
    let f = filter.trim().to_lowercase();
    let mut out: Vec<Collection> = items
        .iter()
        .filter(|c| f.is_empty() || c.name.to_lowercase().contains(&f) || c.subtitle.to_lowercase().contains(&f))
        .cloned()
        .collect();
    // Stable sorts, so equal keys keep Spotify's order.
    match sort {
        "added" => out.sort_by(|a, b| match (a.added_at.is_empty(), b.added_at.is_empty()) {
            (false, false) => b.added_at.cmp(&a.added_at),
            (x, y) => x.cmp(&y),
        }),
        "a-z" => out.sort_by_key(|c| c.name.to_lowercase()),
        "creator" => out.sort_by_key(|c| (c.creator.to_lowercase(), c.name.to_lowercase())),
        "custom" => out.sort_by_key(|c| custom.iter().position(|u| *u == c.uri).unwrap_or(usize::MAX)),
        _ => {}
    }
    if reversed {
        out.reverse();
    }
    out
}

/// The custom order after dragging `moved` before (or after) `target`. Only the shown items
/// move; the rest of the stored order follows in its old order.
pub fn reordered(shown: &[String], stored: &[String], moved: &str, target: &str, after: bool) -> Vec<String> {
    let mut seq: Vec<String> = shown.iter().filter(|u| *u != moved).cloned().collect();
    let at = seq.iter().position(|u| u == target).map_or(seq.len(), |i| i + usize::from(after));
    seq.insert(at, moved.to_owned());
    let rest: Vec<String> = stored.iter().filter(|u| !seq.contains(u)).cloned().collect();
    seq.extend(rest);
    seq
}

/// A cover thumbnail, or a framed glyph when the collection has no image (liked songs).
pub fn cover_widget(app: &App, c: &Collection, size: i32) -> gtk::Widget {
    match c.thumb_url(size as u32) {
        Some(url) => {
            let pic = cover_picture(size);
            if c.kind == "artist" {
                pic.add_css_class("rt-artist");
            }
            let p = pic.clone();
            app.load_image(&url, move |t| p.set_paintable(t.as_ref()));
            square(&pic, size).upcast()
        }
        None => {
            let l = gtk::Label::new(Some(match c.kind.as_str() {
                "liked" => "♥",
                "artist" => "◎",
                _ => "♪",
            }));
            l.add_css_class("rt-cover-placeholder");
            l.set_size_request(size, size);
            l.set_valign(gtk::Align::Center);
            l.upcast()
        }
    }
}

#[derive(Default)]
struct SidebarState {
    tab: String,
    playlists: Vec<Collection>,
    albums: Vec<Collection>,
    artists: Vec<Collection>,
    shown: Vec<Collection>,
    selected: Option<String>,
    /// The collection playing now (marked with the accent bars).
    live: Option<String>,
    /// URI of a library row being dragged to a new place.
    dragging: Option<String>,
}

#[derive(Clone)]
pub struct Sidebar {
    pub root: gtk::Overlay,
    tabs: Vec<gtk::ToggleButton>,
    filter: gtk::Entry,
    sort: gtk::Button,
    reverse: gtk::ToggleButton,
    list: gtk::ListBox,
    state: Rc<RefCell<SidebarState>>,
    /// Set while the chips are switched from code, so their handlers stay quiet.
    updating: Rc<std::cell::Cell<bool>>,
    app: App,
}

impl Sidebar {
    pub fn new(app: &App) -> Self {
        let cfg = app.config.borrow().config.library.clone();
        // One joined row of chips; the active one takes the accent.
        let chips = hbox(0);
        chips.add_css_class("rt-segmented");
        chips.set_homogeneous(true);
        let mut tabs = Vec::new();
        for id in TABS {
            let b = toggle(id, "");
            b.add_css_class("rt-seg");
            b.set_widget_name(id);
            chips.append(&b);
            tabs.push(b);
        }

        let filter = gtk::Entry::new();
        filter.add_css_class("rt-filter");
        filter.set_placeholder_text(Some("filter"));
        filter.update_property(&[gtk::accessible::Property::Label("Filter your library")]);
        filter.set_text(&cfg.filter);
        filter.set_hexpand(true);

        let list = gtk::ListBox::new();
        list.add_css_class("rt-list");
        list.add_css_class("rt-menu-host");
        tab_leaves(&list);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();

        let body = vbox(8);
        body.append(&chips);
        body.append(&filter);
        body.append(&scroll);
        let p = panel("library", &body);
        // Sort and direction sit in the frame line.
        let sort_name = if SORTS.contains(&cfg.sort.as_str()) {
            cfg.sort.as_str()
        } else {
            "recent"
        };
        let sort = gtk::Button::with_label(sort_name);
        sort.set_tooltip_text(Some("Sort: recent, added, a-z, creator, custom"));
        let reverse = gtk::ToggleButton::with_label(if cfg.sort_reversed { "↑" } else { "↓" });
        reverse.set_active(cfg.sort_reversed);
        reverse.set_tooltip_text(Some("Reverse the order"));
        reverse.update_property(&[gtk::accessible::Property::Label("reverse order")]);
        reverse.reset_relation(gtk::AccessibleRelation::LabelledBy);
        p.add_frame_button(&sort);
        p.add_frame_button(&reverse);
        let root = p.root;
        root.set_size_request(240, -1);

        let tab = if TABS.contains(&cfg.last_tab.as_str()) {
            cfg.last_tab.clone()
        } else {
            "playlists".into()
        };
        let this = Self {
            root,
            tabs,
            filter,
            sort,
            reverse,
            list,
            state: Rc::new(RefCell::new(SidebarState {
                tab: tab.clone(),
                ..Default::default()
            })),
            updating: Rc::default(),
            app: app.clone(),
        };
        this.set_sort_label(sort_name);
        this.show_tab(&tab);
        for b in &this.tabs {
            let s = this.clone();
            b.connect_toggled(move |b| {
                if s.updating.get() {
                    return;
                }
                let name = b.widget_name().to_string();
                // Clicking the active chip again goes back to everything.
                let tab = if b.is_active() || name == "all" { name } else { "all".to_owned() };
                s.show_tab(&tab);
                s.rebuild();
                s.app.update_config(|c| c.library.last_tab = tab.clone());
            });
        }
        let s = this.clone();
        this.filter.connect_changed(move |f| {
            s.rebuild();
            let text = f.text().to_string();
            s.app.update_config(|c| c.library.filter = text.clone());
        });
        let s = this.clone();
        this.sort.connect_clicked(move |_| {
            let now = s.app.config.borrow().config.library.sort.clone();
            let i = SORTS.iter().position(|x| *x == now).map_or(0, |i| (i + 1) % SORTS.len());
            s.set_sort(SORTS[i], None);
        });
        let s = this.clone();
        this.reverse.connect_toggled(move |b| {
            if s.updating.get() {
                return;
            }
            let sort = s.app.config.borrow().config.library.sort.clone();
            s.set_sort(&sort, Some(b.is_active()));
        });
        let s = this.clone();
        this.list.connect_row_activated(move |_, row| {
            if let Some(c) = s.at(row.index()) {
                s.app.open_collection(c, true);
            }
        });
        // Double-click plays from the first song.
        let double = gtk::GestureClick::new();
        double.set_button(1);
        let s = this.clone();
        double.connect_pressed(move |_, n, _, y| {
            if n == 2
                && let Some(c) = s.list.row_at_y(y as i32).and_then(|r| s.at(r.index()))
            {
                s.app.play_collection(&c, false);
            }
        });
        this.list.add_controller(double);
        let s = this.clone();
        on_right_click(&this.list, move |_, y| {
            let row = s.list.row_at_y(y as i32)?;
            let c = s.at(row.index())?;
            Some(s.app.collection_menu(&c))
        });

        // Drag a row to a new place: the order becomes "custom".
        let drag = gtk::DragSource::new();
        drag.set_actions(gdk::DragAction::MOVE);
        let s = this.clone();
        drag.connect_prepare(move |src, _, y| {
            let row = s.list.row_at_y(y as i32)?;
            let c = s.at(row.index()).filter(|c| c.kind != "liked")?;
            let tag = gtk::Label::new(Some(&c.name));
            tag.add_css_class("rt-drag-tag");
            src.set_icon(Some(&gtk::WidgetPaintable::new(Some(&tag))), 0, 0);
            s.state.borrow_mut().dragging = Some(c.uri.clone());
            Some(gdk::ContentProvider::for_value(&format!("{REORDER}{}", c.uri).to_value()))
        });
        let s = this.clone();
        drag.connect_drag_end(move |_, _, _| {
            s.state.borrow_mut().dragging = None;
            s.clear_drop_line();
        });
        this.list.add_controller(drag);

        // Drop: a library row moves there; songs go into the playlist (or Liked Songs).
        let drop = gtk::DropTarget::new(gtk::glib::Type::STRING, gdk::DragAction::COPY | gdk::DragAction::MOVE);
        let s = this.clone();
        drop.connect_motion(move |_, _, y| {
            if s.state.borrow().dragging.is_some() {
                return match s.reorder_target(y) {
                    Some((row, after)) => {
                        s.clear_drop_line();
                        row.add_css_class(if after { "rt-drop-below" } else { "rt-drop-above" });
                        gdk::DragAction::MOVE
                    }
                    None => {
                        s.clear_drop_line();
                        gdk::DragAction::empty()
                    }
                };
            }
            match s
                .list
                .row_at_y(y as i32)
                .filter(|r| s.at(r.index()).is_some_and(|c| s.app.accepts_drop(&c)))
            {
                Some(row) => {
                    s.list.drag_highlight_row(&row);
                    gdk::DragAction::COPY
                }
                None => {
                    s.list.drag_unhighlight_row();
                    gdk::DragAction::empty()
                }
            }
        });
        let s = this.clone();
        drop.connect_leave(move |_| {
            s.list.drag_unhighlight_row();
            s.clear_drop_line();
        });
        let s = this.clone();
        drop.connect_drop(move |_, value, _, y| {
            s.list.drag_unhighlight_row();
            s.clear_drop_line();
            let Ok(text) = value.get::<String>() else { return false };
            if let Some(moved) = text.strip_prefix(REORDER) {
                let Some((row, after)) = s.reorder_target(y) else { return false };
                let Some(target) = s.at(row.index()) else { return false };
                s.move_to(moved, &target.uri, after);
                return true;
            }
            let uris: Vec<String> = text.lines().filter(|l| l.starts_with("spotify:")).map(String::from).collect();
            let Some(c) = s.list.row_at_y(y as i32).and_then(|r| s.at(r.index())) else {
                return false;
            };
            if uris.is_empty() || !s.app.accepts_drop(&c) {
                return false;
            }
            s.app.drop_on(&c, uris);
            true
        });
        this.list.add_controller(drop);
        this.rebuild();
        this
    }

    fn at(&self, index: i32) -> Option<Collection> {
        self.state.borrow().shown.get(index.max(0) as usize).cloned()
    }

    /// Activate the chip for `tab` (and only that one) without running the handlers.
    fn show_tab(&self, tab: &str) {
        self.state.borrow_mut().tab = tab.to_owned();
        self.updating.set(true);
        for b in &self.tabs {
            b.set_active(b.widget_name() == tab);
        }
        self.updating.set(false);
    }

    fn set_sort_label(&self, sort: &str) {
        self.sort.set_label(sort);
        self.sort
            .update_property(&[gtk::accessible::Property::Label(&format!("sort: {sort}"))]);
        self.sort.reset_relation(gtk::AccessibleRelation::LabelledBy);
    }

    /// Change the sort (and, with `reversed`, the direction), show it and remember it.
    fn set_sort(&self, sort: &str, reversed: Option<bool>) {
        let reversed = reversed.unwrap_or_else(|| self.app.config.borrow().config.library.sort_reversed);
        self.set_sort_label(sort);
        self.updating.set(true);
        self.reverse.set_active(reversed);
        self.reverse.set_label(if reversed { "↑" } else { "↓" });
        self.updating.set(false);
        let sort = sort.to_owned();
        self.app.update_config(|c| {
            c.library.sort = sort.clone();
            c.library.sort_reversed = reversed;
        });
        self.rebuild();
    }

    /// The row a dragged library row would land next to, and whether after it. Liked Songs stay
    /// on top, so nothing goes before them.
    fn reorder_target(&self, y: f64) -> Option<(gtk::ListBoxRow, bool)> {
        let row = self.list.row_at_y(y as i32)?;
        let c = self.at(row.index())?;
        let moving = self.state.borrow().dragging.clone()?;
        if c.kind == "liked" || c.uri == moving {
            return None;
        }
        let bounds = row.compute_bounds(&self.list)?;
        Some((row, y > (bounds.y() + bounds.height() / 2.0) as f64))
    }

    fn clear_drop_line(&self) {
        let mut child = self.list.first_child();
        while let Some(w) = child {
            w.remove_css_class("rt-drop-above");
            w.remove_css_class("rt-drop-below");
            child = w.next_sibling();
        }
    }

    /// Put `moved` next to `target` in what is shown; the view switches to the custom order.
    fn move_to(&self, moved: &str, target: &str, after: bool) {
        let shown: Vec<String> = self
            .state
            .borrow()
            .shown
            .iter()
            .filter(|c| c.kind != "liked")
            .map(|c| c.uri.clone())
            .collect();
        let stored = self.app.config.borrow().config.library.custom_order.clone();
        let order = reordered(&shown, &stored, moved, target, after);
        self.app.update_config(|c| c.library.custom_order = order.clone());
        self.set_sort("custom", Some(false));
    }

    /// Mark the collection that is playing now.
    pub fn set_live(&self, uri: Option<&str>) {
        if self.state.borrow().live.as_deref() == uri {
            return;
        }
        self.state.borrow_mut().live = uri.map(String::from);
        self.rebuild();
    }

    fn rebuild(&self) {
        let filter = self.filter.text().to_string();
        let (sort, reversed, custom) = {
            let c = &self.app.config.borrow().config.library;
            (c.sort.clone(), c.sort_reversed, c.custom_order.clone())
        };
        let shown = {
            let st = self.state.borrow();
            let items: Vec<Collection> = match st.tab.as_str() {
                "albums" => st.albums.clone(),
                "artists" => st.artists.clone(),
                "playlists" => st.playlists.clone(),
                _ => st.playlists.iter().chain(&st.albums).chain(&st.artists).cloned().collect(),
            };
            let mut shown = visible(&items, &filter, &sort, reversed, &custom);
            if matches!(st.tab.as_str(), "all" | "playlists") && filter.trim().is_empty() {
                shown.insert(0, Collection::liked());
            }
            shown
        };
        clear_list(&self.list);
        let (selected, live) = {
            let st = self.state.borrow();
            (st.selected.clone(), st.live.clone())
        };
        for c in &shown {
            let row = gtk::ListBoxRow::new();
            let b = hbox(10);
            b.append(&cover_widget(&self.app, c, 40));
            let text = vbox(0);
            text.set_valign(gtk::Align::Center);
            text.set_hexpand(true);
            text.append(&ellipsized(&c.name, &["rt-row-title"]));
            text.append(&ellipsized(&c.subtitle, &["rt-dim", "rt-small"]));
            b.append(&text);
            if live.as_deref() == Some(c.uri.as_str()) {
                b.append(&live_bars());
            }
            row.set_child(Some(&b));
            row.set_tooltip_text(Some(&c.name));
            self.list.append(&row);
            if selected.as_deref() == Some(c.uri.as_str()) {
                self.list.select_row(Some(&row));
            }
        }
        if shown.is_empty() {
            self.list.append(&label(
                if filter.trim().is_empty() {
                    "nothing here yet"
                } else {
                    "no matches"
                },
                &["rt-dim"],
            ));
        }
        self.state.borrow_mut().shown = shown;
    }

    pub fn set_playlists(&self, items: Vec<Collection>) {
        self.state.borrow_mut().playlists = items;
        self.rebuild();
    }

    pub fn set_albums(&self, items: Vec<Collection>) {
        self.state.borrow_mut().albums = items;
        self.rebuild();
    }

    pub fn set_artists(&self, items: Vec<Collection>) {
        self.state.borrow_mut().artists = items;
        self.rebuild();
    }

    pub fn playlists(&self) -> Vec<Collection> {
        self.state.borrow().playlists.clone()
    }

    /// Update one playlist's song count after an edit.
    pub fn bump_count(&self, uri: &str, delta: i64) {
        let mut changed = false;
        for p in self.state.borrow_mut().playlists.iter_mut().filter(|p| p.uri == uri) {
            if let Some((owner, rest)) = p.subtitle.split_once(" · ")
                && let Some(n) = rest.strip_suffix(" songs").and_then(|n| n.parse::<i64>().ok())
            {
                p.subtitle = format!("{owner} · {} songs", (n + delta).max(0));
                changed = true;
            }
        }
        if changed {
            self.rebuild();
        }
    }

    pub fn find(&self, uri: &str) -> Option<Collection> {
        if uri == LIKED_URI {
            return Some(Collection::liked());
        }
        let st = self.state.borrow();
        st.playlists
            .iter()
            .chain(st.albums.iter())
            .chain(st.artists.iter())
            .find(|c| c.uri == uri)
            .cloned()
    }

    /// Playlists, albums and artists, in that order.
    pub fn all(&self) -> Vec<Collection> {
        let st = self.state.borrow();
        st.playlists
            .iter()
            .chain(st.albums.iter())
            .chain(st.artists.iter())
            .cloned()
            .collect()
    }

    /// Followed artists first, then the artists of saved albums, each once.
    pub fn artist_ids(&self) -> Vec<String> {
        let st = self.state.borrow();
        let mut ids: Vec<String> = Vec::new();
        let followed = st.artists.iter().map(|a| crate::spotify::client::uri_id(&a.uri).to_owned());
        let from_albums = st.albums.iter().flat_map(|a| a.artist_ids.iter().cloned());
        for id in followed.chain(from_albums) {
            if !id.is_empty() && !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids
    }

    pub fn first_playlist(&self) -> Option<Collection> {
        self.state.borrow().playlists.first().cloned()
    }

    pub fn mark_selected(&self, uri: Option<&str>) {
        self.state.borrow_mut().selected = uri.map(String::from);
        let idx = uri.and_then(|u| self.state.borrow().shown.iter().position(|c| c.uri == u));
        match idx.and_then(|i| self.list.row_at_index(i as i32)) {
            Some(row) => self.list.select_row(Some(&row)),
            None => self.list.unselect_all(),
        }
    }
}

/// Three accent bars of different heights: "playing from here". Still, so an idle window does
/// not redraw.
fn live_bars() -> gtk::Box {
    let b = hbox(2);
    b.add_css_class("rt-live");
    b.set_valign(gtk::Align::Center);
    for h in [6, 10, 4] {
        let bar = gtk::Box::new(gtk::Orientation::Vertical, 0);
        bar.add_css_class("rt-live-bar");
        bar.set_size_request(2, h);
        bar.set_valign(gtk::Align::End);
        b.append(&bar);
    }
    b.set_size_request(-1, 10);
    b.update_property(&[gtk::accessible::Property::Label("playing")]);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(name: &str) -> Collection {
        Collection {
            uri: format!("u:{name}"),
            name: name.into(),
            subtitle: "me".into(),
            ..Default::default()
        }
    }

    #[test]
    fn filter_and_sort() {
        let items = vec![c("zeta"), c("Alpha"), c("beta mix")];
        let names = |v: Vec<Collection>| v.into_iter().map(|c| c.name).collect::<Vec<_>>();
        assert_eq!(names(visible(&items, "", "recent", false, &[])), ["zeta", "Alpha", "beta mix"]);
        assert_eq!(names(visible(&items, "", "a-z", false, &[])), ["Alpha", "beta mix", "zeta"]);
        assert_eq!(names(visible(&items, "", "a-z", true, &[])), ["zeta", "beta mix", "Alpha"]);
        assert_eq!(names(visible(&items, "MIX", "a-z", false, &[])), ["beta mix"]);
    }

    #[test]
    fn added_creator_and_custom() {
        let mut items = vec![c("a"), c("b"), c("c"), c("d")];
        items[0].added_at = "2026-01-01T00:00:00Z".into();
        items[2].added_at = "2026-03-01T00:00:00Z".into();
        for (i, who) in ["mia", "Ben", "ben", "zoe"].iter().enumerate() {
            items[i].creator = (*who).into();
        }
        let names = |v: Vec<Collection>| v.into_iter().map(|c| c.name).collect::<Vec<_>>();
        // Newest first; undated ones after, in Spotify's order.
        assert_eq!(names(visible(&items, "", "added", false, &[])), ["c", "a", "b", "d"]);
        assert_eq!(names(visible(&items, "", "creator", false, &[])), ["b", "c", "a", "d"]);
        let custom = vec!["u:d".to_owned(), "u:b".to_owned()];
        assert_eq!(names(visible(&items, "", "custom", false, &custom)), ["d", "b", "a", "c"]);
    }

    #[test]
    fn reorder_keeps_hidden_items() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let shown = s(&["a", "b", "c"]);
        let stored = s(&["x", "a", "b", "c", "y"]);
        assert_eq!(reordered(&shown, &stored, "c", "a", false), s(&["c", "a", "b", "x", "y"]));
        assert_eq!(reordered(&shown, &stored, "a", "c", true), s(&["b", "c", "a", "x", "y"]));
        assert_eq!(reordered(&shown, &[], "a", "b", true), s(&["b", "a", "c"]));
    }
}
