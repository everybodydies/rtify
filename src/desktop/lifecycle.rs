//! Launch at login through an XDG autostart entry. (A systemd user unit would also work; we use
//! one mechanism only so rtify is never started twice.)

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::paths;

pub fn autostart_entry(exec: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=rtify\nComment=Spotify client (background)\n\
         Exec={} --background\nIcon={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n\
         X-GNOME-Autostart-Delay=3\nX-KDE-autostart-after=panel\n",
        exec.display(),
        paths::APP_ID
    )
}

fn executable() -> PathBuf {
    std::env::current_exe()
        .ok()
        .filter(|p| !p.to_string_lossy().contains("/target/"))
        .unwrap_or_else(|| PathBuf::from("rtify"))
}

pub fn set_autostart(enabled: bool) -> io::Result<()> {
    set_autostart_at(&paths::autostart_file(), &executable(), enabled)
}

pub fn set_autostart_at(file: &Path, exec: &Path, enabled: bool) -> io::Result<()> {
    if enabled {
        crate::persistence::atomic::write(file, autostart_entry(exec).as_bytes())
    } else {
        match fs::remove_file(file) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

pub fn autostart_enabled() -> bool {
    paths::autostart_file().exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles_entry() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("autostart/rtify.desktop");
        set_autostart_at(&f, Path::new("/usr/bin/rtify"), true).unwrap();
        let text = fs::read_to_string(&f).unwrap();
        assert!(text.contains("Exec=/usr/bin/rtify --background"));
        set_autostart_at(&f, Path::new("/usr/bin/rtify"), false).unwrap();
        assert!(!f.exists());
        set_autostart_at(&f, Path::new("/usr/bin/rtify"), false).unwrap();
    }
}
