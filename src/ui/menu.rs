//! Right-click menus: a plain popover with flat rows, one level of submenu ("add to playlist ▸")
//! swapped in place, and Escape/back to return. Opens at the pointer.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::{gdk, prelude::*};

use super::widgets::*;

/// A submenu entry: its text, a faint hint after it, and what it does.
pub type SubmenuEntry = (String, String, Rc<dyn Fn()>);

/// Menu contents; the popover showing them belongs to the parent widget and is reused, never
/// destroyed (destroying popovers can leave GTK 4.14 delivering pointer motion to a dead surface,
/// which crashed during drag-and-drop).
pub struct Menu {
    popover: Rc<RefCell<Option<gtk::Popover>>>,
    main: gtk::Box,
    /// Set while a page switch closes and reopens the popover.
    switching: Rc<Cell<bool>>,
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

/// Show `page` in the open popover. Pages are swapped rather than kept in a `gtk::Stack`, so the
/// popover takes each page's own size (with a stack it kept the main page's height).
fn show_page(popover: &Rc<RefCell<Option<gtk::Popover>>>, switching: &Cell<bool>, page: &gtk::Box) {
    if let Some(p) = popover.borrow().as_ref() {
        // A mapped popover only ever grows; reopening it lets it shrink to the new page.
        switching.set(true);
        p.popdown();
        p.set_child(Some(page));
        p.popup();
        switching.set(false);
    }
}

fn item_button(text: &str) -> gtk::Button {
    item_button_hint(text, "")
}

fn item_button_hint(text: &str, hint: &str) -> gtk::Button {
    let l = label(text, &[]);
    l.set_xalign(0.0);
    let b = gtk::Button::new();
    b.add_css_class("rt-menu-item");
    if hint.is_empty() {
        b.set_child(Some(&l));
        return b;
    }
    l.set_hexpand(true);
    let row = hbox(16);
    row.append(&l);
    let h = ellipsized(hint, &["rt-faint", "rt-small"]);
    h.set_max_width_chars(16);
    row.append(&h);
    b.set_child(Some(&row));
    b.update_property(&[gtk::accessible::Property::Label(text)]);
    b
}

impl Menu {
    pub fn new() -> Self {
        Self {
            popover: Rc::default(),
            main: vbox(0),
            switching: Rc::default(),
        }
    }

    pub fn item(&self, text: &str, f: impl Fn() + 'static) -> &Self {
        self.item_hint(text, "", f)
    }

