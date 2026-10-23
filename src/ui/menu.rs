//! Right-click menus: a plain popover with flat rows, one level of submenu ("add to playlist ▸")
//! shown in place, and Escape/back to return. Opens at the pointer.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::{gdk, prelude::*};

use super::widgets::*;

/// Menu contents; the popover showing them belongs to the parent widget and is reused, never
/// destroyed (destroying popovers can leave GTK 4.14 delivering pointer motion to a dead surface,
/// which crashed during drag-and-drop).
pub struct Menu {
    popover: Rc<RefCell<Option<gtk::Popover>>>,
    stack: gtk::Stack,
    main: gtk::Box,
}

fn popdown(p: &Rc<RefCell<Option<gtk::Popover>>>) {
    if let Some(p) = p.borrow().as_ref() {
        p.popdown();
    }
}

/// The parent's menu popover, created on first use.
fn popover_for(parent: &gtk::Widget) -> gtk::Popover {
    let mut child = parent.first_child();
    while let Some(c) = child {
        if c.has_css_class("rt-menu")
            && let Ok(p) = c.clone().downcast::<gtk::Popover>()
        {
            return p;
        }
        child = c.next_sibling();
    }
    let p = gtk::Popover::new();
    p.add_css_class("rt-menu");
    p.set_has_arrow(false);
    p.set_position(gtk::PositionType::Bottom);
    p.set_halign(gtk::Align::Start);
    p.set_parent(parent);
    p
}

fn item_button(text: &str) -> gtk::Button {
    let l = label(text, &[]);
    l.set_xalign(0.0);
    let b = gtk::Button::new();
    b.set_child(Some(&l));
    b.add_css_class("rt-menu-item");
    b
}

impl Menu {
    pub fn new() -> Self {
        let main = vbox(0);
        let stack = gtk::Stack::new();
        stack.set_vhomogeneous(false);
        stack.set_hhomogeneous(false);
        stack.set_interpolate_size(true);
        stack.add_named(&main, Some("main"));
        Self {
            popover: Rc::default(),
            stack,
            main,
        }
    }

    pub fn item(&self, text: &str, f: impl Fn() + 'static) -> &Self {
        let b = item_button(text);
        let pop = self.popover.clone();
        b.connect_clicked(move |_| {
            popdown(&pop);
            f();
        });
        self.main.append(&b);
        self
    }

    pub fn separator(&self) -> &Self {
        let s = gtk::Separator::new(gtk::Orientation::Horizontal);
        s.set_margin_top(3);
        s.set_margin_bottom(3);
        self.main.append(&s);
        self
    }

    /// A submenu of `(label, action)` entries, with a filter field when the list is long.
    pub fn submenu(&self, text: &str, entries: Vec<(String, Rc<dyn Fn()>)>) -> &Self {
        let page = vbox(0);
        let back = item_button(&format!("‹ {text}"));
        back.add_css_class("rt-menu-back");
        page.append(&back);
        let list = vbox(0);
        let filter = gtk::SearchEntry::new();
        filter.add_css_class("rt-filter");
        filter.set_placeholder_text(Some("find"));
        if entries.len() > 8 {
            page.append(&filter);
        }
        let mut buttons = Vec::new();
        for (name, f) in entries {
            let b = item_button(&name);
            let pop = self.popover.clone();
            b.connect_clicked(move |_| {
                popdown(&pop);
                f();
            });
            list.append(&b);
            buttons.push((name.to_lowercase(), b));
        }
        if buttons.is_empty() {
            list.append(&label("  no playlists you can edit", &["rt-faint"]));
        }
        filter.connect_search_changed(move |e| {
            let q = e.text().to_lowercase();
            for (name, b) in &buttons {
                b.set_visible(q.is_empty() || name.contains(&q));
            }
        });
        let scroll = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(360)
            .build();
        page.append(&scroll);
        let name = format!("sub{}", self.stack.pages().n_items());
        self.stack.add_named(&page, Some(&name));
        let stack = self.stack.clone();
        back.connect_clicked(move |_| stack.set_visible_child_name("main"));
        let opener = item_button(&format!("{text}  ▸"));
        let stack = self.stack.clone();
        opener.connect_clicked(move |_| stack.set_visible_child_name(&name));
        self.main.append(&opener);
        self
    }

    /// Show at `(x, y)` in `parent`'s coordinates.
    /// The popover lives on the nearest ancestor marked `rt-menu-host` (a widget that is never
    /// rebuilt), so rows and cards can be replaced while a menu is open.
    pub fn popup_at(self, parent: &impl IsA<gtk::Widget>, x: f64, y: f64) {
        let mut host = parent.as_ref().clone();
        while !host.has_css_class("rt-menu-host") {
            match host.parent() {
                Some(p) => host = p,
                None => {
                    host = parent.as_ref().clone();
                    break;
                }
            }
        }
        let (x, y) = parent
            .as_ref()
            .compute_point(&host, &gtk::graphene::Point::new(x as f32, y as f32))
            .map(|p| (p.x() as f64, p.y() as f64))
            .unwrap_or((x, y));
        let parent = &host;
        let popover = popover_for(parent);
        popover.set_child(Some(&self.stack));
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        *self.popover.borrow_mut() = Some(popover.clone());
        popover.popup();
    }
}

impl Default for Menu {
    fn default() -> Self {
        Self::new()
    }
}

/// Right-click handler for `widget`: `build(x, y)` returns the menu or `None`.
pub fn on_right_click(widget: &impl IsA<gtk::Widget>, build: impl Fn(f64, f64) -> Option<Menu> + 'static) {
    let g = gtk::GestureClick::new();
    g.set_button(3);
    let w = widget.as_ref().clone();
    g.connect_pressed(move |g, _, x, y| {
        if let Some(menu) = build(x, y) {
            g.set_state(gtk::EventSequenceState::Claimed);
            menu.popup_at(&w, x, y);
        }
    });
    widget.add_controller(g);
}
