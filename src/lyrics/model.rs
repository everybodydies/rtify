use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingMode {
    Static,
    Line,
    Word,
}

/// Where a document came from and what we may do with it. Displayed under the lyrics.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Attribution {
    /// Provider name, e.g. "local file" or "demo".
    pub source: String,
    /// Provider-specific identifier (file path, document id).
    pub source_id: String,
    /// Free text about rights, e.g. "user supplied" or the provider's required credit line.
    pub rights: String,
    pub songwriters: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimedWord {
    pub start_ms: u64,
    pub end_ms: u64,
    /// Includes trailing whitespace when the source separates words, so syllables of one
    /// word concatenate without gaps.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LyricLine {
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
    pub original: String,
    pub romanized: Option<String>,
    pub translated: Option<String>,
    pub words: Vec<TimedWord>,
    /// Who sings this line when the source says (TTML `ttm:agent`), for alignment.
    pub agent: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Translation {
    pub language: String,
    /// "document" when it came with the lyrics, otherwise the translation provider's name.
    pub provenance: String,
}

#[derive(Debug, Clone)]
pub struct LyricDocument {
    pub track_key: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub timing: TimingMode,
    pub lines: Vec<LyricLine>,
    pub attribution: Attribution,
    pub translation: Option<Translation>,
    /// Providers with cache limits set this; the cache drops the document afterwards.
    pub expires_at: Option<SystemTime>,
}

impl LyricDocument {
    pub fn new(lines: Vec<LyricLine>) -> Self {
        let mut doc = Self {
            track_key: String::new(),
            title: None,
            artist: None,
            timing: TimingMode::Static,
            lines,
            attribution: Attribution::default(),
            translation: None,
            expires_at: None,
        };
        doc.normalize();
        doc
    }

    /// Sort timed lines, fill missing end times and derive the timing mode.
    pub fn normalize(&mut self) {
        if self.lines.iter().all(|l| l.start_ms.is_some()) {
            self.lines.sort_by_key(|l| l.start_ms);
        }
        let starts: Vec<Option<u64>> = self.lines.iter().map(|l| l.start_ms).collect();
        for (i, line) in self.lines.iter_mut().enumerate() {
            let Some(start) = line.start_ms else { continue };
            let next = starts.get(i + 1).copied().flatten();
            let last_word = line.words.last().filter(|w| w.end_ms > w.start_ms).map(|w| w.end_ms);
            line.end_ms = Some(match (line.end_ms, last_word, next) {
                (Some(e), _, _) => e,
                (None, Some(w), _) if w > start => w,
                (None, _, Some(n)) => n.max(start),
                // Last line without any hint: keep it lit for a few seconds.
                _ => start + 5000,
            });
        }
        self.timing = if self.lines.iter().any(|l| l.start_ms.is_none()) || self.lines.is_empty() {
            TimingMode::Static
        } else if self.lines.iter().any(|l| !l.words.is_empty()) {
            TimingMode::Word
        } else {
            TimingMode::Line
        };
    }

    pub fn duration_ms(&self) -> u64 {
        self.lines.iter().filter_map(|l| l.end_ms).max().unwrap_or(0)
    }

    pub fn is_expired(&self, now: SystemTime) -> bool {
        self.expires_at.is_some_and(|t| now >= t)
    }
}

/// Assign word end times from the following word / the line end where the source left them open.
pub(crate) fn close_words(words: &mut [TimedWord], line_end: Option<u64>) {
    for i in 0..words.len() {
        if words[i].end_ms > words[i].start_ms {
            continue;
        }
        let next = words.get(i + 1).map(|w| w.start_ms).or(line_end);
        words[i].end_ms = next.filter(|&n| n > words[i].start_ms).unwrap_or(words[i].start_ms + 400);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(start: Option<u64>, text: &str) -> LyricLine {
        LyricLine {
            start_ms: start,
            original: text.into(),
            ..Default::default()
        }
    }

    #[test]
    fn sorts_and_closes_lines() {
        let doc = LyricDocument::new(vec![line(Some(5000), "b"), line(Some(1000), "a")]);
        assert_eq!(doc.timing, TimingMode::Line);
        assert_eq!(doc.lines[0].original, "a");
        assert_eq!(doc.lines[0].end_ms, Some(5000));
        assert_eq!(doc.lines[1].end_ms, Some(10000));
    }

    #[test]
    fn untimed_line_makes_document_static() {
        let doc = LyricDocument::new(vec![line(Some(0), "a"), line(None, "b")]);
        assert_eq!(doc.timing, TimingMode::Static);
        assert_eq!(doc.lines[1].original, "b", "static order is preserved");
    }
}
