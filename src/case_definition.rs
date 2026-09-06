//! ADR 0017 §5: "実行に使った実効ケース定義を Case UID + Case revision ごとに
//! Git 内へ不変の記録として保存し、証跡から参照する。同一定義は共有する。
//! 表示名等は revision 対象外なので、同じキーの定義をそれらの変更で上書き
//! しない。" — this module stores and reads back that frozen definition,
//! keyed by `(case_uid, case_revision)` under
//! `.markharness/case-definitions/<case_uid>/<case_revision>.yml`.
//!
//! Wiring this store into execution evidence (referencing a stored
//! definition from a recorded result) is Step 4's concern, not this
//! module's — this only provides the immutable store itself.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fs_safety::replace_file;
use crate::generate::{Phase, TestCase};

/// The frozen, effective content of a TestCase at one `(case_uid,
/// case_revision)` key. Deliberately excludes every display-only field
/// (`case_id`, label, description, source) that `case_revision` itself
/// ignores (ADR 0017 §3) — storing them here would invite a future "fix the
/// label" overwrite of what is meant to be an immutable record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaseDefinition {
    pub case_uid: String,
    pub case_revision: String,
    pub phases: Vec<Phase>,
}

fn definition_path(root: &Path, case_uid: &str, case_revision: &str) -> PathBuf {
    root.join(crate::project_root::MARKHARNESS_DIR)
        .join("case-definitions")
        .join(case_uid)
        .join(format!("{case_revision}.yml"))
}

pub fn serialize_case_definition(definition: &CaseDefinition) -> String {
    serde_yaml_ng::to_string(definition).expect("CaseDefinition serialization is infallible")
}

fn load_case_definition_at(path: &Path) -> io::Result<CaseDefinition> {
    let content = fs::read_to_string(path)?;
    serde_yaml_ng::from_str(&content).map_err(io::Error::other)
}

/// Writes `testcase`'s frozen definition under its `(case_uid,
/// case_revision)` key, or does nothing and returns `Ok(None)` when
/// `testcase.case_uid` is `None` (an un-migrated Scenario has no stable key
/// to store one under).
///
/// Idempotent and never overwrites: a key that's already stored with
/// matching content is left untouched (ADR 0017 §5's dedup — "同一定義は
/// 共有する"); a key that's already stored with *different* content is an
/// error rather than a silent overwrite, since that can only mean a
/// `case_revision` hash collision between genuinely different effective
/// content or on-disk corruption — either way, not something to paper over
/// by replacing the existing immutable record.
pub fn store_case_definition(root: &Path, testcase: &TestCase) -> io::Result<Option<PathBuf>> {
    let Some(case_uid) = &testcase.case_uid else {
        return Ok(None);
    };
    let definition = CaseDefinition {
        case_uid: case_uid.clone(),
        case_revision: testcase.case_revision.clone(),
        phases: testcase.phases.clone(),
    };
    let path = definition_path(root, case_uid, &testcase.case_revision);
    if path.is_file() {
        let existing = load_case_definition_at(&path)?;
        if existing != definition {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "refusing to overwrite the immutable case definition at {}: its stored \
                     content differs from case_uid {case_uid} case_revision {} (a case_revision \
                     collision between different content, or on-disk corruption)",
                    path.display(),
                    testcase.case_revision
                ),
            ));
        }
        return Ok(Some(path));
    }
    replace_file(
        root,
        &path,
        serialize_case_definition(&definition).as_bytes(),
    )?;
    Ok(Some(path))
}

