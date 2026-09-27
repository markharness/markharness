//! `markharness knowledge remove` (ADR 0034): physical deletion of a
//! Requirement, Feature, Behavior, or Scenario, cascading to every child
//! whose mandatory parent reference (`Behavior.feature`, `Scenario.behavior`)
//! would otherwise dangle, and detaching every optional back-reference
//! (`Feature.requirement_uids`, `Scenario.requirement_uids`) that named a
//! deleted Requirement's UID. CLI-agnostic, mirroring
//! `knowledge_reconcile::execute`'s module shape.

use std::io;
use std::path::Path;

use crate::identity::{EntityKind, feature_ops, knowledge_walk, recovery};
use crate::knowledge;

/// What to delete and, for Behavior/Scenario, how to disambiguate a slug
/// that exists under more than one parent (ADR 0034 §2).
#[derive(Debug, Clone, Copy)]
pub struct RemoveTarget<'a> {
    pub kind: EntityKind,
    pub key: &'a str,
    pub feature: Option<&'a str>,
    pub behavior: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedElement {
    pub kind: EntityKind,
    pub id: String,
    pub uid: Option<String>,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetachedReference {
    pub kind: EntityKind,
    pub id: String,
    pub path: String,
}

/// `knowledge remove`'s result: every element the cascade deleted, and
/// every element whose optional back-reference was rewritten to omit a
/// deleted Requirement's UID.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemoveOutcome {
    pub deleted: Vec<DeletedElement>,
    pub detached: Vec<DetachedReference>,
}

#[derive(Debug)]
pub enum RemoveError {
    /// No element of the requested kind matches `key` (as slug or uid),
    /// within `feature`/`behavior` scoping when given.
    NotFound,
    /// `key` matched more than one element (same slug under different
    /// parents) and no `feature`/`behavior` scoping or uid was specific
    /// enough to pick exactly one. Carries every match's root-relative path
    /// so the caller can disambiguate (ADR 0034 §2).
    Ambiguous(Vec<String>),
    OperationInProgress,
    Io(io::Error),
}

impl From<io::Error> for RemoveError {
    fn from(e: io::Error) -> Self {
        RemoveError::Io(e)
    }
}

