//! DM Mono, the typeface DESIGN.md asks for, ships inside the binary so the look does not depend
//! on what the system has installed. The files are written to the cache directory once and
//! handed to fontconfig before GTK builds its font map.

use std::ffi::{CString, c_char, c_int, c_void};
use std::path::Path;

const FONTS: &[(&str, &[u8])] = &[
    ("DMMono-Regular.ttf", include_bytes!("../../data/fonts/DMMono-Regular.ttf")),
    ("DMMono-Medium.ttf", include_bytes!("../../data/fonts/DMMono-Medium.ttf")),
];

#[link(name = "fontconfig")]
unsafe extern "C" {
    fn FcConfigAppFontAddFile(config: *mut c_void, file: *const c_char) -> c_int;
}

/// Make the bundled fonts available to this process. Call before the first widget is created.
pub fn register_bundled(dir: &Path) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        log::warn!("fonts: {}: {e}", dir.display());
        return;
    }
    for (name, bytes) in FONTS {
        let path = dir.join(name);
        let current = std::fs::metadata(&path).map(|m| m.len() == bytes.len() as u64).unwrap_or(false);
        if !current && let Err(e) = std::fs::write(&path, bytes) {
            log::warn!("fonts: {}: {e}", path.display());
            continue;
        }
        let Ok(c) = CString::new(path.to_string_lossy().as_bytes()) else {
            continue;
        };
        // SAFETY: a null config means fontconfig's current configuration; `c` outlives the call.
        if unsafe { FcConfigAppFontAddFile(std::ptr::null_mut(), c.as_ptr()) } == 0 {
            log::warn!("fonts: fontconfig rejected {}", path.display());
        }
    }
}