/// Reads back the frozen definition stored at `(case_uid, case_revision)`,
/// or `None` if nothing has been stored under that key.
///
/// ADR 0017 §5: verifies the stored file is self-consistent before handing
/// it back — its internal `case_uid`/`case_revision` fields must match the
/// path key it was read from, and its `phases` must actually hash to that
/// `case_revision` (`case_revision` is defined as
/// `generate::compute_case_revision(&phases)`). A mismatch means the file at
/// this path was never produced by `store_case_definition` (hand-edited,
/// swapped in by a mis-resolved merge conflict, or on-disk corruption) and
/// is a hard error — never silently treated as "nothing stored" (`None`)
/// or handed back as if it were trustworthy.
pub fn load_case_definition(
    root: &Path,
    case_uid: &str,
    case_revision: &str,
) -> io::Result<Option<CaseDefinition>> {
    let path = definition_path(root, case_uid, case_revision);
    if !path.is_file() {
        return Ok(None);
    }
    let definition = load_case_definition_at(&path)?;
    if definition.case_uid != case_uid || definition.case_revision != case_revision {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "case definition at {} is stored under case_uid {case_uid} case_revision \
                 {case_revision} but its own fields say case_uid {} case_revision {} \
                 (mismatched file, or on-disk corruption)",
                path.display(),
                definition.case_uid,
                definition.case_revision
            ),
        ));
    }
    let actual_revision = crate::generate::compute_case_revision(&definition.phases);
    if actual_revision != case_revision {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "case definition at {} is stored under case_revision {case_revision} but its \
                 phases hash to {actual_revision} (its content was altered after being frozen, \
                 or a case_revision hash collision)",
                path.display()
            ),
        ));
    }
    Ok(Some(definition))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate::{CaseFilePaths, GeneratedFrom};

    /// The real `case_revision` for a single-phase, single-step TestCase
    /// with this `step` — matches what `generate::compile_testcases` would
    /// actually compute, so tests that go through `load_case_definition`'s
    /// self-consistency check use a `case_revision` that's genuinely the
    /// hash of the phases they store.
    fn real_case_revision(step: &str) -> String {
        crate::generate::compute_case_revision(&[Phase {
            steps: vec![step.to_string()],
            results: vec!["Confirmed.".to_string()],
        }])
    }

    fn sample_testcase(case_uid: Option<&str>, case_revision: &str, step: &str) -> TestCase {
        TestCase {
            case_id: "tc-checkout-pay-valid-card".to_string(),
            case_uid: case_uid.map(str::to_string),
            case_revision: case_revision.to_string(),
            case_files: CaseFilePaths::default(),
            generated_from: GeneratedFrom {
                requirement_ids: vec!["req-shop".to_string()],
                requirement_uids: None,
                feature: "checkout".to_string(),
                feature_uid: None,
                behavior: "pay".to_string(),
                scenario: "valid-card".to_string(),
            },
            phases: vec![Phase {
                steps: vec![step.to_string()],
                results: vec!["Confirmed.".to_string()],
            }],
            axis: vec!["ui".to_string()],
        }
    }

    #[test]
    fn stores_and_loads_back_the_same_definition() {
        let dir = tempfile::tempdir().unwrap();
        let revision = real_case_revision("Do it.");
        let testcase = sample_testcase(Some("case-uid-1"), &revision, "Do it.");

        let path = store_case_definition(dir.path(), &testcase)
            .unwrap()
            .expect("case_uid is present, so a path must be returned");
        assert!(path.is_file());

        let loaded = load_case_definition(dir.path(), "case-uid-1", &revision)
            .unwrap()
            .expect("just-stored definition must load back");
        assert_eq!(loaded.case_uid, "case-uid-1");
        assert_eq!(loaded.case_revision, revision);
        assert_eq!(loaded.phases, testcase.phases);
    }

    #[test]
    fn does_nothing_and_returns_none_when_case_uid_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        let testcase = sample_testcase(None, "rev-1", "Do it.");

        let result = store_case_definition(dir.path(), &testcase).unwrap();

        assert_eq!(result, None);
        assert!(
            !dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("case-definitions")
                .exists()
        );
    }

    #[test]
    fn load_case_definition_returns_none_for_an_unknown_key() {
        let dir = tempfile::tempdir().unwrap();

        let result = load_case_definition(dir.path(), "case-uid-1", "rev-1").unwrap();

        assert_eq!(result, None);
    }

    /// ADR 0017 §5: a stored definition whose own `case_uid` field disagrees
    /// with the path it's stored under (e.g. a merge-conflict resolution
    /// that copied the wrong file into place) must never be trusted as the
    /// definition for that key.
    #[test]
    fn load_case_definition_rejects_a_stored_file_whose_internal_case_uid_disagrees_with_the_path()
    {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("case-definitions")
            .join("case-uid-1")
            .join("rev-1.yml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "case_uid: case-uid-WRONG\ncase_revision: rev-1\nphases: []\n",
        )
        .unwrap();

        let result = load_case_definition(dir.path(), "case-uid-1", "rev-1");

        assert!(
            result.is_err(),
            "expected an error for a case_uid mismatch, got: {result:?}"
        );
    }

    /// ADR 0017 §5: `case_revision` is defined as a hash of `phases`
    /// (`generate::compute_case_revision`) — a stored file whose `phases`
    /// don't actually hash to the `case_revision` it's keyed and labeled
    /// under is corrupt (hand-edited, or a hash collision) and must never be
    /// treated as a valid frozen definition.
    #[test]
    fn load_case_definition_rejects_a_stored_file_whose_phases_do_not_hash_to_its_case_revision() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("case-definitions")
            .join("case-uid-1")
            .join("rev-1.yml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // `rev-1` is not the real hash of these phases, so this file cannot
        // have come from `store_case_definition`.
        fs::write(
            &path,
            "case_uid: case-uid-1\ncase_revision: rev-1\nphases:\n  - steps: [\"Do it.\"]\n    results: [\"Confirmed.\"]\n",
        )
        .unwrap();

        let result = load_case_definition(dir.path(), "case-uid-1", "rev-1");

        assert!(
            result.is_err(),
            "expected an error for a case_revision/phases mismatch, got: {result:?}"
        );
    }

    /// ADR 0017 §5: storing the same `(case_uid, case_revision)` key twice
    /// with matching content is a no-op, not an error — this is how
    /// "同一定義は共有する" plays out when the same Scenario is generated
    /// again unchanged.
    #[test]
    fn storing_the_same_key_twice_with_matching_content_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let testcase = sample_testcase(Some("case-uid-1"), &real_case_revision("Do it."), "Do it.");

        store_case_definition(dir.path(), &testcase).unwrap();
        let second = store_case_definition(dir.path(), &testcase);

        assert!(second.is_ok());
    }

    /// ADR 0017 §5: "同じキーの定義をそれらの変更で上書きしない" — a display-
    /// only field (`case_id` here) differing between two calls at the same
    /// `(case_uid, case_revision)` key must never silently overwrite the
    /// already-stored definition.
    #[test]
    fn refuses_to_overwrite_an_existing_key_with_different_content() {
        let dir = tempfile::tempdir().unwrap();
        let first = sample_testcase(Some("case-uid-1"), "rev-1", "Do it.");
        store_case_definition(dir.path(), &first).unwrap();

        let mut second = sample_testcase(Some("case-uid-1"), "rev-1", "Do it differently.");
        second.case_revision = "rev-1".to_string(); // force a hash-collision-like scenario

        let result = store_case_definition(dir.path(), &second);

        assert!(
            result.is_err(),
            "expected an error for mismatched content at an existing key, got: {result:?}"
        );
        // Reads the file directly rather than through `load_case_definition`:
        // this test's "rev-1" is a deliberately fabricated, non-hash key (to
        // simulate a collision against `store_case_definition`'s own guard),
        // so it would fail `load_case_definition`'s separate self-consistency
        // check for an unrelated reason.
        let reloaded =
            load_case_definition_at(&definition_path(dir.path(), "case-uid-1", "rev-1")).unwrap();
        assert_eq!(
            reloaded.phases, first.phases,
            "the original definition must be left untouched"
        );
    }

    #[test]
    fn different_case_revisions_under_the_same_case_uid_are_stored_separately() {
        let dir = tempfile::tempdir().unwrap();
        let revision1 = real_case_revision("Do it.");
        let revision2 = real_case_revision("Do it differently.");
        let v1 = sample_testcase(Some("case-uid-1"), &revision1, "Do it.");
        let v2 = sample_testcase(Some("case-uid-1"), &revision2, "Do it differently.");

        store_case_definition(dir.path(), &v1).unwrap();
        store_case_definition(dir.path(), &v2).unwrap();

        let loaded_v1 = load_case_definition(dir.path(), "case-uid-1", &revision1)
            .unwrap()
            .unwrap();
        let loaded_v2 = load_case_definition(dir.path(), "case-uid-1", &revision2)
            .unwrap()
            .unwrap();
        assert_ne!(loaded_v1.phases, loaded_v2.phases);
    }
}
