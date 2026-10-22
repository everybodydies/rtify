//! Lyric sources. Every provider returns the same `LyricDocument`, so the renderer never knows
//! where text came from. Only sources whose content the user may display are implemented:
//! files the user placed on disk and the bundled demo songs written for this project.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

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

#[derive(Debug, thiserror::Error)]
pub enum LyricsError {
    #[error("{0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("{0}: {1}")]
    Ttml(PathBuf, ttml::TtmlError),
}

pub trait LyricsProvider: Send + Sync {
    fn name(&self) -> &str;
    fn lookup(&self, query: &TrackQuery) -> Result<Option<LyricDocument>, LyricsError>;
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

pub fn load_file(path: &Path) -> Result<LyricDocument, LyricsError> {
    let text = fs::read_to_string(path).map_err(|e| LyricsError::Io(path.to_owned(), e))?;
    let mut doc = parse_text(&text).map_err(|e| LyricsError::Ttml(path.to_owned(), e))?;
    doc.attribution = Attribution {
        source: "local file".into(),
        source_id: path.display().to_string(),
        rights: "supplied by you".into(),
        songwriters: std::mem::take(&mut doc.attribution.songwriters),
    };
    doc.track_key = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(doc)
}

/// Lowercase, strip characters that cannot appear in file names, collapse whitespace.
pub fn normalize_name(s: &str) -> String {
    s.chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

const EXTENSIONS: [&str; 3] = ["ttml", "lrc", "txt"];

/// Looks for `<track id>.lrc`, `<artist> - <title>.lrc` or `<title>.lrc` (also `.ttml`, `.txt`),
/// case-insensitively, in each directory.
pub struct LocalFileProvider {
    pub dirs: Vec<PathBuf>,
}

impl LocalFileProvider {
    pub fn candidates(query: &TrackQuery) -> Vec<String> {
        let mut names = Vec::new();
        if !query.id.is_empty() {
            names.push(normalize_name(&query.id));
        }
        if let Some(first) = query.artists.first() {
            names.push(normalize_name(&format!("{first} - {}", query.title)));
        }
        if query.artists.len() > 1 {
            names.push(normalize_name(&format!("{} - {}", query.artists.join(", "), query.title)));
        }
        names.push(normalize_name(&query.title));
        names.retain(|n| !n.is_empty());
        names
    }
}

impl LyricsProvider for LocalFileProvider {
    fn name(&self) -> &str {
        "local file"
    }

    fn lookup(&self, query: &TrackQuery) -> Result<Option<LyricDocument>, LyricsError> {
        let wanted = Self::candidates(query);
        for dir in &self.dirs {
            let Ok(entries) = fs::read_dir(dir) else { continue };
            let mut files: HashMap<(String, usize), PathBuf> = HashMap::new();
            for e in entries.flatten() {
                let path = e.path();
                let (Some(stem), Some(ext)) = (path.file_stem(), path.extension()) else {
                    continue;
                };
                let ext = ext.to_string_lossy().to_lowercase();
                if let Some(rank) = EXTENSIONS.iter().position(|x| *x == ext) {
                    files.insert((normalize_name(&stem.to_string_lossy()), rank), path);
                }
            }
            for name in &wanted {
                for rank in 0..EXTENSIONS.len() {
                    if let Some(path) = files.get(&(name.clone(), rank)) {
                        let mut doc = load_file(path)?;
                        doc.track_key = query.key();
                        return Ok(Some(doc));
                    }
                }
            }
        }
        Ok(None)
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

/// In-memory cache that honours provider expiry (`LyricDocument::expires_at`).
#[derive(Default)]
pub struct LyricCache {
    docs: HashMap<String, LyricDocument>,
    order: Vec<String>,
}

impl LyricCache {
    const CAPACITY: usize = 64;

    pub fn get(&mut self, key: &str) -> Option<&LyricDocument> {
        if self.docs.get(key).is_some_and(|d| d.is_expired(SystemTime::now())) {
            self.docs.remove(key);
            self.order.retain(|k| k != key);
        }
        self.docs.get(key)
    }

    pub fn insert(&mut self, doc: LyricDocument) {
        let key = doc.track_key.clone();
        self.order.retain(|k| *k != key);
        self.order.push(key.clone());
        self.docs.insert(key, doc);
        while self.order.len() > Self::CAPACITY {
            let old = self.order.remove(0);
            self.docs.remove(&old);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::lyrics::model::TimingMode;

    #[test]
    fn finds_files_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("The Band - My Song.LRC"), "[00:01.00]hi").unwrap();
        fs::write(dir.path().join("other.lrc"), "[00:01.00]no").unwrap();
        let p = LocalFileProvider {
            dirs: vec![dir.path().into()],
        };
        let q = TrackQuery {
            id: "abc".into(),
            title: "My Song".into(),
            artists: vec!["the band".into()],
            ..Default::default()
        };
        let doc = p.lookup(&q).unwrap().unwrap();
        assert_eq!(doc.lines[0].original, "hi");
        assert_eq!(doc.track_key, "abc");
        assert_eq!(doc.attribution.source, "local file");
    }

    #[test]
    fn id_beats_title_and_ttml_beats_lrc() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("x.lrc"), "[00:01.00]title").unwrap();
        fs::write(dir.path().join("id1.lrc"), "[00:01.00]lrc").unwrap();
        fs::write(
            dir.path().join("id1.ttml"),
            r#"<tt><body><p begin="1s" end="2s">ttml</p></body></tt>"#,
        )
        .unwrap();
        let p = LocalFileProvider {
            dirs: vec![dir.path().into()],
        };
        let q = TrackQuery {
            id: "id1".into(),
            title: "x".into(),
            ..Default::default()
        };
        assert_eq!(p.lookup(&q).unwrap().unwrap().lines[0].original, "ttml");
    }

    #[test]
    fn nothing_found() {
        let p = LocalFileProvider {
            dirs: vec!["/nonexistent".into()],
        };
        assert!(
            p.lookup(&TrackQuery {
                title: "x".into(),
                ..Default::default()
            })
            .unwrap()
            .is_none()
        );
    }

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
    fn cache_expires() {
        let mut c = LyricCache::default();
        let mut d = LyricDocument::new(vec![]);
        d.track_key = "a".into();
        d.expires_at = Some(SystemTime::now() - Duration::from_secs(1));
        c.insert(d);
        assert!(c.get("a").is_none());
        let mut d = LyricDocument::new(vec![]);
        d.track_key = "b".into();
        c.insert(d);
        assert!(c.get("b").is_some());
    }
}
