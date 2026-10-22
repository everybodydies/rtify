//! The application: one GTK process that stays alive (tray, shortcuts, polling) while the window
//! is hidden, until an explicit Quit.

use std::cell::{Cell, OnceCell, RefCell};
use std::ops::Deref;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gtk::{gio, glib, prelude::*};

use crate::cli::Cli;
use crate::commands::Command;
use crate::config::{Config, Notifications};
use crate::desktop::shortcuts::{self, ShortcutEvent};
use crate::desktop::tray::{self, TrayHandle, TrayState};
use crate::lyrics::provider::{self, LocalFileProvider, LyricCache, LyricsProvider, TrackQuery, demo};
use crate::lyrics::synchronization::LyricClock;
use crate::lyrics::{LyricDocument, romanization, translation};
use crate::paths::{self, APP_ID};
use crate::persistence::{ConfigStore, SessionStore};
use crate::runtime::{self, rt, spawn_then};
use crate::services::artwork::Artwork;
use crate::services::playback::{Playback, ReconnectStep, Repeat, reconnect_plan};
use crate::services::polling::{PollInputs, next_poll};
use crate::spotify::model::{Device, Track, pick_image};
use crate::spotify::{ApiError, PlayRequest, SpotifyClient, auth, tokens};
use crate::theme::manager::ThemeManager;
use crate::ui::collection::TrackRow;
use crate::ui::library::{Collection, LIKED_URI, SEARCH_URI};
use crate::ui::lyrics::view::{Clock, ViewOptions};
use crate::ui::window::MainWindow;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayerOp {
    Seek(u64),
    SetVolume(u32),
    Like(bool),
}

/// What the lyrics page currently shows.
#[derive(Debug, Clone, PartialEq)]
enum LyricsSource {
    None,
    Demo(usize),
    File(std::path::PathBuf),
    /// A user-supplied file matching the current Spotify track (capability gate enabled).
    Track(String),
}

/// Rows and the next page offset of a loaded collection.
type TrackPage = (Vec<TrackRow>, Option<u32>);

/// Back/forward history of the main view.
#[derive(Debug, Clone)]
enum Nav {
    Collection(Collection),
    Search(String),
}

pub struct Inner {
    pub gtk_app: gtk::Application,
    pub config: RefCell<ConfigStore>,
    session: RefCell<SessionStore>,
    http: reqwest::Client,
    spotify: RefCell<SpotifyClient>,
    pub theme: Rc<ThemeManager>,
    artwork: Rc<Artwork>,
    playback: RefCell<Playback>,
    devices: RefCell<Vec<Device>>,
    account: RefCell<Option<String>>,
    display_name: RefCell<Option<String>>,
    saved_current: Cell<Option<bool>>,
    logged_in: Cell<bool>,
    window: OnceCell<MainWindow>,
    prefs: RefCell<Option<gtk::Window>>,
    tray: RefCell<Option<TrayHandle>>,
    hold: RefCell<Option<gio::ApplicationHoldGuard>>,
    save_config: RefCell<Option<glib::SourceId>>,
    save_session: RefCell<Option<glib::SourceId>>,
    poll_gen: Cell<u64>,
    failures: Cell<u32>,
    retry_after: Cell<Option<Duration>>,
    queued: RefCell<Vec<Command>>,
    reconciled: Cell<bool>,
    started: Cell<bool>,
    lyrics_source: RefCell<LyricsSource>,
    lyrics_doc: RefCell<Option<LyricDocument>>,
    lyric_cache: RefCell<LyricCache>,
    deepl_key: RefCell<Option<String>>,
    pub shortcut_status: RefCell<String>,
    history: RefCell<Vec<Nav>>,
    history_pos: Cell<usize>,
}

#[derive(Clone)]
pub struct App(Rc<Inner>);

