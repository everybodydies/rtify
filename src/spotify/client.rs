//! Spotify Web API client. Runs on the Tokio runtime; refreshes the access token on demand and
//! reports rate limits instead of retrying blindly.

use std::sync::Arc;
use std::time::Duration;

use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::sync::Mutex;

use super::auth::{self, AuthError, Token};
use super::model::*;

pub fn api_base() -> String {
    std::env::var("RTIFY_API_BASE").unwrap_or_else(|_| "https://api.spotify.com/v1".into())
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("not signed in")]
    NotLoggedIn,
    #[error("{0}")]
    Auth(#[from] AuthError),
    #[error("network: {0}")]
    Http(#[from] reqwest::Error),
    #[error("rate limited, retry in {0}s")]
    RateLimited(u64),
    #[error("Spotify: {1} (HTTP {0})")]
    Status(u16, String),
}

impl ApiError {
    /// "No active device" and similar situations the UI explains instead of reporting as errors.
    pub fn is_no_device(&self) -> bool {
        matches!(self, ApiError::Status(404, m) if m.to_lowercase().contains("device"))
    }

    pub fn needs_premium(&self) -> bool {
        matches!(self, ApiError::Status(403, m) if m.to_lowercase().contains("premium"))
    }
}

/// Called with the new refresh token whenever Spotify rotates it, so it can be persisted.
pub type RefreshHook = Arc<dyn Fn(String) + Send + Sync>;

#[derive(Clone)]
pub struct SpotifyClient {
    http: reqwest::Client,
    base: String,
    client_id: String,
    token: Arc<Mutex<Option<Token>>>,
    on_refresh: Option<RefreshHook>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct PlayRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uris: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_ms: Option<u64>,
}

impl PlayRequest {
    pub fn context(uri: &str, track_uri: Option<&str>) -> Self {
        Self {
            context_uri: Some(uri.into()),
            offset: track_uri.map(|t| serde_json::json!({ "uri": t })),
            ..Default::default()
        }
    }

    pub fn tracks(uris: Vec<String>, start: usize) -> Self {
        Self {
            uris: Some(uris),
            offset: Some(serde_json::json!({ "position": start })),
            ..Default::default()
        }
    }
}

#[derive(serde::Deserialize, Default)]
struct ErrorBody {
    #[serde(default)]
    error: ErrorInner,
}

#[derive(serde::Deserialize, Default)]
struct ErrorInner {
    #[serde(default)]
    message: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct FullAlbum {
    #[serde(flatten)]
    pub album: Album,
    pub tracks: Page<Track>,
}

impl SpotifyClient {
    pub fn new(http: reqwest::Client, client_id: String) -> Self {
        Self {
            http,
            base: api_base(),
            client_id,
            token: Arc::new(Mutex::new(None)),
            on_refresh: None,
        }
    }

    pub fn with_refresh_hook(mut self, hook: RefreshHook) -> Self {
        self.on_refresh = Some(hook);
        self
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub async fn set_token(&self, token: Option<Token>) {
        *self.token.lock().await = token;
    }

    pub async fn refresh_token(&self) -> Option<String> {
        self.token.lock().await.as_ref().and_then(|t| t.refresh_token.clone())
    }

    async fn access_token(&self, force_refresh: bool) -> Result<String, ApiError> {
        let mut guard = self.token.lock().await;
        let token = guard.as_mut().ok_or(ApiError::NotLoggedIn)?;
        if force_refresh || token.needs_refresh() {
            let Some(refresh) = token.refresh_token.clone() else {
                if force_refresh {
                    return Err(ApiError::NotLoggedIn);
                }
                return Ok(token.access_token.clone());
            };
            let new = auth::refresh(&self.http, &self.client_id, &refresh).await?;
            if new.refresh_token != token.refresh_token
                && let (Some(hook), Some(r)) = (&self.on_refresh, &new.refresh_token)
            {
                hook(r.clone());
            }
            *token = new;
        }
        Ok(token.access_token.clone())
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<serde_json::Value>,
    ) -> Result<Option<Vec<u8>>, ApiError> {
        let url = if path.starts_with("http") {
            path.to_owned()
        } else {
            format!("{}{path}", self.base)
        };
        for attempt in 0..2 {
            let token = self.access_token(attempt > 0).await?;
            let mut req = self.http.request(method.clone(), &url).bearer_auth(token).query(query);
            req = match &body {
                Some(b) => req.json(b),
                // Spotify wants a Content-Length on bodiless PUT/POST.
                None if method != Method::GET => req.header("Content-Length", "0"),
                None => req,
            };
            let resp = req.timeout(Duration::from_secs(15)).send().await?;
            let status = resp.status();
            if status == StatusCode::UNAUTHORIZED && attempt == 0 {
                continue;
            }
            if status == StatusCode::TOO_MANY_REQUESTS {
                let wait = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(5);
                return Err(ApiError::RateLimited(wait));
            }
            let bytes = resp.bytes().await?;
            if !status.is_success() {
                let msg = serde_json::from_slice::<ErrorBody>(&bytes)
                    .map(|e| e.error.message)
                    .unwrap_or_default();
                let msg = if msg.is_empty() {
                    status.canonical_reason().unwrap_or("error").to_owned()
                } else {
                    msg
                };
                return Err(ApiError::Status(status.as_u16(), msg));
            }
            if status == StatusCode::NO_CONTENT || bytes.is_empty() {
                return Ok(None);
            }
            return Ok(Some(bytes.to_vec()));
        }
        Err(ApiError::NotLoggedIn)
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<Option<T>, ApiError> {
        match self.send(Method::GET, path, query, None).await? {
            None => Ok(None),
            Some(b) => serde_json::from_slice(&b)
                .map(Some)
                .map_err(|e| ApiError::Status(200, format!("unexpected response: {e}"))),
        }
    }

    async fn get_required<T: DeserializeOwned + Default>(&self, path: &str, query: &[(&str, String)]) -> Result<T, ApiError> {
        Ok(self.get(path, query).await?.unwrap_or_default())
    }

    async fn command(&self, method: Method, path: &str, query: &[(&str, String)], body: Option<serde_json::Value>) -> Result<(), ApiError> {
        self.send(method, path, query, body).await.map(|_| ())
    }

    fn device_query(device: Option<&str>) -> Vec<(&'static str, String)> {
        device.map(|d| vec![("device_id", d.to_owned())]).unwrap_or_default()
    }

    // --- account & playback state -------------------------------------------------------------

    pub async fn me(&self) -> Result<Me, ApiError> {
        self.get_required("/me", &[]).await
    }

    /// `None` when no device is active (HTTP 204).
    pub async fn player(&self) -> Result<Option<PlayerState>, ApiError> {
        self.get("/me/player", &[("additional_types", "episode".into())]).await
    }

    pub async fn devices(&self) -> Result<Vec<Device>, ApiError> {
        Ok(self.get_required::<Devices>("/me/player/devices", &[]).await?.devices)
    }

    // --- playback commands --------------------------------------------------------------------

    pub async fn play(&self, req: Option<&PlayRequest>, device: Option<&str>) -> Result<(), ApiError> {
        let body = req.map(|r| serde_json::to_value(r).unwrap_or_default());
        self.command(Method::PUT, "/me/player/play", &Self::device_query(device), body)
            .await
    }

    pub async fn pause(&self) -> Result<(), ApiError> {
        self.command(Method::PUT, "/me/player/pause", &[], None).await
    }

    pub async fn next(&self) -> Result<(), ApiError> {
        self.command(Method::POST, "/me/player/next", &[], None).await
    }

    pub async fn previous(&self) -> Result<(), ApiError> {
        self.command(Method::POST, "/me/player/previous", &[], None).await
    }

    pub async fn seek(&self, position_ms: u64) -> Result<(), ApiError> {
        self.command(Method::PUT, "/me/player/seek", &[("position_ms", position_ms.to_string())], None)
            .await
    }

    pub async fn volume(&self, percent: u32) -> Result<(), ApiError> {
        self.command(
            Method::PUT,
            "/me/player/volume",
            &[("volume_percent", percent.min(100).to_string())],
            None,
        )
        .await
    }

    pub async fn shuffle(&self, on: bool, device: Option<&str>) -> Result<(), ApiError> {
        let mut q = Self::device_query(device);
        q.push(("state", on.to_string()));
        self.command(Method::PUT, "/me/player/shuffle", &q, None).await
    }

    /// `mode`: `off`, `context` or `track`.
    pub async fn repeat(&self, mode: &str, device: Option<&str>) -> Result<(), ApiError> {
        let mut q = Self::device_query(device);
        q.push(("state", mode.to_owned()));
        self.command(Method::PUT, "/me/player/repeat", &q, None).await
    }

    pub async fn transfer(&self, device_id: &str, play: bool) -> Result<(), ApiError> {
        let body = serde_json::json!({ "device_ids": [device_id], "play": play });
        self.command(Method::PUT, "/me/player", &[], Some(body)).await
    }

    // --- library ------------------------------------------------------------------------------

    fn page_query(offset: u32, limit: u32) -> Vec<(&'static str, String)> {
        vec![("offset", offset.to_string()), ("limit", limit.to_string())]
    }

    pub async fn my_playlists(&self, offset: u32) -> Result<Page<Playlist>, ApiError> {
        self.get_required("/me/playlists", &Self::page_query(offset, 50)).await
    }

    pub async fn saved_tracks(&self, offset: u32) -> Result<Page<SavedTrack>, ApiError> {
        self.get_required("/me/tracks", &Self::page_query(offset, 50)).await
    }

    pub async fn saved_albums(&self, offset: u32) -> Result<Page<SavedAlbum>, ApiError> {
        self.get_required("/me/albums", &Self::page_query(offset, 50)).await
    }

    pub async fn playlist_items(&self, playlist_id: &str, offset: u32) -> Result<Page<PlaylistItem>, ApiError> {
        let mut q = Self::page_query(offset, 100);
        q.push(("additional_types", "track,episode".into()));
        self.get_required(&format!("/playlists/{playlist_id}/items"), &q).await
    }

    pub async fn album(&self, album_id: &str) -> Result<Option<FullAlbum>, ApiError> {
        self.get(&format!("/albums/{album_id}"), &[]).await
    }

    pub async fn search(&self, query: &str) -> Result<SearchResult, ApiError> {
        let q = vec![
            ("q", query.to_owned()),
            ("type", "track,playlist,album".into()),
            ("limit", "20".into()),
        ];
        self.get_required("/search", &q).await
    }

    pub async fn is_saved(&self, uri: &str) -> Result<bool, ApiError> {
        let r: Option<Vec<bool>> = self.get("/me/library/contains", &[("uris", uri.to_owned())]).await?;
        Ok(r.and_then(|v| v.first().copied()).unwrap_or(false))
    }

    pub async fn set_saved(&self, uri: &str, saved: bool) -> Result<(), ApiError> {
        let method = if saved { Method::PUT } else { Method::DELETE };
        self.command(method, "/me/library", &[("uris", uri.to_owned())], None).await
    }
}

/// Base62 id from `spotify:kind:id`.
pub fn uri_id(uri: &str) -> &str {
    uri.rsplit(':').next().unwrap_or(uri)
}

pub fn uri_kind(uri: &str) -> &str {
    uri.split(':').nth(1).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_parts() {
        assert_eq!(uri_id("spotify:playlist:abc"), "abc");
        assert_eq!(uri_kind("spotify:playlist:abc"), "playlist");
        assert_eq!(uri_kind("spotify:user:x:collection"), "user");
    }

    #[test]
    fn play_request_shape() {
        let v = serde_json::to_value(PlayRequest::context("spotify:album:1", Some("spotify:track:2"))).unwrap();
        assert_eq!(
            v,
            serde_json::json!({ "context_uri": "spotify:album:1", "offset": { "uri": "spotify:track:2" } })
        );
        let v = serde_json::to_value(PlayRequest::tracks(vec!["a".into(), "b".into()], 1)).unwrap();
        assert_eq!(v["offset"]["position"], 1);
    }

    #[test]
    fn error_classification() {
        assert!(ApiError::Status(404, "Player command failed: No active device found".into()).is_no_device());
        assert!(ApiError::Status(403, "Player command failed: Premium required".into()).needs_premium());
    }
}
