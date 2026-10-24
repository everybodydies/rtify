//! The subset of Spotify Web API objects rtify reads.

use serde::{Deserialize, Serialize};

/// Spotify sends explicit `null` for some fields (local files have no album or artist uri);
/// `#[serde(default)]` only covers missing ones.
fn null_as_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Image {
    #[serde(deserialize_with = "null_as_default")]
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
    #[serde(deserialize_with = "null_as_default")]
    pub uri: String,
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AlbumRef {
    pub id: Option<String>,
    #[serde(deserialize_with = "null_as_default")]
    pub uri: String,
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
    #[serde(deserialize_with = "null_as_default")]
    pub images: Vec<Image>,
    #[serde(deserialize_with = "null_as_default")]
    pub artists: Vec<ArtistRef>,
}

/// A track or podcast episode (`type` tells which). Episodes carry `show` instead of `album`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    pub id: Option<String>,
    #[serde(deserialize_with = "null_as_default")]
    pub uri: String,
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
    pub duration_ms: u64,
    #[serde(deserialize_with = "null_as_default")]
    pub artists: Vec<ArtistRef>,
    pub album: Option<AlbumRef>,
    pub show: Option<AlbumRef>,
    #[serde(deserialize_with = "null_as_default")]
    pub images: Vec<Image>,
    #[serde(rename = "type")]
    #[serde(deserialize_with = "null_as_default")]
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
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
    #[serde(rename = "type")]
    #[serde(deserialize_with = "null_as_default")]
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
    #[serde(deserialize_with = "null_as_default")]
    pub kind: String,
    #[serde(deserialize_with = "null_as_default")]
    pub uri: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerState {
    pub device: Option<Device>,
    pub shuffle_state: bool,
    #[serde(deserialize_with = "null_as_default")]
    pub repeat_state: String,
    pub progress_ms: Option<u64>,
    pub is_playing: bool,
    pub item: Option<Track>,
    pub context: Option<Context>,
    #[serde(deserialize_with = "null_as_default")]
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
    #[serde(deserialize_with = "null_as_default")]
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
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    #[serde(deserialize_with = "null_as_default")]
    pub uri: String,
    #[serde(deserialize_with = "null_as_default")]
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
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    #[serde(deserialize_with = "null_as_default")]
    pub uri: String,
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
    #[serde(deserialize_with = "null_as_default")]
    pub images: Vec<Image>,
    #[serde(deserialize_with = "null_as_default")]
    pub artists: Vec<ArtistRef>,
    #[serde(deserialize_with = "null_as_default")]
    pub release_date: String,
    /// "album", "single" or "compilation".
    #[serde(deserialize_with = "null_as_default")]
    pub album_type: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Artist {
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    #[serde(deserialize_with = "null_as_default")]
    pub uri: String,
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
    #[serde(deserialize_with = "null_as_default")]
    pub images: Vec<Image>,
    #[serde(deserialize_with = "null_as_default")]
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
    #[serde(default, deserialize_with = "null_as_default")]
    pub added_at: String,
    pub album: Album,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SavedTrack {
    #[serde(default, deserialize_with = "null_as_default")]
    pub added_at: String,
    pub track: Track,
}

/// Playlist entries: the current API calls the payload `item`, older responses `track`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PlaylistItem {
    #[serde(default, deserialize_with = "null_as_default")]
    pub added_at: String,
    #[serde(alias = "track")]
    pub item: Option<Track>,
}

/// One entry of `/me/player/recently-played`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PlayHistory {
    pub track: Track,
    #[serde(default, deserialize_with = "null_as_default")]
    pub played_at: String,
    #[serde(default)]
    pub context: Option<Context>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Devices {
    #[serde(deserialize_with = "null_as_default")]
    pub devices: Vec<Device>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Queue {
    pub currently_playing: Option<Track>,
    #[serde(deserialize_with = "null_as_default")]
    pub queue: Vec<Track>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Me {
    #[serde(deserialize_with = "null_as_default")]
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
    fn local_files_with_null_refs_parse() {
        let item: PlaylistItem = serde_json::from_str(
            r#"{"added_at":"x","track":{"uri":"spotify:local:a:b:c:1","id":null,"name":"c","is_local":true,
                "album":{"uri":null,"id":null,"name":"b","images":[]},
                "artists":[{"uri":null,"id":null,"href":null,"name":"a"}]}}"#,
        )
        .unwrap();
        let t = item.item.unwrap();
        assert_eq!((t.artist_names().as_str(), t.album_name()), ("a", "b"));
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
