//! Main window: three columns between a top bar and the player bar, as in the system24 OLED
//! design (DESIGN.md):
//!
//! ```text
//! ┌ ‹ ›              [ / search tracks, artists, albums   ctrl+k ]          21:40 [settings] ┐
//! │ ─ nav ──── [minimize] │ ─ playlist ──────────── 47 tracks │ ─ now playing ──── playing │
//! │ home               1  │  cover  late night terminal       │  cover                     │
//! │ search             2  │         [pause] [shuffle]         │  title ♡ / artist          │
//! │ ─ library ─────────── │  # title        album  added time │ ─ queue ────── 8 queued ── │
//! │ playlists albums …    │  01 ▣ Distant…  Weath… Sep 28 2:57 │  next from …               │
//! │ ─ player ──────────────────────────────────────────────────────────────────────────── │
//! │ ▣ title    ⇄ ◀◀ ❚❚ ▶▶ ↻   1:15 ━━━━━━━━━━━━━━━━━── 4:06     lyrics ▤ ⌁ volume          │
//! └────────────────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! The left column shrinks to an icon rail; both dividers can be dragged and their widths
//! persist. The middle switches between home, search, the library (playlists, albums, artists,
//! liked songs), new releases, settings and lyrics.

use std::cell::Cell;
use std::rc::Rc;

use gtk::{gdk, glib, prelude::*};

use super::collection::CollectionView;
use super::home::HomeView;
use super::library::Sidebar;
use super::lyrics::LyricsPage;
use super::nav::{NavPanel, Rail};
use super::nowplaying::NowPlayingPanel;
use super::player::PlayerBar;
use super::preferences::SettingsView;
use super::releases::ReleasesView;
use super::search::SearchView;
use super::widgets::*;
use crate::app::App;
use crate::commands::Command;
use crate::services::playback::Playback;

/// Pages of the main view.
pub const PAGES: [&str; 6] = ["home", "search", "library", "releases", "settings", "lyrics"];

/// Column width limits: left (full), right.
pub const LEFT_WIDTH: (i32, i32) = (220, 420);
pub const RIGHT_WIDTH: (i32, i32) = (240, 440);

#[derive(Clone)]
pub struct MainWindow {
    pub window: gtk::ApplicationWindow,
    stack: gtk::Stack,
    body: gtk::Box,
    topsearch: gtk::Box,
    settings_button: gtk::Button,
    clock: gtk::Label,
    status: gtk::Label,
    pub back: gtk::Button,
    pub forward: gtk::Button,
    banner: gtk::Revealer,
    banner_label: gtk::Label,
    toast: gtk::Revealer,
    toast_label: gtk::Label,
    toast_undo: gtk::Button,
    toast_timer: Rc<Cell<Option<glib::SourceId>>>,
    left: gtk::Box,
    left_divider: gtk::Box,
    collapsed: Rc<Cell<bool>>,
    right_divider: gtk::Box,
    pub nav: NavPanel,
    pub rail: Rail,
    pub player: PlayerBar,
    pub sidebar: Sidebar,
    pub collection: CollectionView,
    pub now_playing: NowPlayingPanel,
    pub home: HomeView,
    pub search: SearchView,
    pub releases: ReleasesView,
    pub settings: SettingsView,
    pub lyrics: LyricsPage,
    pub login: LoginPane,
}

#[derive(Clone)]
pub struct LoginPane {
    pub root: gtk::Overlay,
    pub client_id: gtk::Entry,
    pub button: gtk::Button,
    pub status: gtk::Label,
}

