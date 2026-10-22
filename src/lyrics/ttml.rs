//! TTML lyrics (`<p begin end><span begin end>…</span></p>`), including the iTunes-style
//! `<translations>` / `<transliterations>` metadata blocks keyed by `itunes:key`.
//! Background-vocal spans (`ttm:role="x-bg"`) are skipped.

use std::collections::HashMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

use super::model::{LyricDocument, LyricLine, TimedWord, Translation, close_words};

#[derive(Debug, thiserror::Error)]
pub enum TtmlError {
    #[error("xml: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("no <p> elements found")]
    Empty,
}

/// `12.5`, `12.5s`, `1500ms`, `01:02.5`, `00:01:02.500`, `00:01:02:10` (frames ignored).
pub fn parse_time(s: &str) -> Option<u64> {
    let s = s.trim();
    if let Some(ms) = s.strip_suffix("ms") {
        return ms.trim().parse::<f64>().ok().map(|v| v.max(0.0).round() as u64);
    }
    if let Some(sec) = s.strip_suffix('s') {
        return sec.trim().parse::<f64>().ok().map(|v| (v.max(0.0) * 1000.0).round() as u64);
    }
    let parts: Vec<&str> = s.split(':').collect();
    let (h, m, sec) = match parts.as_slice() {
        [sec] => ("0", "0", *sec),
        [m, sec] => ("0", *m, *sec),
        [h, m, sec] | [h, m, sec, _] => (*h, *m, *sec),
        _ => return None,
    };
    let h: u64 = h.parse().ok()?;
    let m: u64 = m.parse().ok()?;
    let sec: f64 = sec.parse().ok()?;
    if !(0.0..61.0).contains(&sec) {
        return None;
    }
    Some(h * 3_600_000 + m * 60_000 + (sec * 1000.0).round() as u64)
}

fn attr(e: &BytesStart, local: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        (a.key.local_name().as_ref() == local).then(|| {
            a.normalized_value(XmlVersion::Implicit1_0)
                .map(|v| v.into_owned())
                .unwrap_or_default()
        })
    })
}

fn local_name(e: &BytesStart) -> String {
    e.local_name().as_ref().to_owned()
}

#[derive(Default)]
struct OpenLine {
    begin: Option<u64>,
    end: Option<u64>,
    key: Option<String>,
    agent: Option<String>,
    text: String,
    words: Vec<TimedWord>,
}

/// Which metadata block we are collecting text for.
#[derive(Clone, Copy, PartialEq)]
enum Meta {
    Translation,
    Transliteration,
}

