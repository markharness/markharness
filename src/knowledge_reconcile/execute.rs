//! Commits a [`Plan`] atomically (ADR 0027 §1 step 7): every new or
//! renamed element's identity event and canonical Knowledge file land as
//! one batch, reusing `identity::recovery`'s existing staging +
//! roll-forward protocol (ADR 0028 §2 keeps that infra shared rather than
//! duplicated) instead of a bespoke crash-recovery mechanism. A pure
//! content-only patch (no id change) carries no identity event — content
//! fields are not part of the immutable identity model's own history, only
//! id/uid lifecycle is — so it is written directly, mirroring
//! `feature_ops::write_feature_fixups`'s existing precedent for the same
//! kind of write.

use std::io;
use std::path::{Path, PathBuf};

use crate::identity::{
    EntityKind, IdentityEvent, IdentityMutation, feature_ops, recovery, registry,
};
use crate::knowledge;
use crate::time::iso8601_utc_now;

use super::intent::IntentDocument;
use super::plan::{FeatureOutcome, Plan, PlanError, RequirementOutcome, build_plan};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedElement {
    pub kind: EntityKind,
    pub uid: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatedElement {
    pub kind: EntityKind,
    pub uid: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnchangedElement {
    pub kind: EntityKind,
    pub uid: String,
    pub id: String,
}

/// `knowledge reconcile`'s result (ADR 0027 §7: `--json` returns at least
/// `created`/`updated`/`unchanged`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReconcileOutcome {
    pub created: Vec<CreatedElement>,
    pub updated: Vec<UpdatedElement>,
    pub unchanged: Vec<UnchangedElement>,
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

/// Why [`reconcile_creation`] could not complete.
#[derive(Debug)]
pub enum ReconcileError {
    Diagnostics(Vec<super::diagnostics::Diagnostic>),
    /// See [`PlanError::NotYetSupported`] — not a stable ADR 0027 §7 code.
    NotYetSupported(String),
    OperationInProgress,
    Io(io::Error),
}

impl From<io::Error> for ReconcileError {
    fn from(e: io::Error) -> Self {
        ReconcileError::Io(e)
    }
}

impl From<PlanError> for ReconcileError {
    fn from(e: PlanError) -> Self {
        match e {
            PlanError::Diagnostics(d) => ReconcileError::Diagnostics(d),
            PlanError::NotYetSupported(m) => ReconcileError::NotYetSupported(m),
            PlanError::Io(e) => ReconcileError::Io(e),
        }
    }
}

fn relative_path_string(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The safe entry point ADR 0027 requires: builds the plan against current
/// state and commits it under one continuous hold of the identity lock,
/// exactly like `feature_ops::rename_id`/`resolve_divergence`/
/// `migrate_entities` do for their own state reads.
///
/// Building a [`Plan`] with [`build_plan`] and only *later* calling
/// [`execute_creation_plan`] — as an earlier version of this module's CLI
/// caller did — reopens the TOCTOU gap those functions' own doc comments
/// warn about: a second `reconcile` (or any other identity operation)
/// could create or change a same-id element between the unlocked state
/// read and the eventual commit, and this module would then overwrite it.
/// Call this function instead of that two-step sequence for any real
/// repository.
pub fn reconcile_creation(
    root: &Path,
    doc: &IntentDocument,
) -> Result<ReconcileOutcome, ReconcileError> {
    let held_lock = match recovery::run_startup_recovery(root, |intent| {
        feature_ops::roll_forward(root, intent)
    })? {
        recovery::StartupRecovery::OperationInProgress => {
            return Err(ReconcileError::OperationInProgress);
        }
        recovery::StartupRecovery::Ready { lock, .. } => lock,
    };
    let outcome = (|| {
        let plan = build_plan(root, doc)?;
        commit_plan(root, &plan).map_err(ReconcileError::from)
    })();
    held_lock.release()?;
    outcome
}

/// Commits `plan` as one crash-recoverable batch (new/renamed elements)
/// plus any pure content-only patches (written directly, no event) and
/// returns what changed. An empty plan writes nothing.
///
/// Acquires the identity lock itself, but — unlike [`reconcile_creation`]
/// — does *not* re-check current state under that lock: `plan` was already
/// built from whatever state was current when its caller built it. Calling
/// this with a `Plan` built separately, and possibly stale, from
/// [`build_plan`] reopens the TOCTOU gap [`reconcile_creation`]'s own doc
/// comment describes. Safe to use only when nothing else can concurrently
/// mutate the same repository between `build_plan` and this call (e.g. a
/// test against an isolated, single-threaded fixture); production callers
/// must use [`reconcile_creation`] instead.
pub fn execute_creation_plan(root: &Path, plan: &Plan) -> Result<ReconcileOutcome, ExecuteError> {
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
    let outcome = commit_plan(root, plan).map_err(ExecuteError::Io);
    held_lock.release()?;
    outcome
}

fn push_issued(
    batch_events: &mut Vec<recovery::BatchEvent>,
    kind: EntityKind,
    uid: &str,
    id: &str,
    recorded_at: &str,
) -> io::Result<()> {
    let event_uid = ulid::Ulid::new().to_string();
    let event = IdentityEvent {
        identity_event_uid: event_uid.clone(),
        entity_uid: uid.to_string(),
        entity_kind: kind,
        previous_identity_event_uid: None,
        previous_identity_event_uids: Vec::new(),
        recorded_at: recorded_at.to_string(),
        mutation: IdentityMutation::Issued { id: id.to_string() },
    };
    batch_events.push(recovery::BatchEvent {
        entity_kind: kind,
        entity_uid: uid.to_string(),
        identity_event_uid: event_uid,
        event_yaml: serde_yaml_ng::to_string(&event).map_err(io::Error::other)?,
    });
    Ok(())
}

fn push_renamed(
    root: &Path,
    batch_events: &mut Vec<recovery::BatchEvent>,
    kind: EntityKind,
    uid: &str,
    from_id: &str,
    to_id: &str,
    recorded_at: &str,
) -> io::Result<()> {
    let replay = registry::resolve_from_working_tree(root, kind, uid)?
        .map_err(|e| io::Error::other(format!("{e:?}")))?;
    let event_uid = ulid::Ulid::new().to_string();
    let event = IdentityEvent {
        identity_event_uid: event_uid.clone(),
        entity_uid: uid.to_string(),
        entity_kind: kind,
        previous_identity_event_uid: Some(replay.current_head_event_uid),
        previous_identity_event_uids: Vec::new(),
        recorded_at: recorded_at.to_string(),
        mutation: IdentityMutation::Renamed {
            from_id: from_id.to_string(),
            to_id: to_id.to_string(),
        },
    };
    batch_events.push(recovery::BatchEvent {
        entity_kind: kind,
        entity_uid: uid.to_string(),
        identity_event_uid: event_uid,
        event_yaml: serde_yaml_ng::to_string(&event).map_err(io::Error::other)?,
    });
    Ok(())
}

fn pending_file(root: &Path, path: &Path, contents: String) -> recovery::PendingKnowledgeFile {
    recovery::PendingKnowledgeFile {
        relative_path: relative_path_string(root, path),
        contents,
    }
}

fn requirement_path(root: &Path, id: &str) -> PathBuf {
    root.join(crate::project_root::MARKHARNESS_DIR)
        .join("knowledge")
        .join("requirements")
        .join(id)
        .join("requirement.yml")
}

fn feature_path(root: &Path, id: &str) -> PathBuf {
    root.join(crate::project_root::MARKHARNESS_DIR)
        .join("knowledge")
        .join("features")
        .join(id)
        .join("feature.yml")
}

fn behavior_path(root: &Path, feature_id: &str, behavior_id: &str) -> PathBuf {
    root.join(crate::project_root::MARKHARNESS_DIR)
        .join("knowledge")
        .join("features")
        .join(feature_id)
        .join(behavior_id)
        .join("behavior.yml")
}

fn scenario_path(root: &Path, feature_id: &str, behavior_id: &str, scenario_id: &str) -> PathBuf {
    root.join(crate::project_root::MARKHARNESS_DIR)
        .join("knowledge")
        .join("features")
        .join(feature_id)
        .join(behavior_id)
        .join(scenario_id)
        .join("scenario.yml")
}

fn commit_plan(root: &Path, plan: &Plan) -> io::Result<ReconcileOutcome> {
    let recorded_at = iso8601_utc_now();
    let mut batch_events = Vec::new();
    let mut files = Vec::new();
    let mut direct_writes: Vec<(PathBuf, String)> = Vec::new();
    let mut outcome = ReconcileOutcome::default();

    for req in &plan.requirements {
        match req {
            RequirementOutcome::New { uid, canonical } => {
                push_issued(
                    &mut batch_events,
                    EntityKind::Requirement,
                    uid,
                    &canonical.id,
                    &recorded_at,
                )?;
                let path = requirement_path(root, &canonical.id);
                files.push(pending_file(
                    root,
                    &path,
                    knowledge::serialize_requirement(canonical),
                ));
                outcome.created.push(CreatedElement {
                    kind: EntityKind::Requirement,
                    uid: uid.clone(),
                    id: canonical.id.clone(),
                });
            }
            RequirementOutcome::Unchanged { uid, id } => {
                outcome.unchanged.push(UnchangedElement {
                    kind: EntityKind::Requirement,
                    uid: uid.clone(),
                    id: id.clone(),
                });
            }
            RequirementOutcome::Updated {
                uid,
                before_id,
                canonical,
                existing_path,
            } => {
                if &canonical.id != before_id {
                    push_renamed(
                        root,
                        &mut batch_events,
                        EntityKind::Requirement,
                        uid,
                        before_id,
                        &canonical.id,
                        &recorded_at,
                    )?;
                    files.push(pending_file(
                        root,
                        existing_path,
                        knowledge::serialize_requirement(canonical),
                    ));
                } else {
                    direct_writes.push((
                        existing_path.clone(),
                        knowledge::serialize_requirement(canonical),
                    ));
                }
                outcome.updated.push(UpdatedElement {
                    kind: EntityKind::Requirement,
                    uid: uid.clone(),
                    id: canonical.id.clone(),
                });
            }
        }
    }

    for feature in &plan.features {
        match feature {
            FeatureOutcome::New {
                uid,
                canonical,
                behaviors,
            } => {
                push_issued(
                    &mut batch_events,
                    EntityKind::Feature,
                    uid,
                    &canonical.id,
                    &recorded_at,
                )?;
                let path = feature_path(root, &canonical.id);
                files.push(pending_file(
                    root,
                    &path,
                    knowledge::serialize_feature(canonical),
                ));
                outcome.created.push(CreatedElement {
                    kind: EntityKind::Feature,
                    uid: uid.clone(),
                    id: canonical.id.clone(),
                });

                for behavior in behaviors {
                    push_issued(
                        &mut batch_events,
                        EntityKind::Behavior,
                        &behavior.uid,
                        &behavior.canonical.id,
                        &recorded_at,
                    )?;
                    let behavior_path = behavior_path(root, &canonical.id, &behavior.canonical.id);
                    files.push(pending_file(
                        root,
                        &behavior_path,
                        knowledge::serialize_behavior(&behavior.canonical),
                    ));
                    outcome.created.push(CreatedElement {
                        kind: EntityKind::Behavior,
                        uid: behavior.uid.clone(),
                        id: behavior.canonical.id.clone(),
                    });

                    for scenario in &behavior.scenarios {
                        push_issued(
                            &mut batch_events,
                            EntityKind::Scenario,
                            &scenario.uid,
                            &scenario.canonical.id,
                            &recorded_at,
                        )?;
                        let scenario_path = scenario_path(
                            root,
                            &canonical.id,
                            &behavior.canonical.id,
                            &scenario.canonical.id,
                        );
                        files.push(pending_file(
                            root,
                            &scenario_path,
                            knowledge::serialize_scenario(&scenario.canonical),
                        ));
                        outcome.created.push(CreatedElement {
                            kind: EntityKind::Scenario,
                            uid: scenario.uid.clone(),
                            id: scenario.canonical.id.clone(),
                        });
                    }
                }
            }
            FeatureOutcome::Unchanged { uid, id } => {
                outcome.unchanged.push(UnchangedElement {
                    kind: EntityKind::Feature,
                    uid: uid.clone(),
                    id: id.clone(),
                });
            }
            FeatureOutcome::Updated {
                uid,
                before_id,
                canonical,
                existing_path,
            } => {
                if &canonical.id != before_id {
                    push_renamed(
                        root,
                        &mut batch_events,
                        EntityKind::Feature,
                        uid,
                        before_id,
                        &canonical.id,
                        &recorded_at,
                    )?;
                    files.push(pending_file(
                        root,
                        existing_path,
                        knowledge::serialize_feature(canonical),
                    ));
                } else {
                    direct_writes.push((
                        existing_path.clone(),
                        knowledge::serialize_feature(canonical),
                    ));
                }
                outcome.updated.push(UpdatedElement {
                    kind: EntityKind::Feature,
                    uid: uid.clone(),
                    id: canonical.id.clone(),
                });
            }
        }
    }

    if !batch_events.is_empty() {
        let intent = recovery::begin_batch_with_payload(
            root,
            batch_events,
            Some(recovery::IntentPayload::KnowledgeReconcile(files)),
        )?;
        recovery::commit_batch(root, &intent)?;
        feature_ops::roll_forward(root, &intent)?;
        recovery::finish(root, &intent)?;
    }

    for (path, contents) in direct_writes {
        crate::fs_safety::replace_file(root, &path, contents.as_bytes())?;
    }

    Ok(outcome)
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

    /// Regression test for the TOCTOU gap a reviewer flagged in an earlier
    /// version of this module's caller: it built a `Plan` with `build_plan`
    /// while unlocked, then only acquired the lock inside a later, separate
    /// `execute_creation_plan` call — leaving a window where a second
    /// concurrent reconcile (or any other identity operation) could create
    /// the same-id element before the first one's commit, and get silently
    /// overwritten. `reconcile_creation` must hold the lock across *both*
    /// the state read and the commit, so a caller who already holds it (as
    /// a genuinely concurrent operation would) is refused up front and
    /// never reaches `build_plan` or `commit_plan` at all.
    #[test]
    fn reconcile_creation_refuses_to_read_or_write_while_the_lock_is_held_elsewhere() {
        let dir = init_project();
        let held_lock = crate::identity::lock::IdentityLock::acquire(dir.path()).unwrap();

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

        let err = reconcile_creation(dir.path(), &doc).unwrap_err();
        assert!(matches!(err, ReconcileError::OperationInProgress));
        assert!(
            !dir.path()
                .join(".markharness/knowledge/requirements/todo/requirement.yml")
                .is_file(),
            "must not have read-then-written state while the lock was held elsewhere"
        );

        held_lock.release().unwrap();
        let outcome = reconcile_creation(dir.path(), &doc).unwrap();
        assert_eq!(outcome.created.len(), 1);
    }

    /// Two real concurrent callers racing the exact same creation must
    /// never both succeed in creating the same id — the lock must
    /// serialize them so the loser's `build_plan` (now running *inside*
    /// the lock it acquired after the winner released) sees the winner's
    /// already-committed Requirement and reports `ambiguous_identity`
    /// instead of overwriting it.
    #[test]
    fn concurrent_reconcile_creation_never_creates_the_same_id_twice() {
        let dir = init_project();
        let root = dir.path();
        const ATTEMPTS: usize = 4;
        let barrier = std::sync::Barrier::new(ATTEMPTS);
        let results: Vec<Result<ReconcileOutcome, ReconcileError>> = std::thread::scope(|scope| {
            let barrier = &barrier;
            let handles: Vec<_> = (0..ATTEMPTS)
                .map(|_| {
                    scope.spawn(move || {
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
                        barrier.wait();
                        reconcile_creation(root, &doc)
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });

        let successes: Vec<_> = results
            .iter()
            .filter_map(|r| r.as_ref().ok())
            .filter(|outcome| !outcome.created.is_empty())
            .collect();
        assert_eq!(
            successes.len(),
            1,
            "exactly one concurrent attempt must actually create 'todo', got {results:?}"
        );

        let events = registry::load_events_from_working_tree(
            root,
            EntityKind::Requirement,
            &successes[0].created[0].uid,
        )
        .unwrap();
        assert_eq!(
            events.len(),
            1,
            "the winning Requirement must have exactly one Issued event, not one per racing attempt"
        );
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

        let outcome = execute_creation_plan(dir.path(), &plan).unwrap();
        assert_eq!(outcome.created.len(), 2);

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
    fn creates_nested_behaviors_and_scenarios_under_a_new_feature() {
        let dir = init_project();
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - id: todo-management
    label: TODO management
    axis: []
    behaviors:
      - id: add-todo
        description: Add a TODO
        scenarios:
          - id: empty-title
            description: An empty title cannot be added
            phases:
              - steps:
                  - action: Attempt to add an empty title
                results:
                  - No TODO is added
";
        let doc = parse_intent(yaml).unwrap();
        let plan = build_plan(dir.path(), &doc).unwrap();
        let outcome = execute_creation_plan(dir.path(), &plan).unwrap();
        assert_eq!(outcome.created.len(), 3);

        let behavior_content = std::fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/features/todo-management/add-todo/behavior.yml"),
        )
        .unwrap();
        let behavior = knowledge::parse_behavior(&behavior_content).unwrap();
        assert!(behavior.uid.is_some());

        let scenario_content = std::fs::read_to_string(dir.path().join(
            ".markharness/knowledge/features/todo-management/add-todo/empty-title/scenario.yml",
        ))
        .unwrap();
        let scenario = knowledge::parse_scenario(&scenario_content).unwrap();
        assert!(scenario.uid.is_some());
        assert_eq!(scenario.behavior, "add-todo");
    }

    #[test]
    fn writes_exactly_one_issued_event_per_new_entity() {
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
        let outcome = execute_creation_plan(dir.path(), &plan).unwrap();

        let events = registry::load_events_from_working_tree(
            dir.path(),
            EntityKind::Requirement,
            &outcome.created[0].uid,
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
        let outcome = execute_creation_plan(dir.path(), &Plan::default()).unwrap();
        assert!(outcome.created.is_empty());
        assert!(!dir.path().join(".markharness/identity-events").exists());
    }

    #[test]
    fn rerunning_the_same_intent_reports_unchanged_and_writes_nothing_new() {
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
        reconcile_creation(dir.path(), &doc).unwrap();

        let outcome = reconcile_creation(dir.path(), &doc).unwrap();
        assert!(outcome.created.is_empty());
        assert_eq!(outcome.unchanged.len(), 1);
    }

    #[test]
    fn uid_selected_patch_rewrites_the_existing_file_without_a_new_identity_event() {
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
        let created = reconcile_creation(dir.path(), &doc).unwrap();
        let uid = created.created[0].uid.clone();

        let patch_yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - uid: {uid}
    label: Renamed label
"
        );
        let patch_doc = parse_intent(&patch_yaml).unwrap();
        let outcome = reconcile_creation(dir.path(), &patch_doc).unwrap();
        assert_eq!(outcome.updated.len(), 1);

        let events =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Requirement, &uid)
                .unwrap();
        assert_eq!(
            events.len(),
            1,
            "a content-only patch adds no identity event"
        );

        let content = std::fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/requirements/todo/requirement.yml"),
        )
        .unwrap();
        assert!(content.contains("label: Renamed label"));
    }

    #[test]
    fn uid_selected_rename_writes_a_renamed_identity_event() {
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
        let created = reconcile_creation(dir.path(), &doc).unwrap();
        let uid = created.created[0].uid.clone();

        let rename_yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - uid: {uid}
    id: task
"
        );
        let rename_doc = parse_intent(&rename_yaml).unwrap();
        let outcome = reconcile_creation(dir.path(), &rename_doc).unwrap();
        assert_eq!(outcome.updated.len(), 1);

        let events =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Requirement, &uid)
                .unwrap();
        assert_eq!(events.len(), 2);
        assert!(events.iter().any(|e| matches!(
            &e.mutation,
            IdentityMutation::Renamed { from_id, to_id } if from_id == "todo" && to_id == "task"
        )));

        let content = std::fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/requirements/todo/requirement.yml"),
        )
        .unwrap();
        assert!(content.contains("id: task"));
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