impl Deref for App {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

thread_local! {
    static APP: OnceCell<App> = const { OnceCell::new() };
}

fn demo_mode() -> bool {
    std::env::var_os("RTIFY_DEMO").is_some_and(|v| !v.is_empty() && v != "0")
}

pub fn run(args: Vec<String>) -> glib::ExitCode {
    let gtk_app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    gtk_app.connect_startup(|a| {
        let app = App::startup(a);
        APP.with(|cell| {
            let _ = cell.set(app);
        });
    });
    gtk_app.connect_command_line(|_, cmd| APP.with(|cell| cell.get().map(|app| app.command_line(cmd)).unwrap_or(glib::ExitCode::FAILURE)));
    gtk_app.connect_activate(|_| APP.with(|cell| cell.get().map(|app| app.present()).unwrap_or_default()));
    gtk_app.connect_shutdown(|_| APP.with(|cell| cell.get().map(|app| app.flush()).unwrap_or_default()));
    gtk_app.run_with_args(&args)
}

impl App {
    fn startup(gtk_app: &gtk::Application) -> App {
        let config = ConfigStore::load(&paths::config_file());
        let session = SessionStore::load(&paths::session_file());
        let http = reqwest::Client::builder()
            .user_agent(concat!("rtify/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("http client");
        let theme = ThemeManager::new(&config.config.appearance);
        let client_id = config.config.spotify.client_id.clone();
        let app = App(Rc::new(Inner {
            gtk_app: gtk_app.clone(),
            spotify: RefCell::new(Self::make_client(&http, &client_id)),
            config: RefCell::new(config),
            session: RefCell::new(session),
            http,
            theme,
            artwork: Rc::default(),
            playback: RefCell::default(),
            devices: RefCell::default(),
            account: RefCell::default(),
            display_name: RefCell::default(),
            saved_current: Cell::new(None),
            logged_in: Cell::new(false),
            window: OnceCell::new(),
            prefs: RefCell::default(),
            tray: RefCell::default(),
            hold: RefCell::default(),
            save_config: RefCell::default(),
            save_session: RefCell::default(),
            poll_gen: Cell::new(0),
            failures: Cell::new(0),
            retry_after: Cell::new(None),
            queued: RefCell::default(),
            reconciled: Cell::new(false),
            started: Cell::new(false),
            lyrics_source: RefCell::new(LyricsSource::None),
            lyrics_doc: RefCell::default(),
            lyric_cache: RefCell::default(),
            deepl_key: RefCell::default(),
            shortcut_status: RefCell::new("not registered".into()),
            history: RefCell::default(),
            history_pos: Cell::new(0),
        }));
        // Background operation: the process outlives its window until Quit.
        *app.hold.borrow_mut() = Some(gtk_app.hold());
        app.register_actions();

        let window = MainWindow::new(&app);
        let _ = app.window.set(window);
        let w = app.win().clone();
        app.theme.connect_changed(move |p| w.lyrics.view.set_palette(p));

        for warning in [app.config.borrow().warning.clone(), app.session.borrow().warning.clone()]
            .into_iter()
            .flatten()
        {
            app.notice(&warning, true);
        }
        if let Some(status) = app.theme.status.borrow().clone() {
            app.notice(&status, true);
        }

        // Last-known state until Spotify answers.
        if let Some(s) = app.session.borrow().current().cloned() {
            let pb = Playback::from_session(&s);
            app.win().update_playback(&pb, None);
            if let Some(url) = pb.track.as_ref().and_then(|t| pick_image(t.images(), 640)).map(|i| i.url.clone()) {
                app.load_cover(&url);
            }
            *app.playback.borrow_mut() = pb;
            if !s.page.is_empty() {
                app.win().show_page(&s.page);
            }
        }
        if app.config.borrow().config.window.compact {
            app.win().set_compact(true);
        }
        app.apply_lyrics_options();
        app.open_demo_lyrics(0, false);
        app.start_tray();
        app.start_shortcuts();
        app.start_progress_timer();
        app.connect_spotify();
        app
    }

    fn make_client(http: &reqwest::Client, client_id: &str) -> SpotifyClient {
        let account = client_id.to_owned();
        SpotifyClient::new(http.clone(), client_id.to_owned()).with_refresh_hook(Arc::new(move |refresh: String| {
            let account = account.clone();
            rt().spawn(async move {
                if let Err(e) = tokens::store(tokens::SPOTIFY_REFRESH, &account, &refresh).await {
                    log::warn!("could not store rotated refresh token: {e}");
                }
            });
        }))
    }

    fn win(&self) -> &MainWindow {
        self.window.get().expect("window built during startup")
    }

    fn client(&self) -> SpotifyClient {
        self.spotify.borrow().clone()
    }

    pub fn notice(&self, text: &str, warning: bool) {
        if warning {
            log::warn!("{text}");
        }
        if let Some(w) = self.window.get() {
            w.notice(text, warning);
        }
    }

    // --- command line & lifecycle --------------------------------------------------------------

    fn command_line(&self, cmd: &gio::ApplicationCommandLine) -> glib::ExitCode {
        let args: Vec<String> = cmd.arguments().iter().map(|a| a.to_string_lossy().into_owned()).collect();
        let cli = match <Cli as clap::Parser>::try_parse_from(&args) {
            Ok(c) => c,
            Err(e) => {
                // main() validated the arguments already; this only guards odd remote callers.
                log::warn!("ignoring invalid remote arguments: {e}");
                return glib::ExitCode::FAILURE;
            }
        };
        let first = !self.started.replace(true);
        let commands = cli.commands();
        // `rtify --reload-theme` / `--quit` from a hook must not launch a whole new instance.
        if first && !commands.is_empty() && commands.iter().all(|c| matches!(c, Command::Quit | Command::ReloadTheme)) {
            self.quit();
            return glib::ExitCode::SUCCESS;
        }
        if let Some(file) = &cli.lyrics_file {
            let path = if file.is_absolute() {
                file.clone()
            } else {
                cmd.cwd().map(|c| c.join(file)).unwrap_or_else(|| file.clone())
            };
            self.open_lyrics_file(&path);
            self.win().show_page("lyrics");
        }
        for c in &commands {
            self.command(*c);
        }
        let wants_window = commands.iter().any(|c| !c.is_background()) || cli.lyrics_file.is_some();
        let start_hidden = cli.background || self.config.borrow().config.window.start_hidden;
        if (first && !start_hidden && commands.iter().all(|c| !c.is_background()))
            || (!first && commands.is_empty() && !cli.background)
            || wants_window
        {
            self.present();
        }
        glib::ExitCode::SUCCESS
    }

    fn present(&self) {
        let w = self.win();
        w.window.present();
    }

    fn quit(&self) {
        self.flush();
        if let Some(handle) = self.tray.borrow_mut().take() {
            rt().block_on(async move {
                let _ = tokio::time::timeout(Duration::from_millis(500), handle.shutdown()).await;
            });
        }
        self.hold.borrow_mut().take();
        if let Some(w) = self.window.get() {
            w.window.destroy();
        }
        self.gtk_app.quit();
    }

    /// Persist everything now (quit, shutdown). Saves also happen as things change, so a crash or
    /// kill loses at most the last debounce interval.
    fn flush(&self) {
        if let Some(w) = self.window.get() {
            self.save_geometry(&w.window);
        }
        if let Some(id) = self.save_config.borrow_mut().take() {
            id.remove();
        }
        if let Err(e) = self.config.borrow().save() {
            log::warn!("saving config: {e}");
        }
        if let Some(id) = self.save_session.borrow_mut().take() {
            id.remove();
        }
        self.write_session();
    }

    pub fn save_geometry(&self, window: &gtk::ApplicationWindow) {
        if !self.config.borrow().config.window.remember_geometry || self.config.borrow().config.window.compact {
            return;
        }
        let maximized = window.is_maximized();
        let (w, h) = (window.width(), window.height());
        self.update_config(|c| {
            c.window.maximized = maximized;
            if !maximized && w > 0 && h > 0 {
                c.window.width = w;
                c.window.height = h;
            }
        });
    }

    /// Mutate the config and schedule a debounced write.
    pub fn update_config(&self, f: impl FnOnce(&mut Config)) {
        let before = self.config.borrow().config.clone();
        f(&mut self.config.borrow_mut().config);
        if self.config.borrow().config == before {
            return;
        }
        if let Some(id) = self.save_config.borrow_mut().take() {
            id.remove();
        }
        let app = self.clone();
        *self.save_config.borrow_mut() = Some(glib::timeout_add_local_once(Duration::from_millis(400), move || {
            app.save_config.borrow_mut().take();
            if let Err(e) = app.config.borrow().save() {
                app.notice(&format!("could not save settings: {e}"), true);
            }
        }));
    }

    fn schedule_session_save(&self) {
        if self.save_session.borrow().is_some() {
            return;
        }
        let app = self.clone();
        *self.save_session.borrow_mut() = Some(glib::timeout_add_local_once(Duration::from_secs(2), move || {
            app.save_session.borrow_mut().take();
            app.write_session();
        }));
    }

    fn write_session(&self) {
        if let Some(account) = self.account.borrow().clone() {
            let pb = self.playback.borrow().clone();
            let page = self.window.get().map(|w| w.page()).filter(|p| p != "login");
            let mut s = self.session.borrow_mut();
            let entry = s.account_mut(&account);
            pb.remember(entry);
            if let Some(p) = page {
                entry.page = p;
            }
        }
        if let Err(e) = self.session.borrow().save() {
            log::warn!("saving session: {e}");
        }
    }

    pub fn remember_selection(&self, uri: &str) {
        if let Some(account) = self.account.borrow().clone() {
            self.session.borrow_mut().account_mut(&account).library_selection = Some(uri.to_owned());
            self.schedule_session_save();
        }
    }

    pub fn visibility_changed(&self) {
        let visible = self.win().window.is_visible();
        if let Some(h) = self.tray.borrow().clone() {
            rt().spawn(async move {
                h.update(|t| t.state.window_visible = visible).await;
            });
        }
        if visible && self.logged_in.get() {
            // Catch up right away instead of waiting for the slow hidden-window interval.
            self.schedule_poll(Duration::from_millis(100));
        }
    }

    // --- actions -------------------------------------------------------------------------------

    fn register_actions(&self) {
        for c in Command::ALL {
            let action = gio::SimpleAction::new(c.name(), None);
            let app = self.clone();
            action.connect_activate(move |_, _| app.command(c));
            self.gtk_app.add_action(&action);
        }
        let accels: &[(Command, &[&str])] = &[
            (Command::PlayPause, &["<Ctrl>space"]),
            (Command::Next, &["<Ctrl>Right"]),
            (Command::Previous, &["<Ctrl>Left"]),
            (Command::ToggleShuffle, &["<Ctrl>s"]),
            (Command::CycleRepeat, &["<Ctrl>r"]),
            (Command::VolumeUp, &["<Ctrl>Up"]),
            (Command::VolumeDown, &["<Ctrl>Down"]),
            (Command::ToggleLyrics, &["<Ctrl>y"]),
            (Command::FocusSearch, &["<Ctrl>l", "<Ctrl>k"]),
            (Command::ToggleCompact, &["<Ctrl><Shift>m"]),
            (Command::Preferences, &["<Ctrl>comma"]),
            (Command::ReloadTheme, &["F5"]),
            (Command::Quit, &["<Ctrl>q"]),
        ];
        for (c, keys) in accels {
            self.gtk_app.set_accels_for_action(&format!("app.{}", c.name()), keys);
        }
    }

    pub fn command(&self, c: Command) {
        let needs_spotify = matches!(
            c,
            Command::PlayPause
                | Command::Next
                | Command::Previous
                | Command::ToggleShuffle
                | Command::CycleRepeat
                | Command::VolumeUp
                | Command::VolumeDown
        );
        if needs_spotify && !self.logged_in.get() {
            // e.g. a media key right after login started: run once connected.
            self.queued.borrow_mut().push(c);
            return;
        }
        let w = self.win();
        match c {
            Command::ToggleWindow => {
                if w.window.is_visible() && w.window.is_active() {
                    self.command(Command::HideWindow);
                } else {
                    self.present();
                }
            }
            Command::ShowWindow => self.present(),
            Command::HideWindow => {
                self.save_geometry(&w.window);
                if self.tray.borrow().is_none() && !self.config.borrow().config.window.minimize_to_tray {
                    self.quit();
                } else {
                    w.window.set_visible(false);
                }
            }
            Command::ToggleLyrics => {
                if w.window.is_visible() && w.page() == "lyrics" {
                    w.show_page("library");
                } else {
                    w.show_page("lyrics");
                    self.present();
                }
                self.schedule_session_save();
            }
            Command::PlayPause => self.play_pause(),
            Command::Next => self.simple(|c| async move { c.next().await }),
            Command::Previous => {
                // Like the official clients: restart the track first if we're past 3 s.
                if self.playback.borrow().position_now() > 3000 {
                    self.player(PlayerOp::Seek(0));
                } else {
                    self.simple(|c| async move { c.previous().await });
                }
            }
            Command::ToggleShuffle => {
                let on = !self.playback.borrow().shuffle;
                self.playback.borrow_mut().shuffle = on;
                self.refresh_player_ui();
                self.simple(move |c| async move { c.shuffle(on, None).await });
            }
            Command::CycleRepeat => {
                let r = self.playback.borrow().repeat.next();
                self.playback.borrow_mut().repeat = r;
                self.refresh_player_ui();
                self.simple(move |c| async move { c.repeat(r.as_str(), None).await });
            }
            Command::VolumeUp | Command::VolumeDown => {
                let cur = self.playback.borrow().device.as_ref().and_then(|d| d.volume_percent).unwrap_or(50) as i32;
                let v = (cur + if c == Command::VolumeUp { 5 } else { -5 }).clamp(0, 100) as u32;
                self.player(PlayerOp::SetVolume(v));
            }
            Command::FocusSearch => {
                self.present();
                w.focus_search();
            }
            Command::ToggleCompact => {
                let compact = !self.config.borrow().config.window.compact;
                if compact {
                    self.save_geometry(&w.window);
                }
                self.update_config(|c| c.window.compact = compact);
                w.set_compact(compact);
                if !compact {
                    let (ww, hh) = {
                        let c = &self.config.borrow().config.window;
                        (c.width, c.height)
                    };
                    w.window.set_default_size(ww, hh);
                }
            }
            Command::ReloadTheme => {
                self.theme.reload();
                if let Some(s) = self.theme.status.borrow().clone() {
                    self.notice(&s, true);
                }
            }
            Command::Preferences => {
                self.present();
                crate::ui::preferences::open(self);
            }
            Command::Quit => self.quit(),
        }
    }

    pub fn set_prefs_window(&self, w: Option<gtk::Window>) {
        *self.prefs.borrow_mut() = w;
    }

    pub fn prefs_window(&self) -> Option<gtk::Window> {
        self.prefs.borrow().clone()
    }

    // --- Spotify session -----------------------------------------------------------------------

    fn connect_spotify(&self) {
        if demo_mode() {
            let app = self.clone();
            let client = self.client();
            spawn_then(
                async move {
                    client
                        .set_token(Some(auth::Token {
                            access_token: "demo".into(),
                            refresh_token: None,
                            expires_at: std::time::SystemTime::now() + Duration::from_secs(86_400 * 365),
                            scope: String::new(),
                        }))
                        .await
                },
                move |_| app.logged_in_ok(),
            );
            return;
        }
        let client_id = self.config.borrow().config.spotify.client_id.clone();
        if client_id.is_empty() {
            self.win().show_page("login");
            return;
        }
        self.win().login.status.set_text("restoring session…");
        let client = self.client();
        let app = self.clone();
        spawn_then(
            async move {
                let refresh = match tokens::load(tokens::SPOTIFY_REFRESH, &client_id).await {
                    Ok(Some(r)) => r,
                    Ok(None) => return Err("not signed in yet".to_owned()),
                    Err(e) => return Err(format!("Secret Service unavailable ({e}); sign in again")),
                };
                let token = auth::refresh(client.http(), &client_id, &refresh)
                    .await
                    .map_err(|e| e.to_string())?;
                client.set_token(Some(token)).await;
                Ok(())
            },
            move |r: Result<(), String>| match r {
                Ok(()) => app.logged_in_ok(),
                Err(e) => {
                    app.win().login.status.set_text(&e);
                    app.win().show_page("login");
                }
            },
        );
    }

    pub fn login(&self, client_id: &str) {
        if client_id.is_empty() {
            self.win().login.status.set_text("enter your app's client ID first");
            return;
        }
        if client_id != self.config.borrow().config.spotify.client_id {
            let id = client_id.to_owned();
            self.update_config(|c| c.spotify.client_id = id);
            *self.spotify.borrow_mut() = Self::make_client(&self.http, client_id);
        }
        let pkce = match auth::Pkce::new() {
            Ok(p) => p,
            Err(e) => return self.win().login.status.set_text(&e.to_string()),
        };
        let port = self.config.borrow().config.spotify.redirect_port;
        let url = pkce.authorize_url(client_id, port);
        let login = self.win().login.clone();
        login.button.set_sensitive(false);
        login.status.set_text("waiting for the browser…");
        let client = self.client();
        let client_id = client_id.to_owned();
        let app = self.clone();
        let (url_tx, url_rx) = async_channel::bounded::<()>(1);
        spawn_then(
            async move {
                // Listen before opening the browser so a busy port fails fast.
                let listener = auth::listen(port).await?;
                let _ = url_tx.send(()).await;
                let code = auth::wait_for_code(listener, &pkce.state, Duration::from_secs(300)).await?;
                let token = auth::exchange_code(client.http(), &client_id, &code, &pkce.verifier, port).await?;
                let stored = match &token.refresh_token {
                    Some(r) => tokens::store(tokens::SPOTIFY_REFRESH, &client_id, r)
                        .await
                        .map_err(|e| e.to_string()),
                    None => Ok(()),
                };
                client.set_token(Some(token)).await;
                Ok::<_, auth::AuthError>(stored)
            },
            move |r| {
                login.button.set_sensitive(true);
                match r {
                    Ok(stored) => {
                        if let Err(e) = stored {
                            app.notice(&format!("signed in, but the login could not be stored in the Secret Service ({e}); you will need to sign in again next time"), true);
                        }
                        login.status.set_text("");
                        app.logged_in_ok();
                    }
                    Err(e) => login.status.set_text(&e.to_string()),
                }
            },
        );
        let window = self.win().window.clone();
        glib::spawn_future_local(async move {
            if url_rx.recv().await.is_ok() {
                gtk::UriLauncher::new(&url).launch(Some(&window), gio::Cancellable::NONE, |r| {
                    if let Err(e) = r {
                        log::warn!("could not open browser: {e}");
                    }
                });
            }
        });
    }

    pub fn logout(&self) {
        let client_id = self.config.borrow().config.spotify.client_id.clone();
        let client = self.client();
        rt().spawn(async move {
            client.set_token(None).await;
            let _ = tokens::delete(tokens::SPOTIFY_REFRESH, &client_id).await;
        });
        self.logged_in.set(false);
        self.poll_gen.set(self.poll_gen.get() + 1);
        self.win().set_online(false, None);
        self.win().show_page("login");
    }

    fn logged_in_ok(&self) {
        self.logged_in.set(true);
        self.reconciled.set(false);
        let client = self.client();
        let app = self.clone();
        spawn_then(async move { client.me().await }, move |r| match r {
            Ok(me) => {
                let who = me.display_name.clone().unwrap_or(me.id.clone());
                *app.account.borrow_mut() = Some(me.id.clone());
                *app.display_name.borrow_mut() = Some(who.clone());
                app.win().set_online(true, Some(&who));
                if app.win().page() == "login" || app.win().page().is_empty() {
                    let page = app
                        .session
                        .borrow()
                        .data
                        .accounts
                        .get(&me.id)
                        .map(|s| s.page.clone())
                        .unwrap_or_default();
                    app.win().show_page(if page.is_empty() { "library" } else { &page });
                }
                if me.product.as_deref().is_some_and(|p| p != "premium") {
                    app.notice("playback control needs Spotify Premium; browsing still works", true);
                }
                app.load_library();
                app.refresh_devices();
                app.poll_now();
                for c in app.queued.take() {
                    app.command(c);
                }
            }
            Err(e) => {
                app.logged_in.set(false);
                app.win().login.status.set_text(&e.to_string());
                app.win().show_page("login");
            }
        });
    }

    // --- playback state ------------------------------------------------------------------------

    fn schedule_poll(&self, delay: Duration) {
        let generation = self.poll_gen.get() + 1;
        self.poll_gen.set(generation);
        let app = self.clone();
        glib::timeout_add_local_once(delay, move || {
            if app.poll_gen.get() == generation {
                app.poll_now();
            }
        });
    }

    fn poll_now(&self) {
        if !self.logged_in.get() {
            return;
        }
        let generation = self.poll_gen.get() + 1;
        self.poll_gen.set(generation);
        let client = self.client();
        let app = self.clone();
        spawn_then(async move { client.player().await }, move |r| {
            match r {
                Ok(state) => {
                    app.retry_after.set(None);
                    if app.failures.get() > 0 {
                        app.win().set_online(true, app.display_name.borrow().as_deref());
                    }
                    app.failures.set(0);
                    app.apply_state(Playback::from_remote(state));
                }
                Err(ApiError::RateLimited(s)) => app.retry_after.set(Some(Duration::from_secs(s))),
                Err(ApiError::NotLoggedIn) => return,
                Err(e) => {
                    app.failures.set(app.failures.get() + 1);
                    if app.failures.get() == 2 {
                        app.win().set_online(false, None);
                        app.notice(&format!("cannot reach Spotify: {e}"), true);
                    }
                }
            }
            if app.poll_gen.get() == generation {
                app.schedule_next_poll();
            }
        });
    }

    fn schedule_next_poll(&self) {
        let (mode, fixed) = {
            let c = &self.config.borrow().config.background;
            (c.polling_mode, c.polling_interval_ms)
        };
        let pb = self.playback.borrow();
        let inputs = PollInputs {
            window_visible: self.win().window.is_visible(),
            playing: pb.playing,
            remaining_ms: pb.remaining_ms(),
            failures: self.failures.get(),
            retry_after: self.retry_after.get(),
        };
        drop(pb);
        self.schedule_poll(next_poll(mode, fixed, inputs));
    }

    fn apply_state(&self, pb: Playback) {
        let old_uri = self.playback.borrow().track_uri().map(String::from);
        let was_stale = self.playback.borrow().stale;
        let new_uri = pb.track_uri().map(String::from);
        let changed = old_uri != new_uri || was_stale;
        *self.playback.borrow_mut() = pb.clone();
        if changed {
            self.saved_current.set(None);
            self.track_changed(&pb, was_stale);
        }
        self.refresh_player_ui();
        self.win().collection.mark_playing(new_uri.as_deref());
        self.update_tray();
        self.update_lyric_clock();
        self.schedule_session_save();
        if !self.reconciled.get() {
            self.reconciled.set(true);
            self.reconcile();
        }
    }

    fn refresh_player_ui(&self) {
        let pb = self.playback.borrow().clone();
        self.win().update_playback(&pb, self.saved_current.get());
    }

    fn start_progress_timer(&self) {
        let app = self.clone();
        glib::timeout_add_local(Duration::from_millis(250), move || {
            if app.win().window.is_visible() {
                let pb = app.playback.borrow();
                app.win().player.tick(&pb);
            }
            glib::ControlFlow::Continue
        });
    }

    fn track_changed(&self, pb: &Playback, first_live: bool) {
        let Some(track) = pb.track.clone() else {
            self.win().set_cover(None);
            return;
        };
        if let Some(url) = pick_image(track.images(), 640).map(|i| i.url.clone()) {
            self.load_cover(&url);
        }
        self.refresh_queue();
        // Liked state for the heart button.
        if !track.uri.is_empty() && !track.is_local {
            let client = self.client();
            let app = self.clone();
            let uri = track.uri.clone();
            spawn_then(async move { client.is_saved(&uri).await }, move |r| {
                if let Ok(saved) = r {
                    app.saved_current.set(Some(saved));
                    app.refresh_player_ui();
                }
            });
        }
        let notify = self.config.borrow().config.background.notifications;
        let hidden = !self.win().window.is_visible();
        if !first_live && pb.playing && (notify == Notifications::Always || (notify == Notifications::Background && hidden)) {
            let n = gio::Notification::new(&track.name);
            n.set_body(Some(&format!("{} · {}", track.artist_names(), track.album_name())));
            n.set_default_action(&format!("app.{}", Command::ShowWindow.name()));
            self.gtk_app.send_notification(Some("track"), &n);
        }
        if self.config.borrow().config.lyrics.spotify_sync {
            self.lookup_track_lyrics(&track);
        }
    }

    fn load_cover(&self, url: &str) {
        let app = self.clone();
        let wanted = url.to_owned();
        self.artwork.load(&self.http, url, move |texture| {
            let current = app
                .playback
                .borrow()
                .track
                .as_ref()
                .and_then(|t| pick_image(t.images(), 640))
                .map(|i| i.url.clone());
            if current.as_deref() == Some(wanted.as_str()) {
                app.win().set_cover(texture.as_ref());
            }
        });
        if self.config.borrow().config.appearance.palette_source == crate::config::PaletteSource::AlbumArt {
            let theme = self.theme.clone();
            self.artwork.accent(&self.http, url, move |accent| theme.set_album_accent(accent));
        }
    }

    fn reconcile(&self) {
        let (restore, resume) = {
            let p = &self.config.borrow().config.playback;
            (p.restore_shuffle_repeat, p.resume_on_startup)
        };
        let saved = self
            .account
            .borrow()
            .as_ref()
            .and_then(|a| self.session.borrow().data.accounts.get(a).cloned());
        let plan = reconnect_plan(restore, resume, saved.as_ref(), &self.playback.borrow(), &self.devices.borrow());
        if plan.is_empty() {
            return;
        }
        let client = self.client();
        let app = self.clone();
        spawn_then(
            async move {
                // One at a time: Spotify does not order concurrent playback commands.
                for step in plan {
                    let r = match &step {
                        ReconnectStep::Resume { context, track, device } => {
                            client
                                .play(Some(&PlayRequest::context(context, track.as_deref())), device.as_deref())
                                .await
                        }
                        ReconnectStep::SetShuffle(on) => client.shuffle(*on, None).await,
                        ReconnectStep::SetRepeat(r) => client.repeat(r.as_str(), None).await,
                    };
                    if let Err(e) = r {
                        return Err(format!("could not restore {step:?}: {e}"));
                    }
                    tokio::time::sleep(Duration::from_millis(300)).await;
                }
                Ok(())
            },
            move |r| {
                if let Err(e) = r {
                    app.notice(&e, true);
                }
                app.schedule_poll(Duration::from_millis(300));
            },
        );
    }

    /// Run a playback command, then refresh state shortly after.
    fn simple<F, Fut>(&self, f: F)
    where
        F: FnOnce(SpotifyClient) -> Fut,
        Fut: std::future::Future<Output = Result<(), ApiError>> + Send + 'static,
    {
        let fut = f(self.client());
        let app = self.clone();
        spawn_then(fut, move |r| app.after_command(r));
    }

    fn after_command(&self, r: Result<(), ApiError>) {
        if let Err(e) = r {
            let msg = if e.is_no_device() {
                "no active Spotify device — start Spotify somewhere or pick a device under [ DEVICES ]".to_owned()
            } else if e.needs_premium() {
                "this needs Spotify Premium".to_owned()
            } else {
                e.to_string()
            };
            self.notice(&msg, true);
        }
        self.schedule_poll(Duration::from_millis(350));
    }

    fn play_pause(&self) {
        let (playing, has_device) = {
            let pb = self.playback.borrow();
            (pb.playing, pb.device.is_some())
        };
        if playing {
            self.playback.borrow_mut().playing = false;
            self.refresh_player_ui();
            return self.simple(|c| async move { c.pause().await });
        }
        if has_device {
            self.playback.borrow_mut().playing = true;
            self.refresh_player_ui();
            return self.simple(|c| async move { c.play(None, None).await });
        }
        // Nothing active: wake the last-used device (or the first one) explicitly.
        let saved = self
            .account
            .borrow()
            .as_ref()
            .and_then(|a| self.session.borrow().data.accounts.get(a).and_then(|s| s.device_id.clone()));
        let devices = self.devices.borrow().clone();
        let target = saved
            .filter(|id| devices.iter().any(|d| d.id.as_deref() == Some(id)))
            .or_else(|| devices.first().and_then(|d| d.id.clone()));
        match target {
            Some(id) => self.simple(move |c| async move { c.transfer(&id, true).await }),
            None => self.notice("no Spotify device available — open Spotify on a phone, speaker or computer", true),
        }
    }

    pub fn player(&self, op: PlayerOp) {
        match op {
            PlayerOp::Seek(ms) => {
                {
                    let mut pb = self.playback.borrow_mut();
                    pb.progress_ms = ms;
                    pb.fetched_at = Some(std::time::Instant::now());
                }
                self.update_lyric_clock();
                self.simple(move |c| async move { c.seek(ms).await });
            }
            PlayerOp::SetVolume(v) => {
                if let Some(d) = self.playback.borrow_mut().device.as_mut() {
                    d.volume_percent = Some(v);
                }
                self.simple(move |c| async move { c.volume(v).await });
            }
            PlayerOp::Like(on) => {
                let Some(uri) = self.playback.borrow().track_uri().map(String::from) else {
                    return;
                };
                self.saved_current.set(Some(on));
                self.refresh_player_ui();
                self.simple(move |c| async move { c.set_saved(&uri, on).await });
            }
        }
    }

    pub fn refresh_devices(&self) {
        let client = self.client();
        let app = self.clone();
        spawn_then(async move { client.devices().await }, move |r| match r {
            Ok(devs) => {
                app.win().player.set_devices(&devs);
                *app.devices.borrow_mut() = devs;
            }
            Err(e) => app.notice(&format!("devices: {e}"), true),
        });
    }

    pub fn transfer_to_row(&self, index: usize) {
        let Some(id) = self.devices.borrow().get(index).and_then(|d| d.id.clone()) else {
            return;
        };
        let play = self.playback.borrow().playing;
        let app = self.clone();
        let client = self.client();
        spawn_then(async move { client.transfer(&id, play).await }, move |r| {
            app.after_command(r);
            let a = app.clone();
            glib::timeout_add_local_once(Duration::from_millis(800), move || a.refresh_devices());
        });
    }

    fn update_tray(&self) {
        let Some(h) = self.tray.borrow().clone() else { return };
        let pb = self.playback.borrow();
        let state = TrayState {
            playing: pb.playing,
            shuffle: pb.shuffle,
            repeat: match pb.repeat {
                Repeat::Off => "off".into(),
                Repeat::Context => "all".into(),
                Repeat::Track => "one".into(),
            },
            title: pb.track.as_ref().map(|t| t.name.clone()).unwrap_or_default(),
            artist: pb.track.as_ref().map(|t| t.artist_names()).unwrap_or_default(),
            window_visible: self.win().window.is_visible(),
            online: self.logged_in.get(),
        };
        drop(pb);
        rt().spawn(async move {
            h.update(|t| {
                if t.state != state {
                    t.state = state;
                }
            })
            .await;
        });
    }

    // --- library -------------------------------------------------------------------------------

    fn load_library(&self) {
        let client = self.client();
        let app = self.clone();
        spawn_then(
            async move {
                let mut playlists = Vec::new();
                let mut offset = 0;
                for _ in 0..20 {
                    let page = client.my_playlists(offset).await?;
                    let n = page.items.len() as u32;
                    playlists.extend(page.items.iter().map(Collection::from_playlist));
                    if page.next.is_none() || n == 0 {
                        break;
                    }
                    offset += n;
                }
                let mut albums = Vec::new();
                let mut offset = 0;
                for _ in 0..4 {
                    let page = client.saved_albums(offset).await?;
                    let n = page.items.len() as u32;
                    albums.extend(page.items.iter().map(|s| Collection::from_album(&s.album)));
                    if page.next.is_none() || n == 0 {
                        break;
                    }
                    offset += n;
                }
                Ok::<_, ApiError>((playlists, albums))
            },
            move |r| match r {
                Ok((playlists, albums)) => {
                    let side = &app.win().sidebar;
                    side.set_playlists(playlists);
                    side.set_albums(albums);
                    // Restore the open collection from the last session; first run shows the
                    // first playlist rather than an empty view.
                    let saved = app
                        .account
                        .borrow()
                        .as_ref()
                        .and_then(|a| app.session.borrow().data.accounts.get(a).and_then(|s| s.library_selection.clone()));
                    let restored = saved.filter(|u| u != SEARCH_URI).and_then(|uri| side.find(&uri));
                    if let Some(c) = restored.or_else(|| side.first_playlist()) {
                        app.open_collection(c, true);
                    }
                }
                Err(e) => app.notice(&format!("library: {e}"), true),
            },
        );
    }

    /// Show a collection in the main view and load its tracks. `record` adds it to back/forward.
    pub fn open_collection(&self, c: Collection, record: bool) {
        if record {
            self.push_history(Nav::Collection(c.clone()));
        }
        let w = self.win();
        w.collection.show(&c);
        w.sidebar.mark_selected(Some(&c.uri));
        if w.page() != "library" {
            w.show_page("library");
        }
        self.remember_selection(&c.uri);
        self.load_tracks(&c.uri, 0);
    }

    fn push_history(&self, nav: Nav) {
        let mut h = self.history.borrow_mut();
        let pos = self.history_pos.get();
        h.truncate(pos + 1);
        h.push(nav);
        if h.len() > 50 {
            h.remove(0);
        }
        self.history_pos.set(h.len() - 1);
        let len = h.len();
        drop(h);
        self.win().set_history_buttons(len > 1, false);
    }

    pub fn navigate_history(&self, delta: i32) {
        let len = self.history.borrow().len();
        let pos = self.history_pos.get() as i64 + delta as i64;
        if pos < 0 || pos >= len as i64 {
            return;
        }
        self.history_pos.set(pos as usize);
        let nav = self.history.borrow()[pos as usize].clone();
        self.win().set_history_buttons(pos > 0, (pos as usize) + 1 < len);
        match nav {
            Nav::Collection(c) => self.open_collection(c, false),
            Nav::Search(q) => self.search(&q, false),
        }
    }

    pub fn load_tracks(&self, uri: &str, offset: u32) {
        let view = self.win().collection.clone();
        view.state.borrow_mut().loading = true;
        let client = self.client();
        let uri = uri.to_owned();
        let kind = crate::spotify::client::uri_kind(&uri).to_owned();
        let id = crate::spotify::client::uri_id(&uri).to_owned();
        let liked = uri == LIKED_URI;
        spawn_then(
            async move {
                if liked {
                    let p = client.saved_tracks(offset).await?;
                    let next = p.next.as_ref().map(|_| offset + p.items.len() as u32);
                    let rows = p
                        .items
                        .into_iter()
                        .map(|s| TrackRow {
                            track: s.track,
                            added_at: s.added_at,
                        })
                        .collect();
                    return Ok((rows, next));
                }
                if kind == "album" {
                    let a = client.album(&id).await?.ok_or(ApiError::Status(404, "album not found".into()))?;
                    let album_ref = crate::spotify::model::AlbumRef {
                        uri: a.album.uri.clone(),
                        name: a.album.name.clone(),
                        images: a.album.images.clone(),
                        ..Default::default()
                    };
                    let rows = a
                        .tracks
                        .items
                        .into_iter()
                        .map(|mut t| {
                            t.album.get_or_insert_with(|| album_ref.clone());
                            TrackRow::new(t)
                        })
                        .collect();
                    return Ok((rows, None));
                }
                let p = client.playlist_items(&id, offset).await?;
                let next = p.next.as_ref().map(|_| offset + p.items.len() as u32);
                let rows = p
                    .items
                    .into_iter()
                    .filter_map(|i| {
                        i.item.map(|track| TrackRow {
                            track,
                            added_at: i.added_at,
                        })
                    })
                    .collect();
                Ok((rows, next))
            },
            move |r: Result<TrackPage, ApiError>| match r {
                Ok((rows, next)) => view.append(&uri, rows, next),
                Err(ApiError::Status(403, _)) => {
                    view.set_error("Spotify doesn't let apps read this playlist (only your own and collaborative ones)")
                }
                Err(e) => view.set_error(&e.to_string()),
            },
        );
    }

    pub fn load_more_tracks(&self) {
        let view = &self.win().collection;
        let (uri, next, loading) = {
            let st = view.state.borrow();
            (view.uri(), st.next_offset, st.loading)
        };
        if let (Some(uri), Some(next), false) = (uri, next, loading)
            && uri != SEARCH_URI
        {
            self.load_tracks(&uri, next);
        }
    }

    pub fn search(&self, q: &str, record: bool) {
        if record {
            self.push_history(Nav::Search(q.to_owned()));
        }
        let client = self.client();
        let app = self.clone();
        let q = q.to_owned();
        let query = q.clone();
        spawn_then(async move { client.search(&q).await }, move |r| match r {
            Ok(res) => {
                let mut cols: Vec<Collection> = res
                    .playlists
                    .map(|p| p.items.into_iter().flatten().map(|p| Collection::from_playlist(&p)).collect())
                    .unwrap_or_default();
                cols.extend(
                    res.albums
                        .map(|a| a.items.iter().map(Collection::from_album).collect::<Vec<_>>())
                        .unwrap_or_default(),
                );
                let tracks = res.tracks.map(|t| t.items).unwrap_or_default();
                let w = app.win();
                w.collection.show_search(&query, cols, tracks);
                w.sidebar.mark_selected(None);
                w.show_page("library");
            }
            Err(e) => app.notice(&format!("search: {e}"), true),
        });
    }

    /// Images for thumbnails and covers, through the shared cache.
    pub fn load_image(&self, url: &str, cb: impl FnOnce(Option<gtk::gdk::Texture>) + 'static) {
        self.artwork.load(&self.http, url, cb);
    }

    fn refresh_queue(&self) {
        let client = self.client();
        let app = self.clone();
        spawn_then(async move { client.queue().await }, move |r| match r {
            Ok(q) => app.win().now_playing.set_queue(&q),
            Err(e) => log::debug!("queue: {e}"),
        });
    }

    pub fn set_now_playing_panel(&self, visible: bool) {
        self.update_config(|c| c.window.now_playing_panel = visible);
        self.win().set_now_playing_panel(visible);
    }

    /// Turn shuffle on/off explicitly (the shuffle-play button).
    pub fn set_shuffle(&self, on: bool) {
        self.playback.borrow_mut().shuffle = on;
        self.refresh_player_ui();
        self.simple(move |c| async move { c.shuffle(on, None).await });
    }

    pub fn play_from(&self, selection: &str, tracks: &[Track], index: usize) {
        let Some(track) = tracks.get(index) else { return };
        let req = match crate::spotify::client::uri_kind(selection) {
            "playlist" | "album" => PlayRequest::context(selection, Some(&track.uri)),
            _ => {
                // Liked songs and search results: queue the visible list from the clicked track.
                let uris: Vec<String> = tracks.iter().skip(index).take(100).map(|t| t.uri.clone()).collect();
                PlayRequest::tracks(uris, 0)
            }
        };
        let device = if self.playback.borrow().device.is_some() {
            None
        } else {
            self.devices.borrow().first().and_then(|d| d.id.clone())
        };
        self.simple(move |c| async move { c.play(Some(&req), device.as_deref()).await });
    }

    // --- tray & shortcuts ----------------------------------------------------------------------

    fn start_tray(&self) {
        if !self.config.borrow().config.background.tray {
            return;
        }
        let (tx, rx) = async_channel::unbounded();
        let app = self.clone();
        runtime::forward(rx, move |c| app.command(c));
        let app = self.clone();
        spawn_then(async move { tray::spawn(tx).await }, move |r| match r {
            Ok(h) => {
                *app.tray.borrow_mut() = Some(h);
                app.update_tray();
            }
            Err(e) => log::warn!("tray unavailable: {e}"),
        });
    }

    pub fn set_tray_enabled(&self, on: bool) {
        if on && self.tray.borrow().is_none() {
            self.start_tray();
        } else if !on && let Some(h) = self.tray.borrow_mut().take() {
            rt().spawn(async move { h.shutdown().await });
        }
    }

    fn start_shortcuts(&self) {
        let cfg = self.config.borrow().config.shortcuts.clone();
        if !cfg.portal {
            *self.shortcut_status.borrow_mut() = "portal disabled".into();
            return;
        }
        let (tx, rx) = async_channel::unbounded();
        let app = self.clone();
        runtime::forward(rx, move |ev| match ev {
            ShortcutEvent::Command(c) => app.command(c),
            ShortcutEvent::Bound(list) => {
                let text = list
                    .iter()
                    .map(|(id, t)| format!("{id}: {}", if t.is_empty() { "unassigned" } else { t }))
                    .collect::<Vec<_>>()
                    .join("\n");
                log::info!("global shortcuts bound:\n{text}");
                *app.shortcut_status.borrow_mut() = format!("registered with the portal\n{text}");
            }
            ShortcutEvent::Unavailable(e) => {
                *app.shortcut_status.borrow_mut() = format!("portal unavailable ({e}); use compositor bindings to `rtify --<action>`");
            }
        });
        rt().spawn(shortcuts::run(cfg.bindings, tx));
    }

    // --- lyrics --------------------------------------------------------------------------------

    pub fn view_options(&self) -> ViewOptions {
        let c = &self.config.borrow().config;
        ViewOptions {
            animation: if c.appearance.animations_enabled {
                c.lyrics.animation
            } else {
                crate::config::LyricAnimation::Off
            },
            reduced_motion: c.appearance.reduced_motion,
            romanized: c.lyrics.romanization,
            translation: c.lyrics.translation_enabled,
            translation_only: c.lyrics.translation_only,
            font_scale: c.lyrics.font_scale,
            offset_ms: c.lyrics.offset_ms,
        }
    }

    pub fn apply_lyrics_options(&self) {
        self.win().lyrics.set_options(self.view_options());
        let gate = self.config.borrow().config.lyrics.spotify_sync;
        self.win()
            .lyrics
            .set_sync_status(if gate { "sync: spotify" } else { "sync: local clock" });
    }

    fn prepare(&self, mut doc: LyricDocument) -> LyricDocument {
        romanization::apply(&mut doc);
        doc
    }

    fn show_lyrics(&self, doc: Option<LyricDocument>, source: LyricsSource, label: &str, clock: Clock) {
        *self.lyrics_source.borrow_mut() = source;
        let doc = doc.map(|d| self.prepare(d));
        self.win().lyrics.show_document(doc.as_ref(), label, clock);
        *self.lyrics_doc.borrow_mut() = doc;
        self.translate_current_lyrics();
    }

    /// `autoplay`: start the local clock right away (user picked the song).
    pub fn open_demo_lyrics(&self, index: usize, autoplay: bool) {
        let doc = demo::song(index);
        let label = doc
            .as_ref()
            .map(|d| {
                format!(
                    "demo · {} — {}",
                    d.title.clone().unwrap_or_default(),
                    d.artist.clone().unwrap_or_default()
                )
            })
            .unwrap_or_default();
        let mut clock = LyricClock::new(doc.as_ref().map(|d| d.duration_ms() + 1500));
        clock.set_playing(autoplay);
        self.show_lyrics(doc, LyricsSource::Demo(index), &label, Clock::Local(clock));
    }

    pub fn open_lyrics_file(&self, path: &Path) {
        match provider::load_file(path) {
            Ok(doc) => {
                let mut clock = LyricClock::new(Some(doc.duration_ms() + 1500));
                clock.set_playing(true);
                let label = format!("file · {}", path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
                self.show_lyrics(Some(doc), LyricsSource::File(path.to_owned()), &label, Clock::Local(clock));
            }
            Err(e) => self.notice(&e.to_string(), true),
        }
    }

    /// Capability gate on: look for a user-supplied file matching the playing track.
    fn lookup_track_lyrics(&self, track: &Track) {
        let query = TrackQuery {
            id: track.id.clone().unwrap_or_default(),
            title: track.name.clone(),
            artists: track.artists.iter().map(|a| a.name.clone()).collect(),
            album: track.album_name().to_owned(),
            duration_ms: track.duration_ms,
        };
        let key = query.key();
        if let Some(doc) = self.lyric_cache.borrow_mut().get(&key).cloned() {
            return self.show_track_lyrics(Some(doc), &key, track);
        }
        let app = self.clone();
        let t = track.clone();
        glib::spawn_future_local(async move {
            let q = query.clone();
            let found = gio::spawn_blocking(move || {
                LocalFileProvider {
                    dirs: vec![paths::lyrics_dir()],
                }
                .lookup(&q)
            })
            .await;
            let doc = match found {
                Ok(Ok(d)) => d,
                Ok(Err(e)) => {
                    app.notice(&e.to_string(), true);
                    None
                }
                Err(_) => None,
            };
            if let Some(d) = &doc {
                app.lyric_cache.borrow_mut().insert(d.clone());
            }
            app.show_track_lyrics(doc, &query.key(), &t);
        });
    }

    fn show_track_lyrics(&self, doc: Option<LyricDocument>, key: &str, track: &Track) {
        let label = format!("{} — {}", track.name, track.artist_names());
        if doc.is_none() {
            // Keep demo/file content if nothing matches; tell the user where files go.
            self.win().lyrics.set_sync_status(&format!(
                "sync: spotify · no file for this track in {}",
                paths::lyrics_dir().display()
            ));
            return;
        }
        self.apply_lyrics_options();
        let clock = Clock::External(self.playback.borrow().snapshot());
        self.show_lyrics(doc, LyricsSource::Track(key.to_owned()), &label, clock);
    }

    fn update_lyric_clock(&self) {
        if matches!(*self.lyrics_source.borrow(), LyricsSource::Track(_)) {
            let snap = self.playback.borrow().snapshot();
            self.win().lyrics.view.set_clock(Clock::External(snap));
        }
    }

    /// Called when the capability gate is toggled in preferences.
    pub fn lyrics_gate_changed(&self) {
        self.apply_lyrics_options();
        let gate = self.config.borrow().config.lyrics.spotify_sync;
        let track = self.playback.borrow().track.clone();
        if gate {
            if let Some(t) = track {
                self.lookup_track_lyrics(&t);
            }
        } else if matches!(*self.lyrics_source.borrow(), LyricsSource::Track(_)) {
            self.open_demo_lyrics(0, false);
        }
    }

    pub fn set_deepl_key(&self, key: &str) {
        let key = key.trim().to_owned();
        *self.deepl_key.borrow_mut() = Some(key.clone()).filter(|k| !k.is_empty());
        let app = self.clone();
        spawn_then(
            async move {
                if key.is_empty() {
                    tokens::delete(tokens::DEEPL_KEY, "default").await
                } else {
                    tokens::store(tokens::DEEPL_KEY, "default", &key).await
                }
            },
            move |r| {
                if let Err(e) = r {
                    app.notice(
                        &format!("could not store the DeepL key in the Secret Service: {e} (kept for this session)"),
                        true,
                    );
                }
            },
        );
    }

    /// Opt-in: translate the shown document if translation is enabled and a key is stored.
    pub fn translate_current_lyrics(&self) {
        let (enabled, target) = {
            let l = &self.config.borrow().config.lyrics;
            (l.translation_enabled, l.translation_language.clone())
        };
        let Some(doc) = self.lyrics_doc.borrow().clone() else { return };
        if !enabled || doc.translation.as_ref().is_some_and(|t| t.language.eq_ignore_ascii_case(&target)) || target.is_empty() {
            return;
        }
        let cached_key = self.deepl_key.borrow().clone();
        let http = self.http.clone();
        let app = self.clone();
        let source = self.lyrics_source.borrow().clone();
        spawn_then(
            async move {
                let key = match cached_key {
                    Some(k) => k,
                    None => tokens::load(tokens::DEEPL_KEY, "default")
                        .await
                        .ok()
                        .flatten()
                        .ok_or(translation::TranslationError::NoKey)?,
                };
                let mut doc = doc;
                translation::translate(&http, &key, &mut doc, &target).await?;
                Ok::<_, translation::TranslationError>((key, doc))
            },
            move |r| match r {
                Ok((key, doc)) => {
                    *app.deepl_key.borrow_mut() = Some(key);
                    if *app.lyrics_source.borrow() != source {
                        return;
                    }
                    let clock = app.win().lyrics.view.clock();
                    app.win().lyrics.view.set_document(Some(doc.clone()));
                    app.win().lyrics.view.set_clock(clock);
                    app.win().lyrics.set_attribution(Some(&doc));
                    *app.lyrics_doc.borrow_mut() = Some(doc);
                }
                Err(translation::TranslationError::NoKey) => {
                    app.win().lyrics.set_sync_status("translation: add a DeepL API key in settings");
                }
                Err(e) => app.notice(&e.to_string(), true),
            },
        );
    }
}
