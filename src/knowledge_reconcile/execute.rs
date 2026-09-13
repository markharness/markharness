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

use super::diagnostics::{Diagnostic, DiagnosticCode};
use super::intent::IntentDocument;
use super::paths::{behavior_path, feature_path, requirement_path, scenario_path};
use super::plan::{
    FeatureOutcome, Plan, PlanError, RequirementOutcome, ScenarioOutcome, build_plan,
    state_fingerprint,
};

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
    /// Includes `stale_plan` (ADR 0027 §6): `plan` was built from state
    /// that no longer matches the repository, most commonly because it was
    /// built separately from this call (see this module's own doc comment
    /// on why that reopens a TOCTOU gap `reconcile_creation` closes).
    Diagnostics(Vec<super::diagnostics::Diagnostic>),
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
        if let Some(diagnostic) = check_not_stale(root, &plan)? {
            return Err(ReconcileError::Diagnostics(vec![diagnostic]));
        }
        commit_plan(root, &plan).map_err(ReconcileError::from)
    })();
    held_lock.release()?;
    outcome
}

/// `--check` (ADR 0027 §6): runs the exact same parse → match → validate →
/// plan pipeline a real run does, under the same lock (a consistent read
/// of current state) — but stops before `commit_plan`, so nothing is
/// written. `--check`'s result is a preview only: the ADR explicitly
/// forbids treating it as a permit for a later write, since state can
/// change in between (that gap is exactly what [`ExecuteError::Diagnostics`]'s
/// `stale_plan` covers on the later real run).
pub fn check_creation(
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
    let outcome = build_plan(root, doc)
        .map(|plan| plan_outcome(&plan))
        .map_err(ReconcileError::from);
    held_lock.release()?;
    outcome
}

