//! New releases from the artists in your library (followed artists and the artists of saved
//! albums), newest first, grouped by age. Spotify's own "new releases" browse list is closed to
//! apps in development mode, so rtify asks each artist for their latest albums and singles.

use gtk::prelude::*;

use super::library::{Collection, cover_widget};
use super::widgets::*;
use crate::app::App;
use crate::spotify::model::Album;

pub const FILTERS: [&str; 3] = ["all", "albums", "singles"];

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `2026-10-21`, `2026-10` or `2026` (Spotify's release date precisions) as a day number; a
/// missing month or day counts as the first.
pub fn release_day(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next().map_or(Some(1), |m| m.parse().ok())?;
    let d: u32 = parts.next().map_or(Some(1), |d| d.parse().ok())?;
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then(|| days_from_civil(y, m, d))
}

/// "today", "yesterday", "5 days ago", then the date itself.
pub fn age_label(date: &str, today: i64) -> String {
    match release_day(date).map(|d| today - d) {
        Some(0) | Some(i64::MIN..0) => "today".into(),
        Some(1) => "yesterday".into(),
        Some(n @ 2..=13) => format!("{n} days ago"),
        _ => date.to_owned(),
    }
}

/// Group heading for a release `age` days old.
pub fn group_of(age: Option<i64>) -> &'static str {
    match age {
        Some(..=7) => "this week",
        Some(8..=31) => "this month",
        Some(32..=365) => "this year",
        _ => "earlier",
    }
}

pub fn matches_filter(a: &Album, filter: &str) -> bool {
    match filter {
        "albums" => a.album_type != "single",
        "singles" => a.album_type == "single",
        _ => true,
    }
}