impl LoginPane {
    fn new(app: &App) -> Self {
        let body = vbox(10);
        body.set_valign(gtk::Align::Center);
        body.set_halign(gtk::Align::Center);
        body.set_size_request(380, -1);
        let intro = label(
            "rtify controls Spotify Connect devices through the Web API (Premium required).\n\n\
             1. create an app at developer.spotify.com/dashboard\n\
             2. add the redirect URI shown below and enable \"Web API\"\n\
             3. paste the app's client ID here and sign in",
            &["rt-dim"],
        );
        intro.set_wrap(true);
        let port = app.config.borrow().config.spotify.redirect_port;
        let redirect = label(
            &format!("redirect URI: {}", crate::spotify::auth::redirect_uri(port)),
            &["rt-accent"],
        );
        redirect.set_selectable(true);
        redirect.set_wrap(true);
        redirect.set_wrap_mode(gtk::pango::WrapMode::Char);
        let client_id = gtk::Entry::new();
        client_id.set_placeholder_text(Some("client ID"));
        client_id.set_text(&app.config.borrow().config.spotify.client_id);
        let button = button("sign in with spotify", "");
        button.add_css_class("rt-primary");
        let status = label("", &["rt-dim"]);
        status.set_wrap(true);
        for w in [
            intro.upcast_ref::<gtk::Widget>(),
            redirect.upcast_ref(),
            client_id.upcast_ref(),
            button.upcast_ref(),
            status.upcast_ref(),
        ] {
            body.append(w);
        }
        let a = app.clone();
        let entry = client_id.clone();
        button.connect_clicked(move |_| a.login(entry.text().trim()));
        let a = app.clone();
        client_id.connect_activate(move |e| a.login(e.text().trim()));
        Self {
            root: panel("sign in", &body).root,
            client_id,
            button,
            status,
        }
    }
}

/// Narrowest the center column gets from dragging a divider or from the right column showing.
const CENTER_MIN: i32 = 560;

/// A 12px gap between two columns that resizes `target` when dragged (a grey line shows on
/// hover). `sign` is +1 when dragging right widens the target, -1 when it narrows it; it never
/// squeezes `center` below CENTER_MIN. A double click goes back to `default`. `done` gets the
/// width once the drag ends.
fn divider(
    target: &impl IsA<gtk::Widget>,
    center: &impl IsA<gtk::Widget>,
    sign: f64,
    limits: (i32, i32),
    default: i32,
    done: impl Fn(i32) + 'static,
) -> gtk::Box {
    let line = hbox(0);
    line.add_css_class("rt-divider-line");
    line.set_halign(gtk::Align::Center);
    line.set_margin_top(9);
    let d = hbox(0);
    d.add_css_class("rt-divider");
    d.set_hexpand(false);
    d.append(&line);
    d.set_cursor_from_name(Some("col-resize"));
    d.set_tooltip_text(Some("drag to resize · double-click to reset"));
    let target = target.clone().upcast::<gtk::Widget>();
    let done = Rc::new(done);
    let start = Rc::new(Cell::new((0, 0)));
    let drag = gtk::GestureDrag::new();
    let (t, c, s) = (target.clone(), center.clone().upcast::<gtk::Widget>(), start.clone());
    drag.connect_drag_begin(move |_, _, _| s.set((t.width(), c.width())));
    let (t, s) = (target.clone(), start.clone());
    drag.connect_drag_update(move |_, dx, _| {
        let (width, center) = s.get();
        let max = limits.1.min(width + (center - CENTER_MIN).max(0)).max(limits.0);
        let w = (width as f64 + sign * dx).round() as i32;
        t.set_size_request(w.clamp(limits.0, max), -1);
    });
    let (t, f) = (target.clone(), done.clone());
    drag.connect_drag_end(move |_, _, _| f(t.width_request()));
    d.add_controller(drag);
    let reset = gtk::GestureClick::new();
    let (t, f) = (target, done);
    reset.connect_pressed(move |_, n, _, _| {
        if n == 2 {
            t.set_size_request(default, -1);
            f(default);
        }
    });
    d.add_controller(reset);
    d
}

