//! Small builders for the System24 widget vocabulary.

use gtk::prelude::*;

/// A titled panel: a short lowercase label sits on the top border line, like a TUI window title.
/// The same line can carry a grey meta text and small framed buttons at its right end.
/// (GtkFrame draws its label inside the border, so this is an overlay instead.)
#[derive(Clone)]
pub struct Panel {
    pub root: gtk::Overlay,
    title: gtk::Label,
    meta: gtk::Label,
    end: gtk::Box,
}

impl Panel {
    pub fn set_title(&self, title: &str) {
        self.title.set_text(&panel_title(title));
    }

    /// Grey text at the right end of the frame line ("47 tracks · 194 min"); empty hides it.
    pub fn set_meta(&self, text: &str) {
        self.meta.set_text(text);
        self.meta.set_visible(!text.is_empty());
        // The meta label comes first; framed buttons follow it.
        self.end.set_visible(!text.is_empty() || self.meta.next_sibling().is_some());
    }

    /// A small framed button set into the frame line at the right end.
    pub fn add_frame_button(&self, b: &impl IsA<gtk::Widget>) {
        b.add_css_class("rt-frame");
        b.set_valign(gtk::Align::Center);
        self.end.append(b);
        self.end.set_visible(true);
    }
}

pub fn panel(title: &str, child: &impl IsA<gtk::Widget>) -> Panel {
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.add_css_class("rt-panel");
    // Half the title height, so the border runs through the middle of the title.
    body.set_margin_top(9);
    child.add_css_class("rt-panel-body");
    body.append(child);
    let title_label = gtk::Label::new(Some(&panel_title(title)));
    title_label.add_css_class("rt-panel-title");
    title_label.set_halign(gtk::Align::Start);
    title_label.set_valign(gtk::Align::Start);
    title_label.set_margin_start(10);
    title_label.set_visible(!title.is_empty());
    let meta = gtk::Label::new(None);
    meta.add_css_class("rt-panel-meta");
    meta.set_visible(false);
    let end = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    end.add_css_class("rt-frame-end");
    end.set_halign(gtk::Align::End);
    end.set_valign(gtk::Align::Start);
    end.set_margin_end(10);
    end.set_size_request(-1, 18);
    end.append(&meta);
    end.set_visible(false);
    let root = gtk::Overlay::new();
    root.add_css_class("rt-panel-host");
    root.set_child(Some(&body));
    root.add_overlay(&title_label);
    root.set_measure_overlay(&title_label, false);
    root.add_overlay(&end);
    root.set_measure_overlay(&end, false);
    Panel {
        root,
        title: title_label,
        meta,
        end,
    }
}

pub fn panel_title(title: &str) -> String {
    title.to_lowercase()
}

pub fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

pub fn ellipsized(text: &str, classes: &[&str]) -> gtk::Label {
    let l = label(text, classes);
    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
    l
}

pub fn button(text: &str, tooltip: &str) -> gtk::Button {
    let b = gtk::Button::with_label(text);
    if !tooltip.is_empty() {
        b.set_tooltip_text(Some(tooltip));
    }
    name_symbol_button(&b, text, tooltip);
    b
}

pub fn toggle(text: &str, tooltip: &str) -> gtk::ToggleButton {
    let b = gtk::ToggleButton::with_label(text);
    if !tooltip.is_empty() {
        b.set_tooltip_text(Some(tooltip));
    }
    name_symbol_button(&b, text, tooltip);
    b
}

/// Buttons labelled with a symbol (◀, ❚❚, ♡, ⚙, +, ⋯) get a spoken name for screen readers:
/// the tooltip without its shortcut hint.
pub fn name_symbol_button(b: &impl IsA<gtk::Accessible>, text: &str, tooltip: &str) {
    if text.chars().any(char::is_alphabetic) {
        return;
    }
    if let Some(name) = accessible_name(tooltip) {
        b.update_property(&[gtk::accessible::Property::Label(&name)]);
        // GtkButton points labelled-by at its label child, which wins over the label property.
        b.reset_relation(gtk::AccessibleRelation::LabelledBy);
    }
}

/// "Next (Ctrl+→)" → "Next".
pub fn accessible_name(tooltip: &str) -> Option<String> {
    let name = tooltip.split(" (").next().unwrap_or_default().trim();
    (!name.is_empty()).then(|| name.to_owned())
}

pub fn hbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}

pub fn vbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}

pub fn spacer() -> gtk::Box {
    let b = hbox(0);
    b.set_hexpand(true);
    b
}

