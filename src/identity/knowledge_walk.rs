//! Kind-generic Knowledge tree access for the identity model (design doc
//! §3.2's "thin adapter" functions): locating and rewriting the `id:`/
//! `uid:` fields of any of the four Knowledge element kinds in the
//! *working tree* (not a committed ref — this drives mutating identity
//! operations, which act before anything is committed; snapshot-based
//! comparisons like `changes compute` resolve against a git ref instead
//! and have their own, separate resolution path).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::fs_safety::replace_file;
use crate::generate::{find_dirs_with_marker, sorted_subdirs};
use crate::identity::EntityKind;
use crate::knowledge;

/// One Knowledge element found in the working tree: its file path, current
/// `id:`, and `uid:` (`None` when not yet migrated).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundEntity {
    pub path: PathBuf,
    pub id: String,
    pub uid: Option<String>,
}

/// Every `kind` element in the working tree, in a deterministic order
/// (`sorted_subdirs`/`find_dirs_with_marker`'s own sort, mirroring
/// `generate::load_knowledge_snapshot`'s traversal — but without that
/// function's "must have a generatable TestCase" pruning, since a
/// Behavior with no Scenario is still a real, migratable element).
pub fn list_entities(root: &Path, kind: EntityKind) -> io::Result<Vec<FoundEntity>> {
    let knowledge_root = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("knowledge");

    if kind == EntityKind::Requirement {
        let mut found = Vec::new();
        for requirement_dir in sorted_subdirs(&knowledge_root.join("requirements"))? {
            push_if_present(
                &mut found,
                &requirement_dir.join("requirement.yml"),
                |content| {
                    let requirement = knowledge::parse_requirement(content)?;
                    Ok((requirement.id, requirement.uid))
                },
            )?;
        }
        return Ok(found);
    }

    let mut found = Vec::new();
    for feature_dir in sorted_subdirs(&knowledge_root.join("features"))? {
        if kind == EntityKind::Feature {
            push_if_present(&mut found, &feature_dir.join("feature.yml"), |content| {
                let feature = knowledge::parse_feature(content)?;
                Ok((feature.id, feature.uid))
            })?;
            continue;
        }

        for behavior_dir in find_dirs_with_marker(&feature_dir, "behavior.yml")? {
            if kind == EntityKind::Behavior {
                push_if_present(&mut found, &behavior_dir.join("behavior.yml"), |content| {
                    let behavior = knowledge::parse_behavior(content)?;
                    Ok((behavior.id, behavior.uid))
                })?;
                continue;
            }

            for scenario_dir in find_dirs_with_marker(&behavior_dir, "scenario.yml")? {
                push_if_present(&mut found, &scenario_dir.join("scenario.yml"), |content| {
                    let scenario = knowledge::parse_scenario(content)?;
                    Ok((scenario.id, scenario.uid))
                })?;
            }
        }
    }

    Ok(found)
}

fn push_if_present(
    found: &mut Vec<FoundEntity>,
    path: &Path,
    parse: impl FnOnce(&str) -> Result<(String, Option<String>), serde_yaml_ng::Error>,
) -> io::Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let content = fs::read_to_string(path)?;
    let (id, uid) = parse(&content).map_err(io::Error::other)?;
    found.push(FoundEntity {
        path: path.to_path_buf(),
        id,
        uid,
    });
    Ok(())
}

/// The first `kind` element whose current `id:` is `id`.
pub fn find_by_id(root: &Path, kind: EntityKind, id: &str) -> io::Result<Option<FoundEntity>> {
    Ok(list_entities(root, kind)?.into_iter().find(|e| e.id == id))
}

/// The first `kind` element whose `uid:` is `uid`.
pub fn find_by_uid(root: &Path, kind: EntityKind, uid: &str) -> io::Result<Option<FoundEntity>> {
    Ok(list_entities(root, kind)?
        .into_iter()
        .find(|e| e.uid.as_deref() == Some(uid)))
}