/// Re-reads `.markharness/knowledge`'s current fingerprint and compares it
/// to the one `plan` was built against (ADR 0027 §6 `stale_plan`). `plan`
/// carries no fingerprint (`state_fingerprint: None`) when it was not
/// produced by [`build_plan`] itself (e.g. a hand-built `Plan::default()`
/// in a test) — nothing to compare against, so this passes it through
/// rather than treating the absence as either fresh or stale.
fn check_not_stale(root: &Path, plan: &Plan) -> io::Result<Option<Diagnostic>> {
    let Some(expected) = &plan.state_fingerprint else {
        return Ok(None);
    };
    let current = state_fingerprint(root)?;
    if &current == expected {
        return Ok(None);
    }
    Ok(Some(Diagnostic::new(
        DiagnosticCode::StalePlan,
        "<document>",
        "the repository's Knowledge changed since this plan was built; re-run reconcile to build a fresh plan before committing",
    )))
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
    let outcome = (|| {
        if let Some(diagnostic) = check_not_stale(root, plan)? {
            return Err(ExecuteError::Diagnostics(vec![diagnostic]));
        }
        commit_plan(root, plan).map_err(ExecuteError::Io)
    })();
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

/// Classifies `plan` into the created/updated/unchanged preview
/// [`ReconcileOutcome`] describes — the read-only half of what
/// [`commit_plan`] does, shared with [`check_creation`] (ADR 0027 §6's
/// `--check`) so the preview a caller sees before committing can never
/// drift from what an actual commit of the same plan would report.
fn plan_outcome(plan: &Plan) -> ReconcileOutcome {
    let mut outcome = ReconcileOutcome::default();

    for req in &plan.requirements {
        match req {
            RequirementOutcome::New { uid, canonical } => outcome.created.push(CreatedElement {
                kind: EntityKind::Requirement,
                uid: uid.clone(),
                id: canonical.id.clone(),
            }),
            RequirementOutcome::Unchanged { uid, id } => outcome.unchanged.push(UnchangedElement {
                kind: EntityKind::Requirement,
                uid: uid.clone(),
                id: id.clone(),
            }),
            RequirementOutcome::Updated { uid, canonical, .. } => {
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
                outcome.created.push(CreatedElement {
                    kind: EntityKind::Feature,
                    uid: uid.clone(),
                    id: canonical.id.clone(),
                });
                for behavior in behaviors {
                    outcome.created.push(CreatedElement {
                        kind: EntityKind::Behavior,
                        uid: behavior.uid.clone(),
                        id: behavior.canonical.id.clone(),
                    });
                    for scenario in &behavior.scenarios {
                        outcome.created.push(CreatedElement {
                            kind: EntityKind::Scenario,
                            uid: scenario.uid.clone(),
                            id: scenario.canonical.id.clone(),
                        });
                    }
                }
            }
            FeatureOutcome::Unchanged { uid, id } => outcome.unchanged.push(UnchangedElement {
                kind: EntityKind::Feature,
                uid: uid.clone(),
                id: id.clone(),
            }),
            FeatureOutcome::Updated { uid, canonical, .. } => {
                outcome.updated.push(UpdatedElement {
                    kind: EntityKind::Feature,
                    uid: uid.clone(),
                    id: canonical.id.clone(),
                });
            }
        }
    }

    for scenario in &plan.scenario_updates {
        match scenario {
            ScenarioOutcome::Unchanged { uid, id } => outcome.unchanged.push(UnchangedElement {
                kind: EntityKind::Scenario,
                uid: uid.clone(),
                id: id.clone(),
            }),
            ScenarioOutcome::Updated { uid, canonical, .. } => {
                outcome.updated.push(UpdatedElement {
                    kind: EntityKind::Scenario,
                    uid: uid.clone(),
                    id: canonical.id.clone(),
                });
            }
        }
    }

    outcome
}

fn commit_plan(root: &Path, plan: &Plan) -> io::Result<ReconcileOutcome> {
    let recorded_at = iso8601_utc_now();
    let mut batch_events = Vec::new();
    let mut files = Vec::new();
    let mut direct_writes: Vec<(PathBuf, String)> = Vec::new();
    let outcome = plan_outcome(plan);

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
            }
            RequirementOutcome::Unchanged { .. } => {}
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
                    }
                }
            }
            FeatureOutcome::Unchanged { .. } => {}
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
            }
        }
    }

    // Scenario reparent/content-patch (ADR 0027 §3): no `IdentityMutation`
    // event exists for this — `feature`/`behavior` fields are content, not
    // identity-tracked lifecycle, and `plan_scenario_reparents` already
    // refuses an accompanying `id` change (the one Scenario edit that
    // *would* need an event) — so these never join `batch_events`, only
    // `scenario_moves`/`direct_writes`.
    let mut scenario_moves: Vec<(PathBuf, PathBuf, String)> = Vec::new();
    for scenario in &plan.scenario_updates {
        match scenario {
            ScenarioOutcome::Unchanged { .. } => {}
            ScenarioOutcome::Updated {
                canonical,
                existing_path,
                new_path,
                ..
            } => {
                let contents = knowledge::serialize_scenario(canonical);
                if new_path == existing_path {
                    direct_writes.push((existing_path.clone(), contents));
                } else {
                    scenario_moves.push((existing_path.clone(), new_path.clone(), contents));
                }
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

    for (from, to, contents) in scenario_moves {
        // Move first, so at every instant exactly one file represents this
        // Scenario (never both old and new, never neither) — then write
        // its full new content at the destination. A crash between these
        // two steps leaves the file at `to` with stale (pre-patch)
        // content; re-running the same reconcile finds it already moved
        // (`new_path == existing_path` on the next plan) and simply
        // finishes the content write, so this is self-correcting on retry
        // without needing its own staging/batch machinery.
        crate::fs_safety::rename_no_follow(root, &from, &to)?;
        crate::fs_safety::replace_file(root, &to, contents.as_bytes())?;
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

    /// ADR 0027 §5: replacing a UID-selected Feature's `contributes_to`
    /// expresses both addition and removal of Requirement relationships
    /// without a separate command — this exercises both directions in one
    /// patch (drop `req_a`, add `req_c`, keep `req_b`).
    #[test]
    fn uid_selected_contributes_to_replacement_adds_and_removes_requirement_relationships() {
        let dir = init_project();
        let setup_yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: req_a
    id: req-a
    source: native
    label: Requirement A
    axis: []
  - key: req_b
    id: req-b
    source: native
    label: Requirement B
    axis: []
  - key: req_c
    id: req-c
    source: native
    label: Requirement C
    axis: []

features:
  - key: feature_todo
    id: todo-management
    contributes_to: [req_a, req_b]
    label: TODO management
    axis: []
";
        let doc = parse_intent(setup_yaml).unwrap();
        let created = reconcile_creation(dir.path(), &doc).unwrap();
        let requirement_uid = |id: &str| -> String {
            created
                .created
                .iter()
                .find(|e| e.kind == EntityKind::Requirement && e.id == id)
                .unwrap()
                .uid
                .clone()
        };
        let uid_a = requirement_uid("req-a");
        let uid_b = requirement_uid("req-b");
        let uid_c = requirement_uid("req-c");
        let feature_uid = created
            .created
            .iter()
            .find(|e| e.kind == EntityKind::Feature)
            .unwrap()
            .uid
            .clone();

        let patch_yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: {feature_uid}
    contributes_to: [{uid_b}, {uid_c}]
"
        );
        let patch_doc = parse_intent(&patch_yaml).unwrap();
        let outcome = reconcile_creation(dir.path(), &patch_doc).unwrap();
        assert_eq!(outcome.updated.len(), 1);

        let feature_content = std::fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/features/todo-management/feature.yml"),
        )
        .unwrap();
        let feature = knowledge::parse_feature(&feature_content).unwrap();
        assert_eq!(feature.requirement_uids, vec![uid_b.clone(), uid_c.clone()]);
        assert!(!feature.requirement_uids.contains(&uid_a));
    }

    /// ADR 0027 §5: omitting `contributes_to` on a UID-selected patch keeps
    /// the current `requirement_uids` untouched; explicitly setting it to
    /// `[]` clears every relationship. These are typed as `Option<Vec<_>>`
    /// (omitted vs. explicit-empty), so both are distinguishable — unlike
    /// `label`/`description`'s known `Option<String>` limitation.
    #[test]
    fn uid_selected_patch_omitting_contributes_to_keeps_it_and_explicit_empty_clears_it() {
        let dir = init_project();
        let setup_yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: req_a
    id: req-a
    source: native
    label: Requirement A
    axis: []

features:
  - key: feature_todo
    id: todo-management
    contributes_to: [req_a]
    label: TODO management
    axis: []
";
        let doc = parse_intent(setup_yaml).unwrap();
        let created = reconcile_creation(dir.path(), &doc).unwrap();
        let feature_uid = created
            .created
            .iter()
            .find(|e| e.kind == EntityKind::Feature)
            .unwrap()
            .uid
            .clone();

        // Omitting contributes_to while patching an unrelated field keeps
        // the existing relationship.
        let keep_yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: {feature_uid}
    label: TODO management (renamed)
"
        );
        reconcile_creation(dir.path(), &parse_intent(&keep_yaml).unwrap()).unwrap();
        let content = std::fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/features/todo-management/feature.yml"),
        )
        .unwrap();
        assert_eq!(
            knowledge::parse_feature(&content)
                .unwrap()
                .requirement_uids
                .len(),
            1
        );

        // Explicit empty list clears every relationship.
        let clear_yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: {feature_uid}
    contributes_to: []
"
        );
        reconcile_creation(dir.path(), &parse_intent(&clear_yaml).unwrap()).unwrap();
        let content = std::fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/features/todo-management/feature.yml"),
        )
        .unwrap();
        assert!(
            knowledge::parse_feature(&content)
                .unwrap()
                .requirement_uids
                .is_empty()
        );
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

    /// End-to-end regression test for the reviewer-flagged corruption risk:
    /// rename a Requirement away from id "todo" (its file stays at the
    /// "todo" directory, per `push_renamed`'s design, matching
    /// `feature_ops::write_id_and_uid`'s existing precedent), then try to
    /// `reconcile_creation` a brand-new Requirement that reuses "todo".
    /// The renamed entity's file must survive untouched, and the attempt
    /// must fail rather than silently overwrite it.
    #[test]
    fn reconciling_a_new_requirement_that_reuses_a_renamed_away_id_does_not_corrupt_the_renamed_file()
     {
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
        let renamed_uid = created.created[0].uid.clone();

        let rename_yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - uid: {renamed_uid}
    id: task
"
        );
        let rename_doc = parse_intent(&rename_yaml).unwrap();
        reconcile_creation(dir.path(), &rename_doc).unwrap();

        let reuse_yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - id: todo
    source: native
    label: A brand new, unrelated TODO requirement
    axis: []
