use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{atomic, migration};
use crate::config::Config;

/// Owns `config.toml`. The UI mutates `config` and calls `save` (debounced by the caller);
/// orderly shutdown flushes once more.
#[derive(Debug)]
pub struct ConfigStore {
    path: PathBuf,
    pub config: Config,
    /// Set when the file comes from a newer rtify: it is used as defaults but never overwritten.
    read_only: bool,
    /// Shown to the user once after loading (backup made, file from the future, ...).
    pub warning: Option<String>,
}

impl ConfigStore {
    pub fn load(path: &Path) -> Self {
        let mut store = Self {
            path: path.to_owned(),
            config: Config::default(),
            read_only: false,
            warning: None,
        };
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return store,
            Err(e) => {
                store.read_only = true;
                store.warning = Some(format!("cannot read {}: {e}; using defaults", path.display()));
                return store;
            }
        };
        let mut table = match toml::from_str::<toml::Table>(&text) {
            Ok(t) => t,
            Err(e) => return store.recover(&e.to_string()),
        };
        let migrated = match migration::migrate(&mut table) {
            Ok(m) => m,
            Err(e @ migration::MigrationError::FromFuture(_)) => {
                store.read_only = true;
                store.warning = Some(format!("{e}; settings are used read-only"));
                if let Ok(c) = toml::Value::Table(table).try_into() {
                    store.config = c;
                }
                return store;
            }
            Err(e) => return store.recover(&e.to_string()),
        };
        match toml::Value::Table(table).try_into::<Config>() {
            Ok(c) => store.config = c,
            Err(e) => return store.recover(&e.to_string()),
        }
        if migrated && let Err(e) = store.save() {
            log::warn!("could not write migrated config: {e}");
        }
        store
    }

    fn recover(mut self, reason: &str) -> Self {
        self.warning = Some(match atomic::quarantine(&self.path) {
            Ok(backup) => format!(
                "invalid settings ({reason}); kept a backup at {} and loaded defaults",
                backup.display()
            ),
            Err(e) => {
                // Without a backup we must not overwrite the user's file.
                self.read_only = true;
                format!("invalid settings ({reason}); backup failed ({e}), running with defaults read-only")
            }
        });
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn save(&self) -> io::Result<()> {
        if self.read_only {
            return Ok(());
        }
        let text = toml::to_string_pretty(&self.config).map_err(io::Error::other)?;
        atomic::write(&self.path, text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PaletteSource;

    #[test]
    fn missing_file_gives_defaults_and_save_creates_it() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        let mut s = ConfigStore::load(&p);
        assert_eq!(s.config, Config::default());
        assert!(s.warning.is_none());
        s.config.appearance.palette_source = PaletteSource::Noctalia;
        s.save().unwrap();
        assert_eq!(ConfigStore::load(&p).config.appearance.palette_source, PaletteSource::Noctalia);
    }

    #[test]
    fn broken_file_is_backed_up_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        fs::write(&p, "this is [not toml").unwrap();
        let s = ConfigStore::load(&p);
        assert!(s.warning.as_deref().unwrap().contains("backup"));
        let backups: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".broken-"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read_to_string(backups[0].path()).unwrap(), "this is [not toml");
        s.save().unwrap();
        assert!(p.exists());
    }

    #[test]
    fn wrong_types_are_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        fs::write(&p, "version = 1\n[window]\nwidth = \"wide\"\n").unwrap();
        let s = ConfigStore::load(&p);
        assert!(s.warning.is_some());
        assert_eq!(s.config, Config::default());
    }

    #[test]
    fn future_file_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        fs::write(&p, "version = 42\n[lyrics]\nromanization = false\n").unwrap();
        let s = ConfigStore::load(&p);
        assert!(!s.config.lyrics.romanization, "known fields are still honoured");
        s.save().unwrap();
        assert!(fs::read_to_string(&p).unwrap().starts_with("version = 42"));
    }

    #[test]
    fn unversioned_file_is_migrated_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        fs::write(&p, "[window]\nstart_hidden = true\n").unwrap();
        let s = ConfigStore::load(&p);
        assert!(s.config.window.start_hidden);
        assert!(fs::read_to_string(&p).unwrap().contains("version = 1"));
    }
}
