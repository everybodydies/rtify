//! Schema migrations for `config.toml`. Each step rewrites the raw TOML table from version N to
//! N + 1, so old files keep loading after field renames.

use crate::config::CONFIG_VERSION;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum MigrationError {
    #[error("config version {0} is newer than this build understands ({CONFIG_VERSION})")]
    FromFuture(u32),
    #[error("config `version` is not a positive integer")]
    BadVersion,
}

type Step = fn(&mut toml::Table);

/// `STEPS[i]` migrates version `i + 1` to `i + 2`. Version 1 is the first schema.
const STEPS: &[Step] = &[v1_to_v2, v2_to_v3];

/// The system24 OLED look moved to the mint accent and a smaller base size. Values that were
/// still the old defaults follow; anything the user picked stays.
fn v1_to_v2(table: &mut toml::Table) {
    let Some(toml::Value::Table(a)) = table.get_mut("appearance") else {
        return;
    };
    if a.get("fixed_accent")
        .and_then(|v| v.as_str())
        .is_some_and(|v| v.eq_ignore_ascii_case("#b589d6"))
    {
        a.insert("fixed_accent".into(), crate::theme::system24::DEFAULT_ACCENT.into());
    }
    if a.get("font_size").and_then(|v| v.as_integer()) == Some(11) {
        a.insert("font_size".into(), toml::Value::Integer(10));
    }
}

/// Volume and mute got default global keys. Files store the whole bindings table, so the new
/// ones are added where the file has no entry for them (an emptied entry stays empty).
fn v2_to_v3(table: &mut toml::Table) {
    let Some(toml::Value::Table(s)) = table.get_mut("shortcuts") else {
        return;
    };
    let Some(toml::Value::Table(b)) = s.get_mut("bindings") else {
        return;
    };
    for (action, trigger) in crate::config::Shortcuts::ADDED_BINDINGS {
        b.entry(action).or_insert_with(|| trigger.into());
    }
}

/// Returns whether anything changed (the caller then rewrites the file).
pub fn migrate(table: &mut toml::Table) -> Result<bool, MigrationError> {
    let version = match table.get("version") {
        // Files written before versioning existed are treated as version 1.
        None => 1,
        Some(toml::Value::Integer(v)) if *v >= 1 => u32::try_from(*v).map_err(|_| MigrationError::BadVersion)?,
        Some(_) => return Err(MigrationError::BadVersion),
    };
    if version > CONFIG_VERSION {
        return Err(MigrationError::FromFuture(version));
    }
    let mut changed = !table.contains_key("version");
    for step in &STEPS[(version - 1) as usize..] {
        step(table);
        changed = true;
    }
    table.insert("version".into(), toml::Value::Integer(CONFIG_VERSION.into()));
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unversioned_becomes_current() {
        let mut t: toml::Table = toml::from_str("[window]\nstart_hidden = true").unwrap();
        assert_eq!(migrate(&mut t), Ok(true));
        assert_eq!(t["version"].as_integer(), Some(CONFIG_VERSION as i64));
    }

    #[test]
    fn current_is_untouched() {
        let mut t: toml::Table = toml::from_str(&format!("version = {CONFIG_VERSION}")).unwrap();
        assert_eq!(migrate(&mut t), Ok(false));
    }

    #[test]
    fn v1_defaults_follow_the_new_look() {
        let mut t: toml::Table = toml::from_str("version = 1\n[appearance]\nfixed_accent = \"#B589D6\"\nfont_size = 11\n").unwrap();
        assert_eq!(migrate(&mut t), Ok(true));
        assert_eq!(t["appearance"]["fixed_accent"].as_str(), Some("#9bd4c9"));
        assert_eq!(t["appearance"]["font_size"].as_integer(), Some(10));
        let mut t: toml::Table = toml::from_str("version = 1\n[appearance]\nfixed_accent = \"#ff8800\"\nfont_size = 13\n").unwrap();
        migrate(&mut t).unwrap();
        assert_eq!(t["appearance"]["fixed_accent"].as_str(), Some("#ff8800"));
        assert_eq!(t["appearance"]["font_size"].as_integer(), Some(13));
    }

    #[test]
    fn v2_gets_the_volume_keys() {
        let mut t: toml::Table = toml::from_str("version = 2\n[shortcuts.bindings]\nnext = \"XF86AudioNext\"\nmute = \"\"\n").unwrap();
        migrate(&mut t).unwrap();
        let b = &t["shortcuts"]["bindings"];
        assert_eq!(b["volume-up"].as_str(), Some("XF86AudioRaiseVolume"));
        assert_eq!(b["mute"].as_str(), Some(""));
        assert_eq!(b["next"].as_str(), Some("XF86AudioNext"));
    }

    #[test]
    fn rejects_future_and_garbage() {
        let mut t: toml::Table = toml::from_str("version = 999").unwrap();
        assert_eq!(migrate(&mut t), Err(MigrationError::FromFuture(999)));
        let mut t: toml::Table = toml::from_str("version = \"one\"").unwrap();
        assert_eq!(migrate(&mut t), Err(MigrationError::BadVersion));
    }
}
