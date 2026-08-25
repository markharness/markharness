//! Knowledge スキーマバージョン(issue #29)。`.markharness/config.toml` の
//! `[knowledge].schema_version` を比較対象refごとに解決し、`changes compute`
//! /`backfill run` が異なるスキーマバージョン間の生の tree SHA 比較で誤った
//! ChangeEvent を生成しないよう fail closed で停止できるようにする。
//!
//! トップレベルの `schema_version`(config.toml自体の形式)や
//! `[identity].schema_version`(ADR 0013)とは責務を分離した、Knowledge
//! 専用のバージョンである。

use std::io;
use std::path::Path;

use crate::git;
use crate::project_root::MARKER_FILE;

/// A Knowledge schema version resolved from a specific Git ref's
/// `config.toml`, plus whether it was inferred rather than recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedSchemaVersion {
    pub version: u32,
    /// `true` when `ref`'s `config.toml` has no `[knowledge].schema_version`
    /// (predates this feature) and `version` was assumed to be legacy
    /// schema version 1.
    pub is_legacy: bool,
}

/// The legacy Knowledge schema version assumed for a ref whose
/// `config.toml` predates `[knowledge].schema_version` (issue #29 §6).
const LEGACY_KNOWLEDGE_SCHEMA_VERSION: u32 = 1;

/// The highest Knowledge schema version this build of the CLI understands.
/// A ref reporting a version above this is a version from a newer CLI
/// (issue #29 §5: "CLIが知らない未来バージョン") and must not be guessed at.
pub const CURRENT_KNOWLEDGE_SCHEMA_VERSION: u32 = 1;

/// The lowest Knowledge schema version that was ever actually assigned.
/// `0` has never been a real schema — it must be rejected the same way an
/// unknown future version is, not treated as "known" merely because it's
/// `<= CURRENT_KNOWLEDGE_SCHEMA_VERSION`.
const MIN_KNOWN_KNOWLEDGE_SCHEMA_VERSION: u32 = 1;

/// Checks whether `from` and `to` can be compared by the existing raw
/// tree-SHA diff (issue #29 §4–5): both must be the same, known version.
/// Fails closed — `io::ErrorKind::Unsupported` — when they differ (no
/// cross-schema converter exists yet) or either is newer than this CLI
/// build knows about, so callers never generate a `ChangeEvent` from an
/// unverified comparison.
pub fn ensure_compatible(
    from: &ResolvedSchemaVersion,
    to: &ResolvedSchemaVersion,
) -> io::Result<()> {
    let is_known = |version: u32| {
        (MIN_KNOWN_KNOWLEDGE_SCHEMA_VERSION..=CURRENT_KNOWLEDGE_SCHEMA_VERSION).contains(&version)
    };
    if !is_known(from.version) || !is_known(to.version) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "Knowledge schema version is not supported: from uses schema {}, to uses schema {}, this CLI knows schema {}..={}. Update markharness, or verify the recorded version, to compare these refs.",
                from.version,
                to.version,
                MIN_KNOWN_KNOWLEDGE_SCHEMA_VERSION,
                CURRENT_KNOWLEDGE_SCHEMA_VERSION
            ),
        ));
    }
    if from.version != to.version {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "Knowledge schema versions differ: from uses schema {}, to uses schema {}. This version of markharness cannot compare these schema versions safely. No ChangeEvent was generated.",
                from.version, to.version
            ),
        ));
    }
    Ok(())
}

