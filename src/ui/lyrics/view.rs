//! `LyricsView`: one custom widget that draws the whole document with Pango, instead of a widget
//! per line or syllable. A frame callback runs only while the widget is mapped.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk, prelude::*};

use super::animations::{Scroller, char_fill, ease_in_out_sine, ease_out_cubic};
use super::layout::{self, DocLayout, LayoutOptions};
use crate::config::LyricAnimation;
use crate::lyrics::synchronization::{self, DISCONTINUITY, LyricClock, PlaybackSnapshot};
use crate::lyrics::{LyricDocument, TimingMode};
use crate::theme::contrast::Rgb;
use crate::theme::system24::Palette;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewOptions {
    pub animation: LyricAnimation,
    pub reduced_motion: bool,
    pub romanized: bool,
    pub translation: bool,
    pub translation_only: bool,
    pub font_scale: f64,
    pub offset_ms: i64,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            animation: LyricAnimation::Smooth,
            reduced_motion: false,
            romanized: true,
            translation: true,
            translation_only: false,
            font_scale: 1.0,
            offset_ms: 0,
        }
    }
}

impl ViewOptions {
    fn effects(&self) -> bool {
        self.animation == LyricAnimation::Smooth && !self.reduced_motion
    }

    fn layout(&self) -> LayoutOptions {
        LayoutOptions {
            romanized: self.romanized,
            translation: self.translation,
            translation_only: self.translation_only,
            font_scale: self.font_scale,
        }
    }
}

/// Where the current position comes from.
#[derive(Debug, Clone, Copy)]
pub enum Clock {
    /// rtify's own clock for local/demo material (play/pause/seek in the lyrics toolbar).
    Local(LyricClock),
    /// Live playback snapshots; only used when the Spotify sync capability is enabled.
    External(PlaybackSnapshot),
}

impl Clock {
    fn position(&self) -> u64 {
        match self {
            Clock::Local(c) => c.now_ms(),
            Clock::External(s) => s.position_at(Instant::now()),
        }
    }

    fn running(&self) -> bool {
        match self {
            Clock::Local(c) => c.playing() && !c.finished(),
            Clock::External(s) => s.playing,
        }
    }
}

pub struct State {
    doc: Option<Rc<LyricDocument>>,
    palette: Palette,
    opts: ViewOptions,
    clock: Clock,
    layout: Option<DocLayout>,
    scroller: Scroller,
    last_frame: Option<Instant>,
    last_pos: u64,
    /// Manual scroll offset; for timed lyrics it decays back to auto-follow.
    user_offset: f64,
    user_until: Option<Instant>,
    message: String,
    started: Instant,
    on_seek: Option<Rc<dyn Fn(u64)>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            doc: None,
            palette: Palette::derive(crate::theme::system24::BaseColors::from_accent(
                Rgb::parse("#b99aff").unwrap(),
                None,
                false,
            )),
            opts: ViewOptions::default(),
            clock: Clock::Local(LyricClock::new(None)),
            layout: None,
            scroller: Scroller::default(),
            last_frame: None,
            last_pos: 0,
            user_offset: 0.0,
            user_until: None,
            message: "no lyrics".into(),
            started: Instant::now(),
            on_seek: None,
        }
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct LyricsView {
        pub state: RefCell<State>,
        pub tick: RefCell<Option<gtk::TickCallbackId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LyricsView {
        const NAME: &'static str = "RtifyLyricsView";
        type Type = super::LyricsView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("lyrics");
            klass.set_accessible_role(gtk::AccessibleRole::Document);
        }
    }

    impl ObjectImpl for LyricsView {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_overflow(gtk::Overflow::Hidden);
            obj.set_hexpand(true);
            obj.set_vexpand(true);
            obj.add_css_class("rt-lyrics");

            let scroll =
                gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::KINETIC);
            let weak = obj.downgrade();
            scroll.connect_scroll(move |_, _, dy| {
                if let Some(v) = weak.upgrade() {
                    v.manual_scroll(dy * 48.0);
                }
                glib::Propagation::Stop
            });
            obj.add_controller(scroll);

            let click = gtk::GestureClick::new();
            let weak = obj.downgrade();
            click.connect_released(move |_, _, _, y| {
                if let Some(v) = weak.upgrade() {
                    v.click_at(y);
                }
            });
            obj.add_controller(click);
        }
    }

    impl WidgetImpl for LyricsView {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            match orientation {
                gtk::Orientation::Horizontal => (200, 600, -1, -1),
                _ => (150, 500, -1, -1),
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            let mut st = self.state.borrow_mut();
            if st.layout.as_ref().is_some_and(|l| l.width != width) {
                st.layout = None;
            }
        }

        fn map(&self) {
            self.parent_map();
            let id = self.obj().add_tick_callback(|w, _| {
                w.on_tick();
                glib::ControlFlow::Continue
            });
            *self.tick.borrow_mut() = Some(id);
        }

        fn unmap(&self) {
            // Hidden lyrics never redraw.
            if let Some(id) = self.tick.borrow_mut().take() {
                id.remove();
            }
            self.state.borrow_mut().last_frame = None;
            self.parent_unmap();
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.obj().draw(snapshot);
        }
    }
}

