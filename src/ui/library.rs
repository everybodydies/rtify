//! "Your library" sidebar: playlists/albums chips, filter, sort, and a list with cover thumbnails.
//! Liked songs are pinned on top like in the official client. Tab, filter and sort persist.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use super::widgets::*;
use crate::app::App;
use crate::spotify::model::{Album, Image, Playlist, pick_image};

pub const TABS: [&str; 2] = ["playlists", "albums"];
pub const SORTS: [&str; 2] = ["recent", "a-z"];
pub const LIKED_URI: &str = "rtify:liked";
pub const SEARCH_URI: &str = "rtify:search";

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Collection {
    pub uri: String,
    pub name: String,
    pub subtitle: String,
    /// "playlist", "album", "liked" or "search".
    pub kind: String,
    pub images: Vec<Image>,
    pub description: String,
}

impl Collection {
    pub fn from_playlist(p: &Playlist) -> Self {
        let owner = p.owner.display_name.clone().unwrap_or_else(|| p.owner.id.clone());
        Self {
            uri: p.uri.clone(),
            name: p.name.clone(),
            subtitle: format!("{} · {} songs", owner, p.count()),
            kind: "playlist".into(),
            images: p.images.clone().unwrap_or_default(),
            description: p.description.clone().unwrap_or_default(),
        }
    }

    pub fn from_album(a: &Album) -> Self {
        let artists = a.artists.iter().map(|x| x.name.as_str()).collect::<Vec<_>>().join(", ");
        let year = a.release_date.get(..4).unwrap_or("");
        Self {
            uri: a.uri.clone(),
            name: a.name.clone(),
            subtitle: if year.is_empty() { artists } else { format!("{artists} · {year}") },
            kind: "album".into(),
            images: a.images.clone(),
            description: String::new(),
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

/// A cover thumbnail, or a framed glyph when the collection has no image (liked songs).
pub fn cover_widget(app: &App, c: &Collection, size: i32) -> gtk::Widget {
    match c.thumb_url(size as u32) {
        Some(url) => {
            let pic = cover_picture(size);
            let p = pic.clone();
            app.load_image(&url, move |t| p.set_paintable(t.as_ref()));
            pic.upcast()
        }
        None => {
            let l = gtk::Label::new(Some(if c.kind == "liked" { "♥" } else { "♪" }));
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
    shown: Vec<Collection>,
    selected: Option<String>,
}

#[derive(Clone)]
pub struct Sidebar {
    pub root: gtk::Overlay,
    tabs: Vec<gtk::ToggleButton>,
    filter: gtk::Entry,
    sort: gtk::DropDown,
    list: gtk::ListBox,
    state: Rc<RefCell<SidebarState>>,
    app: App,
}

impl Sidebar {
    pub fn new(app: &App) -> Self {
        let cfg = app.config.borrow().config.library.clone();
        let chips = hbox(6);
        let mut tabs = Vec::new();
        for id in TABS {
            let b = toggle(id, "");
            b.add_css_class("rt-chip");
            if let Some(first) = tabs.first() {
                b.set_group(Some(first));
            }
            b.set_widget_name(id);
            chips.append(&b);
            tabs.push(b);
        }
        chips.append(&spacer());
        let sort = gtk::DropDown::from_strings(&SORTS);
        sort.set_selected(SORTS.iter().position(|s| *s == cfg.sort).unwrap_or(0) as u32);
        chips.append(&sort);

        let filter = gtk::Entry::new();
        filter.add_css_class("rt-filter");
        filter.set_placeholder_text(Some("filter your library"));
        filter.set_text(&cfg.filter);

        let list = gtk::ListBox::new();
        list.add_css_class("rt-list");
        let scroll = gtk::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();

        let body = vbox(8);
        body.append(&chips);
        body.append(&filter);
        body.append(&scroll);
        let root = panel("your library", &body).root;
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
            list,
            state: Rc::new(RefCell::new(SidebarState {
                tab: tab.clone(),
                ..Default::default()
            })),
            app: app.clone(),
        };
        for b in &this.tabs {
            if b.widget_name() == tab {
                b.set_active(true);
            }
            let s = this.clone();
            b.connect_toggled(move |b| {
                if b.is_active() {
                    let tab = b.widget_name().to_string();
                    s.state.borrow_mut().tab = tab.clone();
                    s.rebuild();
                    s.app.update_config(|c| c.library.last_tab = tab.clone());
                }
            });
        }
        let s = this.clone();
        this.filter.connect_changed(move |f| {
            s.rebuild();
            let text = f.text().to_string();
            s.app.update_config(|c| c.library.filter = text.clone());
        });
        let s = this.clone();
        this.sort.connect_selected_notify(move |d| {
            s.rebuild();
            let sort = SORTS.get(d.selected() as usize).unwrap_or(&"recent").to_string();
            s.app.update_config(|c| c.library.sort = sort.clone());
        });
        let s = this.clone();
        this.list.connect_row_activated(move |_, row| {
            let c = s.state.borrow().shown.get(row.index() as usize).cloned();
            if let Some(c) = c {
                s.app.open_collection(c, true);
            }
        });
        this.rebuild();
        this
    }

    fn rebuild(&self) {
        let filter = self.filter.text().to_string();
        let sort = SORTS.get(self.sort.selected() as usize).copied().unwrap_or("recent");
        let shown = {
            let st = self.state.borrow();
            let items = if st.tab == "albums" { &st.albums } else { &st.playlists };
            let mut shown = visible(items, &filter, sort);
            if st.tab != "albums" && filter.trim().is_empty() {
                shown.insert(0, Collection::liked());
            }
            shown
        };
        clear_list(&self.list);
        let selected = self.state.borrow().selected.clone();
        for c in &shown {
            let row = gtk::ListBoxRow::new();
            let b = hbox(10);
            b.append(&cover_widget(&self.app, c, 40));
            let text = vbox(0);
            text.set_valign(gtk::Align::Center);
            text.append(&ellipsized(&c.name, &["rt-row-title"]));
            text.append(&ellipsized(&c.subtitle, &["rt-faint", "rt-small"]));
            b.append(&text);
            row.set_child(Some(&b));
            self.list.append(&row);
            if selected.as_deref() == Some(c.uri.as_str()) {
                self.list.select_row(Some(&row));
            }
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

    pub fn find(&self, uri: &str) -> Option<Collection> {
        if uri == LIKED_URI {
            return Some(Collection::liked());
        }
        let st = self.state.borrow();
        st.playlists.iter().chain(st.albums.iter()).find(|c| c.uri == uri).cloned()
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
        assert_eq!(names(visible(&items, "", "recent")), ["zeta", "Alpha", "beta mix"]);
        assert_eq!(names(visible(&items, "", "a-z")), ["Alpha", "beta mix", "zeta"]);
        assert_eq!(names(visible(&items, "MIX", "a-z")), ["beta mix"]);
    }
}
