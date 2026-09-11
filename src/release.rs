//! `ReleaseScope` — the list of TestCases a release chose to verify
//! (ADR 0024).
//!
//! A scope records **only** that choice. It has no timestamp, chooser,
//! approval state, result, build, or environment: those belong to whatever
//! tool actually runs and records tests, and the reason for a choice is
//! already in the Git history of this file. Being in a scope means
//! "selected", never "executed" and never "passed" (ADR 0024 §5).

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fs_safety::replace_file;
use crate::git;

/// Fixed at 1 and never bumped (ADR 0026 §7).
const SCHEMA_VERSION: u32 = 1;
const RECORD_KIND: &str = "release_scope";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseScope {
    pub schema_version: u32,
    pub record_kind: String,
    /// The release's display name. A Git tag name is the natural choice.
    pub release_id: String,
    /// The TestCases chosen for verification, by Case UID rather than
    /// display id, so renaming a Scenario cannot detach the selection
    /// (ADR 0013).
    pub case_uids: Vec<String>,
}

#[derive(Debug)]
pub enum ReleaseError {
    /// The release id is unusable as the single path component it becomes.
    UnsafeReleaseId(String),
    NotFound(String),
    Malformed {
        path: String,
        message: String,
    },
    Io(io::Error),
}

impl From<io::Error> for ReleaseError {
    fn from(e: io::Error) -> Self {
        ReleaseError::Io(e)
    }
}

impl std::fmt::Display for ReleaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReleaseError::UnsafeReleaseId(value) => write!(
                f,
                "release id \"{value}\" is not usable as a file name: use ASCII lowercase letters, digits, hyphens, and dots only, and not \".\", \"..\", or a leading dot"
            ),
            ReleaseError::NotFound(id) => {
                write!(f, "no release scope recorded for '{id}'")
            }
            ReleaseError::Malformed { path, message } => write!(f, "{path}: {message}"),
            ReleaseError::Io(e) => write!(f, "filesystem error: {e}"),
        }
    }
}

pub fn releases_dir(root: &Path) -> PathBuf {
    root.join(crate::project_root::MARKHARNESS_DIR)
        .join("releases")
}

fn releases_path_in_repo() -> String {
    format!("{}/releases", crate::project_root::MARKHARNESS_DIR)
}

/// ADR 0024 §3: the release id becomes the sole component of the scope's
/// path, so it is restricted to a shape that cannot cross a directory
/// boundary — the same rule `generate` applies to `id:` for the identical
/// reason. `v1.2.0` and other ordinary tag names pass.
fn require_path_safe(release_id: &str) -> Result<(), ReleaseError> {
    let allowed = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.';
    let rejected = release_id.is_empty()
        || release_id == "."
        || release_id == ".."
        || release_id.starts_with('.')
        || !release_id.chars().all(allowed);
    if rejected {
        return Err(ReleaseError::UnsafeReleaseId(release_id.to_string()));
    }
    Ok(())
}

fn scope_path(root: &Path, release_id: &str) -> PathBuf {
    releases_dir(root).join(format!("{release_id}.yml"))
}

/// Records (replacing any previous list) the TestCases chosen for a release.
///
/// markharness never judges the selection and never generates one: what
/// belongs in a release is a human decision (ADR 0024 §4).
pub fn set_scope(
    root: &Path,
    release_id: &str,
    case_uids: &[String],
) -> Result<ReleaseScope, ReleaseError> {
    require_path_safe(release_id)?;
    let mut case_uids: Vec<String> = case_uids.to_vec();
    case_uids.sort();
    case_uids.dedup();
    let scope = ReleaseScope {
        schema_version: SCHEMA_VERSION,
        record_kind: RECORD_KIND.to_string(),
        release_id: release_id.to_string(),
        case_uids,
    };
    let content =
        serde_yaml_ng::to_string(&scope).expect("release scope serialization is infallible");
    replace_file(root, &scope_path(root, release_id), content.as_bytes())?;
    Ok(scope)
}

fn parse(path: &str, content: &str) -> Result<ReleaseScope, ReleaseError> {
    let scope: ReleaseScope =
        serde_yaml_ng::from_str(content).map_err(|e| ReleaseError::Malformed {
            path: path.to_string(),
            message: e.to_string(),
        })?;
    if scope.record_kind != RECORD_KIND {
        return Err(ReleaseError::Malformed {
            path: path.to_string(),
            message: format!(
                "record_kind is \"{}\", expected \"{RECORD_KIND}\"",
                scope.record_kind
            ),
        });
    }
    if scope.schema_version != SCHEMA_VERSION {
        return Err(ReleaseError::Malformed {
            path: path.to_string(),
            message: format!(
                "schema_version is {}, expected {SCHEMA_VERSION}",
                scope.schema_version
            ),
        });
    }
    Ok(scope)
}

