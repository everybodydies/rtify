//! Main window, laid out like the official client for wide screens:
//!
//! ```text
//! ┌ ◀ ▶ [ RTIFY ]           [ search ............ ]            ● user  ⚙ ▭ — ┐
//! │ [ YOUR LIBRARY ] │ [ PLAYLIST ] header + track table │ [ NOW PLAYING ] │
//! │  chips, filter   │   (or lyrics)                     │  cover, queue   │
//! ├──────────────────────────────────────────────────────────────────────────┤
//! │ cover title ♡        ⇄ ◀◀ ▶ ▶▶ ↻  0:42 ━━━━━━━━ 3:01      lyrics ▤ ⌁ vol │
//! └──────────────────────────────────────────────────────────────────────────┘
//! ```

use gtk::{gdk, glib, prelude::*};

use super::collection::CollectionView;
use super::library::Sidebar;
use super::lyrics::LyricsPage;
use super::nowplaying::NowPlayingPanel;
use super::player::PlayerBar;
use super::widgets::*;
use crate::app::App;
use crate::commands::Command;
use crate::services::playback::Playback;

pub const PAGES: [&str; 2] = ["library", "lyrics"];

#[derive(Clone)]
pub struct MainWindow {
    pub window: gtk::ApplicationWindow,
    stack: gtk::Stack,
    shell: gtk::Paned,
    status: gtk::Label,
    status_dot: gtk::Label,
    search: gtk::SearchEntry,
    pub back: gtk::Button,
    pub forward: gtk::Button,
    banner: gtk::Revealer,
    banner_label: gtk::Label,
    pub player: PlayerBar,
    pub sidebar: Sidebar,
    pub collection: CollectionView,
    pub now_playing: NowPlayingPanel,
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
        body.set_size_request(520, -1);
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
        let client_id = gtk::Entry::new();
        client_id.set_placeholder_text(Some("client ID"));
        client_id.set_text(&app.config.borrow().config.spotify.client_id);
        let button = button("SIGN IN WITH SPOTIFY", "");
        button.add_css_class("rt-primary");
        let status = label("", &["rt-faint"]);
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

        // Top bar.
        let back = button("◀", "Back (Alt+←)");
        back.add_css_class("rt-icon");
        let forward = button("▶", "Forward (Alt+→)");
        forward.add_css_class("rt-icon");
        let brand = label("[ RTIFY ]", &["rt-brand"]);
        brand.set_margin_start(8);
        let left = hbox(2);
        left.append(&back);
        left.append(&forward);
        left.append(&brand);
        let search = gtk::SearchEntry::new();
        search.add_css_class("rt-search");
        search.set_placeholder_text(Some("what do you want to play?   (Ctrl+L)"));
        search.set_size_request(460, -1);
        let status_dot = label("○", &["rt-status-offline"]);
        let status = label("offline", &["rt-status-offline", "rt-small"]);
        let settings = button("⚙", "Settings (Ctrl+,)");
        settings.add_css_class("rt-icon");
        settings.set_action_name(Some(&format!("app.{}", Command::Preferences.name())));
        let compact = button("▭", "Mini player (Ctrl+Shift+M)");
        compact.add_css_class("rt-icon");
        compact.set_action_name(Some(&format!("app.{}", Command::ToggleCompact.name())));
        let hide = button("—", "Hide to tray (keeps playing and listening for shortcuts)");
        hide.add_css_class("rt-icon");
        hide.set_action_name(Some(&format!("app.{}", Command::HideWindow.name())));
        let right = hbox(4);
        right.append(&status_dot);
        right.append(&status);
        right.append(&settings);
        right.append(&compact);
        right.append(&hide);
        let topbar = gtk::CenterBox::new();
        topbar.add_css_class("rt-topbar");
        topbar.set_start_widget(Some(&left));
        topbar.set_center_widget(Some(&search));
        topbar.set_end_widget(Some(&right));

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
        let sidebar = Sidebar::new(app);
        let collection = CollectionView::new(app);
        let now_playing = NowPlayingPanel::new(app);
        let lyrics = LyricsPage::new(app);
        let login = LoginPane::new(app);

        let stack = gtk::Stack::new();
        stack.set_hexpand(true);
        stack.set_vexpand(true);
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.set_transition_duration(120);
        stack.add_named(&collection.root, Some("library"));
        stack.add_named(&lyrics.root, Some("lyrics"));
        stack.add_named(&login.root, Some("login"));

