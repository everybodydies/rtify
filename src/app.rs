//! The application: one GTK process that stays alive (tray, shortcuts, polling) while the window
//! is hidden, until an explicit Quit.

use std::cell::{Cell, OnceCell, RefCell};
use std::ops::Deref;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gtk::{gio, glib, prelude::*};

use crate::cli::Cli;
use crate::commands::Command;
use crate::config::{Config, Notifications};
use crate::desktop::mpris::{self, MprisEvent, MprisHandle, MprisState};
use crate::desktop::shortcuts::{self, ShortcutEvent};
use crate::desktop::tray::{self, TrayHandle, TrayState};
use crate::lyrics::provider::TrackQuery;
#[cfg(feature = "demo")]
use crate::lyrics::provider::demo;
use crate::lyrics::synchronization::LyricClock;
use crate::lyrics::{LyricDocument, google, lrclib, romanization, translation};
use crate::paths::{self, APP_ID};
use crate::persistence::{ConfigStore, SessionStore};
use crate::player::{self, Status as BuiltinStatus};
use crate::runtime::{self, rt, spawn_then};
use crate::services::artwork::Artwork;
use crate::services::history::History;
use crate::services::playback::{Playback, ReconnectStep, Repeat, reconnect_plan};
use crate::services::polling::{PollInputs, next_poll};
use crate::services::undo::{self, UndoStack};
use crate::spotify::model::{Device, Track, pick_image};
use crate::spotify::{ApiError, PlayRequest, SpotifyClient, auth, tokens};
use crate::theme::manager::ThemeManager;
use crate::ui::collection::TrackRow;
use crate::ui::library::{Collection, LIKED_URI, SEARCH_URI};
use crate::ui::lyrics::view::{Clock, ViewOptions};
use crate::ui::menu::Menu;
use crate::ui::window::MainWindow;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayerOp {
    Seek(u64),
    SetVolume(u32),
    Like(bool),
}

/// Synced lyrics run this far ahead of the reported position (as in qtify).
const LYRIC_LEAD_MS: u64 = 250;

/// What the lyrics page currently shows.
#[derive(Debug, Clone, PartialEq)]
enum LyricsSource {
    None,
    #[cfg(feature = "demo")]
    Demo(usize),
    /// Lyrics for the playing Spotify track (LRCLIB), keyed by track.
    Track(String),
}

/// Rows and the next page offset of a loaded collection.
type TrackPage = (Vec<TrackRow>, Option<u32>);

type UndoOp = undo::UndoOp<Collection>;

/// Back/forward history of the main view.
#[derive(Debug, Clone)]
enum Nav {
    Collection(Collection),
    Search(String),
    /// Home, new releases or settings.
    Page(&'static str),
}

impl PartialEq for Nav {
    /// The same place, even if the entry learned more about it (owner, images) since.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Nav::Collection(a), Nav::Collection(b)) => a.uri == b.uri,
            (Nav::Search(a), Nav::Search(b)) => a == b,
            (Nav::Page(a), Nav::Page(b)) => a == b,
            _ => false,
        }
    }
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
    /// The page the lyrics toggle goes back to.
    before_lyrics: RefCell<String>,
    /// When home and new releases were last fetched.
    home_loaded: Cell<Option<std::time::Instant>>,
    releases_loaded: Cell<Option<std::time::Instant>>,
    tray: RefCell<Option<TrayHandle>>,
    mpris: RefCell<Option<MprisHandle>>,
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
    /// LRCLIB/local results per track (including "instrumental" and "not found").
    lyric_results: RefCell<std::collections::HashMap<String, lrclib::Found>>,
    translations: RefCell<std::collections::HashMap<String, LyricDocument>>,
    deepl_key: RefCell<Option<String>>,
    pub shortcut_status: RefCell<String>,
    history: RefCell<History<Nav>>,
    undo: RefCell<UndoStack<Collection>>,
    /// Failed attempts to reach Spotify while signing in at startup (retried with backoff).
    connect_attempts: Cell<u32>,
    /// The built-in Connect device, the account it runs for, and its last status.
    builtin: RefCell<Option<player::Handle>>,
    builtin_account: RefCell<Option<String>>,
    builtin_status: RefCell<BuiltinStatus>,
    /// Bumped on every start/stop so events and lookups of an older instance are dropped.
    builtin_gen: Cell<u64>,
    builtin_restart: RefCell<Option<glib::SourceId>>,
    /// The status line in settings, while that window is open.
    pub builtin_label: RefCell<Option<glib::WeakRef<gtk::Label>>>,
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

/// An API error in words that say what to do about it.
fn explain(e: &ApiError) -> String {
    if e.is_scope() {
        "Spotify needs new permissions for this — sign out in settings and sign in again".to_owned()
    } else if e.is_no_device() {
        "no active Spotify device — start Spotify somewhere or pick a device under [ DEVICES ]".to_owned()
    } else if e.needs_premium() {
        "this needs Spotify Premium".to_owned()
    } else if let ApiError::RateLimited(s) = e {
        format!("Spotify asks to slow down · try again in {s} s")
    } else if e.is_offline() {
        "cannot reach Spotify · check the connection".to_owned()
    } else {
        e.to_string()
    }
}

fn demo_mode() -> bool {
    std::env::var_os("RTIFY_DEMO").is_some_and(|v| !v.is_empty() && v != "0")
}

/// Local wall-clock time for the clock, the greeting and release ages. In demo mode
/// `RTIFY_DEMO_NOW` (ISO 8601, e.g. `2026-10-24T20:15:00Z`) pins it, so screenshots match.
pub fn now_local() -> Option<glib::DateTime> {
    let pinned = demo_mode()
        .then(|| std::env::var("RTIFY_DEMO_NOW").ok())
        .flatten()
        .and_then(|s| glib::DateTime::from_iso8601(&s, None).ok())
        .and_then(|d| d.to_local().ok());
    pinned.or_else(|| glib::DateTime::now_local().ok())
}

