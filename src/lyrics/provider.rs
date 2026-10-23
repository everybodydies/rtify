//! Track identity for lyric lookups, text parsing, and the bundled demo songs.
//! Lyrics for Spotify tracks come from LRCLIB (see `lrclib`).

use super::model::{Attribution, LyricDocument};
use super::{lrc, ttml};

#[derive(Debug, Clone, Default)]
pub struct TrackQuery {
    /// Spotify track id (base62), if known.
    pub id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub duration_ms: u64,
}

impl TrackQuery {
    pub fn key(&self) -> String {
        if self.id.is_empty() {
            format!("{} - {}", self.artists.join(", "), self.title)
        } else {
            self.id.clone()
        }
    }
}

/// Parse lyrics text, sniffing TTML vs. LRC/plain text.
pub fn parse_text(text: &str) -> Result<LyricDocument, ttml::TtmlError> {
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if trimmed.starts_with('<') {
        ttml::parse(trimmed)
    } else {
        Ok(lrc::parse(trimmed))
    }
}

/// Short original songs written for rtify, used to develop and demonstrate the renderer.
pub mod demo {
    use super::*;

    pub const SONGS: &[(&str, &str, &str)] = &[
        ("Neon Harbor", "The Placeholders", include_str!("../../data/demo/neon-harbor.ttml")),
        (
            "Hoshizora Densha",
            "Demo Unit",
            include_str!("../../data/demo/hoshizora-densha.lrc"),
        ),
        ("Bam Bada", "Demo Unit", include_str!("../../data/demo/bam-bada.lrc")),
    ];

    pub fn song(index: usize) -> Option<LyricDocument> {
        let (title, artist, text) = SONGS.get(index)?;
        let mut doc = parse_text(text).ok()?;
        doc.title = Some((*title).into());
        doc.artist = Some((*artist).into());
        doc.track_key = format!("demo:{index}");
        doc.attribution = Attribution {
            source: "demo".into(),
            source_id: format!("demo:{index}"),
            rights: "original demo text, GPL-3.0-or-later".into(),
            songwriters: vec!["rtify contributors".into()],
        };
        Some(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::model::TimingMode;

    #[test]
    fn demo_songs_parse() {
        for i in 0..demo::SONGS.len() {
            let doc = demo::song(i).unwrap();
            assert_ne!(doc.timing, TimingMode::Static, "demo {i}");
            assert!(doc.lines.len() >= 4);
        }
        assert_eq!(demo::song(0).unwrap().timing, TimingMode::Word);
    }

    #[test]
    fn sniffs_format() {
        assert_eq!(parse_text("\u{feff}[00:01.00]a").unwrap().lines[0].original, "a");
        assert_eq!(
            parse_text(r#"<tt><body><p begin="1s" end="2s">x</p></body></tt>"#).unwrap().lines[0].original,
            "x"
        );
    }

    #[test]
    fn query_key_prefers_id() {
        let q = TrackQuery {
            id: "abc".into(),
            title: "t".into(),
            ..Default::default()
        };
        assert_eq!(q.key(), "abc");
        let q = TrackQuery {
            title: "t".into(),
            artists: vec!["a".into()],
            ..Default::default()
        };
        assert_eq!(q.key(), "a - t");
    }
}
