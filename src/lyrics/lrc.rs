//! LRC and enhanced LRC (`<mm:ss.xx>` word stamps) parsing.

use super::model::{LyricDocument, LyricLine, TimedWord, close_words};

/// `mm:ss`, `mm:ss.x`, `mm:ss.xx`, `mm:ss.xxx`, `mm:ss:xx`, `h:mm:ss.xx`.
pub fn parse_timestamp(s: &str) -> Option<u64> {
    let s = s.trim();
    let (main, frac) = match s.rfind(['.', ':']) {
        // A trailing ":xx" after two colon groups is a fraction (mm:ss:xx), otherwise it is seconds.
        Some(i) if s[..i].contains(':') || s.as_bytes()[i] == b'.' => (&s[..i], Some(&s[i + 1..])),
        _ => (s, None),
    };
    let mut secs = 0u64;
    for part in main.split(':') {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) || part.len() > 3 {
            return None;
        }
        secs = secs * 60 + part.parse::<u64>().ok()?;
    }
    if !main.contains(':') {
        return None;
    }
    let ms = match frac {
        None => 0,
        Some(f) if !f.is_empty() && f.len() <= 3 && f.bytes().all(|b| b.is_ascii_digit()) => {
            f.parse::<u64>().ok()? * 10u64.pow(3 - f.len() as u32)
        }
        Some(_) => return None,
    };
    Some(secs * 1000 + ms)
}

fn apply_offset(t: u64, offset: i64) -> u64 {
    // LRC: a positive offset makes lyrics appear sooner.
    (t as i64 - offset).max(0) as u64
}

/// Split `<00:01.00>Hel<00:01.30>lo <00:01.80>world<00:02.40>` into timed words.
/// Returns the plain text and the words; text before the first stamp starts at `line_start`.
fn parse_words(body: &str, line_start: u64, offset: i64) -> (String, Vec<TimedWord>) {
    let mut words: Vec<TimedWord> = Vec::new();
    let mut plain = String::new();
    let mut rest = body;
    let mut current: Option<u64> = None;
    let mut buf = String::new();
    let flush = |buf: &mut String, at: Option<u64>, end: Option<u64>, words: &mut Vec<TimedWord>| {
        if buf.is_empty() {
            return;
        }
        let start = at.unwrap_or(line_start);
        words.push(TimedWord {
            start_ms: start,
            end_ms: end.unwrap_or(start),
            text: std::mem::take(buf),
        });
    };
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix('<')
            && let Some(close) = stripped.find('>')
            && let Some(t) = parse_timestamp(&stripped[..close])
        {
            let t = apply_offset(t, offset);
            flush(&mut buf, current, Some(t), &mut words);
            current = Some(t);
            rest = &stripped[close + 1..];
            continue;
        }
        let ch = rest.chars().next().unwrap();
        buf.push(ch);
        plain.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    flush(&mut buf, current, None, &mut words);
    let plain = plain.trim().to_owned();
    // A single untimed chunk is just a plain line.
    if words.len() == 1 && current.is_none() {
        words.clear();
    }
    // Whitespace-only chunks only carry spacing; merge them into the previous word.
    let mut merged: Vec<TimedWord> = Vec::with_capacity(words.len());
    for w in words {
        match merged.last_mut() {
            Some(prev) if w.text.trim().is_empty() => prev.text.push_str(&w.text),
            _ => merged.push(w),
        }
    }
    if let Some(first) = merged.first_mut() {
        first.text = first.text.trim_start().to_owned();
    }
    if let Some(last) = merged.last_mut() {
        last.text = last.text.trim_end().to_owned();
    }
    (plain, merged)
}