impl MainWindow {
    pub fn new(app: &App) -> Self {
        let window = gtk::ApplicationWindow::builder()
            .application(&app.gtk_app)
            .title("rtify")
            .icon_name(crate::paths::APP_ID)
            .build();
        window.add_css_class("rt-window");
        let cfg = app.config.borrow().config.window.clone();
        if cfg.remember_geometry {
            window.set_default_size(cfg.width.max(360), cfg.height.max(200));
            if cfg.maximized {
                window.maximize();
            }
        } else {
            window.set_default_size(1600, 900);
        }

        // Top bar: back/forward, one search field (hidden while the search view is open),
        // clock and settings.
        let back = button("‹", "Back (Alt+←)");
        back.add_css_class("rt-square");
        let forward = button("›", "Forward (Alt+→)");
        forward.add_css_class("rt-square");
        let left_top = hbox(6);
        left_top.append(&back);
        left_top.append(&forward);
        let topentry = gtk::Entry::new();
        topentry.add_css_class("rt-bare");
        topentry.set_placeholder_text(Some("search tracks, artists, albums"));
        topentry.update_property(&[gtk::accessible::Property::Label("Search Spotify")]);
        topentry.set_hexpand(true);
        let topsearch = hbox(8);
        topsearch.add_css_class("rt-searchbox");
        topsearch.append(&label("/", &["rt-slash"]));
        topsearch.append(&topentry);
        topsearch.append(&label("ctrl+k", &["rt-faint", "rt-small"]));
        topsearch.set_hexpand(false);
        topsearch.set_size_request(520, -1);
        topsearch.set_tooltip_text(Some("Search Spotify (Ctrl+K)"));
        let status = label("offline", &["rt-status-offline", "rt-small"]);
        status.set_visible(false);
        let clock = label("", &["rt-faint", "rt-small", "rt-mono-num"]);
        let settings_button = button("settings", "Settings (Ctrl+,)");
        settings_button.set_action_name(Some(&format!("app.{}", Command::Preferences.name())));
        let right_top = hbox(12);
        right_top.append(&status);
        right_top.append(&clock);
        right_top.append(&settings_button);
        let topbar = gtk::CenterBox::new();
        topbar.add_css_class("rt-topbar");
        topbar.set_start_widget(Some(&left_top));
        topbar.set_center_widget(Some(&topsearch));
        topbar.set_end_widget(Some(&right_top));

        let banner_label = label("", &[]);
        banner_label.set_wrap(true);
        banner_label.set_hexpand(true);
        let dismiss = button("×", "Dismiss");
        dismiss.add_css_class("rt-icon");
        let banner_box = hbox(8);
        banner_box.add_css_class("rt-banner");
        banner_box.append(&banner_label);
        banner_box.append(&dismiss);
        let banner = gtk::Revealer::builder().child(&banner_box).reveal_child(false).build();
        let b = banner.clone();
        dismiss.connect_clicked(move |_| b.set_reveal_child(false));

        let player = PlayerBar::new(app);
        let nav = NavPanel::new(app);
        let rail = Rail::new(app);
        let sidebar = Sidebar::new(app);
        let collection = CollectionView::new(app);
        let now_playing = NowPlayingPanel::new(app);
        let home = HomeView::new(app);
        let search = SearchView::new(app);
        let releases = ReleasesView::new(app);
        let settings = SettingsView::new(app);
        let lyrics = LyricsPage::new(app);
        let login = LoginPane::new(app);

        let stack = gtk::Stack::new();
        stack.set_hexpand(true);
        stack.set_vexpand(true);
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.set_transition_duration(120);
        stack.add_named(&home.root, Some("home"));
        stack.add_named(&search.root, Some("search"));
        stack.add_named(&collection.root, Some("library"));
        stack.add_named(&releases.root, Some("releases"));
        stack.add_named(&settings.root, Some("settings"));
        stack.add_named(&lyrics.root, Some("lyrics"));
        stack.add_named(&login.root, Some("login"));

        // Left column: nav above the library, or the icon rail.
        let left = vbox(12);
        left.append(&nav.root);
        left.append(&sidebar.root);
        sidebar.root.set_vexpand(true);
        left.set_size_request(cfg.sidebar_width.clamp(LEFT_WIDTH.0, LEFT_WIDTH.1), -1);
        let a = app.clone();
        let left_divider = divider(
            &left,
            &stack,
            1.0,
            LEFT_WIDTH,
            crate::config::Window::default().sidebar_width,
            move |w| a.update_config(|c| c.window.sidebar_width = w),
        );
        now_playing
            .root
            .set_size_request(cfg.right_width.clamp(RIGHT_WIDTH.0, RIGHT_WIDTH.1), -1);
        let a = app.clone();
        let right_divider = divider(
            &now_playing.root,
            &stack,
            -1.0,
            RIGHT_WIDTH,
            crate::config::Window::default().right_width,
            move |w| a.update_config(|c| c.window.right_width = w),
        );
        rail.root.set_margin_end(12);

        let body = hbox(0);
        body.add_css_class("rt-shell");
        body.set_vexpand(true);
        body.append(&rail.root);
        body.append(&left);
        body.append(&left_divider);
        body.append(&stack);
        body.append(&right_divider);
        body.append(&now_playing.root);

        // Short confirmations ("added to …  undo") float over the bottom of the content.
        let toast_label = label("", &[]);
        let toast_undo = button("undo", "Undo (Ctrl+Z)");
        toast_undo.add_css_class("rt-chip");
        let toast_box = hbox(12);
        toast_box.add_css_class("rt-toast");
        toast_box.append(&toast_label);
        toast_box.append(&toast_undo);
        let toast = gtk::Revealer::builder()
            .child(&toast_box)
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .transition_duration(150)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .margin_bottom(20)
            .reveal_child(false)
            .build();
        toast.set_can_target(true);
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&body));
        overlay.add_overlay(&toast);

        let player_panel = panel("player", &player.root).root;
        player_panel.set_margin_start(12);
        player_panel.set_margin_end(12);
        player_panel.set_margin_top(10);
        player_panel.set_margin_bottom(12);

        let root = vbox(0);
        root.add_css_class("rt-root");
        root.append(&topbar);
        root.append(&banner);
        root.append(&overlay);
        root.append(&player_panel);
        window.set_child(Some(&root));
        player.set_toggles(false, cfg.now_playing_panel);

        let this = Self {
            window,
            stack,
            body,
            topsearch,
            settings_button,
            clock,
            status,
            back,
            forward,
            banner,
            banner_label,
            toast,
            toast_label,
            toast_undo: toast_undo.clone(),
            toast_timer: Default::default(),
            left,
            left_divider,
            collapsed: Default::default(),
            right_divider,
            nav,
            rail,
            player,
            sidebar,
            collection,
            now_playing,
            home,
            search,
            releases,
            settings,
            lyrics,
            login,
        };
        this.set_sidebar_collapsed(cfg.sidebar_collapsed);
        this.now_playing.queue_root.set_visible(cfg.queue_panel);

        let a = app.clone();
        toast_undo.connect_clicked(move |_| a.undo());
        let a = app.clone();
        this.back.connect_clicked(move |_| a.navigate_history(-1));
        let a = app.clone();
        this.forward.connect_clicked(move |_| a.navigate_history(1));
        // Typing in the top field (or clicking it) continues in the search view.
        let a = app.clone();
        topentry.connect_changed(move |e| {
            let text = e.text().to_string();
            if text.is_empty() {
                return;
            }
            e.set_text("");
            a.show_view("search", true);
            let s = &a.win().search;
            s.set_query(&text);
            s.entry.emit_by_name::<()>("changed", &[]);
            s.focus();
        });
        let click = gtk::GestureClick::new();
        let a = app.clone();
        click.connect_released(move |_, _, _, _| a.command(Command::FocusSearch));
        this.topsearch.add_controller(click);

        let clock = this.clock.clone();
        let tick = move || {
            if let Some(now) = crate::app::now_local() {
                clock.set_text(&now.format("%H:%M").map(|s| s.to_string()).unwrap_or_default());
            }
            glib::ControlFlow::Continue
        };
        tick();
        glib::timeout_add_seconds_local(15, tick);

        // Closing hides to the tray when configured; otherwise it quits.
        let a = app.clone();
        this.window.connect_close_request(move |w| {
            a.save_geometry(w);
            if a.config.borrow().config.window.minimize_to_tray {
                a.command(Command::HideWindow);
            } else {
                a.command(Command::Quit);
            }
            glib::Propagation::Stop
        });
        let a = app.clone();
        this.window.connect_visible_notify(move |_| a.visibility_changed());
        for prop in ["default-width", "maximized", "fullscreened"] {
            let w = this.clone();
            this.window.connect_notify_local(Some(prop), move |_, _| w.window_resized());
        }
        this.window_resized();

        // Plain keys that only make sense inside the window.
        let keys = gtk::EventControllerKey::new();
        let a = app.clone();
        keys.connect_key_pressed(move |ctl, key, _, mods| {
            let typing = ctl
                .widget()
                .and_then(|w| w.root())
                .and_then(|r| r.focus())
                .is_some_and(|f| f.is::<gtk::Text>());
            if mods.contains(gdk::ModifierType::ALT_MASK) {
                match key {
                    gdk::Key::Left => a.navigate_history(-1),
                    gdk::Key::Right => a.navigate_history(1),
                    _ => return glib::Propagation::Proceed,
                }
                return glib::Propagation::Stop;
            }
            if mods == gdk::ModifierType::CONTROL_MASK && !typing {
                match key {
                    gdk::Key::z => a.undo(),
                    gdk::Key::f => a.focus_list_search(),
                    _ => return glib::Propagation::Proceed,
                }
                return glib::Propagation::Stop;
            }
            if typing || !mods.is_empty() {
                return glib::Propagation::Proceed;
            }
            match key {
                gdk::Key::space => a.command(Command::PlayPause),
                gdk::Key::F11 => a.command(Command::ToggleLyrics),
                gdk::Key::_1 | gdk::Key::_2 | gdk::Key::_3 | gdk::Key::_4 => {
                    let i = (key.to_unicode().and_then(|c| c.to_digit(10)).unwrap_or(1) - 1) as usize;
                    a.show_view(super::nav::VIEWS[i].0, true);
                }
                gdk::Key::b => {
                    let collapsed = a.config.borrow().config.window.sidebar_collapsed;
                    a.set_sidebar_collapsed(!collapsed);
                }
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        this.window.add_controller(keys);
        // Mouse back/forward buttons.
        let mouse = gtk::GestureClick::new();
        mouse.set_button(0);
        let a = app.clone();
        mouse.connect_pressed(move |g, _, _, _| match g.current_button() {
            8 => a.navigate_history(-1),
            9 => a.navigate_history(1),
            _ => {}
        });
        this.window.add_controller(mouse);
        this
    }

    pub fn show_page(&self, page: &str) {
        let login = page == "login";
        self.topsearch.set_sensitive(!login);
        if login {
            self.rail.root.set_visible(false);
            self.left.set_visible(false);
            self.left_divider.set_visible(false);
            self.now_playing.root.set_visible(false);
            self.right_divider.set_visible(false);
            self.stack.set_visible_child_name("login");
            return;
        }
        let page = if PAGES.contains(&page) { page } else { "home" };
        self.stack.set_visible_child_name(page);
        self.set_sidebar_collapsed(self.collapsed.get());
        self.topsearch.set_visible(page != "search");
        self.nav.set_current(page);
        self.rail.set_current(page);
        if page == "settings" {
            self.settings_button.add_css_class("rt-current");
        } else {
            self.settings_button.remove_css_class("rt-current");
        }
        let panel = self.player.panel_toggle.is_active();
        self.set_right_visible(panel && self.now_playing_fits());
        self.player.set_toggles(page == "lyrics", panel);
    }

    pub fn set_sidebar_collapsed(&self, collapsed: bool) {
        self.collapsed.set(collapsed);
        let login = self.page() == "login";
        self.rail.root.set_visible(collapsed && !login);
        self.left.set_visible(!collapsed && !login);
        self.left_divider.set_visible(!collapsed && !login);
        self.window_resized();
    }

    pub fn set_queue_panel(&self, visible: bool) {
        self.now_playing.queue_root.set_visible(visible);
    }

    fn set_right_visible(&self, visible: bool) {
        self.now_playing.root.set_visible(visible);
        self.right_divider.set_visible(visible);
    }

    /// Below this window width the right column steps aside (it comes back when the window
    /// grows), so the track table keeps readable titles on half a 1920px screen.
    const NOW_PLAYING_MIN_WINDOW: i32 = 1180;

    /// The right column shows when the window leaves the center column at least CENTER_MIN
    /// next to both side columns (and the window is not narrower than NOW_PLAYING_MIN_WINDOW).
    fn now_playing_fits(&self) -> bool {
        if self.window.is_maximized() || self.window.is_fullscreen() {
            return true;
        }
        let left = if self.collapsed.get() {
            self.rail.root.width_request().max(60) + 12
        } else {
            self.left.width_request() + 12
        };
        // Shell padding, the two 12px dividers' worth of gap and the panel frames.
        let need = left + self.now_playing.root.width_request() + CENTER_MIN + 24 + 24;
        self.window.default_width() >= need.max(Self::NOW_PLAYING_MIN_WINDOW)
    }

    /// Re-check the right column after the window was resized.
    pub fn window_resized(&self) {
        let panel = self.player.panel_toggle.is_active();
        let visible = panel && self.page() != "login" && self.now_playing_fits();
        self.set_right_visible(visible);
    }

    pub fn page(&self) -> String {
        self.stack.visible_child_name().map(|s| s.to_string()).unwrap_or_default()
    }

    pub fn set_now_playing_panel(&self, visible: bool) {
        self.set_right_visible(visible && self.page() != "login" && self.now_playing_fits());
        self.player.set_toggles(self.page() == "lyrics", visible);
    }

    /// Only "offline" is worth a word in the top bar; the account sits in settings.
    pub fn set_online(&self, online: bool, _who: Option<&str>) {
        self.status.set_visible(!online);
    }

    pub fn notice(&self, text: &str, warning: bool) {
        self.banner_label.set_text(text);
        let body = self.banner.child().unwrap();
        if warning {
            body.add_css_class("rt-warning");
        } else {
            body.remove_css_class("rt-warning");
        }
        self.banner.set_reveal_child(!text.is_empty());
    }

    /// A short confirmation; `undo` adds the undo button. Hides itself after a few seconds.
    pub fn toast(&self, text: &str, undo: bool) {
        self.toast_label.set_text(text);
        self.toast_undo.set_visible(undo);
        self.toast.set_reveal_child(true);
        if let Some(id) = self.toast_timer.take() {
            id.remove();
        }
        let (toast, timer) = (self.toast.clone(), self.toast_timer.clone());
        let id = glib::timeout_add_local_once(std::time::Duration::from_secs(if undo { 6 } else { 3 }), move || {
            timer.take();
            toast.set_reveal_child(false);
        });
        self.toast_timer.set(Some(id));
    }

    pub fn update_playback(&self, pb: &Playback, saved: Option<bool>) {
        self.player.update(pb, saved);
        self.now_playing.update(pb, saved);
    }

    pub fn set_cover(&self, texture: Option<&gdk::Texture>) {
        self.player.set_cover(texture);
        self.now_playing.set_cover(texture);
    }

    pub fn focus_search(&self) {
        self.search.focus();
    }

    pub fn set_history_buttons(&self, back: bool, forward: bool) {
        self.back.set_sensitive(back);
        self.forward.set_sensitive(forward);
    }

    /// Mini player: only the player bar (and top bar for settings) stays.
    pub fn set_compact(&self, compact: bool) {
        self.body.set_visible(!compact);
        self.topsearch.set_visible(!compact && self.page() != "search");
        if compact {
            self.window.unmaximize();
            self.window.set_default_size(1000, 1);
        }
    }
}
