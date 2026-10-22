//! XDG locations. `RTIFY_CONFIG_DIR` / `RTIFY_STATE_DIR` override them (tests, portable runs).

use std::env;
use std::path::PathBuf;

pub const APP_ID: &str = "io.github.everybodydies.rtify";
pub const APP_DIR: &str = "rtify";

fn xdg(var: &str, fallback: &str) -> PathBuf {
    env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(fallback))
}

pub fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

pub fn config_home() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config")
}

pub fn config_dir() -> PathBuf {
    env::var_os("RTIFY_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| config_home().join(APP_DIR))
}

pub fn state_dir() -> PathBuf {
    env::var_os("RTIFY_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| xdg("XDG_STATE_HOME", ".local/state").join(APP_DIR))
}

pub fn cache_dir() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache").join(APP_DIR)
}

pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn session_file() -> PathBuf {
    state_dir().join("session.json")
}

/// Written by the optional Noctalia user template (see `data/noctalia/`).
pub fn noctalia_css() -> PathBuf {
    config_dir().join("noctalia.css")
}

/// Drop-in `.lrc` / `.ttml` files the local lyrics provider searches.
pub fn lyrics_dir() -> PathBuf {
    config_dir().join("lyrics")
}

pub fn autostart_file() -> PathBuf {
    config_home().join("autostart").join(format!("{APP_DIR}.desktop"))
}
