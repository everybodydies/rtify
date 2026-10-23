//! The subset of Spotify Web API objects rtify reads.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Image {
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// Smallest image that is at least `min` pixels wide, else the largest one.
pub fn pick_image(images: &[Image], min: u32) -> Option<&Image> {
    let mut sorted: Vec<&Image> = images.iter().collect();
    sorted.sort_by_key(|i| i.width.unwrap_or(0));
    sorted.iter().find(|i| i.width.unwrap_or(0) >= min).or(sorted.last()).copied()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArtistRef {
    pub id: Option<String>,
    pub uri: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AlbumRef {
    pub id: Option<String>,
    pub uri: String,
    pub name: String,
    pub images: Vec<Image>,
    pub artists: Vec<ArtistRef>,
}

/// A track or podcast episode (`type` tells which). Episodes carry `show` instead of `album`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    pub id: Option<String>,
    pub uri: String,
    pub name: String,
    pub duration_ms: u64,
    pub artists: Vec<ArtistRef>,
    pub album: Option<AlbumRef>,
    pub show: Option<AlbumRef>,
    pub images: Vec<Image>,
    #[serde(rename = "type")]
    pub kind: String,
    pub is_local: bool,
}

impl Track {
    pub fn artist_names(&self) -> String {
        if self.artists.is_empty() {
            return self.show.as_ref().map(|s| s.name.clone()).unwrap_or_default();
        }
        self.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
    }

    pub fn album_name(&self) -> &str {
        self.album.as_ref().or(self.show.as_ref()).map(|a| a.name.as_str()).unwrap_or("")
    }

    pub fn images(&self) -> &[Image] {
        match (&self.album, &self.show) {
            (Some(a), _) if !a.images.is_empty() => &a.images,
            (_, Some(s)) if !s.images.is_empty() => &s.images,
            _ => &self.images,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Device {
    pub id: Option<String>,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub is_active: bool,
    pub is_restricted: bool,
    pub volume_percent: Option<u32>,
    pub supports_volume: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Context {
    #[serde(rename = "type")]
    pub kind: String,
    pub uri: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerState {
    pub device: Option<Device>,
    pub shuffle_state: bool,
    pub repeat_state: String,
    pub progress_ms: Option<u64>,
    pub is_playing: bool,
    pub item: Option<Track>,
    pub context: Option<Context>,
    pub currently_playing_type: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Page<T> {
    #[serde(default = "Vec::new")]
    pub items: Vec<T>,
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default)]
    pub total: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Owner {
    pub id: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Total {
    pub total: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Playlist {
    pub id: String,
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub owner: Owner,
    pub collaborative: bool,
    pub images: Option<Vec<Image>>,
    /// Current API name for the item count; older responses use `tracks`.
    pub items: Option<Total>,
    pub tracks: Option<Total>,
}

impl Playlist {
    pub fn count(&self) -> u32 {
        self.items.as_ref().or(self.tracks.as_ref()).map(|t| t.total).unwrap_or(0)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Album {
    pub id: String,
    pub uri: String,
    pub name: String,
    pub images: Vec<Image>,
    pub artists: Vec<ArtistRef>,
    pub release_date: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Artist {
    pub id: String,
    pub uri: String,
    pub name: String,
    pub images: Vec<Image>,
    pub genres: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Cursors {
    pub after: Option<String>,
}

/// `/me/following?type=artist` wraps a cursor page in `artists`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct FollowedArtists {
    pub artists: CursorPage<Artist>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CursorPage<T> {
    #[serde(default = "Vec::new")]
    pub items: Vec<T>,
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default)]
    pub cursors: Option<Cursors>,
}

impl<T> Default for CursorPage<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            next: None,
            cursors: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SavedAlbum {
    #[serde(default)]
    pub added_at: String,
    pub album: Album,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SavedTrack {
    #[serde(default)]
    pub added_at: String,
    pub track: Track,
}

/// Playlist entries: the current API calls the payload `item`, older responses `track`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PlaylistItem {
    #[serde(default)]
    pub added_at: String,
    #[serde(alias = "track")]
    pub item: Option<Track>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Devices {
    pub devices: Vec<Device>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Queue {
    pub currently_playing: Option<Track>,
    pub queue: Vec<Track>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Me {
    pub id: String,
    pub display_name: Option<String>,
    pub product: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SearchResult {
    pub tracks: Option<Page<Track>>,
    pub playlists: Option<Page<Option<Playlist>>>,
    pub albums: Option<Page<Album>>,
    pub artists: Option<Page<Artist>>,
}

impl<T> Default for Page<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            next: None,
            total: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_items_accept_both_names() {
        let new: PlaylistItem = serde_json::from_str(r#"{"added_at":"x","item":{"uri":"spotify:track:1","name":"a"}}"#).unwrap();
        let old: PlaylistItem = serde_json::from_str(r#"{"added_at":"x","track":{"uri":"spotify:track:1","name":"a"}}"#).unwrap();
        assert_eq!(new.item, old.item);
        let p: Playlist = serde_json::from_str(r#"{"id":"p","tracks":{"total":3}}"#).unwrap();
        assert_eq!(p.count(), 3);
    }

    #[test]
    fn image_choice() {
        let imgs = vec![
            Image {
                url: "big".into(),
                width: Some(640),
                height: None,
            },
            Image {
                url: "small".into(),
                width: Some(64),
                height: None,
            },
            Image {
                url: "mid".into(),
                width: Some(300),
                height: None,
            },
        ];
        assert_eq!(pick_image(&imgs, 200).unwrap().url, "mid");
        assert_eq!(pick_image(&imgs, 1000).unwrap().url, "big");
        assert!(pick_image(&[], 10).is_none());
    }

    #[test]
    fn episode_metadata() {
        let t: Track =
            serde_json::from_str(r#"{"uri":"spotify:episode:1","type":"episode","show":{"uri":"s","name":"Show","images":[{"url":"i"}]}}"#)
                .unwrap();
        assert_eq!(t.artist_names(), "Show");
        assert_eq!(t.images()[0].url, "i");
    }
}
