//! Small builders for the System24 widget vocabulary.

use gtk::prelude::*;

/// A titled panel: the title sits on the top border line, like System24's `[ TITLE ]` frames.
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
    root.set_child(Some(&body));
    root.add_overlay(&title_label);
    root.set_measure_overlay(&title_label, false);
    Panel { root, title: title_label }
}

pub fn panel_title(title: &str) -> String {
    format!("[ {} ]", title.to_uppercase())
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
    b
}

pub fn toggle(text: &str, tooltip: &str) -> gtk::ToggleButton {
    let b = gtk::ToggleButton::with_label(text);
    if !tooltip.is_empty() {
        b.set_tooltip_text(Some(tooltip));
    }
    b
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

#[cfg(test)]
mod tests {
    #[test]
    fn times() {
        assert_eq!(super::fmt_time(0), "0:00");
        assert_eq!(super::fmt_time(61_999), "1:01");
        assert_eq!(super::fmt_time(3_723_000), "1:02:03");
        assert_eq!(super::panel_title("now playing"), "[ NOW PLAYING ]");
    }
}