/// Newest first, each release once (the same album can come from several of its artists),
/// grouped by age; groups keep their order and empty ones are left out.
pub fn group_releases(mut albums: Vec<Album>, filter: &str, today: i64) -> Vec<(&'static str, Vec<Album>)> {
    albums.retain(|a| matches_filter(a, filter));
    albums.sort_by(|a, b| b.release_date.cmp(&a.release_date));
    let mut seen = std::collections::HashSet::new();
    albums.retain(|a| seen.insert(a.uri.clone()));
    let mut groups: Vec<(&'static str, Vec<Album>)> = Vec::new();
    for a in albums {
        let g = group_of(release_day(&a.release_date).map(|d| today - d));
        match groups.last_mut() {
            Some((name, items)) if *name == g => items.push(a),
            _ => groups.push((g, vec![a])),
        }
    }
    groups
}

#[derive(Clone)]
pub struct ReleasesView {
    pub root: gtk::Overlay,
    panel: Panel,
    groups: gtk::Box,
    status: gtk::Label,
    albums: std::rc::Rc<std::cell::RefCell<Vec<Album>>>,
    filter: std::rc::Rc<std::cell::RefCell<String>>,
    app: App,
}

impl ReleasesView {
    pub fn new(app: &App) -> Self {
        let filter = app.config.borrow().config.library.releases_filter.clone();
        let kicker = label("from artists in your library", &["rt-kicker"]);
        let title = label("new releases", &["rt-hero-xl"]);
        let text = vbox(4);
        text.set_hexpand(true);
        text.append(&kicker);
        text.append(&title);
        let filter_state = std::rc::Rc::new(std::cell::RefCell::new(filter.clone()));
        let selected = FILTERS.iter().position(|f| *f == filter).unwrap_or(0);
        let chips = {
            let a = app.clone();
            let state = filter_state.clone();
            segmented(&FILTERS, selected, move |i| {
                let f = FILTERS[i].to_owned();
                *state.borrow_mut() = f.clone();
                a.update_config(|c| c.library.releases_filter = f.clone());
                a.win().releases.render();
            })
        };
        chips.set_valign(gtk::Align::End);
        let head = hbox(12);
        head.append(&text);
        head.append(&chips);
        head.set_margin_bottom(14);

        let status = label("", &["rt-faint"]);
        let groups = vbox(28);
        let content = vbox(16);
        content.set_margin_start(16);
        content.set_margin_end(16);
        content.set_margin_top(14);
        content.set_margin_bottom(16);
        content.append(&head);
        content.append(&status);
        content.append(&groups);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&content)
            .vexpand(true)
            .hexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let panel = panel("releases", &scroll);
        Self {
            root: panel.root.clone(),
            panel,
            groups,
            status,
            albums: Default::default(),
            filter: filter_state,
            app: app.clone(),
        }
    }

    pub fn set_status(&self, text: &str) {
        self.status.set_text(text);
        self.status.set_visible(!text.is_empty());
    }

    pub fn set_albums(&self, albums: Vec<Album>) {
        *self.albums.borrow_mut() = albums;
        self.render();
    }

    fn today() -> i64 {
        crate::app::now_local()
            .map(|n| days_from_civil(n.year() as i64, n.month() as u32, n.day_of_month() as u32))
            .unwrap_or(0)
    }

    pub fn render(&self) {
        while let Some(c) = self.groups.first_child() {
            self.groups.remove(&c);
        }
        let today = Self::today();
        let filter = self.filter.borrow().clone();
        let groups = group_releases(self.albums.borrow().clone(), &filter, today);
        let total: usize = groups.iter().map(|(_, g)| g.len()).sum();
        self.panel.set_meta(&if total == 1 {
            "1 release".to_owned()
        } else {
            format!("{total} releases")
        });
        for (name, items) in groups {
            let section = vbox(12);
            section.append(&heading(&format!("{name}  {}", items.len())));
            let flow = gtk::FlowBox::new();
            flow.set_selection_mode(gtk::SelectionMode::None);
            flow.set_homogeneous(false);
            flow.set_min_children_per_line(1);
            flow.set_max_children_per_line(12);
            flow.set_column_spacing(16);
            flow.set_row_spacing(20);
            flow.add_css_class("rt-menu-host");
            for a in &items {
                flow.insert(&self.card(a, today), -1);
            }
            section.append(&flow);
            self.groups.append(&section);
        }
    }

    fn card(&self, a: &Album, today: i64) -> gtk::Button {
        let col = Collection::from_album(a);
        let cover = gtk::Overlay::new();
        cover.set_child(Some(&cover_widget(&self.app, &col, 150)));
        let badge = label(if a.album_type == "single" { "single" } else { "album" }, &["rt-badge"]);
        badge.set_halign(gtk::Align::Start);
        badge.set_valign(gtk::Align::Start);
        cover.add_overlay(&badge);
        let card = vbox(4);
        card.append(&cover);
        let name = ellipsized(&a.name, &["rt-row-title"]);
        name.set_max_width_chars(16);
        name.set_margin_top(4);
        card.append(&name);
        let artists = a.artists.iter().map(|x| x.name.as_str()).collect::<Vec<_>>().join(", ");
        let by = ellipsized(&artists, &["rt-dim", "rt-small"]);
        by.set_max_width_chars(16);
        card.append(&by);
        card.append(&label(&age_label(&a.release_date, today), &["rt-faint", "rt-small"]));
        let b = gtk::Button::new();
        b.add_css_class("rt-card");
        b.set_child(Some(&card));
        b.update_property(&[gtk::accessible::Property::Label(&format!("{}, {artists}", a.name))]);
        let (app, c) = (self.app.clone(), col.clone());
        b.connect_clicked(move |_| app.open_collection(c.clone(), true));
        let double = gtk::GestureClick::new();
        double.set_propagation_phase(gtk::PropagationPhase::Capture);
        let (app, c) = (self.app.clone(), col.clone());
        double.connect_pressed(move |_, n, _, _| {
            if n == 2 {
                app.play_collection(&c, false);
            }
        });
        b.add_controller(double);
        let (app, c) = (self.app.clone(), col);
        super::menu::on_right_click(&b, move |_, _| Some(app.collection_menu(&c)));
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn album(uri: &str, date: &str, kind: &str) -> Album {
        Album {
            uri: uri.into(),
            release_date: date.into(),
            album_type: kind.into(),
            ..Default::default()
        }
    }

    #[test]
    fn calendar_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1) - days_from_civil(2000, 2, 28), 2, "leap year");
        assert_eq!(release_day("2026-10-24"), Some(days_from_civil(2026, 10, 24)));
        assert_eq!(release_day("2026-10"), Some(days_from_civil(2026, 10, 1)));
        assert_eq!(release_day("2026"), Some(days_from_civil(2026, 1, 1)));
        assert_eq!(release_day(""), None);
        assert_eq!(release_day("2026-13-01"), None);
    }

    #[test]
    fn ages_and_groups() {
        let today = days_from_civil(2026, 10, 24);
        assert_eq!(age_label("2026-10-24", today), "today");
        assert_eq!(age_label("2026-10-25", today), "today");
        assert_eq!(age_label("2026-10-23", today), "yesterday");
        assert_eq!(age_label("2026-10-19", today), "5 days ago");
        assert_eq!(age_label("2026-09-01", today), "2026-09-01");
        assert_eq!(group_of(Some(0)), "this week");
        assert_eq!(group_of(Some(20)), "this month");
        assert_eq!(group_of(Some(200)), "this year");
        assert_eq!(group_of(None), "earlier");
    }

    #[test]
    fn releases_are_sorted_deduplicated_and_filtered() {
        let today = days_from_civil(2026, 10, 24);
        let albums = vec![
            album("a", "2020-01-01", "album"),
            album("b", "2026-10-22", "single"),
            album("c", "2026-10-10", "album"),
            album("b", "2026-10-22", "single"),
        ];
        let g = group_releases(albums.clone(), "all", today);
        let names: Vec<_> = g
            .iter()
            .map(|(n, items)| (*n, items.iter().map(|a| a.uri.as_str()).collect::<Vec<_>>()))
            .collect();
        assert_eq!(names, [("this week", vec!["b"]), ("this month", vec!["c"]), ("earlier", vec!["a"])]);
        let singles = group_releases(albums.clone(), "singles", today);
        assert_eq!(singles.len(), 1);
        let albums_only = group_releases(albums, "albums", today);
        assert_eq!(albums_only.iter().map(|(_, g)| g.len()).sum::<usize>(), 2);
    }
}