        let center = hbox(8);
        center.append(&stack);
        center.append(&now_playing.root);
        now_playing.root.set_visible(cfg.now_playing_panel);

        let shell = gtk::Paned::new(gtk::Orientation::Horizontal);
        shell.add_css_class("rt-shell");
        shell.set_start_child(Some(&sidebar.root));
        shell.set_end_child(Some(&center));
        shell.set_resize_start_child(false);
        shell.set_shrink_start_child(false);
        shell.set_shrink_end_child(false);
        shell.set_position(cfg.sidebar_width.clamp(240, 600));
        shell.set_vexpand(true);
        shell.set_margin_bottom(8);

        let root = vbox(0);
        root.add_css_class("rt-root");
        root.append(&topbar);
        root.append(&banner);
        root.append(&shell);
        root.append(&player.root);
        window.set_child(Some(&root));
        player.set_toggles(false, cfg.now_playing_panel);

        let this = Self {
            window,
            stack,
            shell,
            status,
            status_dot,
            search,
            back,
            forward,
            banner,
            banner_label,
            player,
            sidebar,
            collection,
            now_playing,
            lyrics,
            login,
        };

        let a = app.clone();
        this.back.connect_clicked(move |_| a.navigate_history(-1));
        let a = app.clone();
        this.forward.connect_clicked(move |_| a.navigate_history(1));
        let a = app.clone();
        this.search.connect_activate(move |e| {
            let q = e.text().trim().to_string();
            if !q.is_empty() {
                a.search(&q, true);
            }
        });
        let a = app.clone();
        this.shell.connect_position_notify(move |p| {
            let w = p.position();
            if w > 0 {
                a.update_config(|c| c.window.sidebar_width = w);
            }
        });

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
            if typing || !mods.is_empty() {
                return glib::Propagation::Proceed;
            }
            match key {
                gdk::Key::space => a.command(Command::PlayPause),
                gdk::Key::F11 => a.command(Command::ToggleLyrics),
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
        self.sidebar.root.set_visible(!login);
        self.search.set_sensitive(!login);
        if login {
            self.now_playing.root.set_visible(false);
            self.stack.set_visible_child_name("login");
            return;
        }
        let page = if PAGES.contains(&page) { page } else { "library" };
        self.stack.set_visible_child_name(page);
        let panel = self.player.panel_toggle.is_active();
        self.now_playing.root.set_visible(panel);
        self.player.set_toggles(page == "lyrics", panel);
    }

    pub fn page(&self) -> String {
        self.stack.visible_child_name().map(|s| s.to_string()).unwrap_or_default()
    }

    pub fn set_now_playing_panel(&self, visible: bool) {
        self.now_playing.root.set_visible(visible && self.page() != "login");
        self.player.set_toggles(self.page() == "lyrics", visible);
    }

    pub fn set_online(&self, online: bool, who: Option<&str>) {
        self.status.set_text(&match (online, who) {
            (true, Some(w)) => w.to_owned(),
            (true, None) => "online".into(),
            (false, _) => "offline".into(),
        });
        self.status_dot.set_text(if online { "●" } else { "○" });
        let class = if online { "rt-status-online" } else { "rt-status-offline" };
        self.status.set_css_classes(&[class, "rt-small"]);
        self.status_dot.set_css_classes(&[class, "rt-dot"]);
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

    pub fn update_playback(&self, pb: &Playback, saved: Option<bool>) {
        self.player.update(pb, saved);
        self.now_playing.update(pb);
    }

    pub fn set_cover(&self, texture: Option<&gdk::Texture>) {
        self.player.set_cover(texture);
        self.now_playing.set_cover(texture);
    }

    pub fn focus_search(&self) {
        self.search.grab_focus();
    }

    pub fn set_history_buttons(&self, back: bool, forward: bool) {
        self.back.set_sensitive(back);
        self.forward.set_sensitive(forward);
    }

    /// Mini player: only the player bar (and top bar for settings) stays.
    pub fn set_compact(&self, compact: bool) {
        self.shell.set_visible(!compact);
        self.search.set_visible(!compact);
        if compact {
            self.window.unmaximize();
            self.window.set_default_size(1000, 1);
        }
    }
}
