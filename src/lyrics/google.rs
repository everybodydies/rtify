//! Translation and romanization in one request, the way qtify does it: Google's public
//! `translate_a/single` endpoint with `dt=t` (translation) and `dt=rm` (romanization). Lines are
//! joined with `\n|\n`, a separator that survives both. Only called when the user enabled it.

use super::model::{LyricDocument, Translation};

pub const PROVIDER: &str = "Google Translate";

pub fn base() -> String {
    std::env::var("RTIFY_TRANSLATE_BASE").unwrap_or_else(|_| "https://translate.googleapis.com".into())
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Result {
    /// Detected source language.
    pub lang: String,
    /// Per line; `None` when the line is empty or unchanged.
    pub translated: Vec<Option<String>>,
    pub romanized: Vec<Option<String>>,
}

fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace() && !".,!?¿¡'\"’()…:;–—-".contains(*c))
        .collect()
}

fn split(text: &str, n: usize, originals: &[&str]) -> Vec<Option<String>> {
    let parts: Vec<&str> = text.split('|').map(str::trim).collect();
    if parts.len() != n {
        return vec![None; n];
    }
    parts
        .iter()
        .zip(originals)
        .map(|(p, o)| (!o.is_empty() && !p.is_empty() && norm(p) != norm(o)).then(|| p.to_string()))
        .collect()
}

/// Parse the nested-array response (`[[[tr, orig, …], …, [null, null, null, rm]], null, lang]`).
pub fn parse(json: &serde_json::Value, originals: &[&str]) -> Option<Result> {
    let segs = json.get(0)?.as_array()?;
    let (mut tr, mut rm) = (String::new(), String::new());
    for seg in segs {
        match seg.get(0).and_then(|v| v.as_str()) {
            Some(t) => tr.push_str(t),
            None => {
                if let Some(r) = seg.get(3).and_then(|v| v.as_str()) {
                    rm.push_str(r);
                }
            }
        }
    }
    let n = originals.len();
    Some(Result {
        lang: json.get(2).and_then(|v| v.as_str()).unwrap_or_default().to_owned(),
        translated: split(&tr, n, originals),
        romanized: split(&rm, n, originals),
    })
}

pub async fn request(http: &reqwest::Client, doc: &LyricDocument, target: &str) -> std::result::Result<Result, String> {
    let originals: Vec<&str> = doc.lines.iter().map(|l| l.original.as_str()).collect();
    let q = originals
        .iter()
        .map(|t| if t.is_empty() { "♪" } else { t })
        .collect::<Vec<_>>()
        .join("\n|\n");
    let url = format!("{}/translate_a/single", base());
    let resp = http
        .post(url)
        .query(&[("client", "gtx"), ("sl", "auto"), ("dt", "t"), ("dt", "rm"), ("tl", target)])
        .form(&[("q", q)])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("translation unavailable (HTTP {})", resp.status().as_u16()));
    }
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    parse(&json, &originals).ok_or_else(|| "unexpected translation response".into())
}

/// Merge into `doc`: translations replace, romanizations only fill lines the local romanizers
/// could not handle.
pub fn apply(doc: &mut LyricDocument, r: &Result, target: &str) {
    let same_language = r.lang.split('-').next() == target.split('-').next();
    for (i, line) in doc.lines.iter_mut().enumerate() {
        if !same_language {
            line.translated = r.translated.get(i).cloned().flatten();
        }
        if line.romanized.is_none() {
            line.romanized = r.romanized.get(i).cloned().flatten();
        }
    }
    if !same_language && r.translated.iter().any(Option::is_some) {
        doc.translation = Some(Translation {
            language: target.to_owned(),
            provenance: PROVIDER.into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_translation_and_romanization() {
        let resp = json!([
            [
                ["hello\n|\n", "こんにちは", null, null, 3],
                ["world", "世界", null, null, 3],
                [null, null, null, "kon'nichiwa | sekai"]
            ],
            null,
            "ja"
        ]);
        let r = parse(&resp, &["こんにちは", "世界"]).unwrap();
        assert_eq!(r.lang, "ja");
        assert_eq!(r.translated, vec![Some("hello".into()), Some("world".into())]);
        assert_eq!(r.romanized, vec![Some("kon'nichiwa".into()), Some("sekai".into())]);
    }

    #[test]
    fn unchanged_and_mismatched_lines_are_dropped() {
        let resp = json!([[["Hallo!\n|\n", "", null, null, 3], ["same", "", null, null, 3]], null, "de"]);
        let r = parse(&resp, &["hallo", "same"]).unwrap();
        assert_eq!(r.translated, vec![None, None]);
        let r = parse(&resp, &["a", "b", "c"]).unwrap();
        assert_eq!(r.translated, vec![None, None, None]);
    }

    #[test]
    fn apply_keeps_local_romanization() {
        let mut doc = crate::lyrics::lrc::parse("[00:01.00]一\n[00:02.00]二");
        doc.lines[0].romanized = Some("local".into());
        let r = Result {
            lang: "ja".into(),
            translated: vec![Some("one".into()), Some("two".into())],
            romanized: vec![Some("ichi".into()), Some("ni".into())],
        };
        apply(&mut doc, &r, "en");
        assert_eq!(doc.lines[0].romanized.as_deref(), Some("local"));
        assert_eq!(doc.lines[1].romanized.as_deref(), Some("ni"));
        assert_eq!(doc.lines[1].translated.as_deref(), Some("two"));
        assert_eq!(doc.translation.unwrap().provenance, PROVIDER);
    }
}
