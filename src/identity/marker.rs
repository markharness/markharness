//! Project-level `[identity]` marker in `.markharness/config.toml`
//! (ADR 0013 「移行」節): the single authoritative flag for whether a
//! project has completed the schema version 2 cutover. Whether a project
//! is migrated is judged by this marker, never by counting how many
//! elements happen to carry a `uid` (ADR: 「移行済みかどうかはFeature数や
//! UIDの有無ではなくproject markerで判定する」).

use std::io;
use std::path::Path;

use crate::project_root::MARKER_FILE;

/// The `[identity].schema_version` value written once the schema version 2
/// cutover has completed (ADR 0013).
pub const IDENTITY_SCHEMA_VERSION: u32 = 2;

const UID_MODE: &str = "uid";

/// Reads `[identity]` from `config.toml`, if present. `None` means the
/// project has not gone through the cutover yet (pre-Phase-5 project, or a
/// project still mid-migration).
pub fn is_uid_mode(root: &Path) -> io::Result<bool> {
    let path = root.join(MARKER_FILE);
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    let parsed: toml::Table = content
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{path:?}: {e}")))?;
    let mode = parsed
        .get("identity")
        .and_then(|identity| identity.get("mode"))
        .and_then(|mode| mode.as_str());
    Ok(mode == Some(UID_MODE))
}

/// Idempotently writes `[identity]\nschema_version = 2\nmode = "uid"\n`
/// into `config.toml`, preserving any other keys/tables already there
/// (e.g. the top-level `schema_version` `init` writes, or user
/// customizations — see `project_root::MARKER_FILE`'s doc comment).
pub fn mark_uid_mode(root: &Path) -> io::Result<()> {
    let path = root.join(MARKER_FILE);
    let existing = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let mut parsed: toml::Table = existing
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{path:?}: {e}")))?;

    let mut identity = toml::Table::new();
    identity.insert(
        "schema_version".to_string(),
        toml::Value::Integer(IDENTITY_SCHEMA_VERSION as i64),
    );
    identity.insert(
        "mode".to_string(),
        toml::Value::String(UID_MODE.to_string()),
    );
    parsed.insert("identity".to_string(), toml::Value::Table(identity));

    let content = toml::to_string_pretty(&parsed)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    crate::fs_safety::replace_file(root, &path, content.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_marker(root: &Path, content: &str) {
        let path = root.join(MARKER_FILE);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn is_uid_mode_is_false_when_config_toml_is_missing() {
        let dir = tempfile::tempdir().unwrap();

        assert!(!is_uid_mode(dir.path()).unwrap());
    }

    #[test]
    fn is_uid_mode_is_false_before_the_identity_marker_is_written() {
        let dir = tempfile::tempdir().unwrap();
        init_marker(dir.path(), "schema_version = 1\n");

        assert!(!is_uid_mode(dir.path()).unwrap());
    }

    #[test]
    fn mark_uid_mode_makes_is_uid_mode_true() {
        let dir = tempfile::tempdir().unwrap();
        init_marker(dir.path(), "schema_version = 1\n");

        mark_uid_mode(dir.path()).unwrap();

        assert!(is_uid_mode(dir.path()).unwrap());
    }

    #[test]
    fn mark_uid_mode_preserves_pre_existing_top_level_keys() {
        let dir = tempfile::tempdir().unwrap();
        init_marker(dir.path(), "schema_version = 1\ncustomized = true\n");

        mark_uid_mode(dir.path()).unwrap();

        let content = std::fs::read_to_string(dir.path().join(MARKER_FILE)).unwrap();
        assert!(content.contains("customized = true"));
        assert!(content.contains("schema_version = 1"));
    }

    #[test]
    fn mark_uid_mode_writes_the_expected_schema_version_and_mode() {
        let dir = tempfile::tempdir().unwrap();
        init_marker(dir.path(), "schema_version = 1\n");

        mark_uid_mode(dir.path()).unwrap();

        let content = std::fs::read_to_string(dir.path().join(MARKER_FILE)).unwrap();
        let parsed: toml::Table = content.parse().unwrap();
        let identity = parsed["identity"].as_table().unwrap();
        assert_eq!(identity["schema_version"].as_integer(), Some(2));
        assert_eq!(identity["mode"].as_str(), Some("uid"));
    }

    #[test]
    fn mark_uid_mode_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        init_marker(dir.path(), "schema_version = 1\n");

        mark_uid_mode(dir.path()).unwrap();
        mark_uid_mode(dir.path()).unwrap();

        assert!(is_uid_mode(dir.path()).unwrap());
    }

    #[test]
    fn mark_uid_mode_creates_config_toml_when_it_does_not_exist_yet() {
        let dir = tempfile::tempdir().unwrap();

        mark_uid_mode(dir.path()).unwrap();

        assert!(is_uid_mode(dir.path()).unwrap());
    }
}