pub fn parse(xml: &str) -> Result<LyricDocument, TtmlError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut lines: Vec<OpenLine> = Vec::new();
    let mut line: Option<OpenLine> = None;
    // Stack of open spans inside a <p>: (begin, end, skip).
    let mut spans: Vec<(Option<u64>, Option<u64>, bool)> = Vec::new();
    let mut span_text = String::new();
    let mut meta: Option<Meta> = None;
    let mut meta_lang: Option<String> = None;
    let mut meta_for: Option<String> = None;
    let mut meta_text = String::new();
    let mut translations: HashMap<String, String> = HashMap::new();
    let mut romanizations: HashMap<String, String> = HashMap::new();
    let mut songwriters = Vec::new();
    let mut in_songwriter = false;

    loop {
        match reader.read_event()? {
            Event::Start(e) => match local_name(&e).as_str() {
                "p" if meta.is_none() => {
                    line = Some(OpenLine {
                        begin: attr(&e, "begin").and_then(|v| parse_time(&v)),
                        end: attr(&e, "end").and_then(|v| parse_time(&v)),
                        key: attr(&e, "key"),
                        agent: attr(&e, "agent"),
                        ..Default::default()
                    });
                }
                "span" if line.is_some() => {
                    let skip = spans.iter().any(|s| s.2) || attr(&e, "role").is_some_and(|r| r.starts_with("x-bg"));
                    spans.push((
                        attr(&e, "begin").and_then(|v| parse_time(&v)),
                        attr(&e, "end").and_then(|v| parse_time(&v)),
                        skip,
                    ));
                    span_text.clear();
                }
                "translation" => {
                    meta = Some(Meta::Translation);
                    meta_lang = attr(&e, "lang");
                }
                "transliteration" => meta = Some(Meta::Transliteration),
                "text" if meta.is_some() => {
                    meta_for = attr(&e, "for");
                    meta_text.clear();
                }
                "songwriter" => in_songwriter = true,
                _ => {}
            },
            Event::Empty(e) if local_name(&e) == "br" => {
                if let Some(l) = line.as_mut() {
                    l.text.push(' ');
                }
            }
            Event::Text(t) => {
                let text = t.xml10_content().into_owned();
                if meta_for.is_some() {
                    meta_text.push_str(&text);
                } else if in_songwriter {
                    songwriters.push(text.trim().to_owned());
                } else if let Some(l) = line.as_mut() {
                    let skipping = spans.last().is_some_and(|s| s.2);
                    if skipping {
                        continue;
                    }
                    l.text.push_str(&text);
                    if spans.is_empty() {
                        // Whitespace between spans separates words.
                        if text.chars().all(char::is_whitespace)
                            && let Some(w) = l.words.last_mut()
                        {
                            w.text.push(' ');
                        }
                    } else {
                        span_text.push_str(&text);
                    }
                }
            }
            Event::GeneralRef(r) => {
                // Entities such as &amp; arrive separately from text.
                let resolved = r.resolve_char_ref().ok().flatten().map(String::from).or_else(|| {
                    match r.as_ref() {
                        "amp" => Some("&"),
                        "lt" => Some("<"),
                        "gt" => Some(">"),
                        "apos" => Some("'"),
                        "quot" => Some("\""),
                        _ => None,
                    }
                    .map(String::from)
                });
                if let Some(s) = resolved {
                    if meta_for.is_some() {
                        meta_text.push_str(&s);
                    } else if let Some(l) = line.as_mut()
                        && !spans.last().is_some_and(|s| s.2)
                    {
                        l.text.push_str(&s);
                        if !spans.is_empty() {
                            span_text.push_str(&s);
                        }
                    }
                }
            }
            Event::End(e) => match e.local_name().as_ref() {
                "span" if line.is_some() => {
                    if let Some((begin, end, skip)) = spans.pop()
                        && !skip
                        && let (Some(begin), Some(l)) = (begin, line.as_mut())
                        && !span_text.is_empty()
                    {
                        l.words.push(TimedWord {
                            start_ms: begin,
                            end_ms: end.unwrap_or(begin),
                            text: std::mem::take(&mut span_text),
                        });
                    }
                    span_text.clear();
                }
                "p" => {
                    if let Some(l) = line.take() {
                        lines.push(l);
                    }
                    spans.clear();
                }
                "text" => {
                    if let Some(key) = meta_for.take() {
                        let target = if meta == Some(Meta::Translation) {
                            &mut translations
                        } else {
                            &mut romanizations
                        };
                        target.insert(key, meta_text.trim().to_owned());
                    }
                }
                "translation" | "transliteration" => meta = None,
                "songwriter" => in_songwriter = false,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }

    if lines.is_empty() {
        return Err(TtmlError::Empty);
    }
    let has_translation = !translations.is_empty();
    let out: Vec<LyricLine> = lines
        .into_iter()
        .map(|l| {
            let mut words = l.words;
            if let Some(last) = words.last_mut() {
                last.text = last.text.trim_end().to_owned();
            }
            let begin = l.begin.or_else(|| words.first().map(|w| w.start_ms));
            close_words(&mut words, l.end);
            let key = l.key.unwrap_or_default();
            LyricLine {
                start_ms: begin,
                end_ms: l.end.or_else(|| words.last().map(|w| w.end_ms)),
                original: l.text.split_whitespace().collect::<Vec<_>>().join(" "),
                romanized: romanizations.get(&key).cloned(),
                translated: translations.get(&key).cloned(),
                words,
                agent: l.agent,
            }
        })
        .collect();
    let mut doc = LyricDocument::new(out);
    doc.attribution.songwriters = songwriters;
    if has_translation {
        doc.translation = Some(Translation {
            language: meta_lang.unwrap_or_default(),
            provenance: "document".into(),
        });
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::model::TimingMode;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml" xmlns:itunes="http://music.apple.com/lyric-ttml-internal" xmlns:ttm="http://www.w3.org/ns/ttml#metadata" itunes:timing="Word">
 <head><metadata>
  <iTunesMetadata xmlns="http://music.apple.com/lyric-ttml-internal">
   <translations><translation type="replacement" xml:lang="en">
    <text for="L1">Hello, world</text>
   </translation></translations>
   <transliterations><transliteration xml:lang="ja-Latn">
    <text for="L1">konnichiwa sekai</text>
   </transliteration></transliterations>
   <songwriters><songwriter>A. Writer</songwriter></songwriters>
  </iTunesMetadata>
 </metadata></head>
 <body><div>
  <p begin="00:01.000" end="00:03.000" itunes:key="L1" ttm:agent="v1"><span begin="00:01.000" end="00:01.800">こんにちは</span> <span begin="00:01.900" end="00:02.600">世界</span><span ttm:role="x-bg"><span begin="00:02.6" end="00:03">(ah)</span></span></p>
  <p begin="3.5s" end="5s" itunes:key="L2"><span begin="3.5s" end="4s">Rock</span><span begin="4s" end="4.4s">et</span> <span begin="4.4s" end="5s">&amp; roll</span></p>
 </div></body>
</tt>"#;

    #[test]
    fn times() {
        assert_eq!(parse_time("1.5s"), Some(1500));
        assert_eq!(parse_time("250ms"), Some(250));
        assert_eq!(parse_time("01:02.5"), Some(62_500));
        assert_eq!(parse_time("00:01:02.500"), Some(62_500));
        assert_eq!(parse_time("12.25"), Some(12_250));
        assert_eq!(parse_time("abc"), None);
    }

    #[test]
    fn words_translations_and_background() {
        let doc = parse(SAMPLE).unwrap();
        assert_eq!(doc.timing, TimingMode::Word);
        assert_eq!(doc.lines.len(), 2);
        let l1 = &doc.lines[0];
        assert_eq!(l1.original, "こんにちは 世界");
        assert_eq!(l1.translated.as_deref(), Some("Hello, world"));
        assert_eq!(l1.romanized.as_deref(), Some("konnichiwa sekai"));
        assert_eq!(l1.agent.as_deref(), Some("v1"));
        assert_eq!(l1.words.len(), 2, "x-bg spans are skipped");
        assert_eq!(l1.words[0].text, "こんにちは ");
        let l2 = &doc.lines[1];
        let texts: Vec<_> = l2.words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, ["Rock", "et ", "& roll"], "syllables join without spaces");
        assert_eq!(l2.original, "Rocket & roll");
        assert_eq!(doc.translation.as_ref().unwrap().language, "en");
        assert_eq!(doc.attribution.songwriters, ["A. Writer"]);
    }

    #[test]
    fn line_only_ttml() {
        let doc = parse(r#"<tt><body><div><p begin="1s" end="2s">one</p><p begin="2s" end="3s">two</p></div></body></tt>"#).unwrap();
        assert_eq!(doc.timing, TimingMode::Line);
        assert_eq!(doc.lines[1].original, "two");
    }

    #[test]
    fn empty_is_error() {
        assert!(parse("<tt><body/></tt>").is_err());
    }
}
