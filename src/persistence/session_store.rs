//! Last-known session state (`~/.local/state/rtify/session.json`), kept per Spotify account.
//! This is a hint for the UI until live state arrives, never an instruction to change playback.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::atomic;

pub const SESSION_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LastTrack {
    pub uri: String,
    pub name: String,
    pub artists: String,
    pub album: String,
    pub image_url: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountSession {
    pub last_track: Option<LastTrack>,
    pub last_context_uri: Option<String>,
    /// URI of the playlist/album selected in the library pane.
    pub library_selection: Option<String>,
    pub page: String,
    pub shuffle: Option<bool>,
    /// `off`, `context` or `track`, as Spotify names them.
    pub repeat: Option<String>,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    /// Target of "add to last playlist".
    pub last_playlist: Option<String>,
    /// The "sign in for the built-in player" notice was shown once already.
    pub player_hint_shown: bool,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionFile {
    pub version: u32,
    pub last_account: Option<String>,
    pub accounts: BTreeMap<String, AccountSession>,
}

impl Default for SessionFile {
    fn default() -> Self {
        Self {
            version: SESSION_VERSION,
            last_account: None,
            accounts: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
pub struct SessionStore {
    path: PathBuf,
    pub data: SessionFile,
    pub warning: Option<String>,
}

impl SessionStore {
    pub fn load(path: &Path) -> Self {
        let mut store = Self {
            path: path.to_owned(),
            data: SessionFile::default(),
            warning: None,
        };
        match fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<SessionFile>(&bytes) {
                Ok(d) if d.version <= SESSION_VERSION => store.data = d,
                Ok(d) => store.warning = Some(format!("session file version {} is newer; ignoring it", d.version)),
                Err(e) => {
                    let backup = atomic::quarantine(path).map(|p| p.display().to_string());
                    store.warning = Some(format!(
                        "invalid session state ({e}); backup: {}",
                        backup.unwrap_or_else(|e| e.to_string())
                    ));
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => store.warning = Some(format!("cannot read session state: {e}")),
        }
        store
    }

    /// The account the UI should restore before login completes.
    pub fn current(&self) -> Option<&AccountSession> {
        self.data.last_account.as_ref().and_then(|a| self.data.accounts.get(a))
    }

    pub fn account_mut(&mut self, account: &str) -> &mut AccountSession {
        self.data.last_account = Some(account.to_owned());
        self.data.accounts.entry(account.to_owned()).or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&self.data).map_err(io::Error::other)?;
        atomic::write(&self.path, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts_are_separate() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("session.json");
        let mut s = SessionStore::load(&p);
        s.account_mut("alice").shuffle = Some(true);
        s.account_mut("bob").repeat = Some("track".into());
        s.save().unwrap();
        let s = SessionStore::load(&p);
        assert_eq!(s.data.last_account.as_deref(), Some("bob"));
        assert_eq!(s.current().unwrap().repeat.as_deref(), Some("track"));
        assert_eq!(s.data.accounts["alice"].shuffle, Some(true));
        assert_eq!(s.data.accounts["alice"].repeat, None);
    }

    #[test]
    fn garbage_is_quarantined() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("session.json");
        fs::write(&p, "{ nope").unwrap();
        let s = SessionStore::load(&p);
        assert!(s.warning.is_some());
        assert!(!p.exists());
        assert!(s.current().is_none());
    }
}