glib::wrapper! {
    pub struct LyricsView(ObjectSubclass<imp::LyricsView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for LyricsView {
    fn default() -> Self {
        glib::Object::new()
    }
}

fn rgba(c: Rgb, alpha: f64) -> gdk::RGBA {
    gdk::RGBA::new(c.r as f32, c.g as f32, c.b as f32, alpha.clamp(0.0, 1.0) as f32)
}

impl LyricsView {
    fn st(&self) -> std::cell::RefMut<'_, State> {
        self.imp().state.borrow_mut()
    }

    pub fn set_document(&self, doc: Option<LyricDocument>) {
        {
            let mut st = self.st();
            st.doc = doc.map(Rc::new);
            st.layout = None;
            st.user_offset = 0.0;
            st.user_until = None;
            st.last_frame = None;
        }
        self.update_property(&[gtk::accessible::Property::Label(&self.accessible_text())]);
        self.queue_draw();
    }

    pub fn document(&self) -> Option<Rc<LyricDocument>> {
        self.imp().state.borrow().doc.clone()
    }

    pub fn set_message(&self, message: &str) {
        self.st().message = message.to_owned();
        self.queue_draw();
    }

    pub fn set_palette(&self, palette: &Palette) {
        self.st().palette = *palette;
        self.queue_draw();
    }

    pub fn set_options(&self, opts: ViewOptions) {
        let mut st = self.st();
        if st.opts.layout() != opts.layout() {
            st.layout = None;
        }
        st.opts = opts;
        drop(st);
        self.queue_draw();
    }

    pub fn options(&self) -> ViewOptions {
        self.imp().state.borrow().opts
    }

    pub fn set_clock(&self, clock: Clock) {
        self.st().clock = clock;
        self.queue_draw();
    }

    pub fn clock(&self) -> Clock {
        self.imp().state.borrow().clock
    }

    /// Modify the local clock in place (no-op while an external clock drives the view).
    pub fn with_local_clock(&self, f: impl FnOnce(&mut LyricClock)) {
        if let Clock::Local(c) = &mut self.st().clock {
            f(c);
        }
        self.queue_draw();
    }

    pub fn connect_seek(&self, f: impl Fn(u64) + 'static) {
        self.st().on_seek = Some(Rc::new(f));
    }

    fn accessible_text(&self) -> String {
        let st = self.imp().state.borrow();
        st.doc
            .as_ref()
            .map(|d| d.lines.iter().map(|l| l.original.as_str()).collect::<Vec<_>>().join("\n"))
            .unwrap_or_default()
    }

    fn position(st: &State) -> u64 {
        (st.clock.position() as i64 + st.opts.offset_ms).max(0) as u64
    }

    fn ensure_layout(&self, st: &mut State) {
        let width = self.width();
        if st.layout.is_some() || width <= 0 {
            return;
        }
        if let Some(doc) = &st.doc {
            let ctx = self.pango_context();
            st.layout = Some(layout::build(&ctx, doc, width, st.opts.layout()));
            st.scroller = Scroller::default();
            st.last_frame = None;
        }
    }

    /// Scroll target: the active (or upcoming) line, centered on the anchor.
    fn target(st: &State, pos: u64) -> f64 {
        let (Some(doc), Some(layout)) = (&st.doc, &st.layout) else {
            return 0.0;
        };
        if doc.timing == TimingMode::Static {
            return 0.0;
        }
        match synchronization::cursor(doc, pos).line.and_then(|i| layout.lines.get(i)) {
            Some(l) => l.y + l.main_height / 2.0,
            None => 0.0,
        }
    }

    fn on_tick(&self) {
        let now = Instant::now();
        let mut redraw = false;
        {
            let mut st = self.st();
            self.ensure_layout(&mut st);
            let dt = st.last_frame.map(|t| now.duration_since(t).as_secs_f64()).unwrap_or(0.0).min(0.25);
            let first = st.last_frame.is_none();
            st.last_frame = Some(now);
            let pos = Self::position(&st);
            let jumped = (pos as i64 - st.last_pos as i64 - (dt * 1000.0) as i64).unsigned_abs() > DISCONTINUITY.as_millis() as u64;
            st.last_pos = pos;
            let target = Self::target(&st, pos);
            st.scroller.target = target;
            let smooth = st.opts.animation != LyricAnimation::Off && !st.opts.reduced_motion;
            if first || jumped || !smooth {
                // Seeks and track changes cut instead of scrolling through the whole song.
                if st.scroller.value != target {
                    redraw = true;
                }
                st.scroller.snap(target);
            } else if st.scroller.step(dt, 9.0) {
                redraw = true;
            }
            if let Some(until) = st.user_until
                && now >= until
            {
                st.user_offset *= (-dt * 6.0).exp();
                if st.user_offset.abs() < 0.5 {
                    st.user_offset = 0.0;
                    st.user_until = None;
                }
                redraw = true;
            }
            if st.doc.is_some() && st.clock.running() {
                redraw = true;
            }
        }
        if redraw {
            self.queue_draw();
        }
    }

    fn manual_scroll(&self, dy: f64) {
        let mut st = self.st();
        st.user_offset += dy;
        let max = st.layout.as_ref().map(|l| l.height).unwrap_or(0.0);
        st.user_offset = st.user_offset.clamp(-max, max);
        let timed = st.doc.as_ref().is_some_and(|d| d.timing != TimingMode::Static);
        st.user_until = timed.then(|| Instant::now() + std::time::Duration::from_secs(3));
        drop(st);
        self.queue_draw();
    }

    fn anchor(&self, st: &State) -> f64 {
        let static_doc = st.doc.as_ref().is_none_or(|d| d.timing == TimingMode::Static);
        if static_doc { 24.0 } else { self.height() as f64 * 0.38 }
    }

    fn click_at(&self, y: f64) {
        let (seek, cb) = {
            let st = self.imp().state.borrow();
            let (Some(doc), Some(layout)) = (&st.doc, &st.layout) else { return };
            let doc_y = y - self.anchor(&st) + st.scroller.value + st.user_offset;
            let Some(start) = layout.line_at(doc_y).and_then(|i| doc.lines.get(i)).and_then(|l| l.start_ms) else {
                return;
            };
            let seek = (start as i64 - st.opts.offset_ms).max(0) as u64;
            (seek, st.on_seek.clone())
        };
        let mut st = self.st();
        st.user_offset = 0.0;
        st.user_until = None;
        match &mut st.clock {
            Clock::Local(c) => c.seek(seek),
            Clock::External(_) => {
                drop(st);
                if let Some(cb) = cb {
                    cb(seek);
                }
            }
        }
        self.queue_draw();
    }

    fn draw(&self, snapshot: &gtk::Snapshot) {
        let mut st = self.st();
        self.ensure_layout(&mut st);
        let (w, h) = (self.width() as f64, self.height() as f64);
        let p = st.palette;
        let effects = st.opts.effects();
        let t = st.started.elapsed().as_secs_f64();

        if effects && st.doc.is_some() {
            draw_background(snapshot, w, h, p.accent, t);
        }

        let Some(doc) = st.doc.clone() else {
            let l = self.create_pango_layout(Some(&st.message));
            l.set_width((w.max(10.0) * gtk::pango::SCALE as f64) as i32);
            l.set_alignment(gtk::pango::Alignment::Center);
            let (_, lh) = l.pixel_size();
            snapshot.save();
            snapshot.translate(&graphene::Point::new(0.0, ((h - lh as f64) / 2.0) as f32));
            snapshot.append_layout(&l, &rgba(p.text_faint, 1.0));
            snapshot.restore();
            return;
        };
        let Some(layout) = st.layout.as_ref() else { return };
        let pos = Self::position(&st);
        let timed = doc.timing != TimingMode::Static;
        let cursor = synchronization::cursor(&doc, pos);
        let anchor = self.anchor(&st);
        let scroll = st.scroller.value + st.user_offset;
        let word_mode = doc.timing == TimingMode::Word && st.opts.animation == LyricAnimation::Smooth;
        let reduced = st.opts.reduced_motion;

        for (i, line) in layout.lines.iter().enumerate() {
            let top = anchor + line.y - scroll;
            if top > h + 40.0 || top + line.height < -40.0 {
                continue;
            }
            let data = &doc.lines[i];
            let current = cursor.line == Some(i);
            let active = timed && current && cursor.active;
            let distance = cursor.line.map(|c| (i as f64 - c as f64).abs()).unwrap_or(0.0);
            let past = timed && cursor.line.is_some_and(|c| i < c);

            let (color, alpha) = if !timed {
                (p.text, 1.0)
            } else if active {
                (p.text_dim, 1.0)
            } else if past {
                (p.text_dim, (0.75 - 0.12 * distance).max(0.3))
            } else {
                (p.text_faint, (1.0 - 0.15 * distance).max(0.35))
            };

            snapshot.save();
            snapshot.translate(&graphene::Point::new(layout.padding as f32, top as f32));
            // Emphasis: the active line grows in over 300 ms, others sit slightly smaller.
            if effects && timed {
                let since = data.start_ms.map(|s| pos.saturating_sub(s)).unwrap_or(0) as f64;
                let grow = if active {
                    0.955 + 0.045 * ease_out_cubic(since / 300.0)
                } else {
                    0.955
                };
                snapshot.translate(&graphene::Point::new(0.0, (line.main_height / 2.0) as f32));
                snapshot.scale(grow as f32, grow as f32);
                snapshot.translate(&graphene::Point::new(0.0, (-line.main_height / 2.0) as f32));
            }
            let blur = if effects && timed && distance >= 2.0 {
                ((distance - 1.0).min(3.0) * 1.4) as f32
            } else {
                0.0
            };
            if blur > 0.0 {
                snapshot.push_blur(blur as f64);
            }

            snapshot.append_layout(&line.main, &rgba(color, alpha));
            if active {
                if word_mode && !line.words.is_empty() {
                    let fills: Vec<f64> = synchronization::word_progress(data, pos).collect();
                    for ((start, end), fill) in line.words.iter().zip(fills) {
                        if fill <= 0.0 {
                            continue;
                        }
                        // Reduced motion: whole words switch on, no sweep.
                        let fill = if reduced { 1.0 } else { fill };
                        fill_range(snapshot, &line.main, *start, *end, fill, rgba(p.accent, 1.0));
                    }
                } else {
                    snapshot.append_layout(&line.main, &rgba(p.accent, 1.0));
                }
            }
            for (sub, dy) in &line.subs {
                snapshot.save();
                snapshot.translate(&graphene::Point::new(0.0, *dy as f32));
                let sub_color = if active { p.text } else { p.text_faint };
                snapshot.append_layout(sub, &rgba(sub_color, if active { 0.85 } else { alpha * 0.8 }));
                snapshot.restore();
            }
            if blur > 0.0 {
                snapshot.pop();
            }
            snapshot.restore();

            // Instrumental gap: three dots that fill while waiting for this line.
            if timed && current && !cursor.active && line.gap_above > 0.0 {
                let gap_start = i.checked_sub(1).and_then(|k| doc.lines[k].end_ms).unwrap_or(0);
                let gap_end = data.start_ms.unwrap_or(gap_start);
                let progress = if gap_end > gap_start {
                    (pos.saturating_sub(gap_start)) as f64 / (gap_end - gap_start) as f64
                } else {
                    0.0
                };
                draw_gap_dots(
                    snapshot,
                    layout.padding,
                    top - line.gap_above * 0.6,
                    progress,
                    p,
                    if effects { t } else { 0.0 },
                );
            }
        }
    }
}

/// Paint `[start, end)` of `layout` in `color`, character by character up to `fill` (0..1).
fn fill_range(snapshot: &gtk::Snapshot, layout: &gtk::pango::Layout, start: usize, end: usize, fill: f64, color: gdk::RGBA) {
    let text = layout.text();
    let Some(word) = text.get(start..end) else { return };
    let chars: Vec<(usize, char)> = word.char_indices().filter(|(_, c)| !c.is_whitespace()).collect();
    let n = chars.len();
    let scale = gtk::pango::SCALE as f32;
    for (k, (offset, _)) in chars.iter().enumerate() {
        let f = char_fill(fill, k, n);
        if f <= 0.0 {
            break;
        }
        let r = layout.index_to_pos((start + offset) as i32);
        let (mut x, mut width) = (r.x() as f32 / scale, r.width() as f32 / scale);
        if width < 0.0 {
            // Right-to-left run: the rectangle extends leftwards; fill from the right edge.
            x += width;
            width = -width;
            x += width * (1.0 - f as f32);
        }
        let rect = graphene::Rect::new(x, r.y() as f32 / scale, width * f as f32, r.height() as f32 / scale);
        snapshot.push_clip(&rect);
        snapshot.append_layout(layout, &color);
        snapshot.pop();
    }
}

fn draw_gap_dots(snapshot: &gtk::Snapshot, x: f64, y: f64, progress: f64, p: Palette, t: f64) {
    // Gentle breathing: triangle wave through a sine ease, ~0.8 Hz.
    let triangle = 1.0 - (2.0 * (t * 0.8).fract() - 1.0).abs();
    let pulse = if t > 0.0 { 1.0 + 0.12 * ease_in_out_sine(triangle) } else { 1.0 };
    for k in 0..3 {
        let lit = ((progress * 3.0) - k as f64).clamp(0.0, 1.0);
        let r = 5.0 * pulse;
        let cx = x + r + k as f64 * 18.0;
        let rect = graphene::Rect::new((cx - r) as f32, (y - r) as f32, (r * 2.0) as f32, (r * 2.0) as f32);
        let rounded = gsk::RoundedRect::from_rect(rect, r as f32);
        snapshot.push_rounded_clip(&rounded);
        let base = rgba(p.text_faint, 0.6);
        snapshot.append_color(&base, &rect);
        if lit > 0.0 {
            snapshot.append_color(&rgba(p.accent, lit), &rect);
        }
        snapshot.pop();
    }
}

/// Slowly drifting accent glow behind the lyrics (effects level 5).
fn draw_background(snapshot: &gtk::Snapshot, w: f64, h: f64, accent: Rgb, t: f64) {
    let bounds = graphene::Rect::new(0.0, 0.0, w as f32, h as f32);
    let blobs = [(0.28, 0.32, 0.07, 0.05, 0.55), (0.72, 0.68, 0.05, 0.09, 0.45)];
    for (bx, by, fx, fy, size) in blobs {
        let cx = w * (bx + 0.12 * (t * fx * std::f64::consts::TAU).sin());
        let cy = h * (by + 0.12 * (t * fy * std::f64::consts::TAU).cos());
        let r = (w.max(h) * size) as f32;
        let stops = [
            gsk::ColorStop::new(0.0, rgba(accent, 0.13)),
            gsk::ColorStop::new(1.0, rgba(accent, 0.0)),
        ];
        snapshot.append_radial_gradient(&bounds, &graphene::Point::new(cx as f32, cy as f32), r, r, 0.0, 1.0, &stops);
    }
}
