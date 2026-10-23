//! Pango layouts for a lyric document at a given width. Rebuilt only when the document, the
//! width or the display options change — never per frame.

use gtk::pango;

use crate::lyrics::LyricDocument;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutOptions {
    pub romanized: bool,
    pub translation: bool,
    pub translation_only: bool,
    pub font_scale: f64,
}

pub struct LineLayout {
    /// Top of the line block relative to the document start.
    pub y: f64,
    pub height: f64,
    pub main: pango::Layout,
    pub main_height: f64,
    /// Romanization / translation under the line: (layout, y offset within the block).
    pub subs: Vec<(pango::Layout, f64)>,
    /// Byte range of each timed word inside `main`'s text; empty when the line has no word timing
    /// (or translation-only mode replaced the text).
    pub words: Vec<(usize, usize)>,
    /// Height reserved above the line for the instrumental-gap indicator.
    pub gap_above: f64,
}

pub struct DocLayout {
    pub width: i32,
    pub padding: f64,
    pub lines: Vec<LineLayout>,
    pub height: f64,
}

pub const GAP_INDICATOR_MS: u64 = 3000;

fn font(scale: f64, size: f64, bold: bool) -> pango::FontDescription {
    // Proportional sans for lyrics; fontconfig supplies CJK, Hangul, Arabic … fallback fonts.
    let mut f = pango::FontDescription::from_string("Sans");
    f.set_weight(if bold { pango::Weight::Bold } else { pango::Weight::Medium });
    f.set_absolute_size(size * scale * pango::SCALE as f64);
    f
}

fn make(ctx: &pango::Context, text: &str, font: &pango::FontDescription, width: f64) -> (pango::Layout, f64) {
    let l = pango::Layout::new(ctx);
    l.set_font_description(Some(font));
    l.set_auto_dir(true);
    l.set_wrap(pango::WrapMode::WordChar);
    l.set_width((width.max(50.0) * pango::SCALE as f64) as i32);
    l.set_text(text);
    let (_, h) = l.pixel_size();
    (l, h as f64)
}

pub fn build(ctx: &pango::Context, doc: &LyricDocument, width: i32, opts: LayoutOptions) -> DocLayout {
    let s = opts.font_scale.clamp(0.5, 3.0);
    let padding = (width as f64 * 0.06).clamp(16.0, 64.0);
    let text_width = width as f64 - padding * 2.0;
    let main_font = font(s, 30.0, true);
    let sub_font = font(s, 17.0, false);
    let spacing = 22.0 * s;
    let mut y = 0.0;
    let mut lines = Vec::with_capacity(doc.lines.len());
    let mut prev_end: Option<u64> = None;
    for line in &doc.lines {
        let translated = opts.translation.then_some(line.translated.as_deref()).flatten();
        let (text, words) = match translated {
            Some(t) if opts.translation_only => (t.to_owned(), Vec::new()),
            _ if !line.words.is_empty() => {
                let mut text = String::new();
                let mut ranges = Vec::with_capacity(line.words.len());
                for w in &line.words {
                    let start = text.len();
                    text.push_str(&w.text);
                    ranges.push((start, text.len()));
                }
                (text, ranges)
            }
            _ => (line.original.clone(), Vec::new()),
        };
        let gap_above = match (prev_end, line.start_ms) {
            (Some(end), Some(start)) if start >= end + GAP_INDICATOR_MS => 40.0 * s,
            (None, Some(start)) if start >= GAP_INDICATOR_MS => 40.0 * s,
            _ => 0.0,
        };
        prev_end = line.end_ms.or(prev_end);
        y += gap_above;
        let (main, main_height) = make(ctx, &text, &main_font, text_width);
        let mut block = main_height;
        let mut subs = Vec::new();
        let mut add_sub = |t: &str, block: &mut f64| {
            let (l, h) = make(ctx, t, &sub_font, text_width);
            subs.push((l, *block + 4.0 * s));
            *block += h + 4.0 * s;
        };
        if opts.romanized
            && !opts.translation_only
            && let Some(r) = &line.romanized
        {
            add_sub(r, &mut block);
        }
        if let Some(t) = translated
            && !opts.translation_only
        {
            add_sub(t, &mut block);
        }
        lines.push(LineLayout {
            y,
            height: block,
            main,
            main_height,
            subs,
            words,
            gap_above,
        });
        y += block + spacing;
    }
    DocLayout {
        width,
        padding,
        lines,
        height: y,
    }
}

impl DocLayout {
    /// Index of the line whose block contains document-relative `y`.
    pub fn line_at(&self, y: f64) -> Option<usize> {
        self.lines.iter().position(|l| y >= l.y - l.gap_above && y < l.y + l.height + 11.0)
    }
}
