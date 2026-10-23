//! Small builders for the System24 widget vocabulary.

use gtk::prelude::*;

/// A titled panel: a short lowercase label sits on the top border line, like a TUI window title.
/// (GtkFrame draws its label inside the border, so this is an overlay instead.)
#[derive(Clone)]
pub struct Panel {
    pub root: gtk::Overlay,
    title: gtk::Label,
}

impl Panel {
    pub fn set_title(&self, title: &str) {
        self.title.set_text(&panel_title(title));
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
    title_label.set_margin_start(12);
    let root = gtk::Overlay::new();
    root.add_css_class("rt-panel-host");
    root.set_child(Some(&body));
    root.add_overlay(&title_label);
    root.set_measure_overlay(&title_label, false);
    Panel { root, title: title_label }
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

pub fn fmt_time(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
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