fn relative_path_string(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The one element `resolve_target` settled on.
struct Resolved {
    entity: knowledge_walk::FoundEntity,
}

/// Finds every `kind` element whose slug is `key`, scoped by
/// `target.feature`/`target.behavior` when given, and returns it only when
/// exactly one match remains — otherwise [`RemoveError::NotFound`] or
/// [`RemoveError::Ambiguous`].
fn resolve_target(root: &Path, target: &RemoveTarget) -> Result<Resolved, RemoveError> {
    match target.kind {
        EntityKind::Requirement | EntityKind::Feature => {
            // Project-unique id (ADR 0034 §2): no ambiguity is possible, and
            // a uid also resolves unambiguously since at most one element
            // can ever carry it.
            let by_id = knowledge_walk::find_by_id(root, target.kind, target.key)?;
            let found = match by_id {
                Some(found) => found,
                None => knowledge_walk::find_by_uid(root, target.kind, target.key)?
                    .ok_or(RemoveError::NotFound)?,
            };
            Ok(Resolved { entity: found })
        }
        EntityKind::Behavior => resolve_scoped(root, target, EntityKind::Behavior),
        EntityKind::Scenario => resolve_scoped(root, target, EntityKind::Scenario),
    }
}

/// Shared resolution for Behavior/Scenario (ADR 0034 §2): a slug may exist
/// under more than one parent, so every candidate is inspected before
/// deciding whether the result is unambiguous.
fn resolve_scoped(
    root: &Path,
    target: &RemoveTarget,
    kind: EntityKind,
) -> Result<Resolved, RemoveError> {
    let all = knowledge_walk::list_entities(root, kind)?;

    // A uid always resolves unambiguously when it matches, regardless of
    // slug collisions elsewhere in the tree.
    if let Some(found) = all.iter().find(|e| e.uid.as_deref() == Some(target.key)) {
        return Ok(Resolved {
            entity: found.clone(),
        });
    }

    let mut candidates: Vec<&knowledge_walk::FoundEntity> =
        all.iter().filter(|e| e.id == target.key).collect();

    if let Some(feature) = target.feature {
        candidates.retain(|e| {
            parent_ids(root, kind, &e.path).ok() == Some(scope(feature, target.behavior))
        });
    }

    match candidates.len() {
        0 => Err(RemoveError::NotFound),
        1 => Ok(Resolved {
            entity: candidates[0].clone(),
        }),
        _ => Err(RemoveError::Ambiguous(
            candidates
                .iter()
                .map(|e| relative_path_string(root, &e.path))
                .collect(),
        )),
    }
}

fn scope(feature: &str, behavior: Option<&str>) -> (Option<String>, Option<String>) {
    (Some(feature.to_string()), behavior.map(str::to_string))
}

/// Reads `path`'s parent Feature id (and, for a Scenario, its parent
/// Behavior id) straight from the Knowledge tree's own directory layout
/// (`features/<feature_id>/<behavior_id>/<scenario_id>/`), rather than from
/// the file's own content — a Behavior/Scenario's `feature`/`behavior`
/// field already names its parent by id, but re-deriving it from the path
/// keeps this in sync even for a not-yet-migrated tree with no uid to
/// cross-check against.
fn parent_ids(
    root: &Path,
    kind: EntityKind,
    path: &Path,
) -> io::Result<(Option<String>, Option<String>)> {
    let features_root = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("knowledge")
        .join("features");
    let relative = path.strip_prefix(&features_root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} is not under {}",
                path.display(),
                features_root.display()
            ),
        )
    })?;
    let mut components = relative.components();
    let feature_id = components
        .next()
        .map(|c| c.as_os_str().to_string_lossy().into_owned());
    let behavior_id = match kind {
        EntityKind::Scenario => components
            .next()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
        _ => None,
    };
    Ok((feature_id, behavior_id))
}

/// Every Behavior/Scenario file that must be physically deleted alongside
/// `resolved` because it structurally depends on it (ADR 0034 §3):
/// Requirement/Scenario contribute nothing here (a Requirement's
/// dependents keep existing and only lose a back-reference; a Scenario has
/// no Knowledge-element children), a Feature's cascade is every Behavior
/// nested under its own directory plus their Scenarios, and a Behavior's
/// cascade is every Scenario nested under its own directory.
///
/// Scoped by physical directory containment (`find_dirs_with_marker`
/// starting from `resolved`'s own directory), not by comparing
/// `Behavior.feature`/`Scenario.behavior` id text against the parent's id:
/// a Behavior/Scenario id is only unique *within* its parent (ADR 0034 §2),
/// so a same-named Behavior/Scenario nested under a *different* Feature/
/// Behavior must never match here. Matching by containment instead of by id
/// text makes that cross-parent collision structurally impossible rather
/// than relying on every caller remembering to scope the comparison.
fn compute_cascade(
    root: &Path,
    kind: EntityKind,
    resolved: &Resolved,
) -> io::Result<Vec<DeletedElement>> {
    let mut deleted = vec![DeletedElement {
        kind,
        id: resolved.entity.id.clone(),
        uid: resolved.entity.uid.clone(),
        path: relative_path_string(root, &resolved.entity.path),
    }];

    match kind {
        EntityKind::Requirement | EntityKind::Scenario => {}
        EntityKind::Feature => {
            let feature_dir = resolved
                .entity
                .path
                .parent()
                .expect("feature.yml always has a parent directory");
            for behavior_dir in crate::generate::find_dirs_with_marker(feature_dir, "behavior.yml")?
            {
                let behavior_path = behavior_dir.join("behavior.yml");
                let content = std::fs::read_to_string(&behavior_path)?;
                let parsed = knowledge::parse_behavior(&content).map_err(io::Error::other)?;
                deleted.push(DeletedElement {
                    kind: EntityKind::Behavior,
                    id: parsed.id,
                    uid: parsed.uid,
                    path: relative_path_string(root, &behavior_path),
                });
                deleted.extend(scenarios_of(root, &behavior_dir)?);
            }
        }
        EntityKind::Behavior => {
            let behavior_dir = resolved
                .entity
                .path
                .parent()
                .expect("behavior.yml always has a parent directory");
            deleted.extend(scenarios_of(root, behavior_dir)?);
        }
    }
    Ok(deleted)
}

