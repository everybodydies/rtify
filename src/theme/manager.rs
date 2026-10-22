//! Applies the palette to the running app: one `GtkCssProvider` regenerated whenever the palette
//! source, the Noctalia file or the album art changes. Changes cross-fade unless reduced motion.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::{gdk, gio, glib, prelude::*};

use super::contrast::Rgb;
use super::noctalia;
use super::system24::{self, BaseColors, Palette};
use crate::config::{Appearance, PaletteSource};
use crate::paths;

const FADE: Duration = Duration::from_millis(350);

type Listener = Box<dyn Fn(&Palette)>;

pub struct ThemeManager {
    provider: gtk::CssProvider,
    appearance: RefCell<Appearance>,
    current: Cell<BaseColors>,
    album: Cell<Option<Rgb>>,
    fade: RefCell<Option<glib::SourceId>>,
    monitor: RefCell<Option<gio::FileMonitor>>,
    listeners: RefCell<Vec<Listener>>,
    /// Last problem with the selected source (shown in preferences / banner).
    pub status: RefCell<Option<String>>,
}

impl ThemeManager {
    pub fn new(appearance: &Appearance) -> Rc<Self> {
        let provider = gtk::CssProvider::new();
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
        let initial = fixed_colors(appearance);
        let tm = Rc::new(Self {
            provider,
            appearance: RefCell::new(appearance.clone()),
            current: Cell::new(initial),
            album: Cell::new(None),
            fade: RefCell::new(None),
            monitor: RefCell::new(None),
            listeners: RefCell::new(Vec::new()),
            status: RefCell::new(None),
        });
        tm.watch_noctalia();
        let target = tm.target();
        tm.current.set(target);
        tm.render(target);
        tm
    }

    pub fn palette(&self) -> Palette {
        Palette::derive(self.current.get())
    }

    pub fn connect_changed(&self, f: impl Fn(&Palette) + 'static) {
        f(&self.palette());
        self.listeners.borrow_mut().push(Box::new(f));
    }

    pub fn set_appearance(self: &Rc<Self>, appearance: &Appearance) {
        *self.appearance.borrow_mut() = appearance.clone();
        self.apply(true);
    }

    /// New album artwork accent (or `None` for artwork without a usable color).
    pub fn set_album_accent(self: &Rc<Self>, accent: Option<Rgb>) {
        if self.album.get() == accent {
            return;
        }
        self.album.set(accent);
        if self.appearance.borrow().palette_source == PaletteSource::AlbumArt {
            self.apply(true);
        }
    }

    /// Re-read external sources (`rtify --reload-theme`, Noctalia post_hook, file monitor).
    pub fn reload(self: &Rc<Self>) {
        self.apply(true);
    }

    fn target(&self) -> BaseColors {
        let a = self.appearance.borrow();
        *self.status.borrow_mut() = None;
        match a.palette_source {
            PaletteSource::Fixed => fixed_colors(&a),
            PaletteSource::Noctalia => match noctalia::load(&paths::noctalia_css()) {
                Ok(c) => c,
                Err(e) => {
                    *self.status.borrow_mut() = Some(format!("noctalia palette unavailable ({e}); using the fixed palette"));
                    fixed_colors(&a)
                }
            },
            PaletteSource::AlbumArt => {
                let mut base = fixed_colors(&a);
                if let Some(accent) = self.album.get() {
                    // Keep the surfaces, tint them slightly toward the artwork.
                    base = BaseColors::from_accent(accent, Rgb::parse(&a.fixed_background), a.light);
                }
                base
            }
        }
    }

    fn apply(self: &Rc<Self>, animate: bool) {
        let target = self.target();
        let from = self.current.get();
        if let Some(id) = self.fade.borrow_mut().take() {
            id.remove();
        }
        let reduced = {
            let a = self.appearance.borrow();
            a.reduced_motion || !a.animations_enabled
        };
        if !animate || reduced || from == target {
            self.current.set(target);
            self.render(target);
            return;
        }
        let start = Instant::now();
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local(Duration::from_millis(33), move || {
            let Some(tm) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let t = (start.elapsed().as_secs_f64() / FADE.as_secs_f64()).min(1.0);
            let eased = 1.0 - (1.0 - t).powi(3);
            let c = from.lerp(target, eased);
            tm.current.set(c);
            tm.render(c);
            if t >= 1.0 {
                tm.fade.borrow_mut().take();
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
        *self.fade.borrow_mut() = Some(id);
    }

    fn render(&self, base: BaseColors) {
        let palette = Palette::derive(base);
        let (family, size) = {
            let a = self.appearance.borrow();
            (a.font_family.clone(), a.font_size)
        };
        self.provider.load_from_string(&system24::css(&palette, &family, size));
        for l in self.listeners.borrow().iter() {
            l(&palette);
        }
    }

    fn watch_noctalia(self: &Rc<Self>) {
        let file = gio::File::for_path(paths::noctalia_css());
        match file.monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) {
            Ok(monitor) => {
                monitor.set_rate_limit(250);
                let weak = Rc::downgrade(self);
                monitor.connect_changed(move |_, _, _, event| {
                    use gio::FileMonitorEvent as E;
                    if !matches!(event, E::ChangesDoneHint | E::Created | E::Renamed | E::MovedIn) {
                        return;
                    }
                    if let Some(tm) = weak.upgrade()
                        && tm.appearance.borrow().palette_source == PaletteSource::Noctalia
                    {
                        tm.apply(true);
                    }
                });
                *self.monitor.borrow_mut() = Some(monitor);
            }
            Err(e) => log::warn!("cannot watch {}: {e}", paths::noctalia_css().display()),
        }
    }
}

pub fn fixed_colors(a: &Appearance) -> BaseColors {
    let accent = Rgb::parse(&a.fixed_accent).unwrap_or(Rgb::parse("#b99aff").unwrap());
    BaseColors::from_accent(accent, Rgb::parse(&a.fixed_background), a.light)
}
