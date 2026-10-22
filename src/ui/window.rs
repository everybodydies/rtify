//! Main window: header with page tabs, a notice banner, the page stack and the player panel.

use gtk::{glib, prelude::*};

use super::library::LibraryPane;
use super::lyrics::LyricsPage;
use super::player::PlayerBar;
use super::widgets::*;
use crate::app::App;
use crate::commands::Command;
use crate::spotify::model::Device;

pub const PAGES: [&str; 3] = ["library", "lyrics", "devices"];

#[derive(Clone)]
pub struct MainWindow {
    pub window: gtk::ApplicationWindow,
    stack: gtk::Stack,
    page_buttons: Vec<gtk::ToggleButton>,
    header_tabs: gtk::Box,
    status: gtk::Label,
    banner: gtk::Revealer,
    banner_label: gtk::Label,
    pub player: PlayerBar,
    pub library: LibraryPane,
    pub lyrics: LyricsPage,
    devices: gtk::ListBox,
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
        button.add_css_class("rt-accent");
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
            window.set_default_size(1100, 720);
        }

        // Header.
        let brand = label("[ RTIFY ]", &["rt-accent"]);
        let status = label("○ OFFLINE", &["rt-status-offline"]);
        let header_tabs = hbox(4);
        let mut page_buttons = Vec::new();
        for page in PAGES {
            let b = toggle(&format!("[ {} ]", page.to_uppercase()), "");
            b.add_css_class("flat");
            if let Some(first) = page_buttons.first() {
                b.set_group(Some(first));
            }
            b.set_widget_name(page);
            header_tabs.append(&b);
            page_buttons.push(b);
        }
        let settings = button("settings", "Preferences (Ctrl+,)");
        settings.add_css_class("flat");
        settings.set_action_name(Some(&format!("app.{}", Command::Preferences.name())));
        let compact = button("▭", "Compact/full window (Ctrl+Shift+M)");
        compact.add_css_class("flat");
        compact.set_action_name(Some(&format!("app.{}", Command::ToggleCompact.name())));
        let hide = button("—", "Hide to tray (keeps playing and listening for shortcuts)");
        hide.add_css_class("flat");
        hide.set_action_name(Some(&format!("app.{}", Command::HideWindow.name())));
        let header = hbox(10);
        header.append(&brand);
        header.append(&status);
        header.append(&spacer());
        header.append(&header_tabs);
        header.append(&spacer());
        header.append(&settings);
        header.append(&compact);
        header.append(&hide);

        let banner_label = label("", &[]);
        banner_label.set_wrap(true);
        banner_label.set_hexpand(true);
        let dismiss = button("×", "Dismiss");
        dismiss.add_css_class("flat");
        let banner_box = hbox(8);
        banner_box.add_css_class("rt-banner");
        banner_box.append(&banner_label);
        banner_box.append(&dismiss);
        let banner = gtk::Revealer::builder().child(&banner_box).reveal_child(false).build();
        let b = banner.clone();
        dismiss.connect_clicked(move |_| b.set_reveal_child(false));

        let player = PlayerBar::new(app);
        let library = LibraryPane::new(app);
        let lyrics = LyricsPage::new(app);
        let login = LoginPane::new(app);

        let devices = gtk::ListBox::new();
        devices.add_css_class("rt-list");
        let refresh = button("refresh", "Reload the device list");
        let devices_box = vbox(6);
        devices_box.append(&label("activate a device to transfer playback to it", &["rt-faint"]));
        devices_box.append(&devices);
        devices_box.append(&refresh);
        let devices_panel = panel("devices", &devices_box).root;

        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.set_transition_duration(120);
        stack.add_named(&library.root, Some("library"));
        stack.add_named(&lyrics.root, Some("lyrics"));
        stack.add_named(&devices_panel, Some("devices"));
        stack.add_named(&login.root, Some("login"));

        let root = vbox(8);
        root.add_css_class("rt-root");
        root.append(&header);
        root.append(&banner);
        root.append(&stack);
        root.append(&player.root);
        window.set_child(Some(&root));

        let this = Self {
            window,
            stack,
            page_buttons,
            header_tabs,
            status,
            banner,
            banner_label,
            player,
            library,
            lyrics,
            devices,
            login,
        };

        for b in &this.page_buttons {
            let (w, a) = (this.clone(), app.clone());
            b.connect_toggled(move |b| {
                if b.is_active() {
                    let page = b.widget_name().to_string();
                    w.stack.set_visible_child_name(&page);
                    a.page_changed(&page);
                }
            });
        }
        let a = app.clone();
        refresh.connect_clicked(move |_| a.refresh_devices());
        let a = app.clone();
        this.devices
            .connect_row_activated(move |_, row| a.transfer_to_row(row.index() as usize));

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
            if typing || !mods.is_empty() {
                return glib::Propagation::Proceed;
            }
            match key {
                gtk::gdk::Key::space => a.command(Command::PlayPause),
                gtk::gdk::Key::F11 => a.command(Command::ToggleLyrics),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        this.window.add_controller(keys);
        this
    }

    pub fn show_page(&self, page: &str) {
        if page == "login" {
            self.stack.set_visible_child_name("login");
            self.header_tabs.set_sensitive(false);
            return;
        }
        self.header_tabs.set_sensitive(true);
        let page = if PAGES.contains(&page) { page } else { "library" };
        for b in &self.page_buttons {
            if b.widget_name() == page {
                b.set_active(true);
            }
        }
        self.stack.set_visible_child_name(page);
    }

    pub fn page(&self) -> String {
        self.stack.visible_child_name().map(|s| s.to_string()).unwrap_or_default()
    }

    pub fn set_online(&self, online: bool, who: Option<&str>) {
        self.status.set_text(&match (online, who) {
            (true, Some(w)) => format!("● ONLINE · {w}"),
            (true, None) => "● ONLINE".into(),
            (false, _) => "○ OFFLINE".into(),
        });
        self.status
            .set_css_classes(&[if online { "rt-status-online" } else { "rt-status-offline" }]);
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

    pub fn set_devices(&self, devices: &[Device]) {
        clear_list(&self.devices);
        if devices.is_empty() {
            self.devices
                .append(&label("no devices — open Spotify on a phone, speaker or computer", &["rt-faint"]));
            return;
        }
        for d in devices {
            let row = hbox(10);
            row.append(&label(
                if d.is_active { "●" } else { "○" },
                &[if d.is_active { "rt-accent" } else { "rt-faint" }],
            ));
            row.append(&ellipsized(&d.name, &["rt-row-title"]));
            row.append(&label(&d.kind.to_lowercase(), &["rt-faint"]));
            row.append(&spacer());
            if let Some(v) = d.volume_percent {
                row.append(&label(&format!("vol {v}%"), &["rt-dim"]));
            }
            if d.is_restricted {
                row.append(&label("restricted", &["rt-error"]));
            }
            self.devices.append(&row);
        }
    }

    pub fn set_compact(&self, compact: bool) {
        self.stack.set_visible(!compact);
        self.header_tabs.set_visible(!compact);
        self.player.set_compact(compact);
        if compact {
            self.window.unmaximize();
            self.window.set_default_size(560, 1);
        }
    }
}