/// A fixed-size square cover image (filled in later via `set_paintable`).
pub fn cover_picture(size: i32) -> gtk::Picture {
    let pic = gtk::Picture::new();
    pic.set_size_request(size, size);
    pic.set_content_fit(gtk::ContentFit::Cover);
    pic.set_can_shrink(true);
    pic.set_halign(gtk::Align::Start);
    pic.set_valign(gtk::Align::Center);
    pic.add_css_class("rt-cover");
    pic
}

/// `pic` at exactly `size` × `size`, cropped to fill (a picture alone grows to its image's
/// natural size in a vertical box).
pub fn square(pic: &gtk::Picture, size: i32) -> gtk::Overlay {
    let space = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    space.set_size_request(size, size);
    let o = gtk::Overlay::new();
    o.set_child(Some(&space));
    o.add_overlay(pic);
    o.set_measure_overlay(pic, false);
    o.set_halign(gtk::Align::Start);
    o.set_valign(gtk::Align::Center);
    o
}

pub fn fmt_time(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// An accent section heading followed by a faint rule to the right edge ("jump back in ────").
pub fn heading(text: &str) -> gtk::Box {
    let b = hbox(10);
    b.add_css_class("rt-heading");
    let l = label(text, &[]);
    b.append(&l);
    let rule = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    rule.add_css_class("rt-rule");
    rule.set_hexpand(true);
    rule.set_valign(gtk::Align::Center);
    b.append(&rule);
    b
}

/// A row of linked option buttons ("on" "off", "fixed" "noctalia" "album art"); exactly one is
/// checked. `on_pick` gets the index the user picked.
pub fn segmented(options: &[&str], selected: usize, on_pick: impl Fn(usize) + 'static) -> gtk::Box {
    let b = hbox(4);
    let on_pick = std::rc::Rc::new(on_pick);
    let mut first: Option<gtk::ToggleButton> = None;
    for (i, text) in options.iter().enumerate() {
        let t = gtk::ToggleButton::with_label(text);
        t.add_css_class("rt-seg");
        if let Some(f) = &first {
            t.set_group(Some(f));
        } else {
            first = Some(t.clone());
        }
        t.set_active(i == selected);
        let f = on_pick.clone();
        t.connect_toggled(move |t| {
            if t.is_active() {
                f(i);
            }
        });
        b.append(&t);
    }
    b
}

/// Remove all rows (but not other children such as a menu popover parented to the list).
pub fn clear_list(list: &gtk::ListBox) {
    while let Some(row) = list.row_at_index(0) {
        list.remove(&row);
    }
}

/// Tab and Shift+Tab leave the list instead of walking through every row (GtkListBox treats
/// Tab like the arrow keys); the arrows still move between rows.
pub fn tab_leaves(list: &gtk::ListBox) {
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = list.downgrade();
    keys.connect_key_pressed(move |_, key, _, mods| {
        let backward = match key {
            gtk::gdk::Key::ISO_Left_Tab => true,
            gtk::gdk::Key::Tab | gtk::gdk::Key::KP_Tab => mods.contains(gtk::gdk::ModifierType::SHIFT_MASK),
            _ => return gtk::glib::Propagation::Proceed,
        };
        let (Some(list), dir) = (
            weak.upgrade(),
            if backward {
                gtk::DirectionType::TabBackward
            } else {
                gtk::DirectionType::TabForward
            },
        ) else {
            return gtk::glib::Propagation::Proceed;
        };
        let Some(root) = list.root() else {
            return gtk::glib::Propagation::Proceed;
        };
        // While the list cannot take focus, the window's focus chain steps over it.
        list.set_can_focus(false);
        let moved = root.child_focus(dir);
        list.set_can_focus(true);
        if moved {
            gtk::glib::Propagation::Stop
        } else {
            gtk::glib::Propagation::Proceed
        }
    });
    list.add_controller(keys);
}

#[cfg(test)]
mod tests {
    #[test]
    fn times() {
        assert_eq!(super::fmt_time(0), "0:00");
        assert_eq!(super::fmt_time(61_999), "1:01");
        assert_eq!(super::fmt_time(3_723_000), "1:02:03");
        assert_eq!(super::panel_title("now playing"), "now playing");
        assert_eq!(super::accessible_name("Next (Ctrl+→)").as_deref(), Some("Next"));
        assert_eq!(
            super::accessible_name("Spotify Connect devices").as_deref(),
            Some("Spotify Connect devices")
        );
        assert_eq!(super::accessible_name(""), None);
    }
}
