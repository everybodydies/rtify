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
const STEPS: &[Step] = &[];

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
    fn rejects_future_and_garbage() {
        let mut t: toml::Table = toml::from_str("version = 999").unwrap();
        assert_eq!(migrate(&mut t), Err(MigrationError::FromFuture(999)));
        let mut t: toml::Table = toml::from_str("version = \"one\"").unwrap();
        assert_eq!(migrate(&mut t), Err(MigrationError::BadVersion));
    }
}