pub fn run(args: Vec<String>) -> glib::ExitCode {
    crate::theme::fonts::register_bundled(&paths::cache_dir().join("fonts"));
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
            // Upper bound for every request (lyrics, translation, artwork, sign-in); some set less.
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
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
            before_lyrics: RefCell::new("home".into()),
            home_loaded: Cell::new(None),
            releases_loaded: Cell::new(None),
            tray: RefCell::default(),
            mpris: RefCell::default(),
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
            lyric_results: RefCell::default(),
            translations: RefCell::default(),
            deepl_key: RefCell::default(),
            shortcut_status: RefCell::new("not registered".into()),
            history: RefCell::default(),
            undo: RefCell::default(),
            connect_attempts: Cell::new(0),
            builtin: RefCell::default(),
            builtin_account: RefCell::default(),
            builtin_status: RefCell::new(BuiltinStatus::Off),
            builtin_gen: Cell::new(0),
            builtin_restart: RefCell::default(),
            builtin_label: RefCell::default(),
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
        app.win().lyrics.view.set_message("nothing playing");
        app.start_tray();
        app.start_mpris();
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

    pub fn win(&self) -> &MainWindow {
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
        for c in &commands {
            self.command(*c);
        }
        let wants_window = commands.iter().any(|c| !c.is_background());
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
        // Disconnect so the device leaves Spotify's lists now, not after a server timeout.
        if let Some(handle) = self.builtin.borrow_mut().take() {
            rt().block_on(async move {
                let _ = tokio::time::timeout(Duration::from_millis(1500), handle.shutdown()).await;
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
                    let back = self.before_lyrics.borrow().clone();
                    w.show_page(&back);
                } else {
                    if w.page() != "lyrics" && w.page() != "login" {
                        *self.before_lyrics.borrow_mut() = w.page();
                    }
                    w.show_page("lyrics");
                    self.present();
                    self.load_track_lyrics();
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
                self.set_repeat(r);
            }
            Command::VolumeUp | Command::VolumeDown => {
                let cur = self.playback.borrow().device.as_ref().and_then(|d| d.volume_percent).unwrap_or(50) as i32;
                let v = (cur + if c == Command::VolumeUp { 5 } else { -5 }).clamp(0, 100) as u32;
                self.player(PlayerOp::SetVolume(v));
            }
            Command::FocusSearch => {
                self.present();
                self.show_view("search", true);
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
                self.show_view("settings", true);
            }
            Command::Quit => self.quit(),
        }
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
                    Ok(None) => return Err((false, "not signed in yet".to_owned())),
                    Err(e) => return Err((false, format!("Secret Service unavailable ({e}); sign in again"))),
                };
                let token = auth::refresh(client.http(), &client_id, &refresh).await.map_err(|e| {
                    let e = ApiError::Auth(e);
                    (e.is_offline(), e.to_string())
                })?;
                client.set_token(Some(token)).await;
                Ok(())
            },
            // `offline`: the stored login is fine, Spotify just cannot be reached right now.
            move |r: Result<(), (bool, String)>| match r {
                Ok(()) => app.logged_in_ok(),
                Err((true, _)) => app.retry_connect(Self::connect_spotify),
                Err((false, e)) => {
                    app.win().login.status.set_text(&e);
                    app.win().show_page("login");
                }
            },
        );
    }

    /// Spotify cannot be reached while signing in: keep the last-known state on screen and try
    /// again with backoff (5 s, 10 s, … up to a minute) instead of asking to sign in again.
    fn retry_connect(&self, again: fn(&App)) {
        let attempt = self.connect_attempts.get() + 1;
        self.connect_attempts.set(attempt);
        let secs = (5u64 << (attempt - 1).min(4)).min(60);
        self.win().set_online(false, None);
        self.win()
            .login
            .status
            .set_text(&format!("cannot reach Spotify · retrying in {secs} s"));
        if attempt == 1 {
            self.notice("cannot reach Spotify · showing the last known state, retrying", true);
        }
        let app = self.clone();
        glib::timeout_add_local_once(Duration::from_secs(secs), move || again(&app));
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
        self.open_browser_when_listening(url_rx, url);
    }

    /// Open `url` once the redirect listener is up (a busy port fails before the browser opens).
    fn open_browser_when_listening(&self, listening: async_channel::Receiver<()>, url: String) {
        let window = self.win().window.clone();
        glib::spawn_future_local(async move {
            if listening.recv().await.is_ok() {
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
        let account = self.account.borrow().clone();
        self.stop_builtin_player();
        rt().spawn(async move {
            client.set_token(None).await;
            let _ = tokens::delete(tokens::SPOTIFY_REFRESH, &client_id).await;
            if let Some(account) = account {
                let _ = tokens::delete(tokens::PLAYER_LOGIN, &account).await;
            }
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
                if app.connect_attempts.replace(0) > 0 {
                    app.notice("", false);
                }
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
                    app.win().show_page(if page.is_empty() { "home" } else { &page });
                    let shown = app.win().page();
                    if let Some(p) = crate::ui::window::PAGES
                        .iter()
                        .find(|p| **p == shown && !matches!(**p, "library" | "lyrics" | "search"))
                    {
                        app.push_history(Nav::Page(p));
                    }
                }
                if me.product.as_deref().is_some_and(|p| p != "premium") {
                    app.notice("playback control needs Spotify Premium; browsing still works", true);
                }
                app.load_library();
                app.refresh_devices();
                app.start_builtin_player(false);
                app.poll_now();
                for c in app.queued.take() {
                    app.command(c);
                }
            }
            Err(e) if e.is_offline() => app.retry_connect(Self::logged_in_ok),
            Err(e) => {
                app.logged_in.set(false);
                app.win().login.status.set_text(&explain(&e));
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
                    if app.failures.get() >= 2 {
                        // Back online: drop the "cannot reach Spotify" banner.
                        app.notice("", false);
                    }
                    app.failures.set(0);
                    app.apply_state(Playback::from_remote(state));
                }
                Err(ApiError::RateLimited(s)) => app.retry_after.set(Some(Duration::from_secs(s))),
                Err(e) if e.is_signed_out() => return app.session_expired(),
                Err(e) => {
                    app.failures.set(app.failures.get() + 1);
                    if app.failures.get() == 2 {
                        app.win().set_online(false, None);
                        app.notice(&format!("{} · retrying", explain(&e)), true);
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
        self.update_mpris(&pb);
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
        self.load_track_lyrics();
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
            if e.is_signed_out() {
                return self.session_expired();
            }
            self.notice(&explain(&e), true);
        }
        self.schedule_poll(Duration::from_millis(350));
    }

    /// Spotify no longer accepts the login: stop polling and ask to sign in again, keeping the
    /// client ID.
    fn session_expired(&self) {
        if !self.logged_in.replace(false) {
            return;
        }
        self.poll_gen.set(self.poll_gen.get() + 1);
        self.win().set_online(false, None);
        self.win().login.status.set_text("Spotify ended the session · sign in again");
        self.win().show_page("login");
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
            .or_else(|| devices.iter().find(|d| self.is_builtin_device(d)).and_then(|d| d.id.clone()))
            .or_else(|| devices.first().and_then(|d| d.id.clone()));
        match target {
            Some(id) => self.simple(move |c| async move { c.transfer(&id, true).await }),
            None if self.builtin_wanted() => self.notice(
                "no Spotify device available yet — the built-in player is not ready (settings → playback)",
                true,
            ),
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
                let client = self.client();
                let app = self.clone();
                let track = uri.clone();
                spawn_then(async move { client.set_saved(&track, on).await }, move |r| {
                    // The heart flipped right away; put it back if Spotify refused, unless the
                    // track changed or the heart was clicked again meanwhile.
                    let same = app.playback.borrow().track_uri() == Some(uri.as_str());
                    if r.is_err() && same && app.saved_current.get() == Some(on) {
                        app.saved_current.set(Some(!on));
                        app.refresh_player_ui();
                    }
                    app.after_command(r);
                });
            }
        }
    }

    pub fn refresh_devices(&self) {
        let client = self.client();
        let app = self.clone();
        spawn_then(async move { client.devices().await }, move |r| match r {
            Ok(devs) => {
                app.win().player.set_devices(&devs, &|d| app.is_builtin_device(d));
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

    // --- built-in player ------------------------------------------------------------------------

    fn builtin_wanted(&self) -> bool {
        player::AVAILABLE && !demo_mode() && self.config.borrow().config.player.enabled
    }

    /// Whether `d` in Spotify's device list is this rtify's own player.
    pub fn is_builtin_device(&self, d: &Device) -> bool {
        if self.builtin.borrow().is_none() {
            return false;
        }
        d.id.as_deref() == Some(player::device_id().as_str()) || d.name == player::device_name(&self.config.borrow().config.player)
    }

    pub fn builtin_status_text(&self) -> String {
        let name = player::device_name(&self.config.borrow().config.player);
        self.builtin_status.borrow().describe(&name)
    }

    fn set_builtin_status(&self, status: BuiltinStatus) {
        *self.builtin_status.borrow_mut() = status;
        self.show_builtin_status(&self.builtin_status_text());
    }

    fn show_builtin_status(&self, text: &str) {
        if let Some(l) = self.builtin_label.borrow().as_ref().and_then(|w| w.upgrade()) {
            l.set_text(text);
        }
    }

    /// Run the built-in device for the signed-in account with its stored login. `restart` also
    /// replaces one that already runs (new name or quality).
    fn start_builtin_player(&self, restart: bool) {
        if !self.builtin_wanted() {
            return self.stop_builtin_player();
        }
        let Some(account) = self.account.borrow().clone() else { return };
        let running = self.builtin.borrow().is_some() && self.builtin_account.borrow().as_deref() == Some(&account);
        if running && !restart {
            return;
        }
        let generation = self.builtin_gen.get();
        let app = self.clone();
        let who = account.clone();
        spawn_then(async move { tokens::load(tokens::PLAYER_LOGIN, &who).await }, move |r| {
            if app.builtin_gen.get() != generation || app.account.borrow().as_deref() != Some(&account) {
                return;
            }
            match r {
                Ok(Some(json)) => app.launch_builtin(player::Login::Stored(json)),
                Ok(None) if running => app.set_builtin_status(BuiltinStatus::SignInNeeded("sign in again".into())),
                Ok(None) => {
                    app.set_builtin_status(BuiltinStatus::SignInNeeded("sign in once below".into()));
                    app.notice(
                        "built-in player: sign in once under settings → playback to play on this computer (or turn it off there)",
                        false,
                    );
                }
                Err(e) => app.set_builtin_status(BuiltinStatus::SignInNeeded(format!("Secret Service unavailable ({e})"))),
            }
        });
    }

    fn launch_builtin(&self, login: player::Login) {
        let generation = self.builtin_gen.get() + 1;
        self.builtin_gen.set(generation);
        let settings = player::Settings::from_config(&self.config.borrow().config.player);
        let (tx, rx) = async_channel::unbounded();
        let previous = self.builtin.borrow_mut().take();
        *self.builtin.borrow_mut() = Some(player::Handle::start(settings, login, tx, previous));
        *self.builtin_account.borrow_mut() = self.account.borrow().clone();
        self.set_builtin_status(BuiltinStatus::Connecting);
        let app = self.clone();
        runtime::forward(rx, move |ev| {
            if app.builtin_gen.get() == generation {
                app.builtin_event(ev);
            }
        });
    }

    fn stop_builtin_player(&self) {
        self.builtin_gen.set(self.builtin_gen.get() + 1);
        if let Some(handle) = self.builtin.borrow_mut().take() {
            rt().spawn(handle.shutdown());
            let a = self.clone();
            glib::timeout_add_local_once(Duration::from_millis(1500), move || a.refresh_devices_if_online());
        }
        self.builtin_account.borrow_mut().take();
        self.set_builtin_status(BuiltinStatus::Off);
    }

    fn refresh_devices_if_online(&self) {
        if self.logged_in.get() {
            self.refresh_devices();
        }
    }

    fn builtin_event(&self, ev: player::Event) {
        match ev {
            player::Event::Status(status) => {
                match &status {
                    BuiltinStatus::Ready { username } => {
                        if self.account.borrow().as_deref().is_some_and(|a| a != username) {
                            self.notice(
                                &format!("the built-in player signed in as {username}, not as this account · sign in again under settings → playback"),
                                true,
                            );
                        }
                        // Spotify lists a new device after a moment.
                        let a = self.clone();
                        glib::timeout_add_local_once(Duration::from_millis(1200), move || a.refresh_devices_if_online());
                    }
                    BuiltinStatus::SignInNeeded(why) => {
                        self.builtin.borrow_mut().take();
                        self.notice(&format!("built-in player: {why} · settings → playback → sign in"), true);
                    }
                    _ => {}
                }
                self.set_builtin_status(status);
            }
            player::Event::Login(json) => {
                let Some(account) = self.builtin_account.borrow().clone() else {
                    return;
                };
                let app = self.clone();
                spawn_then(
                    async move { tokens::store(tokens::PLAYER_LOGIN, &account, &json).await },
                    move |r| {
                        if let Err(e) = r {
                            app.notice(&format!("built-in player: the login could not be stored in the Secret Service ({e}); you will need to sign in again next time"), true);
                        }
                    },
                );
            }
            player::Event::Changed => self.schedule_poll(Duration::from_millis(300)),
            player::Event::Audio(msg) => self.notice(&format!("built-in player: {msg}"), true),
        }
    }

    /// Settings → built-in player → play on this computer.
    pub fn set_builtin_enabled(&self, on: bool) {
        self.update_config(|c| c.player.enabled = on);
        if on {
            self.start_builtin_player(false);
        } else {
            self.stop_builtin_player();
        }
    }

    /// Name, quality or normalisation changed: restart the device once typing pauses.
    pub fn builtin_settings_changed(&self) {
        if let Some(id) = self.builtin_restart.borrow_mut().take() {
            id.remove();
        }
        let app = self.clone();
        let id = glib::timeout_add_local_once(Duration::from_millis(800), move || {
            app.builtin_restart.borrow_mut().take();
            if app.builtin.borrow().is_some() {
                app.start_builtin_player(true);
            }
        });
        *self.builtin_restart.borrow_mut() = Some(id);
    }

    /// One-time browser sign-in for the built-in player. It uses Spotify's own desktop client
    /// (the only kind its streaming servers accept) with just the `streaming` scope; afterwards
    /// the player keeps reusable credentials and never needs the browser again.
    pub fn builtin_sign_in(&self) {
        if !player::AVAILABLE {
            return;
        }
        if !self.logged_in.get() || self.account.borrow().is_none() {
            return self.show_builtin_status("sign in to Spotify first (above)");
        }
        let pkce = match auth::Pkce::new() {
            Ok(p) => p,
            Err(e) => return self.show_builtin_status(&e.to_string()),
        };
        let port = self.config.borrow().config.spotify.redirect_port;
        let redirect = auth::player_redirect_uri(port);
        let url = pkce.authorize_url_for(auth::PLAYER_CLIENT_ID, &redirect, auth::PLAYER_SCOPES);
        self.show_builtin_status("waiting for the browser…");
        let http = self.http.clone();
        let app = self.clone();
        let account = self.account.borrow().clone();
        let (url_tx, url_rx) = async_channel::bounded::<()>(1);
        spawn_then(
            async move {
                let listener = auth::listen(port).await?;
                let _ = url_tx.send(()).await;
                let code = auth::wait_for_code_at(listener, "/login", &pkce.state, Duration::from_secs(300)).await?;
                let token = auth::exchange_code_for(&http, auth::PLAYER_CLIENT_ID, &code, &pkce.verifier, &redirect).await?;
                Ok::<_, auth::AuthError>(token.access_token)
            },
            move |r| match r {
                Ok(_) if *app.account.borrow() != account => {}
                Ok(token) => {
                    if !app.config.borrow().config.player.enabled {
                        app.update_config(|c| c.player.enabled = true);
                    }
                    app.launch_builtin(player::Login::Token(token));
                }
                Err(e) => app.show_builtin_status(&format!("sign-in failed: {e}")),
            },
        );
        self.open_browser_when_listening(url_rx, url);
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
                // Followed artists are optional (older logins lack the scope).
                let mut artists = Vec::new();
                let mut after: Option<String> = None;
                for _ in 0..4 {
                    let Ok(page) = client.followed_artists(after.as_deref()).await else {
                        break;
                    };
                    artists.extend(page.items.iter().map(Collection::from_artist));
                    after = page.cursors.and_then(|c| c.after);
                    if page.next.is_none() || after.is_none() {
                        break;
                    }
                }
                Ok::<_, ApiError>((playlists, albums, artists))
            },
            move |r| match r {
                Ok((playlists, albums, artists)) => {
                    let side = &app.win().sidebar;
                    side.set_playlists(playlists);
                    side.set_albums(albums);
                    side.set_artists(artists);
                    app.win().rail.set_items(&side.all());
                    app.home_loaded.set(None);
                    app.releases_loaded.set(None);
                    match app.win().page().as_str() {
                        "home" => app.load_home(),
                        "releases" => app.load_releases(),
                        _ => {}
                    }
                    // Restore the open collection from the last session; first run shows the
                    // first playlist rather than an empty view.
                    let saved = app
                        .account
                        .borrow()
                        .as_ref()
                        .and_then(|a| app.session.borrow().data.accounts.get(a).and_then(|s| s.library_selection.clone()));
                    let restored = saved.filter(|u| u != SEARCH_URI).and_then(|uri| side.find(&uri));
                    if let Some(c) = restored.or_else(|| side.first_playlist()) {
                        // Recorded in back/forward only when the library page is what shows.
                        let record = app.win().page() == "library";
                        app.show_collection(c, record, false);
                    }
                }
                Err(e) => app.notice(&format!("library: {e}"), true),
            },
        );
    }

    fn is_editable(&self, c: &Collection) -> bool {
        c.kind == "playlist" && (c.collaborative || self.account.borrow().as_deref() == Some(c.owner_id.as_str()))
    }

    /// Show a collection in the main view and load its tracks. `record` adds it to back/forward.
    /// Revisits come from the cache, with the scroll position where you left it.
    pub fn open_collection(&self, c: Collection, record: bool) {
        self.show_collection(c, record, true);
    }

    /// `switch_page`: bring the library page to the front (false when restoring at startup, so
    /// a session that ended on the lyrics page comes back there).
    fn show_collection(&self, c: Collection, record: bool, switch_page: bool) {
        // Prefer the full library entry (owner, images) when we only have a uri and a name.
        let c = self.win().sidebar.find(&c.uri).filter(|f| f.kind == c.kind).unwrap_or(c);
        if record {
            self.push_history(Nav::Collection(c.clone()));
        }
        let editable = self.is_editable(&c);
        let w = self.win();
        w.sidebar.mark_selected(Some(&c.uri));
        if switch_page && w.page() != "library" {
            w.show_page("library");
        }
        self.remember_selection(&c.uri);
        if w.collection.restore(&c, editable) {
            return;
        }
        w.collection.show(&c, editable);
        self.load_tracks(&c.uri, 0);
    }

    /// Open an album/artist/playlist by URI (from a context menu).
    pub fn open_uri_view(&self, uri: &str, name: &str) {
        let kind = crate::spotify::client::uri_kind(uri).to_owned();
        let c = self.win().sidebar.find(uri).unwrap_or(Collection {
            uri: uri.to_owned(),
            name: name.to_owned(),
            kind,
            ..Default::default()
        });
        self.open_collection(c, true);
    }

    fn push_history(&self, nav: Nav) {
        let (back, forward) = {
            let mut h = self.history.borrow_mut();
            h.push(nav);
            (h.can_back(), h.can_forward())
        };
        self.win().set_history_buttons(back, forward);
    }

    pub fn navigate_history(&self, delta: i32) {
        let (nav, back, forward) = {
            let mut h = self.history.borrow_mut();
            let Some(nav) = h.go(delta) else { return };
            (nav, h.can_back(), h.can_forward())
        };
        self.win().set_history_buttons(back, forward);
        match nav {
            Nav::Collection(c) => self.open_collection(c, false),
            Nav::Search(q) => {
                self.show_view("search", false);
                self.win().search.set_query(&q);
                if !q.is_empty() {
                    self.search(&q, false);
                }
            }
            Nav::Page(p) => self.show_view(p, false),
        }
    }

    /// Bring a page of the main view to the front: home, search, library, releases or
    /// settings. `record` adds it to back/forward. Home and new releases fetch what they show
    /// on first sight and again after a while.
    pub fn show_view(&self, view: &str, record: bool) {
        let w = self.win();
        if w.page() == "login" {
            return;
        }
        match view {
            "library" => {
                let current = w.collection.state.borrow().collection.clone();
                match current.or_else(|| w.sidebar.first_playlist()) {
                    Some(c) => self.open_collection(c, record),
                    None => w.show_page("library"),
                }
                return;
            }
            "search" => {
                if record {
                    self.push_history(Nav::Search(w.search.query()));
                }
                w.show_page("search");
                w.search.focus();
                return;
            }
            _ => {}
        }
        let Some(page) = crate::ui::window::PAGES.iter().find(|p| **p == view) else {
            return;
        };
        if record {
            self.push_history(Nav::Page(page));
        }
        w.show_page(page);
        self.schedule_session_save();
        let stale = |at: Option<std::time::Instant>, secs: u64| at.is_none_or(|t| t.elapsed() > Duration::from_secs(secs));
        match view {
            "home" => {
                w.home.refresh_greeting();
                if stale(self.home_loaded.get(), 120) {
                    self.load_home();
                }
            }
            "releases" if stale(self.releases_loaded.get(), 30 * 60) => self.load_releases(),
            // Values change elsewhere too (B, the panel toggle, album art): show them as they are.
            "settings" => w.settings.rebuild(),
            _ => {}
        }
    }

    /// Home: "jump back in" from the contexts played last, "recently played" songs.
    pub fn load_home(&self) {
        if !self.logged_in.get() {
            return;
        }
        self.home_loaded.set(Some(std::time::Instant::now()));
        let client = self.client();
        let app = self.clone();
        spawn_then(async move { client.recently_played().await }, move |r| {
            let w = app.win();
            let mut library = vec![Collection::liked()];
            library.extend(w.sidebar.all());
            w.home.set_library_size(library.len() - 1);
            let history = r.unwrap_or_else(|e| {
                log::debug!("recently played: {e}");
                Vec::new()
            });
            let contexts: Vec<String> = history.iter().filter_map(|h| h.context.as_ref().map(|c| c.uri.clone())).collect();
            w.home.set_jump(&crate::ui::home::jump_back_in(&contexts, &library, 9));
            w.home
                .set_recent(crate::ui::home::unique_tracks(history.into_iter().map(|h| h.track).collect(), 12));
        });
    }

    /// New releases from followed artists and the artists of saved albums.
    pub fn load_releases(&self) {
        if !self.logged_in.get() {
            return;
        }
        self.releases_loaded.set(Some(std::time::Instant::now()));
        let w = self.win();
        let mut ids: Vec<String> = w.sidebar.artist_ids();
        ids.truncate(40);
        if ids.is_empty() {
            w.releases
                .set_status("follow artists or save albums to see their new releases here");
            w.releases.set_albums(Vec::new());
            return;
        }
        w.releases.set_status("loading…");
        let client = self.client();
        let app = self.clone();
        spawn_then(
            async move {
                use futures_util::StreamExt;
                let results: Vec<_> = futures_util::stream::iter(ids)
                    .map(|id| {
                        let c = client.clone();
                        async move { c.artist_releases(&id).await }
                    })
                    .buffer_unordered(4)
                    .collect()
                    .await;
                let failed = results.iter().filter(|r| r.is_err()).count();
                let first_error = results.iter().find_map(|r| r.as_ref().err().map(|e| e.to_string()));
                let albums: Vec<_> = results.into_iter().filter_map(Result::ok).flatten().collect();
                (albums, failed, first_error)
            },
            move |(albums, failed, error): (Vec<crate::spotify::model::Album>, usize, Option<String>)| {
                let r = &app.win().releases;
                match error {
                    Some(e) if albums.is_empty() => r.set_status(&format!("could not load releases: {e}")),
                    Some(_) => r.set_status(&format!("{failed} artists could not be loaded")),
                    None => r.set_status(""),
                }
                r.set_albums(albums);
            },
        );
    }

    pub fn set_sidebar_collapsed(&self, collapsed: bool) {
        self.update_config(|c| c.window.sidebar_collapsed = collapsed);
        self.win().set_sidebar_collapsed(collapsed);
    }

    pub fn set_queue_panel(&self, visible: bool) {
        self.update_config(|c| c.window.queue_panel = visible);
        self.win().set_queue_panel(visible);
    }

    /// Apply the layout settings (after a reset in settings).
    pub fn apply_layout(&self) {
        let c = self.config.borrow().config.window.clone();
        let w = self.win();
        w.set_sidebar_collapsed(c.sidebar_collapsed);
        w.set_queue_panel(c.queue_panel);
        w.set_now_playing_panel(c.now_playing_panel);
    }

    pub fn load_tracks(&self, uri: &str, offset: u32) {
        let view = self.win().collection.clone();
        view.state.borrow_mut().loading = true;
        let client = self.client();
        let uri = uri.to_owned();
        let kind = crate::spotify::client::uri_kind(&uri).to_owned();
        let id = crate::spotify::client::uri_id(&uri).to_owned();
        let name = view.state.borrow().collection.as_ref().map(|c| c.name.clone()).unwrap_or_default();
        let liked = uri == LIKED_URI;
        if kind == "artist" {
            // Albums as cards, songs below.
            let (client2, v2, uri2) = (client.clone(), view.clone(), uri.clone());
            spawn_then(async move { client2.artist_albums(&id).await }, move |r| {
                if let Ok(p) = r {
                    v2.show_artist_albums(&uri2, p.items.iter().map(Collection::from_album).collect());
                }
            });
            spawn_then(async move { client.artist_tracks(&name).await }, move |r| match r {
                Ok(tracks) => view.append(&uri, tracks.into_iter().map(TrackRow::new).collect(), None),
                Err(e) => view.set_error(&e.to_string()),
            });
            return;
        }
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
            {
                let app = self.clone();
                move |r: Result<TrackPage, ApiError>| match r {
                    Ok((rows, next)) => {
                        view.append(&uri, rows, next);
                        // Keep going: counts, total length, sorting and in-list search need it all.
                        if next.is_some() {
                            app.load_more_tracks();
                        }
                    }
                    Err(ApiError::Status(403, _)) => {
                        view.set_error("Spotify doesn't let apps read this playlist (only your own and collaborative ones)")
                    }
                    Err(e) => view.set_error(&e.to_string()),
                }
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

    /// Search Spotify and show the results in the search view. While typing, `record`
    /// refines the current search entry of back/forward instead of adding one per pause.
    pub fn search(&self, q: &str, record: bool) {
        if record {
            let refine = matches!(self.history.borrow().current(), Some(Nav::Search(_)));
            if refine {
                self.history.borrow_mut().replace_current(Nav::Search(q.to_owned()));
            } else {
                self.push_history(Nav::Search(q.to_owned()));
            }
            self.update_config(|c| crate::ui::search::remember(&mut c.library.recent_searches, q));
        }
        let client = self.client();
        let app = self.clone();
        let q = q.to_owned();
        let query = q.clone();
        spawn_then(async move { client.search(&q).await }, move |r| match r {
            Ok(res) => {
                let results = crate::ui::search::Results {
                    playlists: res
                        .playlists
                        .map(|p| p.items.into_iter().flatten().map(|p| Collection::from_playlist(&p)).collect())
                        .unwrap_or_default(),
                    albums: res
                        .albums
                        .map(|a| a.items.iter().map(Collection::from_album).collect())
                        .unwrap_or_default(),
                    artists: res
                        .artists
                        .map(|a| a.items.iter().map(Collection::from_artist).collect())
                        .unwrap_or_default(),
                    tracks: res.tracks.map(|t| t.items).unwrap_or_default(),
                };
                let w = app.win();
                if w.page() != "search" {
                    w.show_page("search");
                    w.search.set_query(&query);
                }
                w.search.set_results(&query, results);
            }
            Err(e) => app.win().search.set_error(&format!("search: {e}")),
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
            Ok(q) => {
                let from = app
                    .playback
                    .borrow()
                    .context_uri
                    .clone()
                    .and_then(|uri| app.win().sidebar.find(&uri))
                    .map(|c| c.name);
                app.win().now_playing.set_queue(&q, from.as_deref());
            }
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

    /// `as_context`: play the playlist/album itself from the track (Spotify's order); otherwise
    /// play exactly the given list (sorted or filtered views, liked songs, search, artists).
    pub fn play_from(&self, selection: &str, tracks: &[Track], index: usize, as_context: bool) {
        let Some(track) = tracks.get(index) else { return };
        let kind = crate::spotify::client::uri_kind(selection);
        let req = if as_context && matches!(kind, "playlist" | "album") {
            PlayRequest::context(selection, Some(&track.uri))
        } else {
            let uris: Vec<String> = tracks.iter().skip(index).take(100).map(|t| t.uri.clone()).collect();
            PlayRequest::tracks(uris, 0)
        };
        let device = if self.playback.borrow().device.is_some() {
            None
        } else {
            self.devices.borrow().first().and_then(|d| d.id.clone())
        };
        self.simple(move |c| async move { c.play(Some(&req), device.as_deref()).await });
    }

    pub fn play_collection(&self, c: &Collection, shuffle: bool) {
        if shuffle {
            self.set_shuffle(true);
        }
        let req = match c.kind.as_str() {
            "playlist" | "album" | "artist" => PlayRequest::context(&c.uri, None),
            _ => {
                // Liked songs: open and play from the top (the list is the context).
                return self.open_collection(c.clone(), true);
            }
        };
        self.simple(move |cl| async move { cl.play(Some(&req), None).await });
    }

    pub fn focus_list_search(&self) {
        let w = self.win();
        if w.page() != "library" {
            w.show_page("library");
        }
        w.collection.focus_filter();
    }

    pub fn toast(&self, text: &str, undo: bool) {
        self.win().toast(text, undo);
    }

    pub fn queue_uris(&self, uris: Vec<String>) {
        let n = uris.len();
        let client = self.client();
        let app = self.clone();
        spawn_then(
            async move {
                // One at a time keeps the order.
                for u in &uris {
                    client.add_to_queue(u).await?;
                }
                Ok::<_, ApiError>(())
            },
            move |r| match r {
                Ok(()) => {
                    app.toast(
                        &if n == 1 {
                            "added to queue".to_owned()
                        } else {
                            format!("{n} songs added to queue")
                        },
                        false,
                    );
                    app.refresh_queue();
                }
                Err(e) => app.after_command(Err(e)),
            },
        );
    }

    pub fn editable_playlists(&self) -> Vec<Collection> {
        self.win().sidebar.playlists().into_iter().filter(|p| self.is_editable(p)).collect()
    }

    pub fn last_playlist(&self) -> Option<Collection> {
        let uri = self
            .account
            .borrow()
            .as_ref()
            .and_then(|a| self.session.borrow().data.accounts.get(a).and_then(|s| s.last_playlist.clone()))?;
        self.win().sidebar.find(&uri).filter(|p| self.is_editable(p))
    }

    pub fn accepts_drop(&self, c: &Collection) -> bool {
        c.kind == "liked" || self.is_editable(c)
    }

    pub fn drop_on(&self, c: &Collection, uris: Vec<String>) {
        if c.kind == "liked" {
            self.set_saved_many(uris, true);
        } else {
            self.add_to_playlist(c, uris);
        }
    }

    /// Add songs to a playlist, skipping ones already in it. The playlist's contents are always
    /// checked first (from the open view or fetched): Spotify's removal deletes every occurrence,
    /// so undoing an add of a song that was already there would otherwise delete the original.
    pub fn add_to_playlist(&self, p: &Collection, uris: Vec<String>) {
        let mut wanted: Vec<String> = Vec::new();
        for u in uris {
            if !wanted.contains(&u) {
                wanted.push(u);
            }
        }
        if let Some(account) = self.account.borrow().clone() {
            self.session.borrow_mut().account_mut(&account).last_playlist = Some(p.uri.clone());
            self.schedule_session_save();
        }
        let known = self.win().collection.known_uris(&p.uri);
        let id = crate::spotify::client::uri_id(&p.uri).to_owned();
        let client = self.client();
        let app = self.clone();
        let p = p.clone();
        spawn_then(
            async move {
                let existing = match known {
                    Some(k) => k,
                    None => {
                        let mut all = Vec::new();
                        let mut offset = 0;
                        loop {
                            let page = client.playlist_items(&id, offset).await?;
                            let n = page.items.len() as u32;
                            all.extend(page.items.into_iter().filter_map(|i| i.item.map(|t| t.uri)));
                            if page.next.is_none() || n == 0 || all.len() >= 10_000 {
                                break;
                            }
                            offset += n;
                        }
                        all
                    }
                };
                let fresh: Vec<String> = wanted.into_iter().filter(|u| !existing.contains(u)).collect();
                if !fresh.is_empty() {
                    client.add_to_playlist(&id, &fresh, None).await?;
                }
                Ok::<_, ApiError>(fresh)
            },
            move |r| match r {
                Ok(fresh) if fresh.is_empty() => app.toast(&format!("already in “{}”", p.name), false),
                Ok(fresh) => {
                    let n = fresh.len();
                    app.after_playlist_edit(&p, n as i64);
                    app.undo.borrow_mut().push(UndoOp::Added {
                        playlist: p.clone(),
                        uris: fresh,
                    });
                    app.toast(
                        &format!(
                            "added {} to “{}”",
                            if n == 1 { "1 song".into() } else { format!("{n} songs") },
                            p.name
                        ),
                        true,
                    );
                }
                Err(e) => app.after_command(Err(e)),
            },
        );
    }

    pub fn remove_from_playlist(&self, p: &Collection, uris: Vec<String>) {
        let positions = self.win().collection.positions_of(&p.uri, &uris);
        let id = crate::spotify::client::uri_id(&p.uri).to_owned();
        let client = self.client();
        let app = self.clone();
        let p = p.clone();
        let gone = uris.clone();
        spawn_then(async move { client.remove_from_playlist(&id, &gone).await }, move |r| match r {
            Ok(()) => {
                let n = positions.len().max(uris.len());
                app.win().collection.remove_uris(&p.uri, &uris);
                app.win().sidebar.bump_count(&p.uri, -(n as i64));
                // Without known positions there is nothing to put back, so no undo button.
                let undoable = !positions.is_empty();
                app.undo.borrow_mut().push(UndoOp::Removed {
                    playlist: p.clone(),
                    entries: positions,
                });
                app.toast(
                    &format!(
                        "removed {} from “{}”",
                        if n == 1 { "1 song".into() } else { format!("{n} songs") },
                        p.name
                    ),
                    undoable,
                );
            }
            Err(e) => app.after_command(Err(e)),
        });
    }

    fn after_playlist_edit(&self, p: &Collection, delta: i64) {
        let w = self.win();
        w.collection.invalidate(&p.uri);
        w.sidebar.bump_count(&p.uri, delta);
        if w.collection.uri().as_deref() == Some(p.uri.as_str()) {
            let editable = self.is_editable(p);
            let (adj_value, view) = (w.collection.scroll_value(), w.collection.clone());
            view.show(p, editable);
            self.load_tracks(&p.uri, 0);
            view.set_scroll_after_load(adj_value);
        }
    }

    /// Ctrl+Z / the toast's "undo": revert the last playlist change. If Spotify refuses, the
    /// change (or what is left of it) goes back on the stack, so Ctrl+Z can try again.
    pub fn undo(&self) {
        let Some(op) = self.undo.borrow_mut().pop() else {
            return self.toast("nothing to undo", false);
        };
        let client = self.client();
        let app = self.clone();
        match op {
            UndoOp::Added { playlist, uris } => {
                let id = crate::spotify::client::uri_id(&playlist.uri).to_owned();
                let n = uris.len() as i64;
                let gone = uris.clone();
                spawn_then(async move { client.remove_from_playlist(&id, &gone).await }, move |r| match r {
                    Ok(()) => {
                        app.after_playlist_edit(&playlist, -n);
                        app.toast(&format!("undone · removed again from “{}”", playlist.name), false);
                    }
                    Err(e) => {
                        app.undo.borrow_mut().push(UndoOp::Added { playlist, uris });
                        app.undo_failed(e);
                    }
                });
            }
            UndoOp::Removed { playlist, entries } => {
                let id = crate::spotify::client::uri_id(&playlist.uri).to_owned();
                let steps = undo::restore_steps(entries);
                spawn_then(
                    async move {
                        for (done, (uri, pos)) in steps.iter().enumerate() {
                            if let Err(e) = client.add_to_playlist(&id, std::slice::from_ref(uri), Some(*pos)).await {
                                return (done, Err((e, steps[done..].to_vec())));
                            }
                        }
                        (steps.len(), Ok(()))
                    },
                    move |(done, r)| {
                        if done > 0 {
                            app.after_playlist_edit(&playlist, done as i64);
                        }
                        match r {
                            Ok(()) => app.toast(&format!("undone · restored in “{}”", playlist.name), false),
                            Err((e, rest)) => {
                                app.undo.borrow_mut().push(UndoOp::Removed { playlist, entries: rest });
                                app.undo_failed(e);
                            }
                        }
                    },
                );
            }
        }
    }

    fn undo_failed(&self, e: ApiError) {
        if e.is_signed_out() {
            return self.session_expired();
        }
        self.notice(&format!("could not undo ({}) · press Ctrl+Z to try again", explain(&e)), true);
    }

    pub fn set_saved_many(&self, uris: Vec<String>, on: bool) {
        let n = uris.len();
        let current = self.playback.borrow().track_uri().map(String::from);
        let client = self.client();
        let app = self.clone();
        let list = uris.clone();
        spawn_then(async move { client.set_saved_many(&list, on).await }, move |r| match r {
            Ok(()) => {
                if current.as_ref().is_some_and(|c| uris.contains(c)) {
                    app.saved_current.set(Some(on));
                    app.refresh_player_ui();
                }
                let w = app.win();
                w.collection.invalidate(LIKED_URI);
                if w.collection.uri().as_deref() == Some(LIKED_URI) {
                    if on {
                        app.open_collection(Collection::liked(), false);
                    } else {
                        w.collection.remove_uris(LIKED_URI, &uris);
                    }
                }
                let what = if n == 1 { "1 song".to_owned() } else { format!("{n} songs") };
                app.toast(
                    &if on {
                        format!("saved {what} to liked songs")
                    } else {
                        format!("removed {what} from liked songs")
                    },
                    false,
                );
            }
            Err(e) => app.after_command(Err(e)),
        });
    }

    pub fn copy_links(&self, uris: &[String]) {
        let links: Vec<String> = uris
            .iter()
            .map(|u| format!("https://open.spotify.com/{}", u.trim_start_matches("spotify:").replace(':', "/")))
            .collect();
        self.win().window.clipboard().set_text(&links.join("\n"));
        self.toast(if links.len() == 1 { "link copied" } else { "links copied" }, false);
    }

    /// Right-click menu for a playlist, album, artist or Liked Songs.
    pub fn collection_menu(&self, c: &Collection) -> Menu {
        let menu = Menu::new();
        let (a, c1) = (self.clone(), c.clone());
        menu.item("open", move || a.open_collection(c1.clone(), true));
        let (a, c1) = (self.clone(), c.clone());
        menu.item("play", move || a.play_collection(&c1, false));
        if c.kind != "liked" {
            let (a, c1) = (self.clone(), c.clone());
            menu.item("shuffle play", move || a.play_collection(&c1, true));
        }
        if c.kind == "album" {
            let saved = self.win().sidebar.find(&c.uri).is_some();
            let (a, u) = (self.clone(), c.uri.clone());
            menu.item(
                if saved {
                    "remove from your library"
                } else {
                    "save to your library"
                },
                move || a.save_collection(&u, !saved),
            );
        }
        if c.kind != "liked" {
            menu.separator();
            let (a, u) = (self.clone(), c.uri.clone());
            menu.item("copy link", move || a.copy_links(std::slice::from_ref(&u)));
        }
        menu
    }

    fn save_collection(&self, uri: &str, on: bool) {
        let client = self.client();
        let app = self.clone();
        let u = uri.to_owned();
        spawn_then(async move { client.set_saved_many(&[u], on).await }, move |r| match r {
            Ok(()) => {
                app.toast(if on { "saved to your library" } else { "removed from your library" }, false);
                app.load_library_quiet();
            }
            Err(e) => app.after_command(Err(e)),
        });
    }

    /// Refresh the sidebar without changing what the main view shows.
    fn load_library_quiet(&self) {
        let client = self.client();
        let app = self.clone();
        spawn_then(
            async move {
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
                Ok::<_, ApiError>(albums)
            },
            move |r| {
                if let Ok(albums) = r {
                    app.win().sidebar.set_albums(albums);
                }
            },
        );
    }

    // --- MPRIS --------------------------------------------------------------------------------

    fn start_mpris(&self) {
        let (tx, rx) = async_channel::unbounded();
        let app = self.clone();
        runtime::forward(rx, move |ev| app.mpris_event(ev));
        let app = self.clone();
        spawn_then(async move { mpris::start(tx).await }, move |r| match r {
            Ok(h) => {
                *app.mpris.borrow_mut() = Some(h);
                let pb = app.playback.borrow().clone();
                app.update_mpris(&pb);
            }
            Err(e) => log::warn!("MPRIS unavailable: {e}"),
        });
    }

    fn update_mpris(&self, pb: &Playback) {
        let Some(h) = self.mpris.borrow().clone() else { return };
        let t = pb.track.as_ref();
        let state = MprisState {
            playing: pb.playing && !pb.stale,
            has_track: t.is_some(),
            track_id: t.map(|t| crate::spotify::client::uri_id(&t.uri).to_owned()).unwrap_or_default(),
            uri: t.map(|t| t.uri.clone()).unwrap_or_default(),
            title: t.map(|t| t.name.clone()).unwrap_or_default(),
            artists: t
                .map(|t| {
                    let names: Vec<String> = t.artists.iter().map(|a| a.name.clone()).collect();
                    if names.is_empty() { vec![t.artist_names()] } else { names }
                })
                .unwrap_or_default(),
            album: t.map(|t| t.album_name().to_owned()).unwrap_or_default(),
            art_url: t
                .and_then(|t| pick_image(t.images(), 300))
                .map(|i| i.url.clone())
                .unwrap_or_default(),
            length_ms: pb.duration_ms(),
            position_ms: pb.position_now(),
            volume: pb.device.as_ref().and_then(|d| d.volume_percent),
            shuffle: pb.shuffle,
            loop_status: match pb.repeat {
                Repeat::Off => "None",
                Repeat::Context => "Playlist",
                Repeat::Track => "Track",
            }
            .into(),
        };
        rt().spawn(async move {
            if let Err(e) = h.update(state).await {
                log::debug!("mpris update: {e}");
            }
        });
    }

    fn mpris_event(&self, ev: MprisEvent) {
        let playing = self.playback.borrow().playing;
        match ev {
            MprisEvent::Command(c) => self.command(c),
            MprisEvent::Play if !playing => self.command(Command::PlayPause),
            MprisEvent::Pause if playing => self.command(Command::PlayPause),
            MprisEvent::Play | MprisEvent::Pause => {}
            MprisEvent::SeekTo(ms) => self.player(PlayerOp::Seek(ms)),
            MprisEvent::SeekBy(d) => {
                let pb = self.playback.borrow().clone();
                let to = (pb.position_now() as i64 + d).clamp(0, pb.duration_ms() as i64) as u64;
                self.player(PlayerOp::Seek(to));
            }
            MprisEvent::SetVolume(v) => self.player(PlayerOp::SetVolume(v)),
            MprisEvent::SetShuffle(on) => self.set_shuffle(on),
            MprisEvent::SetLoop(l) => self.set_repeat(match l.as_str() {
                "Track" => Repeat::Track,
                "Playlist" => Repeat::Context,
                _ => Repeat::Off,
            }),
            MprisEvent::OpenUri(uri) => self.open_uri(&uri),
        }
    }

    pub fn set_repeat(&self, r: Repeat) {
        self.playback.borrow_mut().repeat = r;
        self.refresh_player_ui();
        self.simple(move |c| async move { c.repeat(r.as_str(), None).await });
    }

    /// Play a `spotify:…` URI or an open.spotify.com link.
    pub fn open_uri(&self, uri: &str) {
        let Some(uri) = crate::spotify::client::normalize_uri(uri) else {
            return self.notice(&format!("not a Spotify link: {uri}"), true);
        };
        let req = if crate::spotify::client::uri_kind(&uri) == "track" || crate::spotify::client::uri_kind(&uri) == "episode" {
            PlayRequest::tracks(vec![uri], 0)
        } else {
            PlayRequest::context(&uri, None)
        };
        self.simple(move |c| async move { c.play(Some(&req), None).await });
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
        let follow = self.config.borrow().config.lyrics.follow_playback;
        self.win().lyrics.set_options(self.view_options(), follow);
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
    #[cfg(feature = "demo")]
    pub fn open_demo_lyrics(&self, index: usize, autoplay: bool) {
        let doc = demo::song(index);
        let label = doc
            .as_ref()
            .map(|d| format!("{} — {}", d.title.clone().unwrap_or_default(), d.artist.clone().unwrap_or_default()))
            .unwrap_or_default();
        let mut clock = LyricClock::new(doc.as_ref().map(|d| d.duration_ms() + 1500));
        clock.set_playing(autoplay);
        self.show_lyrics(doc, LyricsSource::Demo(index), &label, Clock::Local(clock));
    }

    fn track_query(track: &Track) -> TrackQuery {
        TrackQuery {
            id: track.id.clone().unwrap_or_default(),
            title: track.name.clone(),
            artists: track.artists.iter().map(|a| a.name.clone()).collect(),
            album: track.album_name().to_owned(),
            duration_ms: track.duration_ms,
        }
    }

    /// Like qtify: lyrics for the playing track from LRCLIB, synced to playback. Only fetched
    /// while the lyrics page is open.
    pub fn load_track_lyrics(&self) {
        if !self.config.borrow().config.lyrics.follow_playback || self.win().page() != "lyrics" {
            return;
        }
        let track = self.playback.borrow().track.clone();
        let Some(track) = track.filter(|t| t.kind != "episode" && !t.uri.is_empty()) else {
            *self.lyrics_source.borrow_mut() = LyricsSource::None;
            *self.lyrics_doc.borrow_mut() = None;
            self.win().lyrics.show_document(None, "", Clock::Local(LyricClock::new(None)));
            self.win().lyrics.view.set_message("nothing playing");
            return;
        };
        let query = Self::track_query(&track);
        let key = query.key();
        if *self.lyrics_source.borrow() == LyricsSource::Track(key.clone()) {
            return;
        }
        *self.lyrics_source.borrow_mut() = LyricsSource::Track(key.clone());
        if let Some(found) = self.lyric_results.borrow().get(&key).cloned() {
            return self.show_found(found, &key, &track);
        }
        self.win().lyrics.show_document(None, "", self.lyric_clock());
        self.win().lyrics.view.set_message("searching lyrics…");
        let http = self.http.clone();
        let app = self.clone();
        spawn_then(async move { lrclib::fetch(&http, &query).await }, move |r| match r {
            Ok(found) => {
                app.lyric_results.borrow_mut().insert(key.clone(), found.clone());
                if *app.lyrics_source.borrow() == LyricsSource::Track(key.clone()) {
                    app.show_found(found, &key, &track);
                }
            }
            Err(e) => {
                log::info!("lrclib: {e}");
                // Network errors are not cached, so reopening the page retries.
                if *app.lyrics_source.borrow() == LyricsSource::Track(key.clone()) {
                    app.win().lyrics.view.set_message("lyrics service unreachable");
                    *app.lyrics_source.borrow_mut() = LyricsSource::None;
                }
            }
        });
    }

    fn show_found(&self, found: lrclib::Found, key: &str, track: &Track) {
        let label = format!("{} — {}", track.name, track.artist_names());
        let source = LyricsSource::Track(key.to_owned());
        match found {
            lrclib::Found::Lyrics(doc) => self.show_lyrics(Some(*doc), source, &label, self.lyric_clock()),
            lrclib::Found::Instrumental => {
                self.show_lyrics(None, source, &label, self.lyric_clock());
                self.win().lyrics.view.set_message("♪ instrumental");
            }
            lrclib::Found::Missing => {
                self.show_lyrics(None, source, &label, self.lyric_clock());
                self.win().lyrics.view.set_message("no lyrics found for this song");
            }
        }
    }

    /// Playback clock for synced lyrics, slightly ahead so lines light up as they are sung.
    fn lyric_clock(&self) -> Clock {
        let mut snap = self.playback.borrow().snapshot();
        snap.position_ms += LYRIC_LEAD_MS;
        Clock::External(snap)
    }

    fn update_lyric_clock(&self) {
        if matches!(*self.lyrics_source.borrow(), LyricsSource::Track(_)) {
            let clock = self.lyric_clock();
            self.win().lyrics.view.set_clock(clock);
        }
    }

    /// Called when "follow playback" is toggled.
    pub fn lyrics_follow_changed(&self) {
        *self.lyrics_source.borrow_mut() = LyricsSource::None;
        if self.config.borrow().config.lyrics.follow_playback {
            self.load_track_lyrics();
        } else {
            #[cfg(feature = "demo")]
            self.open_demo_lyrics(0, false);
            #[cfg(not(feature = "demo"))]
            self.show_lyrics(None, LyricsSource::None, "", self.lyric_clock());
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

    /// Opt-in: translate the shown document (Google like qtify, or DeepL with a stored key).
    pub fn translate_current_lyrics(&self) {
        let (enabled, target, provider) = {
            let l = &self.config.borrow().config.lyrics;
            (
                l.translation_enabled,
                l.translation_language.clone(),
                l.translation_provider.clone(),
            )
        };
        let Some(doc) = self.lyrics_doc.borrow().clone() else { return };
        if !enabled || target.is_empty() || doc.translation.as_ref().is_some_and(|t| t.language.eq_ignore_ascii_case(&target)) {
            return;
        }
        let cache_key = format!("{}|{target}|{provider}", doc.track_key);
        if let Some(cached) = self.translations.borrow().get(&cache_key).cloned() {
            return self.replace_lyrics_doc(cached);
        }
        let cached_key = self.deepl_key.borrow().clone();
        let http = self.http.clone();
        let app = self.clone();
        let source = self.lyrics_source.borrow().clone();
        spawn_then(
            async move {
                let mut doc = doc;
                if provider == "deepl" {
                    let key = match cached_key {
                        Some(k) => k,
                        None => tokens::load(tokens::DEEPL_KEY, "default")
                            .await
                            .ok()
                            .flatten()
                            .ok_or_else(|| "add a DeepL API key in settings".to_owned())?,
                    };
                    translation::translate(&http, &key, &mut doc, &target)
                        .await
                        .map_err(|e| e.to_string())?;
                    return Ok((Some(key), doc));
                }
                let r = google::request(&http, &doc, &target).await?;
                google::apply(&mut doc, &r, &target);
                Ok::<_, String>((None, doc))
            },
            move |r| match r {
                Ok((key, doc)) => {
                    if key.is_some() {
                        *app.deepl_key.borrow_mut() = key;
                    }
                    app.translations.borrow_mut().insert(cache_key, doc.clone());
                    if *app.lyrics_source.borrow() == source {
                        app.replace_lyrics_doc(doc);
                    }
                }
                Err(e) => app.win().lyrics.set_status(&format!("translation: {e}")),
            },
        );
    }

    /// Swap in an updated document (translation arrived) without disturbing the clock.
    fn replace_lyrics_doc(&self, doc: LyricDocument) {
        let w = self.win();
        let clock = w.lyrics.view.clock();
        w.lyrics.view.set_document(Some(doc.clone()));
        w.lyrics.view.set_clock(clock);
        w.lyrics.set_attribution(Some(&doc));
        *self.lyrics_doc.borrow_mut() = Some(doc);
    }
}
