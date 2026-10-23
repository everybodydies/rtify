//! Lyrics subsystem: models, parsers, language support and timing. Independent of Spotify and
//! of where the text came from.

pub mod google;
pub mod lrc;
pub mod lrclib;
pub mod model;
pub mod provider;
pub mod romanization;
pub mod synchronization;
pub mod translation;
pub mod ttml;

pub use model::{LyricDocument, LyricLine, TimingMode};
