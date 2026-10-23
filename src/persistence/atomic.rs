use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Write `data` to `path` through a temporary file in the same directory followed by a rename,
/// so a crash leaves either the old or the new file, never a truncated one.
pub fn write(path: &Path, data: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    let result = (|| {
        let mut f = File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        fs::rename(&tmp, path)?;
        // Persist the rename itself; best effort, some filesystems refuse to fsync directories.
        if let Ok(d) = File::open(dir) {
            let _ = d.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Move an unreadable file aside (`name.broken-<unix time>`) so it is never silently overwritten.
pub fn quarantine(path: &Path) -> io::Result<PathBuf> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let mut backup = path.with_file_name(format!("{name}.broken-{stamp}"));
    let mut n = 1;
    while backup.exists() {
        backup = path.with_file_name(format!("{name}.broken-{stamp}-{n}"));
        n += 1;
    }
    fs::rename(path, &backup)?;
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub/a.txt");
        write(&p, b"one").unwrap();
        write(&p, b"two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        let leftovers: Vec<_> = fs::read_dir(p.parent().unwrap()).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "temporary file must not remain");
    }

    #[test]
    fn quarantine_keeps_every_backup() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.toml");
        fs::write(&p, "x").unwrap();
        let a = quarantine(&p).unwrap();
        fs::write(&p, "y").unwrap();
        let b = quarantine(&p).unwrap();
        assert_ne!(a, b);
        assert_eq!(fs::read_to_string(a).unwrap(), "x");
        assert_eq!(fs::read_to_string(b).unwrap(), "y");
        assert!(!p.exists());
    }
}
