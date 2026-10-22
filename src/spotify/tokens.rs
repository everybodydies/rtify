//! Secrets live in the Secret Service (GNOME Keyring, KWallet, KeePassXC …) via `oo7`.
//! When no Secret Service is running, secrets are kept for this session only.

use std::collections::HashMap;

pub const SPOTIFY_REFRESH: &str = "spotify-refresh-token";
pub const DEEPL_KEY: &str = "deepl-api-key";

fn attributes<'a>(kind: &'a str, account: &'a str) -> HashMap<&'a str, &'a str> {
    HashMap::from([("application", "rtify"), ("kind", kind), ("account", account)])
}

pub async fn load(kind: &str, account: &str) -> Result<Option<String>, oo7::Error> {
    let keyring = oo7::Keyring::new().await?;
    let _ = keyring.unlock().await;
    let items = keyring.search_items(&attributes(kind, account)).await?;
    let Some(item) = items.first() else { return Ok(None) };
    let secret = item.secret().await?;
    Ok(Some(String::from_utf8_lossy(&secret).into_owned()).filter(|s| !s.is_empty()))
}

pub async fn store(kind: &str, account: &str, secret: &str) -> Result<(), oo7::Error> {
    let keyring = oo7::Keyring::new().await?;
    let _ = keyring.unlock().await;
    let label = match kind {
        SPOTIFY_REFRESH => "rtify: Spotify login",
        DEEPL_KEY => "rtify: DeepL API key",
        _ => "rtify",
    };
    keyring.create_item(label, &attributes(kind, account), secret, true).await
}

pub async fn delete(kind: &str, account: &str) -> Result<(), oo7::Error> {
    let keyring = oo7::Keyring::new().await?;
    keyring.delete(&attributes(kind, account)).await
}
