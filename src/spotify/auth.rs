//! OAuth 2.0 Authorization Code with PKCE against accounts.spotify.com. No client secret:
//! users register their own Spotify app and paste its client ID.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub const SCOPES: &[&str] = &[
    "user-read-private",
    "user-read-playback-state",
    "user-modify-playback-state",
    "user-read-currently-playing",
    "user-read-recently-played",
    "user-library-read",
    "user-library-modify",
    "playlist-read-private",
    "playlist-read-collaborative",
    "playlist-modify-private",
    "playlist-modify-public",
    "user-follow-read",
];

pub fn accounts_base() -> String {
    std::env::var("RTIFY_ACCOUNTS_BASE").unwrap_or_else(|_| "https://accounts.spotify.com".into())
}

pub fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}/callback")
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("no Spotify client ID configured")]
    NoClientId,
    #[error("cannot listen on 127.0.0.1:{0}: {1}")]
    Listen(u16, std::io::Error),
    #[error("login was cancelled or denied: {0}")]
    Denied(String),
    #[error("state mismatch in OAuth callback")]
    State,
    #[error("timed out waiting for the browser")]
    Timeout,
    #[error("token request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Spotify rejected the token request: {0}")]
    Rejected(String),
    #[error("randomness unavailable: {0}")]
    Random(String),
}

#[derive(Debug, Clone)]
pub struct Token {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: SystemTime,
    pub scope: String,
}

impl Token {
    /// Refresh a minute early so requests never race the expiry.
    pub fn needs_refresh(&self) -> bool {
        SystemTime::now() + Duration::from_secs(60) >= self.expires_at
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
    #[serde(default)]
    scope: String,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: String,
    #[serde(default)]
    error_description: String,
}

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
    pub state: String,
}

fn random_string(bytes: usize) -> Result<String, AuthError> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| AuthError::Random(e.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

pub fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

impl Pkce {
    pub fn new() -> Result<Self, AuthError> {
        let verifier = random_string(64)?;
        Ok(Self {
            challenge: challenge_for(&verifier),
            verifier,
            state: random_string(16)?,
        })
    }

    pub fn authorize_url(&self, client_id: &str, port: u16) -> String {
        let mut url = url::Url::parse(&format!("{}/authorize", accounts_base())).expect("valid accounts base");
        url.query_pairs_mut()
            .append_pair("client_id", client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &redirect_uri(port))
            .append_pair("code_challenge_method", "S256")
            .append_pair("code_challenge", &self.challenge)
            .append_pair("state", &self.state)
            .append_pair("scope", &SCOPES.join(" "));
        url.into()
    }
}

const PAGE: &str = "<!doctype html><meta charset=utf-8><title>rtify</title>\
<body style=\"background:#16181d;color:#d2d5db;font:14px 'JetBrains Mono',monospace;display:grid;place-items:center;height:100vh;margin:0\">\
<div style=\"border:1px solid #303540;padding:24px 32px\">[ rtify ] {}</div>";

/// Bind the redirect listener. Done before opening the browser so the port is known to work.
pub async fn listen(port: u16) -> Result<TcpListener, AuthError> {
    TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| AuthError::Listen(port, e))
}

/// Accept connections until the `/callback` request arrives; returns the authorization code.
pub async fn wait_for_code(listener: TcpListener, expected_state: &str, timeout: Duration) -> Result<String, AuthError> {
    let fut = async {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { continue };
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let path = req.split_whitespace().nth(1).unwrap_or("");
            if !path.starts_with("/callback") {
                let _ = sock
                    .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await;
                continue;
            }
            let url = url::Url::parse(&format!("http://localhost{path}")).ok();
            let get = |k: &str| {
                url.as_ref()
                    .and_then(|u| u.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.into_owned()))
            };
            let result = match (get("code"), get("error"), get("state")) {
                (_, _, s) if s.as_deref() != Some(expected_state) => Err(AuthError::State),
                (Some(code), _, _) => Ok(code),
                (None, err, _) => Err(AuthError::Denied(err.unwrap_or_else(|| "unknown".into()))),
            };
            let msg = match &result {
                Ok(_) => "signed in — you can close this tab.".to_owned(),
                Err(e) => format!("sign-in failed: {e}"),
            };
            let body = PAGE.replace("{}", &msg);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            return result;
        }
    };
    tokio::time::timeout(timeout, fut).await.map_err(|_| AuthError::Timeout)?
}

async fn token_request(http: &reqwest::Client, form: &[(&str, &str)], old_refresh: Option<&str>) -> Result<Token, AuthError> {
    let resp = http.post(format!("{}/api/token", accounts_base())).form(form).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<ErrorResponse>(&text)
            .map(|e| format!("{} {}", e.error, e.error_description))
            .unwrap_or_else(|_| format!("HTTP {status}"));
        return Err(AuthError::Rejected(msg.trim().to_owned()));
    }
    let t: TokenResponse = resp.json().await?;
    Ok(Token {
        access_token: t.access_token,
        // Spotify may omit a new refresh token on refresh; keep the old one then.
        refresh_token: t.refresh_token.or_else(|| old_refresh.map(String::from)),
        expires_at: SystemTime::now() + Duration::from_secs(t.expires_in),
        scope: t.scope,
    })
}

pub async fn exchange_code(http: &reqwest::Client, client_id: &str, code: &str, verifier: &str, port: u16) -> Result<Token, AuthError> {
    let redirect = redirect_uri(port);
    token_request(
        http,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &redirect),
            ("client_id", client_id),
            ("code_verifier", verifier),
        ],
        None,
    )
    .await
}

pub async fn refresh(http: &reqwest::Client, client_id: &str, refresh_token: &str) -> Result<Token, AuthError> {
    token_request(
        http,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id),
        ],
        Some(refresh_token),
    )
    .await
}

pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc7636_challenge() {
        // Test vector from RFC 7636, appendix B.
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn authorize_url_has_pkce_parameters() {
        let p = Pkce::new().unwrap();
        assert!(p.verifier.len() >= 43);
        let url = p.authorize_url("abc", 8898);
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8898%2Fcallback"));
        assert!(url.contains(&format!("state={}", p.state)));
    }

    #[tokio::test]
    async fn callback_listener_checks_state() {
        let listener = listen(0).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(wait_for_code(listener, "s1", Duration::from_secs(5)));
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(b"GET /callback?code=xyz&state=s1 HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        assert!(out.contains("signed in"));
        assert_eq!(task.await.unwrap().unwrap(), "xyz");
    }
}