pub fn parse(text: &str) -> LyricDocument {
    let mut offset = 0i64;
    let mut title = None;
    let mut artist = None;
    let mut lines = Vec::new();
    let mut any_timed = false;
    let mut untimed = Vec::new();

    for raw in text.lines() {
        let raw = raw.trim_end_matches('\r');
        let mut rest = raw.trim_start();
        let mut stamps = Vec::new();
        let mut is_tag = false;
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(close) = stripped.find(']') else { break };
            let inner = &stripped[..close];
            if let Some(t) = parse_timestamp(inner) {
                stamps.push(t);
            } else if let Some((key, value)) = inner.split_once(':') {
                let value = value.trim();
                match key.trim().to_ascii_lowercase().as_str() {
                    "offset" => offset = value.trim_start_matches('+').parse().unwrap_or(0),
                    "ti" => title = Some(value.to_owned()),
                    "ar" => artist = Some(value.to_owned()),
                    _ => {}
                }
                is_tag = true;
            } else {
                break;
            }
            rest = &stripped[close + 1..];
        }
        if stamps.is_empty() {
            if !is_tag && !rest.trim().is_empty() {
                untimed.push(rest.trim().to_owned());
            }
            continue;
        }
        any_timed = true;
        for stamp in stamps {
            let start = apply_offset(stamp, offset);
            let (original, words) = parse_words(rest, start, offset);
            lines.push(LyricLine {
                start_ms: Some(start),
                original,
                words,
                ..Default::default()
            });
        }
    }

    if !any_timed {
        lines = untimed
            .into_iter()
            .map(|t| LyricLine {
                original: t,
                ..Default::default()
            })
            .collect();
    }
    // Empty timed lines are instrumental gaps: they end the previous line but are not shown.
    let mut doc = LyricDocument::new(lines);
    let ends: Vec<Option<u64>> = doc.lines.iter().map(|l| l.end_ms).collect();
    for (line, end) in doc.lines.iter_mut().zip(ends) {
        close_words(&mut line.words, end);
    }
    doc.lines.retain(|l| !l.original.is_empty());
    doc.normalize();
    doc.title = title;
    doc.artist = artist;
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::model::TimingMode;

    #[test]
    fn timestamps() {
        assert_eq!(parse_timestamp("01:02.5"), Some(62_500));
        assert_eq!(parse_timestamp("01:02.50"), Some(62_500));
        assert_eq!(parse_timestamp("01:02.503"), Some(62_503));
        assert_eq!(parse_timestamp("01:02:50"), Some(62_500));
        assert_eq!(parse_timestamp("01:02"), Some(62_000));
        assert_eq!(parse_timestamp("1:01:02.00"), Some(3_662_000));
        assert_eq!(parse_timestamp("ar:Someone"), None);
        assert_eq!(parse_timestamp("12"), None);
    }

    #[test]
    fn plain_lrc_with_tags_and_repeats() {
        let doc = parse("[ti:Song]\n[ar:Band]\n[00:01.00]one\n[00:05.00][00:09.00]chorus\n[00:07.00]two\n");
        assert_eq!(doc.timing, TimingMode::Line);
        assert_eq!(doc.title.as_deref(), Some("Song"));
        let texts: Vec<_> = doc.lines.iter().map(|l| l.original.as_str()).collect();
        assert_eq!(texts, ["one", "chorus", "two", "chorus"]);
        assert_eq!(doc.lines[0].end_ms, Some(5000));
    }

    #[test]
    fn offset_shifts_earlier() {
        let doc = parse("[offset:+500]\n[00:01.00]a\n[00:00.20]b\n");
        assert_eq!(doc.lines[0].start_ms, Some(0));
        assert_eq!(doc.lines[1].start_ms, Some(500));
    }

    #[test]
    fn enhanced_words() {
        let doc = parse("[00:01.00]<00:01.00>Hel<00:01.30>lo <00:01.80>world<00:02.40>\n[00:03.00]next\n");
        assert_eq!(doc.timing, TimingMode::Word);
        let w = &doc.lines[0].words;
        assert_eq!(w.len(), 3);
        assert_eq!((w[0].text.as_str(), w[0].start_ms, w[0].end_ms), ("Hel", 1000, 1300));
        assert_eq!((w[1].text.as_str(), w[1].end_ms), ("lo ", 1800));
        assert_eq!((w[2].text.as_str(), w[2].end_ms), ("world", 2400));
        assert_eq!(doc.lines[0].original, "Hello world");
        assert!(doc.lines[1].words.is_empty());
    }

    #[test]
    fn last_word_closes_at_line_end() {
        let doc = parse("[00:01.00]<00:01.00>a <00:02.00>b\n[00:04.00]c\n");
        assert_eq!(doc.lines[0].words[1].end_ms, 4000);
    }

    #[test]
    fn empty_lines_are_gaps() {
        let doc = parse("[00:01.00]a\n[00:03.00]\n[00:10.00]b\n");
        assert_eq!(doc.lines.len(), 2);
        assert_eq!(doc.lines[0].end_ms, Some(3000));
    }

    #[test]
    fn untimed_text_is_static() {
        let doc = parse("just\nsome words\n\n");
        assert_eq!(doc.timing, TimingMode::Static);
        assert_eq!(doc.lines.len(), 2);
    }

    #[test]
    fn utf8_and_crlf() {
        let doc = parse("[00:01.00]<00:01.00>君の<00:01.50>名前\r\n[00:02.00]안녕\r\n");
        assert_eq!(doc.lines[0].original, "君の名前");
        assert_eq!(doc.lines[0].words[1].text, "名前");
        assert_eq!(doc.lines[1].original, "안녕");
    }
}