/// Every Scenario physically nested under `behavior_dir` (ADR 0034 §3). See
/// [`compute_cascade`]'s doc comment for why this scopes by directory
/// containment rather than by comparing `Scenario.behavior` id text.
fn scenarios_of(root: &Path, behavior_dir: &Path) -> io::Result<Vec<DeletedElement>> {
    let mut deleted = Vec::new();
    for scenario_dir in crate::generate::find_dirs_with_marker(behavior_dir, "scenario.yml")? {
        let scenario_path = scenario_dir.join("scenario.yml");
        let content = std::fs::read_to_string(&scenario_path)?;
        let parsed = knowledge::parse_scenario(&content).map_err(io::Error::other)?;
        deleted.push(DeletedElement {
            kind: EntityKind::Scenario,
            id: parsed.id,
            uid: parsed.uid,
            path: relative_path_string(root, &scenario_path),
        });
    }
    Ok(deleted)
}

/// Every Feature/Scenario whose `requirement_uids` names `requirement_uid`
/// (ADR 0034 §3): rewriting them to omit it is how deleting a Requirement
/// avoids leaving a dangling UID in an optional many-to-many reference.
fn compute_detach(
    root: &Path,
    requirement_uid: &str,
) -> io::Result<(Vec<DetachedReference>, Vec<recovery::PendingKnowledgeFile>)> {
    let mut detached = Vec::new();
    let mut files = Vec::new();

    for found in knowledge_walk::list_entities(root, EntityKind::Feature)? {
        let content = std::fs::read_to_string(&found.path)?;
        let mut feature = knowledge::parse_feature(&content).map_err(io::Error::other)?;
        if !feature
            .requirement_uids
            .iter()
            .any(|u| u == requirement_uid)
        {
            continue;
        }
        feature.requirement_uids.retain(|u| u != requirement_uid);
        detached.push(DetachedReference {
            kind: EntityKind::Feature,
            id: feature.id.clone(),
            path: relative_path_string(root, &found.path),
        });
        files.push(recovery::PendingKnowledgeFile {
            relative_path: relative_path_string(root, &found.path),
            contents: knowledge::serialize_feature(&feature),
        });
    }

    for found in knowledge_walk::list_entities(root, EntityKind::Scenario)? {
        let content = std::fs::read_to_string(&found.path)?;
        let mut scenario = knowledge::parse_scenario(&content).map_err(io::Error::other)?;
        if !scenario
            .requirement_uids
            .iter()
            .any(|u| u == requirement_uid)
        {
            continue;
        }
        scenario.requirement_uids.retain(|u| u != requirement_uid);
        detached.push(DetachedReference {
            kind: EntityKind::Scenario,
            id: scenario.id.clone(),
            path: relative_path_string(root, &found.path),
        });
        files.push(recovery::PendingKnowledgeFile {
            relative_path: relative_path_string(root, &found.path),
            contents: knowledge::serialize_scenario(&scenario),
        });
    }

    Ok((detached, files))
}