    /// An entry with grey text at its right end: its key ("p"), or what it leads to.
    pub fn item_hint(&self, text: &str, hint: &str, f: impl Fn() + 'static) -> &Self {
        let b = item_button_hint(text, hint);
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

    /// A submenu of `(label, hint, action)` entries, with a filter field when the list is long.
    pub fn submenu(&self, text: &str, entries: Vec<SubmenuEntry>) -> &Self {
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
        for (name, hint, f) in entries {
            let b = item_button_hint(&name, &hint);
            let pop = self.popover.clone();
            b.connect_clicked(move |_| {
                popdown(&pop);
                f();
            });
            list.append(&b);
            buttons.push((name.to_lowercase(), b));
        }
        if buttons.is_empty() {
            list.append(&label("  no playlists you can edit", &["rt-dim"]));
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
        let (main, pop, sw) = (self.main.clone(), self.popover.clone(), self.switching.clone());
        back.connect_clicked(move |_| show_page(&pop, &sw, &main));
        let opener = item_button_hint(text, "›");
        let (pop, sw) = (self.popover.clone(), self.switching.clone());
        opener.connect_clicked(move |_| show_page(&pop, &sw, &page));
        self.main.append(&opener);
        self
    }

    /// Show at `(x, y)` in `parent`'s coordinates.
    /// The popover lives on the nearest ancestor marked `rt-menu-host` (a widget that is never
    /// rebuilt), so rows and cards can be replaced while a menu is open.
    pub fn popup_at(self, parent: &impl IsA<gtk::Widget>, x: f64, y: f64) {
        // The clicked row or card, which gets focus back when the menu closes.
        let clicked = parent.as_ref();
        let row = if let Some(list) = clicked.downcast_ref::<gtk::ListBox>() {
            list.row_at_y(y as i32).map(|r| r.upcast())
        } else if let Some(flow) = clicked.downcast_ref::<gtk::FlowBox>() {
            flow.child_at_pos(x as i32, y as i32).map(|c| c.upcast())
        } else {
            let mut w = Some(clicked.clone());
            while let Some(c) = &w {
                if c.is::<gtk::ListBoxRow>() || c.is::<gtk::FlowBoxChild>() {
                    break;
                }
                w = c.parent();
            }
            w
        };
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
        popover.set_child(Some(&self.main));
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        *self.popover.borrow_mut() = Some(popover.clone());
        if let Some(row) = row {
            refocus_on_close(&popover, row, self.switching.clone());
        }
        popover.popup();
    }
}

/// When a popover closes, GTK moves focus to its parent: the whole list, which then scrolls to its
/// end. Hand focus to the clicked row instead (once; the popover is reused), and keep the list
/// where it was when the menu opened, also across the next frame's scroll-to-focus.
fn refocus_on_close(popover: &gtk::Popover, row: gtk::Widget, switching: Rc<Cell<bool>>) {
    let adjustment = row
        .ancestor(gtk::ScrolledWindow::static_type())
        .and_downcast::<gtk::ScrolledWindow>()
        .map(|s| s.vadjustment());
    let scrolled_to = adjustment.as_ref().map(|a| a.value());
    let handler: Rc<RefCell<Option<gtk::glib::SignalHandlerId>>> = Rc::default();
    let h = handler.clone();
    *handler.borrow_mut() = Some(popover.connect_closed(move |p| {
        if switching.get() {
            return;
        }
        if let Some(id) = h.borrow_mut().take() {
            p.disconnect(id);
        }
        let row = row.clone();
        let restore = adjustment.clone().zip(scrolled_to);
        gtk::glib::idle_add_local_once(move || {
            if row.root().is_none() {
                return;
            }
            row.grab_focus();
            let Some((adjustment, value)) = restore else { return };
            adjustment.set_value(value);
            let frames = Cell::new(2);
            row.add_tick_callback(move |_, _| {
                adjustment.set_value(value);
                frames.set(frames.get() - 1);
                if frames.get() > 0 {
                    gtk::glib::ControlFlow::Continue
                } else {
                    gtk::glib::ControlFlow::Break
                }
            });
        });
    }));
}

impl Default for Menu {
    fn default() -> Self {
        Self::new()
    }
}

/// Right-click handler for `widget`: `build(x, y)` returns the menu or `None`.
pub fn on_right_click(widget: &impl IsA<gtk::Widget>, build: impl Fn(f64, f64) -> Option<Menu> + 'static) {
    let build = Rc::new(build);
    let g = gtk::GestureClick::new();
    g.set_button(3);
    let w = widget.as_ref().clone();
    let b = build.clone();
    g.connect_pressed(move |g, _, x, y| {
        if let Some(menu) = b(x, y) {
            g.set_state(gtk::EventSequenceState::Claimed);
            menu.popup_at(&w, x, y);
        }
    });
    widget.add_controller(g);
    // The same menu from the keyboard (Menu key or Shift+F10), at the focused row or card.
    let keys = gtk::EventControllerKey::new();
    let w = widget.as_ref().clone();
    keys.connect_key_pressed(move |_, key, _, mods| {
        let wanted = key == gdk::Key::Menu || (key == gdk::Key::F10 && mods.contains(gdk::ModifierType::SHIFT_MASK));
        let focus = w.root().and_then(|r| r.focus()).filter(|f| f == &w || f.is_ancestor(&w));
        let (true, Some(focus)) = (wanted, focus) else {
            return gtk::glib::Propagation::Proceed;
        };
        let at = gtk::graphene::Point::new(12.0, focus.height() as f32 / 2.0);
        let Some(p) = focus.compute_point(&w, &at) else {
            return gtk::glib::Propagation::Proceed;
        };
        match build(p.x() as f64, p.y() as f64) {
            Some(menu) => {
                menu.popup_at(&w, p.x() as f64, p.y() as f64);
                gtk::glib::Propagation::Stop
            }
            None => gtk::glib::Propagation::Proceed,
        }
    });
    widget.add_controller(keys);
}