/// Resolves the Knowledge schema version recorded in `git_ref`'s own
/// `.markharness/config.toml` (the authoritative copy — issue #29 §2, not
/// `milestone.yml`'s audit copy and not the running CLI's version). A ref
/// with no `config.toml`, or a `config.toml` with no `[knowledge]` table or
/// `schema_version` field, is treated as legacy schema version 1.
pub fn resolve(root: &Path, git_ref: &str) -> io::Result<ResolvedSchemaVersion> {
    let Some(blob_sha) = git::tree_sha(root, git_ref, MARKER_FILE)? else {
        return Ok(ResolvedSchemaVersion {
            version: LEGACY_KNOWLEDGE_SCHEMA_VERSION,
            is_legacy: true,
        });
    };
    let content = git::show_blob_by_sha(root, &blob_sha)?;
    let parsed: toml::Table = content
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{git_ref}: {e}")))?;

    // `knowledge` missing entirely is the legacy case; `knowledge` present
    // but not a table (e.g. `knowledge = "oops"`, or a mistyped
    // `[[knowledge]]` array of tables) is malformed metadata that must not
    // collapse into the same "absent" fallback — `Value::get` returns
    // `None` for a non-table receiver too, which would otherwise make the
    // two indistinguishable.
    let knowledge_table = match parsed.get("knowledge") {
        None => {
            return Ok(ResolvedSchemaVersion {
                version: LEGACY_KNOWLEDGE_SCHEMA_VERSION,
                is_legacy: true,
            });
        }
        Some(value) => value.as_table().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{git_ref}: [knowledge] must be a table"),
            )
        })?,
    };
    let Some(version_value) = knowledge_table.get("schema_version") else {
        return Ok(ResolvedSchemaVersion {
            version: LEGACY_KNOWLEDGE_SCHEMA_VERSION,
            is_legacy: true,
        });
    };

    // A *present but malformed* value is a hard error, not a silent
    // downgrade to legacy — `as_integer()` returning `None` for a non-table
    // absence and for "the value isn't even an integer" must not collapse
    // into the same fallback, and a value outside `u32`'s range must not be
    // truncated (Codex review: `as u32` would wrap a value like
    // `4294967297` down to the supported version `1`, silently bypassing
    // the fail-closed gate `ensure_compatible` exists to enforce).
    let version_int = version_value.as_integer().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{git_ref}: [knowledge].schema_version must be an integer"),
        )
    })?;
    let version: u32 = version_int.try_into().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{git_ref}: [knowledge].schema_version {version_int} is out of range (must fit in u32)"
            ),
        )
    })?;
    Ok(ResolvedSchemaVersion {
        version,
        is_legacy: false,
    })
}

