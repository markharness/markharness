//! Commits a [`Plan`] atomically (ADR 0027 §1 step 7): every new element's
//! identity event and canonical Knowledge file land as one batch, reusing
//! `identity::recovery`'s existing staging + roll-forward protocol (ADR
//! 0028 §2 keeps that infra shared rather than duplicated) instead of a
//! bespoke crash-recovery mechanism.

use std::io;
use std::path::Path;

use crate::identity::{EntityKind, IdentityEvent, IdentityMutation, feature_ops, recovery};
use crate::knowledge;
use crate::time::iso8601_utc_now;

use super::plan::Plan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedElement {
    pub kind: EntityKind,
    pub uid: String,
    pub id: String,
}

#[derive(Debug)]
pub enum ExecuteError {
    /// A concurrent identity operation is genuinely in progress (design
    /// doc §6.3) — the caller must retry later, not race it.
    OperationInProgress,
    Io(io::Error),
}

impl From<io::Error> for ExecuteError {
    fn from(e: io::Error) -> Self {
        ExecuteError::Io(e)
    }
}

fn relative_path_string(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Commits every new element in `plan` as one crash-recoverable batch and
/// returns what was created. An empty plan writes nothing and returns an
/// empty list.
pub fn execute_creation_plan(
    root: &Path,
    plan: &Plan,
) -> Result<Vec<CreatedElement>, ExecuteError> {
    // Reusing the exact lock `run_startup_recovery` acquired for the
    // check-and-commit below (rather than releasing and reacquiring) keeps
    // recovery and this operation as one continuous critical section — see
    // `recovery::run_startup_recovery`'s own doc comment for why the gap
    // between two separate acquires matters.
    let held_lock = match recovery::run_startup_recovery(root, |intent| {
        feature_ops::roll_forward(root, intent)
    })? {
        recovery::StartupRecovery::OperationInProgress => {
            return Err(ExecuteError::OperationInProgress);
        }
        recovery::StartupRecovery::Ready { lock, .. } => lock,
    };
    let outcome = commit_plan(root, plan);
    held_lock.release()?;
    outcome
}

fn commit_plan(root: &Path, plan: &Plan) -> Result<Vec<CreatedElement>, ExecuteError> {
    if plan.is_empty() {
        return Ok(Vec::new());
    }

    let recorded_at = iso8601_utc_now();
    let mut batch_events = Vec::new();
    let mut files = Vec::new();
    let mut created = Vec::new();

    for req in &plan.new_requirements {
        let event_uid = ulid::Ulid::new().to_string();
        let event = IdentityEvent {
            identity_event_uid: event_uid.clone(),
            entity_uid: req.uid.clone(),
            entity_kind: EntityKind::Requirement,
            previous_identity_event_uid: None,
            previous_identity_event_uids: Vec::new(),
            recorded_at: recorded_at.clone(),
            mutation: IdentityMutation::Issued { id: req.id.clone() },
        };
        batch_events.push(recovery::BatchEvent {
            entity_kind: EntityKind::Requirement,
            entity_uid: req.uid.clone(),
            identity_event_uid: event_uid,
            event_yaml: serde_yaml_ng::to_string(&event).map_err(io::Error::other)?,
        });
        let path = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge")
            .join("requirements")
            .join(&req.id)
            .join("requirement.yml");
        files.push(recovery::PendingKnowledgeFile {
            relative_path: relative_path_string(root, &path),
            contents: knowledge::serialize_requirement(&req.to_canonical()),
        });
        created.push(CreatedElement {
            kind: EntityKind::Requirement,
            uid: req.uid.clone(),
            id: req.id.clone(),
        });
    }

    for feature in &plan.new_features {
        let event_uid = ulid::Ulid::new().to_string();
        let event = IdentityEvent {
            identity_event_uid: event_uid.clone(),
            entity_uid: feature.uid.clone(),
            entity_kind: EntityKind::Feature,
            previous_identity_event_uid: None,
            previous_identity_event_uids: Vec::new(),
            recorded_at: recorded_at.clone(),
            mutation: IdentityMutation::Issued {
                id: feature.id.clone(),
            },
        };
        batch_events.push(recovery::BatchEvent {
            entity_kind: EntityKind::Feature,
            entity_uid: feature.uid.clone(),
            identity_event_uid: event_uid,
            event_yaml: serde_yaml_ng::to_string(&event).map_err(io::Error::other)?,
        });
        let path = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge")
            .join("features")
            .join(&feature.id)
            .join("feature.yml");
        files.push(recovery::PendingKnowledgeFile {
            relative_path: relative_path_string(root, &path),
            contents: knowledge::serialize_feature(&feature.to_canonical()),
        });
        created.push(CreatedElement {
            kind: EntityKind::Feature,
            uid: feature.uid.clone(),
            id: feature.id.clone(),
        });
    }

    let intent = recovery::begin_batch_with_payload(
        root,
        batch_events,
        Some(recovery::IntentPayload::KnowledgeReconcile(files)),
    )?;
    recovery::commit_batch(root, &intent)?;
    feature_ops::roll_forward(root, &intent)?;
    recovery::finish(root, &intent)?;
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_reconcile::intent::parse_intent;
    use crate::knowledge_reconcile::plan::build_plan;

    fn init_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".markharness/knowledge")).unwrap();
        dir
    }

    #[test]
    fn creates_a_requirement_and_feature_with_uids_embedded() {
        let dir = init_project();
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: req_todo
    id: todo
    source: native
    label: TODO management
    axis: [functional]

features:
  - key: feature_todo
    id: todo-management
    contributes_to: [req_todo]
    label: TODO management
    axis: [functional]
";
        let doc = parse_intent(yaml).unwrap();
        let plan = build_plan(dir.path(), &doc).unwrap();

        let created = execute_creation_plan(dir.path(), &plan).unwrap();
        assert_eq!(created.len(), 2);

        let requirement_content = std::fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/requirements/todo/requirement.yml"),
        )
        .unwrap();
        let requirement = knowledge::parse_requirement(&requirement_content).unwrap();
        assert!(requirement.uid.is_some());

        let feature_content = std::fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/features/todo-management/feature.yml"),
        )
        .unwrap();
        let feature = knowledge::parse_feature(&feature_content).unwrap();
        assert_eq!(feature.requirement_uids, vec![requirement.uid.unwrap()]);
    }

    #[test]
    fn writes_exactly_one_issued_event_per_new_entity() {
        use crate::identity::registry;

        let dir = init_project();
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - id: todo
    source: native
    label: TODO management
    axis: []
";
        let doc = parse_intent(yaml).unwrap();
        let plan = build_plan(dir.path(), &doc).unwrap();
        let created = execute_creation_plan(dir.path(), &plan).unwrap();

        let events = registry::load_events_from_working_tree(
            dir.path(),
            EntityKind::Requirement,
            &created[0].uid,
        )
        .unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].mutation,
            IdentityMutation::Issued { id } if id == "todo"
        ));
    }

    #[test]
    fn an_empty_plan_writes_nothing() {
        let dir = init_project();
        let created = execute_creation_plan(dir.path(), &Plan::default()).unwrap();
        assert!(created.is_empty());
        assert!(!dir.path().join(".markharness/identity-events").exists());
    }

    /// Simulates a crash after the batch's logical commit point (the first
    /// identity event landing at its final path) but before the pending
    /// Knowledge files were written: a later command's startup recovery
    /// must finish writing them from the durably-persisted
    /// `KnowledgeReconcile` payload, not lose them.
    #[test]
    fn a_subsequent_command_rolls_forward_pending_files_after_a_kill_right_after_commit() {
        let dir = init_project();
        let requirement_uid = "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string();
        let event_uid = "01ARZ3NDEKTSV4RRFFQ69G5FE0".to_string();
        let event = IdentityEvent {
            identity_event_uid: event_uid.clone(),
            entity_uid: requirement_uid.clone(),
            entity_kind: EntityKind::Requirement,
            previous_identity_event_uid: None,
            previous_identity_event_uids: Vec::new(),
            recorded_at: "2026-09-13T00:00:00Z".to_string(),
            mutation: IdentityMutation::Issued {
                id: "todo".to_string(),
            },
        };
        let requirement = crate::knowledge::Requirement {
            id: "todo".to_string(),
            source: crate::knowledge::RequirementSource::Native,
            label: Some("TODO management".to_string()),
            axis: Vec::new(),
            description: None,
            source_locator: None,
            source_revision: None,
            related_issues: Vec::new(),
            uid: Some(requirement_uid.clone()),
        };
        let intent = recovery::begin_batch_with_payload(
            dir.path(),
            vec![recovery::BatchEvent {
                entity_kind: EntityKind::Requirement,
                entity_uid: requirement_uid.clone(),
                identity_event_uid: event_uid.clone(),
                event_yaml: serde_yaml_ng::to_string(&event).unwrap(),
            }],
            Some(recovery::IntentPayload::KnowledgeReconcile(vec![
                recovery::PendingKnowledgeFile {
                    relative_path: ".markharness/knowledge/requirements/todo/requirement.yml"
                        .to_string(),
                    contents: knowledge::serialize_requirement(&requirement),
                },
            ])),
        )
        .unwrap();
        recovery::commit_batch(dir.path(), &intent).unwrap();
        // Deliberately no roll_forward/finish call: this is the crash point.

        assert!(
            !dir.path()
                .join(".markharness/knowledge/requirements/todo/requirement.yml")
                .is_file(),
            "the file must not exist yet before recovery runs"
        );

        // Any subsequent identity/reconcile operation runs startup recovery
        // first; simulate that here directly.
        recovery::run_startup_recovery(dir.path(), |intent| {
            feature_ops::roll_forward(dir.path(), intent)
        })
        .unwrap();

        let content = std::fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/requirements/todo/requirement.yml"),
        )
        .unwrap();
        assert!(content.contains("id: todo"));
        assert!(content.contains(&requirement_uid));
    }
}