/// Reads a release's scope from the working tree.
pub fn read_scope(root: &Path, release_id: &str) -> Result<ReleaseScope, ReleaseError> {
    require_path_safe(release_id)?;
    let path = scope_path(root, release_id);
    if !path.is_file() {
        return Err(ReleaseError::NotFound(release_id.to_string()));
    }
    let content = std::fs::read_to_string(&path)?;
    parse(&path.to_string_lossy(), &content)
}

/// Reads a release's scope as of `git_ref`. Scopes live under Git precisely
/// so a past release's selection can be reproduced exactly (ADR 0024 §3).
pub fn read_scope_at(
    root: &Path,
    git_ref: &str,
    release_id: &str,
) -> Result<ReleaseScope, ReleaseError> {
    require_path_safe(release_id)?;
    let path_in_repo = format!("{}/{release_id}.yml", releases_path_in_repo());
    let entries = git::ls_tree_recursive(root, git_ref, &path_in_repo)?;
    let Some(entry) = entries
        .into_iter()
        .find(|entry| entry.kind == git::ObjectKind::Blob)
    else {
        return Err(ReleaseError::NotFound(release_id.to_string()));
    };
    let content = git::show_blob_by_sha(root, &entry.sha)?;
    parse(&path_in_repo, &content)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        dir
    }

    fn uids(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn set_scope_records_only_the_selected_case_uids() {
        let dir = project();
        set_scope(dir.path(), "v1.2.0", &uids(&["case-b", "case-a"])).unwrap();

        let content = std::fs::read_to_string(scope_path(dir.path(), "v1.2.0")).unwrap();
        assert!(content.contains("record_kind: release_scope"), "{content}");
        assert!(content.contains("schema_version: 1"), "{content}");
        // AC27: there is nowhere to put a date, a chooser, or a result.
        for absent in ["selected_at", "selected_by", "approved", "result", "build"] {
            assert!(!content.contains(absent), "{content}");
        }
    }

    /// The file's content depends on the set that was selected, not on the
    /// order the ids happened to be typed in.
    #[test]
    fn set_scope_sorts_and_deduplicates() {
        let dir = project();
        let scope = set_scope(dir.path(), "v1", &uids(&["case-b", "case-a", "case-b"])).unwrap();
        assert_eq!(scope.case_uids, uids(&["case-a", "case-b"]));
    }

    #[test]
    fn set_scope_replaces_the_previous_selection() {
        let dir = project();
        set_scope(dir.path(), "v1", &uids(&["case-a"])).unwrap();
        set_scope(dir.path(), "v1", &uids(&["case-b"])).unwrap();

        assert_eq!(
            read_scope(dir.path(), "v1").unwrap().case_uids,
            uids(&["case-b"])
        );
    }

    /// AC28: a traversal-shaped id is refused before any file is created.
    #[test]
    fn a_path_shaped_release_id_is_refused_before_anything_is_written() {
        let dir = project();
        for hostile in [
            "../../etc/passwd",
            "..",
            ".",
            ".hidden",
            "a/b",
            "a\\b",
            "C:v1",
            "",
            "V1",
        ] {
            assert!(
                matches!(
                    set_scope(dir.path(), hostile, &uids(&["case-a"])),
                    Err(ReleaseError::UnsafeReleaseId(_))
                ),
                "{hostile} must be refused"
            );
        }
        assert!(!releases_dir(dir.path()).exists());
    }

    #[test]
    fn an_ordinary_tag_name_is_accepted() {
        let dir = project();
        assert!(set_scope(dir.path(), "v1.2.0", &uids(&["case-a"])).is_ok());
        assert!(set_scope(dir.path(), "2026-08-release", &uids(&["case-a"])).is_ok());
    }

    #[test]
    fn reading_a_release_with_no_recorded_scope_reports_not_found() {
        let dir = project();
        assert!(matches!(
            read_scope(dir.path(), "v9"),
            Err(ReleaseError::NotFound(_))
        ));
    }

    #[test]
    fn a_scope_carrying_an_execution_fact_field_is_malformed() {
        let dir = project();
        set_scope(dir.path(), "v1", &uids(&["case-a"])).unwrap();
        let path = scope_path(dir.path(), "v1");
        let content = std::fs::read_to_string(&path).unwrap() + "result: pass\n";
        #[allow(clippy::disallowed_methods)]
        std::fs::write(&path, content).unwrap();

        assert!(matches!(
            read_scope(dir.path(), "v1"),
            Err(ReleaseError::Malformed { .. })
        ));
    }
}
