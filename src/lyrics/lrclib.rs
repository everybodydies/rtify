//! LRCLIB (https://lrclib.net), the open lyrics database qtify uses: exact lookup by
//! artist/title/album/duration, then a fuzzy search that prefers synced results.

use serde::Deserialize;

use super::lrc;
use super::model::{Attribution, LyricDocument};
use super::provider::TrackQuery;

pub fn base() -> String {
    std::env::var("RTIFY_LRCLIB_BASE").unwrap_or_else(|_| "https://lrclib.net".into())
}

const CLIENT: &str = concat!("rtify/", env!("CARGO_PKG_VERSION"), " (https://github.com/everybodydies/rtify)");

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Record {
    pub id: Option<u64>,
    pub synced_lyrics: Option<String>,
    pub plain_lyrics: Option<String>,
    pub instrumental: bool,
}

/// What a lookup produced.
#[derive(Debug, Clone)]
pub enum Found {
    Lyrics(Box<LyricDocument>),
    Instrumental,
    Missing,
}

pub fn document(r: &Record, key: &str) -> Found {
    let text = r
        .synced_lyrics
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or(r.plain_lyrics.as_deref().filter(|s| !s.trim().is_empty()));
    let Some(text) = text else {
        return if r.instrumental { Found::Instrumental } else { Found::Missing };
    };
    let mut doc = lrc::parse(text);
    doc.track_key = key.to_owned();
    doc.attribution = Attribution {
        source: "LRCLIB".into(),
        source_id: r.id.map(|i| format!("lrclib:{i}")).unwrap_or_default(),
        rights: "community lyrics, lrclib.net".into(),
        songwriters: Vec::new(),
    };
    if doc.lines.is_empty() {
        Found::Missing
    } else {
        Found::Lyrics(Box::new(doc))
    }
}

/// Pick the best fuzzy-search hit: synced lyrics first, then plain, then instrumental.
pub fn best(list: &[Record]) -> Option<&Record> {
    list.iter()
        .find(|r| r.synced_lyrics.as_deref().is_some_and(|s| !s.trim().is_empty()))
        .or_else(|| {
            list.iter()
                .find(|r| r.plain_lyrics.as_deref().is_some_and(|s| !s.trim().is_empty()))
        })
        .or_else(|| list.first())
}

pub async fn fetch(http: &reqwest::Client, q: &TrackQuery) -> Result<Found, reqwest::Error> {
    let artist = q.artists.first().cloned().unwrap_or_default();
    let exact = [
        ("artist_name", artist.clone()),
        ("track_name", q.title.clone()),
        ("album_name", q.album.clone()),
        ("duration", (q.duration_ms / 1000).to_string()),
    ];
    let resp = http
        .get(format!("{}/api/get", base()))
        .header("Lrclib-Client", CLIENT)
        .query(&exact)
        .send()
        .await?;
    if resp.status().is_success()
        && let Ok(r) = resp.json::<Record>().await
        && !matches!(document(&r, &q.key()), Found::Missing)
    {
        return Ok(document(&r, &q.key()));
    }
    let fuzzy = [("track_name", q.title.clone()), ("artist_name", artist)];
    let resp = http
        .get(format!("{}/api/search", base()))
        .header("Lrclib-Client", CLIENT)
        .query(&fuzzy)
        .send()
        .await?;
    if !resp.status().is_success() {
        return Ok(Found::Missing);
    }
    let list: Vec<Record> = resp.json().await.unwrap_or_default();
    Ok(best(&list).map(|r| document(r, &q.key())).unwrap_or(Found::Missing))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::TimingMode;

    fn rec(synced: Option<&str>, plain: Option<&str>, instrumental: bool) -> Record {
        Record {
            id: Some(1),
            synced_lyrics: synced.map(String::from),
            plain_lyrics: plain.map(String::from),
            instrumental,
        }
    }

    #[test]
    fn synced_beats_plain() {
        match document(&rec(Some("[00:01.00]a\n[00:02.00]b"), Some("a\nb"), false), "k") {
            Found::Lyrics(d) => {
                assert_eq!(d.timing, TimingMode::Line);
                assert_eq!(d.attribution.source, "LRCLIB");
                assert_eq!(d.track_key, "k");
            }
            other => panic!("{other:?}"),
        }
        match document(&rec(None, Some("only\ntext"), false), "k") {
            Found::Lyrics(d) => assert_eq!(d.timing, TimingMode::Static),
            other => panic!("{other:?}"),
        }
        assert!(matches!(document(&rec(None, None, true), "k"), Found::Instrumental));
        assert!(matches!(document(&rec(Some("  "), None, false), "k"), Found::Missing));
    }

    #[test]
    fn best_prefers_synced() {
        let list = vec![rec(None, Some("p"), false), rec(Some("[00:01.00]s"), None, false)];
        assert_eq!(best(&list).unwrap().synced_lyrics.as_deref(), Some("[00:01.00]s"));
        assert!(best(&[]).is_none());
    }
}