/// The warning text for a ref whose Knowledge schema version was inferred
/// as legacy (issue #29 §6), or `None` when the version was actually
/// recorded. `git_ref` is included so a human can tell which side of the
/// comparison needs the warning addressed.
pub fn legacy_warning(git_ref: &str, resolved: &ResolvedSchemaVersion) -> Option<String> {
    resolved.is_legacy.then(|| {
        format!(
            "Knowledge schema version is not recorded at ref {git_ref}; assuming legacy schema version {}.",
            resolved.version
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_git(root: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        dir
    }

    fn commit_all(root: &Path, message: &str) {
        run_git(root, &["add", "-A"]);
        run_git(root, &["commit", "-q", "-m", message]);
    }

    #[test]
    fn resolve_reads_the_recorded_knowledge_schema_version() {
        let dir = init_repo();
        std::fs::create_dir_all(dir.path().join(".markharness")).unwrap();
        std::fs::write(
            dir.path().join(MARKER_FILE),
            "schema_version = 1\n\n[knowledge]\nschema_version = 1\n",
        )
        .unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]);

        let resolved = resolve(dir.path(), "m1").unwrap();

        assert_eq!(resolved.version, 1);
        assert!(!resolved.is_legacy);
    }

    #[test]
    fn resolve_falls_back_to_legacy_v1_when_config_toml_is_missing_at_the_ref() {
        let dir = init_repo();
        std::fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]);

        let resolved = resolve(dir.path(), "m1").unwrap();

        assert_eq!(resolved.version, 1);
        assert!(resolved.is_legacy);
    }

    #[test]
    fn resolve_falls_back_to_legacy_v1_when_knowledge_table_is_absent() {
        let dir = init_repo();
        std::fs::create_dir_all(dir.path().join(".markharness")).unwrap();
        std::fs::write(dir.path().join(MARKER_FILE), "schema_version = 1\n").unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]);

        let resolved = resolve(dir.path(), "m1").unwrap();

        assert_eq!(resolved.version, 1);
        assert!(resolved.is_legacy);
    }

    #[test]
    fn resolve_reads_a_non_default_knowledge_schema_version() {
        let dir = init_repo();
        std::fs::create_dir_all(dir.path().join(".markharness")).unwrap();
        std::fs::write(
            dir.path().join(MARKER_FILE),
            "schema_version = 1\n\n[knowledge]\nschema_version = 2\n",
        )
        .unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]);

        let resolved = resolve(dir.path(), "m1").unwrap();

        assert_eq!(resolved.version, 2);
        assert!(!resolved.is_legacy);
    }

    /// Codex review finding: `[knowledge]` present but not a table (e.g. a
    /// plain `knowledge = "oops"` key, or a mistyped `[[knowledge]]` array
    /// of tables) must not be treated the same as `[knowledge]` being
    /// absent entirely — that would silently fall back to legacy schema
    /// version 1 even though the ref plainly attempted to record something,
    /// bypassing the fail-closed gate on real but malformed metadata.
    #[test]
    fn resolve_errors_when_knowledge_key_is_present_but_not_a_table() {
        let dir = init_repo();
        std::fs::create_dir_all(dir.path().join(".markharness")).unwrap();
        std::fs::write(
            dir.path().join(MARKER_FILE),
            "schema_version = 1\nknowledge = \"oops\"\n",
        )
        .unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]);

        let err = resolve(dir.path(), "m1").unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    /// Codex review finding: a huge recorded value must not silently wrap
    /// to a small, apparently-known version via `as u32` truncation — that
    /// would let a schema-only migration bypass the fail-closed gate.
    #[test]
    fn resolve_errors_when_recorded_schema_version_overflows_u32() {
        let dir = init_repo();
        std::fs::create_dir_all(dir.path().join(".markharness")).unwrap();
        std::fs::write(
            dir.path().join(MARKER_FILE),
            "schema_version = 1\n\n[knowledge]\nschema_version = 4294967297\n",
        )
        .unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]);

        let err = resolve(dir.path(), "m1").unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn resolve_errors_when_recorded_schema_version_is_negative() {
        let dir = init_repo();
        std::fs::create_dir_all(dir.path().join(".markharness")).unwrap();
        std::fs::write(
            dir.path().join(MARKER_FILE),
            "schema_version = 1\n\n[knowledge]\nschema_version = -1\n",
        )
        .unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]);

        let err = resolve(dir.path(), "m1").unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn resolve_errors_when_recorded_schema_version_is_not_an_integer() {
        let dir = init_repo();
        std::fs::create_dir_all(dir.path().join(".markharness")).unwrap();
        std::fs::write(
            dir.path().join(MARKER_FILE),
            "schema_version = 1\n\n[knowledge]\nschema_version = \"one\"\n",
        )
        .unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]);

        let err = resolve(dir.path(), "m1").unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    fn known(version: u32) -> ResolvedSchemaVersion {
        ResolvedSchemaVersion {
            version,
            is_legacy: false,
        }
    }

    #[test]
    fn ensure_compatible_allows_the_same_known_version_on_both_sides() {
        assert!(ensure_compatible(&known(1), &known(1)).is_ok());
    }

    #[test]
    fn ensure_compatible_rejects_differing_known_versions() {
        let err = ensure_compatible(&known(1), &known(2)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Unsupported);
    }

    /// Codex review finding: `0` was never assigned to any real Knowledge
    /// schema (versions start at 1) — it must not slip through as "known"
    /// just because it's `<= CURRENT_KNOWLEDGE_SCHEMA_VERSION`.
    #[test]
    fn ensure_compatible_rejects_version_zero_even_on_both_sides() {
        let err = ensure_compatible(&known(0), &known(0)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Unsupported);
    }

    #[test]
    fn ensure_compatible_rejects_a_version_newer_than_this_cli_knows() {
        let err =
            ensure_compatible(&known(CURRENT_KNOWLEDGE_SCHEMA_VERSION + 1), &known(1)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Unsupported);
    }

    #[test]
    fn legacy_warning_is_none_for_a_recorded_version() {
        assert_eq!(legacy_warning("m1", &known(1)), None);
    }

    #[test]
    fn legacy_warning_names_the_ref_and_assumed_version_for_a_legacy_fallback() {
        let resolved = ResolvedSchemaVersion {
            version: 1,
            is_legacy: true,
        };

        let warning = legacy_warning("m1", &resolved).unwrap();

        assert!(warning.contains("m1"));
        assert!(warning.contains("legacy schema version 1"));
    }
}