/// The safe entry point (mirrors `knowledge_reconcile::execute::reconcile_creation`):
/// resolves `target` and commits its cascade delete plus back-reference
/// detachment under one continuous hold of the identity lock, so a
/// concurrent identity mutation cannot land between resolution and commit.
pub fn remove_element(root: &Path, target: &RemoveTarget) -> Result<RemoveOutcome, RemoveError> {
    let held_lock = match recovery::run_startup_recovery(root, |intent| {
        feature_ops::roll_forward(root, intent)
    })? {
        recovery::StartupRecovery::OperationInProgress => {
            return Err(RemoveError::OperationInProgress);
        }
        recovery::StartupRecovery::Ready { lock, .. } => lock,
    };
    let outcome = (|| {
        let resolved = resolve_target(root, target)?;
        let deleted = compute_cascade(root, target.kind, &resolved)?;

        let (detached, files) = match (target.kind, &resolved.entity.uid) {
            (EntityKind::Requirement, Some(uid)) => compute_detach(root, uid)?,
            _ => (Vec::new(), Vec::new()),
        };

        let deletes: Vec<recovery::PendingKnowledgeDelete> = deleted
            .iter()
            .map(|d| recovery::PendingKnowledgeDelete {
                relative_path: d.path.clone(),
            })
            .collect();

        if !deletes.is_empty() || !files.is_empty() {
            let intent = recovery::begin_batch_with_payload(
                root,
                Vec::new(),
                Some(recovery::IntentPayload::KnowledgeRemove { deletes, files }),
            )?;
            recovery::commit_batch(root, &intent)?;
            feature_ops::roll_forward(root, &intent)?;
            recovery::finish(root, &intent)?;
        }

        Ok(RemoveOutcome { deleted, detached })
    })();
    held_lock.release()?;
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(dir: &Path, relative: &str, contents: &str) {
        let path = dir.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn init_tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            ".markharness/knowledge/requirements/controls/requirement.yml",
            "id: controls\nsource: native\nlabel: controls\naxis: []\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FR0\n",
        );
        write(
            dir.path(),
            ".markharness/knowledge/features/player-jump/feature.yml",
            "id: player-jump\nrequirement_uids: [01ARZ3NDEKTSV4RRFFQ69G5FR0]\nlabel: player-jump\naxis: []\n",
        );
        write(
            dir.path(),
            ".markharness/knowledge/features/player-jump/jump/behavior.yml",
            "id: jump\nfeature: player-jump\nlabel: jump\naxis: []\ndescription: |\n  d\nprocedures: {}\n",
        );
        write(
            dir.path(),
            ".markharness/knowledge/features/player-jump/jump/basic/scenario.yml",
            "id: basic\nbehavior: jump\nlabel: basic\ndescription: |\n  d\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"Confirmed.\"\n",
        );
        dir
    }

    #[test]
    fn removes_a_requirement_with_no_back_references() {
        let dir = init_tree();
        write(
            dir.path(),
            ".markharness/knowledge/features/player-jump/feature.yml",
            "id: player-jump\nrequirement_uids: []\nlabel: player-jump\naxis: []\n",
        );

        let outcome = remove_element(
            dir.path(),
            &RemoveTarget {
                kind: EntityKind::Requirement,
                key: "controls",
                feature: None,
                behavior: None,
            },
        )
        .unwrap();

        assert_eq!(outcome.deleted.len(), 1);
        assert!(
            !dir.path()
                .join(".markharness/knowledge/requirements/controls/requirement.yml")
                .exists()
        );
    }

    #[test]
    fn removing_a_requirement_detaches_it_from_referencing_features_and_scenarios() {
        let dir = init_tree();
        write(
            dir.path(),
            ".markharness/knowledge/features/player-jump/jump/basic/scenario.yml",
            "id: basic\nbehavior: jump\nlabel: basic\ndescription: |\n  d\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"Confirmed.\"\nrequirement_uids: [01ARZ3NDEKTSV4RRFFQ69G5FR0]\n",
        );

        let outcome = remove_element(
            dir.path(),
            &RemoveTarget {
                kind: EntityKind::Requirement,
                key: "controls",
                feature: None,
                behavior: None,
            },
        )
        .unwrap();

        assert_eq!(outcome.detached.len(), 2);
        let feature_content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/features/player-jump/feature.yml"),
        )
        .unwrap();
        let feature = knowledge::parse_feature(&feature_content).unwrap();
        assert!(feature.requirement_uids.is_empty());

        let scenario_content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/features/player-jump/jump/basic/scenario.yml"),
        )
        .unwrap();
        let scenario = knowledge::parse_scenario(&scenario_content).unwrap();
        assert!(scenario.requirement_uids.is_empty());
    }

    #[test]
    fn removing_a_feature_cascades_to_its_behaviors_and_scenarios() {
        let dir = init_tree();

        let outcome = remove_element(
            dir.path(),
            &RemoveTarget {
                kind: EntityKind::Feature,
                key: "player-jump",
                feature: None,
                behavior: None,
            },
        )
        .unwrap();

        assert_eq!(outcome.deleted.len(), 3);
        assert!(
            !dir.path()
                .join(".markharness/knowledge/features/player-jump/feature.yml")
                .exists()
        );
        assert!(
            !dir.path()
                .join(".markharness/knowledge/features/player-jump/jump/behavior.yml")
                .exists()
        );
        assert!(
            !dir.path()
                .join(".markharness/knowledge/features/player-jump/jump/basic/scenario.yml")
                .exists()
        );
    }

    #[test]
    fn removing_a_behavior_cascades_to_its_scenarios() {
        let dir = init_tree();

        let outcome = remove_element(
            dir.path(),
            &RemoveTarget {
                kind: EntityKind::Behavior,
                key: "jump",
                feature: Some("player-jump"),
                behavior: None,
            },
        )
        .unwrap();

        assert_eq!(outcome.deleted.len(), 2);
        assert!(
            !dir.path()
                .join(".markharness/knowledge/features/player-jump/jump/behavior.yml")
                .exists()
        );
        assert!(
            !dir.path()
                .join(".markharness/knowledge/features/player-jump/jump/basic/scenario.yml")
                .exists()
        );
    }

    /// Regression: a Behavior id (and, transitively, a Scenario id) is only
    /// unique *within* its parent (ADR 0034 §2) — the same slug can exist
    /// under a different Feature. Cascading Feature deletion down to
    /// Scenarios must be scoped by directory, never by matching
    /// `Scenario.behavior`'s bare id text against the deleted Behavior's id
    /// tree-wide, or a same-named Behavior/Scenario under an unrelated
    /// Feature would be deleted too.
    #[test]
    fn removing_a_feature_never_deletes_a_same_named_behavior_or_scenario_under_another_feature() {
        let dir = init_tree();
        write(
            dir.path(),
            ".markharness/knowledge/features/player-dash/feature.yml",
            "id: player-dash\nrequirement_uids: []\nlabel: player-dash\naxis: []\n",
        );
        write(
            dir.path(),
            ".markharness/knowledge/features/player-dash/jump/behavior.yml",
            "id: jump\nfeature: player-dash\nlabel: jump\naxis: []\ndescription: |\n  d\nprocedures: {}\n",
        );
        write(
            dir.path(),
            ".markharness/knowledge/features/player-dash/jump/basic/scenario.yml",
            "id: basic\nbehavior: jump\nlabel: basic\ndescription: |\n  d\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"Confirmed.\"\n",
        );

        let outcome = remove_element(
            dir.path(),
            &RemoveTarget {
                kind: EntityKind::Feature,
                key: "player-jump",
                feature: None,
                behavior: None,
            },
        )
        .unwrap();

        assert_eq!(outcome.deleted.len(), 3);
        assert!(
            !dir.path()
                .join(".markharness/knowledge/features/player-jump")
                .join("jump/behavior.yml")
                .exists()
        );
        assert!(
            dir.path()
                .join(".markharness/knowledge/features/player-dash/jump/behavior.yml")
                .exists(),
            "a same-named Behavior under an unrelated Feature must survive"
        );
        assert!(
            dir.path()
                .join(".markharness/knowledge/features/player-dash/jump/basic/scenario.yml")
                .exists(),
            "a same-named Scenario under an unrelated Feature's Behavior must survive"
        );
    }

    /// Same regression, but for a single Behavior deletion cascading to
    /// Scenarios: the deleted Behavior's own Scenario must go, but a
    /// same-named Scenario nested under a different Feature's same-named
    /// Behavior must not.
    #[test]
    fn removing_a_behavior_never_deletes_a_same_named_scenario_under_another_features_behavior() {
        let dir = init_tree();
        write(
            dir.path(),
            ".markharness/knowledge/features/player-dash/feature.yml",
            "id: player-dash\nrequirement_uids: []\nlabel: player-dash\naxis: []\n",
        );
        write(
            dir.path(),
            ".markharness/knowledge/features/player-dash/jump/behavior.yml",
            "id: jump\nfeature: player-dash\nlabel: jump\naxis: []\ndescription: |\n  d\nprocedures: {}\n",
        );
        write(
            dir.path(),
            ".markharness/knowledge/features/player-dash/jump/basic/scenario.yml",
            "id: basic\nbehavior: jump\nlabel: basic\ndescription: |\n  d\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"Confirmed.\"\n",
        );

        let outcome = remove_element(
            dir.path(),
            &RemoveTarget {
                kind: EntityKind::Behavior,
                key: "jump",
                feature: Some("player-jump"),
                behavior: None,
            },
        )
        .unwrap();

        assert_eq!(outcome.deleted.len(), 2);
        assert!(
            !dir.path()
                .join(".markharness/knowledge/features/player-jump/jump/basic/scenario.yml")
                .exists()
        );
        assert!(
            dir.path()
                .join(".markharness/knowledge/features/player-dash/jump/basic/scenario.yml")
                .exists(),
            "a same-named Scenario under a different Feature's same-named Behavior must survive"
        );
    }

    #[test]
    fn removing_a_scenario_deletes_only_that_scenario() {
        let dir = init_tree();

        let outcome = remove_element(
            dir.path(),
            &RemoveTarget {
                kind: EntityKind::Scenario,
                key: "basic",
                feature: Some("player-jump"),
                behavior: Some("jump"),
            },
        )
        .unwrap();

        assert_eq!(outcome.deleted.len(), 1);
        assert!(
            !dir.path()
                .join(".markharness/knowledge/features/player-jump/jump/basic/scenario.yml")
                .exists()
        );
        assert!(
            dir.path()
                .join(".markharness/knowledge/features/player-jump/jump/behavior.yml")
                .exists()
        );
    }

    #[test]
    fn returns_not_found_for_an_unknown_key() {
        let dir = init_tree();

        let result = remove_element(
            dir.path(),
            &RemoveTarget {
                kind: EntityKind::Requirement,
                key: "does-not-exist",
                feature: None,
                behavior: None,
            },
        );

        assert!(matches!(result, Err(RemoveError::NotFound)));
    }

    /// Security regression: `key`/`--feature`/`--behavior` must never be
    /// usable to reach a file outside `.markharness/knowledge/`. This is
    /// true by construction — `resolve_target`/`resolve_scoped` only ever
    /// compare these strings against `id`/`uid` values `list_entities`
    /// already read from real, disk-discovered files (see the `==`
    /// comparisons in `resolve_target`/`resolve_scoped`); they are never
    /// joined onto `root` to build a path. This test pins that down for a
    /// path-traversal-shaped key that matches no real entity: the result
    /// must be a plain `NotFound`, and nothing outside the tempdir must be
    /// touched.
    #[test]
    fn a_path_traversal_shaped_key_that_matches_nothing_is_not_found_and_deletes_nothing() {
        let dir = init_tree();
        let outside = std::env::temp_dir();
        let sentinel = outside.join("markharness-knowledge-remove-traversal-sentinel.txt");
        fs::write(&sentinel, "must not be touched").unwrap();

        for (kind, feature, behavior) in [
            (EntityKind::Requirement, None, None),
            (EntityKind::Feature, None, None),
            (EntityKind::Behavior, Some("player-jump"), None),
            (EntityKind::Scenario, Some("player-jump"), Some("jump")),
        ] {
            let result = remove_element(
                dir.path(),
                &RemoveTarget {
                    kind,
                    key: "../../../../../../etc/passwd",
                    feature,
                    behavior,
                },
            );
            assert!(
                matches!(result, Err(RemoveError::NotFound)),
                "expected NotFound for {kind:?}, got {result:?}"
            );
        }

        assert_eq!(
            fs::read_to_string(&sentinel).unwrap(),
            "must not be touched"
        );
        fs::remove_file(&sentinel).unwrap();
    }

    /// Security regression: even when a Behavior/Scenario's own `id`/
    /// `feature`/`behavior` *field content* is itself a path-traversal
    /// string (nothing validates that content — `id:` is free-form YAML),
    /// resolving and deleting it must only ever touch the file's real,
    /// contained path — the one `list_entities` found by walking real
    /// directories — never a path built from that field's content. Proves
    /// `compute_cascade`'s `parsed.feature != resolved.entity.id`-style
    /// checks are pure string comparisons, not path construction.
    #[test]
    fn a_behaviors_own_id_field_containing_path_traversal_text_does_not_escape_deletion() {
        let dir = init_tree();
        write(
            dir.path(),
            ".markharness/knowledge/features/player-jump/sneaky/behavior.yml",
            "id: \"../../../../../../etc/passwd\"\nfeature: player-jump\nlabel: sneaky\naxis: []\ndescription: |\n  d\nprocedures: {}\n",
        );

        let outcome = remove_element(
            dir.path(),
            &RemoveTarget {
                kind: EntityKind::Behavior,
                key: "../../../../../../etc/passwd",
                feature: Some("player-jump"),
                behavior: None,
            },
        )
        .unwrap();

        assert_eq!(outcome.deleted.len(), 1);
        assert_eq!(
            outcome.deleted[0].path,
            ".markharness/knowledge/features/player-jump/sneaky/behavior.yml"
        );
        assert!(
            !dir.path()
                .join(".markharness/knowledge/features/player-jump/sneaky/behavior.yml")
                .exists()
        );
    }

    #[test]
    fn ambiguous_behavior_slug_across_two_features_is_rejected_without_scoping() {
        let dir = init_tree();
        write(
            dir.path(),
            ".markharness/knowledge/features/player-dash/feature.yml",
            "id: player-dash\nrequirement_uids: []\nlabel: player-dash\naxis: []\n",
        );
        write(
            dir.path(),
            ".markharness/knowledge/features/player-dash/jump/behavior.yml",
            "id: jump\nfeature: player-dash\nlabel: jump\naxis: []\ndescription: |\n  d\nprocedures: {}\n",
        );

        let result = remove_element(
            dir.path(),
            &RemoveTarget {
                kind: EntityKind::Behavior,
                key: "jump",
                feature: None,
                behavior: None,
            },
        );

        match result {
            Err(RemoveError::Ambiguous(paths)) => assert_eq!(paths.len(), 2),
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn feature_scoping_disambiguates_a_behavior_slug_shared_across_features() {
        let dir = init_tree();
        write(
            dir.path(),
            ".markharness/knowledge/features/player-dash/feature.yml",
            "id: player-dash\nrequirement_uids: []\nlabel: player-dash\naxis: []\n",
        );
        write(
            dir.path(),
            ".markharness/knowledge/features/player-dash/jump/behavior.yml",
            "id: jump\nfeature: player-dash\nlabel: jump\naxis: []\ndescription: |\n  d\nprocedures: {}\n",
        );

        let outcome = remove_element(
            dir.path(),
            &RemoveTarget {
                kind: EntityKind::Behavior,
                key: "jump",
                feature: Some("player-dash"),
                behavior: None,
            },
        )
        .unwrap();

        assert_eq!(
            outcome.deleted[0].path,
            ".markharness/knowledge/features/player-dash/jump/behavior.yml"
        );
        assert!(
            dir.path()
                .join(".markharness/knowledge/features/player-jump/jump/behavior.yml")
                .exists()
        );
    }
}