";
        let reuse_doc = parse_intent(reuse_yaml).unwrap();
        let err = reconcile_creation(dir.path(), &reuse_doc).unwrap_err();
        assert!(matches!(err, ReconcileError::Diagnostics(_)));

        let content = std::fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/requirements/todo/requirement.yml"),
        )
        .unwrap();
        assert!(
            content.contains("id: task"),
            "the renamed entity's file must survive untouched, got: {content}"
        );
        assert!(content.contains(&renamed_uid));

        let events = registry::load_events_from_working_tree(
            dir.path(),
            EntityKind::Requirement,
            &renamed_uid,
        )
        .unwrap();
        assert_eq!(
            events.len(),
            2,
            "no spurious Issued event for a second entity sharing the same uid"
        );
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

    /// End-to-end regression test for ADR 0027 §3's explicit Scenario
    /// reparent: creates a Feature with two Behaviors and a Scenario under
    /// the first, then `reconcile_creation`s a reparent into the second.
    /// The physical file must actually move (old path gone, new path has
    /// the content, `behavior:` field updated) — a leftover at the old
    /// path would mean this reused the same buggy "leave the old file
    /// where it was" shortcut the reviewer already flagged once for
    /// Requirement/Feature rename.
    #[test]
    fn reconcile_creation_physically_moves_a_reparented_scenarios_file() {
        let dir = init_project();
        let create_yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - id: todo-management
    label: TODO management
    axis: []
    behaviors:
      - id: capture
        description: Capture a TODO.
        scenarios:
          - id: empty-title
            description: An empty title cannot be added
            phases:
              - steps:
                  - action: Attempt to add an empty title
                results:
                  - No TODO is added
      - id: review
        description: Review a TODO.
";
        let doc = parse_intent(create_yaml).unwrap();
        let created = reconcile_creation(dir.path(), &doc).unwrap();
        let feature_uid = created
            .created
            .iter()
            .find(|e| e.kind == EntityKind::Feature)
            .unwrap()
            .uid
            .clone();
        let review_uid = created
            .created
            .iter()
            .find(|e| e.kind == EntityKind::Behavior && e.id == "review")
            .unwrap()
            .uid
            .clone();
        let scenario_uid = created
            .created
            .iter()
            .find(|e| e.kind == EntityKind::Scenario)
            .unwrap()
            .uid
            .clone();

        let old_path = dir.path().join(
            ".markharness/knowledge/features/todo-management/capture/empty-title/scenario.yml",
        );
        let new_path = dir.path().join(
            ".markharness/knowledge/features/todo-management/review/empty-title/scenario.yml",
        );
        assert!(old_path.is_file());
        assert!(!new_path.is_file());

        let reparent_yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: {feature_uid}
    behaviors:
      - uid: {review_uid}
        scenarios:
          - uid: {scenario_uid}
"
        );
        let outcome =
            reconcile_creation(dir.path(), &parse_intent(&reparent_yaml).unwrap()).unwrap();
        assert_eq!(outcome.updated.len(), 1);

        assert!(
            !old_path.is_file(),
            "the old file must be gone after reparent"
        );
        assert!(new_path.is_file(), "the new file must exist after reparent");
        let content = std::fs::read_to_string(&new_path).unwrap();
        let scenario = knowledge::parse_scenario(&content).unwrap();
        assert_eq!(scenario.behavior, "review");
        assert_eq!(scenario.uid, Some(scenario_uid));
    }

    /// ADR 0027 §6: `--check` reports exactly what a real run's outcome
    /// would be (same created/updated/unchanged classification, since both
    /// go through [`plan_outcome`]) but performs no writes at all — no
    /// identity events, no Knowledge files, nothing under
    /// `.identity-staging/`.
    #[test]
    fn check_creation_reports_the_same_preview_as_a_real_run_but_writes_nothing() {
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

        let preview = check_creation(dir.path(), &doc).unwrap();
        assert_eq!(preview.created.len(), 1);
        assert_eq!(preview.created[0].id, "todo");
        assert!(
            !dir.path()
                .join(".markharness/knowledge/requirements/todo/requirement.yml")
                .is_file(),
            "--check must not write the Requirement file"
        );
        assert!(
            !dir.path().join(".markharness/identity-events").exists(),
            "--check must not write any identity event"
        );

        let real = reconcile_creation(dir.path(), &doc).unwrap();
        // UIDs differ (each build mints its own for a "New" outcome), but
        // the shape of the preview must match the real result exactly.
        assert_eq!(preview.created.len(), real.created.len());
        assert_eq!(preview.created[0].kind, real.created[0].kind);
        assert_eq!(preview.created[0].id, real.created[0].id);
        assert_eq!(preview.updated, real.updated);
        assert_eq!(preview.unchanged, real.unchanged);
    }

    /// Re-running `--check` against a repository that already matches the
    /// Intent reports the same `unchanged` classification a real re-run
    /// would (ADR 0027 §3's content-comparison rule), not `created`.
    #[test]
    fn check_creation_on_an_already_reconciled_repository_reports_unchanged() {
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

        let preview = check_creation(dir.path(), &doc).unwrap();
        assert!(preview.created.is_empty());
        assert!(preview.updated.is_empty());
        assert_eq!(preview.unchanged.len(), 1);
    }

    /// ADR 0027 §6 `stale_plan`: a `Plan` built with [`build_plan`] and
    /// then committed later (as the exposed but TOCTOU-prone
    /// `execute_creation_plan` allows) must not blindly overwrite state
    /// that changed in between — here, a second `reconcile_creation` runs
    /// and actually creates the same Requirement before the stale plan's
    /// commit is attempted.
    #[test]
    fn execute_creation_plan_refuses_a_plan_that_has_gone_stale() {
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
        let stale_plan = build_plan(dir.path(), &doc).unwrap();

        // The repository changes after the plan was built but before it is
        // committed — exactly the gap `execute_creation_plan`'s own doc
        // comment warns callers not to reopen.
        reconcile_creation(dir.path(), &doc).unwrap();

        let err = execute_creation_plan(dir.path(), &stale_plan).unwrap_err();
        match err {
            ExecuteError::Diagnostics(diagnostics) => {
                assert_eq!(diagnostics.len(), 1);
                assert_eq!(
                    diagnostics[0].code,
                    super::super::diagnostics::DiagnosticCode::StalePlan
                );
            }
            other => panic!("expected Diagnostics(stale_plan), got {other:?}"),
        }

        // The stale plan's own (would-be duplicate) uid was never
        // committed at all — only the second, non-stale call's Requirement
        // exists.
        let events = registry::load_events_from_working_tree(
            dir.path(),
            EntityKind::Requirement,
            stale_plan.requirements[0].uid(),
        )
        .unwrap();
        assert!(
            events.is_empty(),
            "the stale plan's own uid must never have been committed"
        );
        let content = std::fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/requirements/todo/requirement.yml"),
        )
        .unwrap();
        assert!(!content.contains(stale_plan.requirements[0].uid()));
    }

    /// Crash-recovery, pre-commit: `begin_batch_with_payload` stages the
    /// intent (writes `.identity-staging/<op>/intent.yml`) but the process
    /// dies before `commit_batch` ever writes the identity event to its
    /// final location. On restart, `recover_incomplete_operations` must
    /// find `is_committed` false and discard the staging entry rather than
    /// roll it forward — the operation never truly happened, so nothing
    /// (not the identity event, not the Requirement file) should exist
    /// afterward, and staging must be cleaned up so a fresh reconcile of
    /// the same Intent can proceed normally.
    #[test]
    fn a_subsequent_command_discards_a_staged_but_never_committed_batch() {
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
        // Deliberately no `commit_batch` call: this is the crash point,
        // before the event ever reached its final location.
        recovery::begin_batch_with_payload(
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

        assert!(
            !dir.path()
                .join(".markharness/knowledge/requirements/todo/requirement.yml")
                .is_file()
        );

        recovery::run_startup_recovery(dir.path(), |intent| {
            feature_ops::roll_forward(dir.path(), intent)
        })
        .unwrap();

        assert!(
            !dir.path()
                .join(".markharness/knowledge/requirements/todo/requirement.yml")
                .is_file(),
            "a never-committed batch must not have its pending file written"
        );
        assert!(
            registry::load_events_from_working_tree(
                dir.path(),
                EntityKind::Requirement,
                &requirement_uid,
            )
            .unwrap()
            .is_empty(),
            "a never-committed batch must not have its identity event recorded"
        );

        // Staging is clean, so reconciling the same Intent for real now
        // proceeds exactly as if nothing had happened.
        let doc = parse_intent(
            "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - id: todo
    source: native
    label: TODO management
    axis: []
",
        )
        .unwrap();
        let outcome = reconcile_creation(dir.path(), &doc).unwrap();
        assert_eq!(outcome.created.len(), 1);
    }

    /// Reparenting alongside an unrelated new Requirement in the same
    /// Intent must not corrupt either: the reparent is not part of the
    /// identity-event batch (no `IdentityMutation` exists for a Scenario's
    /// `behavior:` change), so it must still complete correctly even when
    /// a real batch commit also happens in the same `reconcile_creation`
    /// call.
    #[test]
    fn reparenting_a_scenario_alongside_an_unrelated_new_requirement_in_one_call() {
        let dir = init_project();
        let create_yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - id: todo-management
    label: TODO management
    axis: []
    behaviors:
      - id: capture
        description: Capture a TODO.
        scenarios:
          - id: empty-title
            description: An empty title cannot be added
            phases:
              - steps:
                  - action: Attempt to add an empty title
                results:
                  - No TODO is added
      - id: review
        description: Review a TODO.
";
        let created = reconcile_creation(dir.path(), &parse_intent(create_yaml).unwrap()).unwrap();
        let feature_uid = created
            .created
            .iter()
            .find(|e| e.kind == EntityKind::Feature)
            .unwrap()
            .uid
            .clone();
        let review_uid = created
            .created
            .iter()
            .find(|e| e.kind == EntityKind::Behavior && e.id == "review")
            .unwrap()
            .uid
            .clone();
        let scenario_uid = created
            .created
            .iter()
            .find(|e| e.kind == EntityKind::Scenario)
            .unwrap()
            .uid
            .clone();

        let combined_yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - id: unrelated
    source: native
    label: Unrelated requirement
    axis: []

features:
  - uid: {feature_uid}
    behaviors:
      - uid: {review_uid}
        scenarios:
          - uid: {scenario_uid}
"
        );
        let outcome =
            reconcile_creation(dir.path(), &parse_intent(&combined_yaml).unwrap()).unwrap();
        assert_eq!(outcome.created.len(), 1, "the unrelated Requirement");
        assert_eq!(outcome.updated.len(), 1, "the reparented Scenario");

        let new_path = dir.path().join(
            ".markharness/knowledge/features/todo-management/review/empty-title/scenario.yml",
        );
        assert!(new_path.is_file());
    }
}