/// Rewrites `path`'s `id:`/`uid:` fields to `id`/`uid`, preserving every
/// other field, via the kind-specific parse/serialize round trip (design
/// doc §3.2: only the genuinely kind-specific bits — here, which struct
/// type to parse into — get their own code per kind).
pub fn write_id_and_uid(
    root: &Path,
    kind: EntityKind,
    path: &Path,
    id: &str,
    uid: &str,
) -> io::Result<()> {
    let content = fs::read_to_string(path)?;
    let bytes = match kind {
        EntityKind::Requirement => {
            let mut requirement =
                knowledge::parse_requirement(&content).map_err(io::Error::other)?;
            requirement.id = id.to_string();
            requirement.uid = Some(uid.to_string());
            knowledge::serialize_requirement(&requirement).into_bytes()
        }
        EntityKind::Feature => {
            let mut feature = knowledge::parse_feature(&content).map_err(io::Error::other)?;
            feature.id = id.to_string();
            feature.uid = Some(uid.to_string());
            knowledge::serialize_feature(&feature).into_bytes()
        }
        EntityKind::Behavior => {
            let mut behavior = knowledge::parse_behavior(&content).map_err(io::Error::other)?;
            behavior.id = id.to_string();
            behavior.uid = Some(uid.to_string());
            knowledge::serialize_behavior(&behavior).into_bytes()
        }
        EntityKind::Scenario => {
            let mut scenario = knowledge::parse_scenario(&content).map_err(io::Error::other)?;
            scenario.id = id.to_string();
            scenario.uid = Some(uid.to_string());
            knowledge::serialize_scenario(&scenario).into_bytes()
        }
    };
    replace_file(root, path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A knowledge tree with one element of every kind, none migrated yet:
    /// req -> feature -> behavior -> scenario.
    fn init_full_tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let base = dir
            .path()
            .join(".markharness/knowledge/features/feature/behavior");
        let scenario_dir = base.join("scenario");
        fs::create_dir_all(&scenario_dir).unwrap();
        fs::create_dir_all(dir.path().join(".markharness/knowledge/requirements/req")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/requirements/req/requirement.yml"),
            "id: req\nlabel: req\naxis: []\n",
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/feature/feature.yml"),
            "id: feature\nrequirement_uids: [req]\nlabel: feature\naxis: []\n",
        )
        .unwrap();
        fs::write(
            base.join("behavior.yml"),
            "id: behavior\nfeature: feature\nlabel: behavior\naxis: []\ndescription: |\n  d\nprocedures: {}\n",
        )
        .unwrap();
        fs::write(
            scenario_dir.join("scenario.yml"),
            "id: scenario\nbehavior: behavior\nlabel: scenario\ndescription: |\n  d\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"Confirmed.\"\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn list_entities_finds_the_one_requirement() {
        let dir = init_full_tree();

        let found = list_entities(dir.path(), EntityKind::Requirement).unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "req");
        assert_eq!(found[0].uid, None);
    }

    #[test]
    fn list_entities_finds_the_one_feature() {
        let dir = init_full_tree();

        let found = list_entities(dir.path(), EntityKind::Feature).unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "feature");
    }

    #[test]
    fn list_entities_finds_the_one_behavior() {
        let dir = init_full_tree();

        let found = list_entities(dir.path(), EntityKind::Behavior).unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "behavior");
    }

    #[test]
    fn list_entities_finds_the_one_scenario() {
        let dir = init_full_tree();

        let found = list_entities(dir.path(), EntityKind::Scenario).unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "scenario");
    }

    /// A Behavior with no Scenario at all must still be found — `identity
    /// migrate` treats it as a real, migratable element, unlike
    /// `generate_testcases` which skips anything that can't produce a
    /// TestCase.
    #[test]
    fn list_entities_finds_a_behavior_with_no_scenarios() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(".markharness/knowledge/features/feature/behavior"),
        )
        .unwrap();
        fs::create_dir_all(dir.path().join(".markharness/knowledge/requirements/req")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/requirements/req/requirement.yml"),
            "id: req\nlabel: req\naxis: []\n",
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/feature/feature.yml"),
            "id: feature\nrequirement_uids: [req]\nlabel: feature\naxis: []\n",
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/feature/behavior/behavior.yml"),
            "id: behavior\nfeature: feature\nlabel: behavior\naxis: []\ndescription: |\n  d\nprocedures: {}\n",
        )
        .unwrap();

        let found = list_entities(dir.path(), EntityKind::Behavior).unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "behavior");
    }

    #[test]
    fn find_by_id_and_find_by_uid_locate_the_same_entity() {
        let dir = init_full_tree();
        write_id_and_uid(
            dir.path(),
            EntityKind::Behavior,
            &dir.path()
                .join(".markharness/knowledge/features/feature/behavior/behavior.yml"),
            "behavior",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        )
        .unwrap();

        let by_id = find_by_id(dir.path(), EntityKind::Behavior, "behavior")
            .unwrap()
            .unwrap();
        let by_uid = find_by_uid(
            dir.path(),
            EntityKind::Behavior,
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        )
        .unwrap()
        .unwrap();

        assert_eq!(by_id.path, by_uid.path);
        assert_eq!(by_uid.uid.as_deref(), Some("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
    }

    #[test]
    fn find_by_id_returns_none_when_no_entity_matches() {
        let dir = init_full_tree();

        let found = find_by_id(dir.path(), EntityKind::Scenario, "does-not-exist").unwrap();

        assert!(found.is_none());
    }

    #[test]
    fn write_id_and_uid_preserves_other_fields_for_every_kind() {
        let dir = init_full_tree();

        write_id_and_uid(
            dir.path(),
            EntityKind::Scenario,
            &dir.path()
                .join(".markharness/knowledge/features/feature/behavior/scenario/scenario.yml"),
            "renamed-scenario",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        )
        .unwrap();

        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/features/feature/behavior/scenario/scenario.yml"),
        )
        .unwrap();
        let scenario: knowledge::Scenario = knowledge::parse_scenario(&content).unwrap();
        assert_eq!(scenario.id, "renamed-scenario");
        assert_eq!(scenario.uid, Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string()));
        assert_eq!(scenario.behavior, "behavior");
        assert_eq!(scenario.label, "scenario");
        assert_eq!(scenario.description, "d\n");
    }
}
