//! The "nav" panel at the top of the left column (home, search, library, new releases, each with
//! its number key), and the icon rail the whole column shrinks to when it is minimized.

use gtk::prelude::*;

use super::library::{Collection, cover_widget};
use super::widgets::*;
use crate::app::App;

/// Views reachable from the nav panel: page name, label, rail label.
pub const VIEWS: [(&str, &str, &str); 4] = [
    ("home", "home", "hom"),
    ("search", "search", "sea"),
    ("library", "library", "lib"),
    ("releases", "new releases", "new"),
];

#[derive(Clone)]
pub struct NavPanel {
    pub root: gtk::Overlay,
    list: gtk::ListBox,
}

impl NavPanel {
    pub fn new(app: &App) -> Self {
        let list = gtk::ListBox::new();
        list.add_css_class("rt-list");
        list.add_css_class("rt-nav");
        tab_leaves(&list);
        for (i, (_, text, _)) in VIEWS.iter().enumerate() {
            let row = hbox(8);
            let name = label(text, &[]);
            name.set_hexpand(true);
            row.append(&name);
            row.append(&label(&(i + 1).to_string(), &["rt-key"]));
            let item = gtk::ListBoxRow::builder().child(&row).build();
            item.update_property(&[gtk::accessible::Property::Label(text)]);
            list.append(&item);
        }
        let p = panel("nav", &list);
        let minimize = button("minimize", "Shrink the sidebar to icons (B)");
        let a = app.clone();
        minimize.connect_clicked(move |_| a.set_sidebar_collapsed(true));
        p.add_frame_button(&minimize);
        let a = app.clone();
        list.connect_row_activated(move |_, row| {
            if let Some((view, _, _)) = VIEWS.get(row.index() as usize) {
                a.show_view(view, true);
            }
        });
        Self { root: p.root, list }
    }

    /// Mark the nav entry of the visible page (none for settings, lyrics or an artist).
    pub fn set_current(&self, page: &str) {
        match VIEWS.iter().position(|(v, _, _)| *v == page) {
            Some(i) => self.list.select_row(self.list.row_at_index(i as i32).as_ref()),
            None => self.list.unselect_all(),
        }
    }
}

/// The minimized left column: short nav entries and the library as cover thumbnails.
#[derive(Clone)]
pub struct Rail {
    pub root: gtk::Overlay,
    nav: Vec<gtk::ToggleButton>,
    covers: gtk::Box,
    app: App,
}

impl Rail {
    pub fn new(app: &App) -> Self {
        let body = vbox(6);
        let mut nav = Vec::new();
        for (view, text, short) in VIEWS {
            let b = gtk::ToggleButton::with_label(short);
            b.add_css_class("rt-icon");
            b.set_tooltip_text(Some(text));
            b.update_property(&[gtk::accessible::Property::Label(text)]);
            let a = app.clone();
            b.connect_clicked(move |_| a.show_view(view, true));
            body.append(&b);
            nav.push(b);
        }
        let rule = hbox(0);
        rule.add_css_class("rt-rule");
        rule.set_margin_top(4);
        rule.set_margin_bottom(4);
        body.append(&rule);
        let covers = vbox(8);
        covers.set_halign(gtk::Align::Center);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&covers)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::External)
            .build();
        body.append(&scroll);
        let p = panel("", &body);
        let show = button("show", "Show the sidebar (B)");
        let a = app.clone();
        show.connect_clicked(move |_| a.set_sidebar_collapsed(false));
        p.add_frame_button(&show);
        p.root.set_size_request(60, -1);
        Self {
            root: p.root,
            nav,
            covers,
            app: app.clone(),
        }
    }

    pub fn set_current(&self, page: &str) {
        for (b, (view, _, _)) in self.nav.iter().zip(VIEWS) {
            b.set_active(view == page);
        }
    }

    pub fn set_items(&self, items: &[Collection]) {
        while let Some(c) = self.covers.first_child() {
            self.covers.remove(&c);
        }
        for c in items.iter().take(60) {
            let b = gtk::Button::new();
            b.add_css_class("rt-card");
            b.set_child(Some(&cover_widget(&self.app, c, 36)));
            b.set_tooltip_text(Some(&c.name));
            b.update_property(&[gtk::accessible::Property::Label(&c.name)]);
            let (a, c) = (self.app.clone(), c.clone());
            b.connect_clicked(move |_| a.open_collection(c.clone(), true));
            self.covers.append(&b);
        }
    }
}
