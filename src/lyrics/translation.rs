//! Opt-in lyric translation through a configured DeepL account. Nothing is sent unless the user
//! enabled translation and stored an API key in the Secret Service; documents that already carry
//! a translation keep it.

use serde::Deserialize;

use super::model::{LyricDocument, Translation};

pub const PROVIDER: &str = "DeepL";

#[derive(Debug, thiserror::Error)]
pub enum TranslationError {
    #[error("no DeepL API key stored")]
    NoKey,
    #[error("DeepL request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("DeepL returned {0} translations for {1} lines")]
    Mismatch(usize, usize),
}

#[derive(Deserialize)]
struct Response {
    translations: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    text: String,
}

fn endpoint(key: &str) -> String {
    if let Ok(base) = std::env::var("RTIFY_DEEPL_BASE") {
        return format!("{base}/v2/translate");
    }
    // Free-tier keys end in ":fx" and use a separate host.
    let host = if key.ends_with(":fx") {
        "api-free.deepl.com"
    } else {
        "api.deepl.com"
    };
    format!("https://{host}/v2/translate")
}

/// Translate every line of `doc` into `target` (ISO code, e.g. "en", "de", "pt-BR").
/// Lines are sent in one request; DeepL returns them in order.
pub async fn translate(http: &reqwest::Client, key: &str, doc: &mut LyricDocument, target: &str) -> Result<(), TranslationError> {
    if key.is_empty() {
        return Err(TranslationError::NoKey);
    }
    if doc.translation.as_ref().is_some_and(|t| t.language.eq_ignore_ascii_case(target)) {
        return Ok(());
    }
    let texts: Vec<&str> = doc.lines.iter().map(|l| l.original.as_str()).collect();
    if texts.is_empty() {
        return Ok(());
    }
    let body = serde_json::json!({ "text": texts, "target_lang": target.to_uppercase(), "preserve_formatting": true });
    let resp: Response = http
        .post(endpoint(key))
        .header("Authorization", format!("DeepL-Auth-Key {key}"))
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if resp.translations.len() != doc.lines.len() {
        return Err(TranslationError::Mismatch(resp.translations.len(), doc.lines.len()));
    }
    for (line, t) in doc.lines.iter_mut().zip(resp.translations) {
        // Identical output means the line was already in the target language.
        line.translated = (t.text.trim() != line.original.trim()).then_some(t.text);
    }
    doc.translation = Some(Translation {
        language: target.to_owned(),
        provenance: PROVIDER.into(),
    });
    Ok(())
}
