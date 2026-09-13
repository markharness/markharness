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
use std::path::Path;

use crate::identity::{
    EntityKind, IdentityEvent, IdentityMutation, feature_ops, lock, recovery, registry,
};
use crate::knowledge;
use crate::time::iso8601_utc_now;

use super::diagnostics::{Diagnostic, DiagnosticCode};
use super::intent::IntentDocument;
use super::paths::{behavior_path, feature_path, requirement_path, scenario_path};
use super::plan::{
    BackReferenceFixup, BehaviorOutcome, FeatureOutcome, Plan, PlanError, RequirementOutcome,
    ScenarioOutcome, build_plan, state_fingerprint,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedElement {
    pub kind: EntityKind,
    pub uid: String,
    pub id: String,
    /// Root-relative, forward-slash-normalized path of the canonical
    /// Knowledge file this element was written to (ADR 0027 §7: the result
    /// reports each element's changed path, not just its identity).
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatedElement {
    pub kind: EntityKind,
    pub uid: String,
    pub id: String,
    /// Where this element's file lives *after* the update.
    pub path: String,
    /// Where it lived before, when the update moved it (a reparented
    /// Scenario). `None` when the file stayed put — including a
    /// Requirement/Feature rename, which by design rewrites `id:` without
    /// moving the directory.
    pub previous_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnchangedElement {
    pub kind: EntityKind,
    pub uid: String,
    pub id: String,
    pub path: String,
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
    OperationInProgress,
    /// [`check_creation`] only: a previous operation's staging entry is
    /// still on disk, and resolving it (discard or roll-forward) is itself
    /// a write `--check` must never perform. Run a real (non-`--check`)
    /// `knowledge reconcile`, or any other identity command — all of them
    /// run startup recovery — to complete it, then retry.
    RecoveryPending,
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
///
/// Deliberately does *not* call [`recovery::run_startup_recovery`]:
/// finishing a leftover staging entry from an earlier crash — discarding
/// it or rolling it forward — writes real repository state (Knowledge
/// files, identity events), which `--check` must never do. Instead this
/// acquires the lock directly and only *peeks* at whether recovery is
/// pending ([`recovery::has_incomplete_operations`]); if so, it refuses
/// with [`ReconcileError::RecoveryPending`] rather than resolving it.
pub fn check_creation(
    root: &Path,
    doc: &IntentDocument,
) -> Result<ReconcileOutcome, ReconcileError> {
    let held_lock = match lock::IdentityLock::acquire(root) {
        Ok(held_lock) => held_lock,
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
            return Err(ReconcileError::OperationInProgress);
        }
        Err(e) => return Err(ReconcileError::Io(e)),
    };
    let outcome = (|| {
        if recovery::has_incomplete_operations(root)? {
            return Err(ReconcileError::RecoveryPending);
        }
        let plan = build_plan(root, doc)?;
        Ok(plan_outcome(root, &plan))
    })();
    held_lock.release()?;
    outcome
}

/// Re-reads the current [`state_fingerprint`] and compares it to the one
/// `plan` was built against (ADR 0027 §6 `stale_plan`). Passes a `plan`
/// carrying no fingerprint through unchecked, for the reason
/// `Plan::state_fingerprint` documents.
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
        "the repository's Knowledge or Axis registry changed since this plan was built; re-run reconcile to build a fresh plan before committing",
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
fn plan_outcome(root: &Path, plan: &Plan) -> ReconcileOutcome {
    let mut outcome = ReconcileOutcome::default();
    let rel = |path: &Path| relative_path_string(root, path);

    for req in &plan.requirements {
        match req {
            RequirementOutcome::New { uid, canonical } => outcome.created.push(CreatedElement {
                kind: EntityKind::Requirement,
                uid: uid.clone(),
                id: canonical.id.clone(),
                path: rel(&requirement_path(root, &canonical.id)),
            }),
            RequirementOutcome::Unchanged { uid, id, path } => {
                outcome.unchanged.push(UnchangedElement {
                    kind: EntityKind::Requirement,
                    uid: uid.clone(),
                    id: id.clone(),
                    path: rel(path),
                });
            }
            RequirementOutcome::Updated {
                uid,
                canonical,
                existing_path,
                ..
            } => {
                outcome.updated.push(UpdatedElement {
                    kind: EntityKind::Requirement,
                    uid: uid.clone(),
                    id: canonical.id.clone(),
                    path: rel(existing_path),
                    previous_path: None,
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
                    path: rel(&feature_path(root, &canonical.id)),
                });
                for behavior in behaviors {
                    outcome.created.push(CreatedElement {
                        kind: EntityKind::Behavior,
                        uid: behavior.uid.clone(),
                        id: behavior.canonical.id.clone(),
                        path: rel(&behavior_path(root, &canonical.id, &behavior.canonical.id)),
                    });
                    for scenario in &behavior.scenarios {
                        outcome.created.push(CreatedElement {
                            kind: EntityKind::Scenario,
                            uid: scenario.uid.clone(),
                            id: scenario.canonical.id.clone(),
                            path: rel(&scenario_path(
                                root,
                                &canonical.id,
                                &behavior.canonical.id,
                                &scenario.canonical.id,
                            )),
                        });
                    }
                }
            }
            FeatureOutcome::Unchanged { uid, id, path } => {
                outcome.unchanged.push(UnchangedElement {
                    kind: EntityKind::Feature,
                    uid: uid.clone(),
                    id: id.clone(),
                    path: rel(path),
                });
            }
            FeatureOutcome::Updated {
                uid,
                canonical,
                existing_path,
                ..
            } => {
                outcome.updated.push(UpdatedElement {
                    kind: EntityKind::Feature,
                    uid: uid.clone(),
                    id: canonical.id.clone(),
                    path: rel(existing_path),
                    previous_path: None,
                });
            }
        }
    }

    for new in &plan.new_behaviors {
        let behavior_dir = new.parent_dir.join(&new.behavior.canonical.id);
        outcome.created.push(CreatedElement {
            kind: EntityKind::Behavior,
            uid: new.behavior.uid.clone(),
            id: new.behavior.canonical.id.clone(),
            path: rel(&behavior_dir.join("behavior.yml")),
        });
        for scenario in &new.behavior.scenarios {
            outcome.created.push(CreatedElement {
                kind: EntityKind::Scenario,
                uid: scenario.uid.clone(),
                id: scenario.canonical.id.clone(),
                path: rel(&behavior_dir
                    .join(&scenario.canonical.id)
                    .join("scenario.yml")),
            });
        }
    }

    for new in &plan.new_scenarios {
        outcome.created.push(CreatedElement {
            kind: EntityKind::Scenario,
            uid: new.scenario.uid.clone(),
            id: new.scenario.canonical.id.clone(),
            path: rel(&new
                .parent_dir
                .join(&new.scenario.canonical.id)
                .join("scenario.yml")),
        });
    }

    for behavior in &plan.behavior_updates {
        match behavior {
            BehaviorOutcome::Unchanged { uid, id, path } => {
                outcome.unchanged.push(UnchangedElement {
                    kind: EntityKind::Behavior,
                    uid: uid.clone(),
                    id: id.clone(),
                    path: rel(path),
                });
            }
            BehaviorOutcome::Updated {
                uid,
                canonical,
                existing_path,
                ..
            } => {
                outcome.updated.push(UpdatedElement {
                    kind: EntityKind::Behavior,
                    uid: uid.clone(),
                    id: canonical.id.clone(),
                    path: rel(existing_path),
                    previous_path: None,
                });
            }
        }
    }

    for scenario in &plan.scenario_updates {
        match scenario {
            ScenarioOutcome::Unchanged { uid, id, path } => {
                outcome.unchanged.push(UnchangedElement {
                    kind: EntityKind::Scenario,
                    uid: uid.clone(),
                    id: id.clone(),
                    path: rel(path),
                });
            }
            ScenarioOutcome::Updated {
                uid,
                canonical,
                existing_path,
                new_path,
                ..
            } => {
                outcome.updated.push(UpdatedElement {
                    kind: EntityKind::Scenario,
                    uid: uid.clone(),
                    id: canonical.id.clone(),
                    path: rel(new_path),
                    previous_path: (new_path != existing_path).then(|| rel(existing_path)),
                });
            }
        }
    }

    for fixup in &plan.back_reference_fixups {
        let (kind, uid, id, path) = match fixup {
            BackReferenceFixup::Behavior {
                uid,
                canonical,
                path,
            } => (EntityKind::Behavior, uid, canonical.id.clone(), path),
            BackReferenceFixup::Scenario {
                uid,
                canonical,
                path,
            } => (EntityKind::Scenario, uid, canonical.id.clone(), path),
        };
        outcome.updated.push(UpdatedElement {
            kind,
            uid: uid.clone().unwrap_or_default(),
            id,
            path: rel(path),
            previous_path: None,
        });
    }

    outcome
}

/// Commits `plan` as one crash-recoverable transaction (ADR 0027 §1 step
/// 7). Every canonical Knowledge write it performs — brand-new elements,
/// renames, content-only patches and Scenario reparents alike — is
/// recorded in the recovery payload *before* the commit point and carried
/// out by `feature_ops::roll_forward` after it, so a crash anywhere
/// converges: before that point on the old state, after it on the new one.
/// Writing any part of it directly here instead (as an earlier version did
/// for content-only patches and reparents) leaves a partially updated tree
/// that no later command knows to finish.
fn commit_plan(root: &Path, plan: &Plan) -> io::Result<ReconcileOutcome> {
    let recorded_at = iso8601_utc_now();
    let mut batch_events = Vec::new();
    let mut files = Vec::new();
    let outcome = plan_outcome(root, plan);

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
                // The rewritten file is identical either way; only an `id`
                // change additionally needs a `Renamed` event, since
                // content fields are not part of identity history.
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
                }
                files.push(pending_file(
                    root,
                    existing_path,
                    knowledge::serialize_requirement(canonical),
                ));
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
                }
                files.push(pending_file(
                    root,
                    existing_path,
                    knowledge::serialize_feature(canonical),
                ));
            }
        }
    }

    // Brand-new children of parents that already exist. Their paths come
    // from the parent's own directory rather than from its display id: a
    // renamed parent keeps its directory, so an id-derived path would
    // scatter the new child away from its siblings.
    for new in &plan.new_behaviors {
        let behavior_dir = new.parent_dir.join(&new.behavior.canonical.id);
        push_issued(
            &mut batch_events,
            EntityKind::Behavior,
            &new.behavior.uid,
            &new.behavior.canonical.id,
            &recorded_at,
        )?;
        files.push(pending_file(
            root,
            &behavior_dir.join("behavior.yml"),
            knowledge::serialize_behavior(&new.behavior.canonical),
        ));
        for scenario in &new.behavior.scenarios {
            push_issued(
                &mut batch_events,
                EntityKind::Scenario,
                &scenario.uid,
                &scenario.canonical.id,
                &recorded_at,
            )?;
            files.push(pending_file(
                root,
                &behavior_dir
                    .join(&scenario.canonical.id)
                    .join("scenario.yml"),
                knowledge::serialize_scenario(&scenario.canonical),
            ));
        }
    }

    for new in &plan.new_scenarios {
        push_issued(
            &mut batch_events,
            EntityKind::Scenario,
            &new.scenario.uid,
            &new.scenario.canonical.id,
            &recorded_at,
        )?;
        files.push(pending_file(
            root,
            &new.parent_dir
                .join(&new.scenario.canonical.id)
                .join("scenario.yml"),
            knowledge::serialize_scenario(&new.scenario.canonical),
        ));
    }

    // A Behavior patch and a Scenario reparent/patch (ADR 0027 §3, §5)
    // carry no `IdentityMutation`: `feature`/`behavior` and every field
    // they touch are content, not identity-tracked lifecycle, and
    // `plan_existing_behaviors` already refuses the one edit that *would*
    // need an event — an `id` change. They still ride the same payload, so
    // they commit and recover together with everything else.
    for behavior in &plan.behavior_updates {
        match behavior {
            BehaviorOutcome::Unchanged { .. } => {}
            BehaviorOutcome::Updated {
                uid,
                before_id,
                canonical,
                existing_path,
            } => {
                if &canonical.id != before_id {
                    push_renamed(
                        root,
                        &mut batch_events,
                        EntityKind::Behavior,
                        uid,
                        before_id,
                        &canonical.id,
                        &recorded_at,
                    )?;
                }
                files.push(pending_file(
                    root,
                    existing_path,
                    knowledge::serialize_behavior(canonical),
                ));
            }
        }
    }

    for fixup in &plan.back_reference_fixups {
        let (path, contents) = match fixup {
            BackReferenceFixup::Behavior {
                canonical, path, ..
            } => (path, knowledge::serialize_behavior(canonical)),
            BackReferenceFixup::Scenario {
                canonical, path, ..
            } => (path, knowledge::serialize_scenario(canonical)),
        };
        files.push(pending_file(root, path, contents));
    }

    let mut moves = Vec::new();
    for scenario in &plan.scenario_updates {
        match scenario {
            ScenarioOutcome::Unchanged { .. } => {}
            ScenarioOutcome::Updated {
                uid,
                before_id,
                canonical,
                existing_path,
                new_path,
            } => {
                if &canonical.id != before_id {
                    push_renamed(
                        root,
                        &mut batch_events,
                        EntityKind::Scenario,
                        uid,
                        before_id,
                        &canonical.id,
                        &recorded_at,
                    )?;
                }
                let contents = knowledge::serialize_scenario(canonical);
                if new_path == existing_path {
                    files.push(pending_file(root, new_path, contents));
                } else {
                    moves.push(recovery::PendingKnowledgeMove {
                        from_relative_path: relative_path_string(root, existing_path),
                        to_relative_path: relative_path_string(root, new_path),
                        contents,
                    });
                }
            }
        }
    }

    if batch_events.is_empty() && files.is_empty() && moves.is_empty() {
        return Ok(outcome);
    }
    let intent = recovery::begin_batch_with_payload(
        root,
        batch_events,
        Some(recovery::IntentPayload::KnowledgeReconcile { files, moves }),
    )?;
    recovery::commit_batch(root, &intent)?;
    feature_ops::roll_forward(root, &intent)?;
    recovery::finish(root, &intent)?;

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_reconcile::intent::parse_intent;
    use crate::knowledge_reconcile::plan::build_plan;

    /// A project fixture shaped like a real one: `build_plan` resolves
    /// `axis` against the registry, so the Axis these tests use has to be
    /// registered here the way `axes add` would have registered it.
    fn init_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".markharness/knowledge")).unwrap();
        std::fs::create_dir_all(dir.path().join(".markharness/axes")).unwrap();
        std::fs::write(
            dir.path().join(".markharness/axes/functional.yml"),
            "id: functional
label: Functional
",
        )
        .unwrap();
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

    /// A UID-selected Behavior's own fields must actually reach disk (ADR
    /// 0027 §5). An earlier version walked the Behavior only as reparent
    /// context and silently discarded `label`/`axis`/`description`/
    /// `procedures` while still reporting success.
    #[test]
    fn a_uid_selected_behaviors_patched_fields_reach_disk_without_an_identity_event() {
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
";
        let created = reconcile_creation(dir.path(), &parse_intent(create_yaml).unwrap()).unwrap();
        let uid_of = |kind: EntityKind| {
            created
                .created
                .iter()
                .find(|e| e.kind == kind)
                .unwrap()
                .uid
                .clone()
        };
        let feature_uid = uid_of(EntityKind::Feature);
        let behavior_uid = uid_of(EntityKind::Behavior);

        let patch_yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: {feature_uid}
    behaviors:
      - uid: {behavior_uid}
        label: Capture (renamed label)
        description: A new description.
        procedures:
          - name: validate_title
            steps: [Check the title is non-empty]
"
        );
        let outcome = reconcile_creation(dir.path(), &parse_intent(&patch_yaml).unwrap()).unwrap();
        assert_eq!(outcome.updated.len(), 1);
        assert_eq!(outcome.updated[0].kind, EntityKind::Behavior);

        let behavior = knowledge::parse_behavior(
            &std::fs::read_to_string(
                dir.path()
                    .join(".markharness/knowledge/features/todo-management/capture/behavior.yml"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(behavior.label, "Capture (renamed label)");
        assert_eq!(behavior.description, "A new description.\n");
        assert_eq!(
            behavior.procedures.get("validate_title").unwrap().steps,
            vec!["Check the title is non-empty".to_string()]
        );
        assert_eq!(behavior.uid, Some(behavior_uid.clone()));

        let events = registry::load_events_from_working_tree(
            dir.path(),
            EntityKind::Behavior,
            &behavior_uid,
        )
        .unwrap();
        assert_eq!(
            events.len(),
            1,
            "a content-only Behavior patch adds no identity event"
        );
    }

    /// ADR 0027 §6: the same Intent against the same state must produce
    /// the same plan. A `description` is the one field where that nearly
    /// broke — the serializer always writes it as a literal block scalar,
    /// so it reads back newline-terminated while the Intent's own value is
    /// not — which would make every re-run report a change and rewrite the
    /// file forever. Covers both a re-run of a creation Intent and a
    /// re-run of a UID-selected patch.
    #[test]
    fn re_running_an_intent_that_sets_descriptions_settles_on_unchanged() {
        let dir = init_project();
        // No nested `behaviors` here: re-running a creation Intent that
        // has them is refused outright (a no-UID Feature cannot carry
        // `behaviors` yet), so this covers the elements whose re-run
        // comparison actually runs today.
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - id: todo
    source: native
    label: TODO management
    description: Requirements about TODOs.
    axis: []

features:
  - id: todo-management
    label: TODO management
    description: The TODO management feature.
    axis: []
";
        let doc = parse_intent(yaml).unwrap();
        let first = reconcile_creation(dir.path(), &doc).unwrap();
        assert_eq!(first.created.len(), 2);

        let second = reconcile_creation(dir.path(), &doc).unwrap();
        assert!(second.created.is_empty(), "{second:?}");
        assert!(
            second.updated.is_empty(),
            "a re-run must not keep rewriting descriptions: {second:?}"
        );
        assert_eq!(second.unchanged.len(), 2, "the Requirement and the Feature");

        // The same holds for a UID-selected patch that sets a description.
        let requirement_uid = first
            .created
            .iter()
            .find(|e| e.kind == EntityKind::Requirement)
            .unwrap()
            .uid
            .clone();
        let patch = format!(
            "format: markharness/knowledge-intent/v1\nmode: merge\n\nrequirements:\n  - uid: {requirement_uid}\n    description: A patched description.\n"
        );
        let patch_doc = parse_intent(&patch).unwrap();
        assert_eq!(
            reconcile_creation(dir.path(), &patch_doc)
                .unwrap()
                .updated
                .len(),
            1
        );
        let repeated = reconcile_creation(dir.path(), &patch_doc).unwrap();
        assert!(repeated.updated.is_empty(), "{repeated:?}");
        assert_eq!(repeated.unchanged.len(), 1);
    }

    /// Creates a Feature with one Behavior and one Scenario and returns
    /// their uids in that order.
    fn rename_fixture(dir: &tempfile::TempDir) -> (String, String, String) {
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
";
        let created = reconcile_creation(dir.path(), &parse_intent(create_yaml).unwrap()).unwrap();
        let uid_of = |kind: EntityKind| {
            created
                .created
                .iter()
                .find(|e| e.kind == kind)
                .unwrap()
                .uid
                .clone()
        };
        (
            uid_of(EntityKind::Feature),
            uid_of(EntityKind::Behavior),
            uid_of(EntityKind::Scenario),
        )
    }

    /// ADR 0027 §3: a UID-selected Behavior whose display id changes is an
    /// explicit rename — it records an `IdentityMutation::Renamed` event
    /// like a Requirement or Feature does, and its child Scenario's
    /// `behavior:` back-reference follows. Re-running the same Intent then
    /// settles on `unchanged` rather than renaming again.
    #[test]
    fn renaming_a_behavior_records_an_event_updates_children_and_settles_on_rerun() {
        let dir = init_project();
        let (feature_uid, behavior_uid, _) = rename_fixture(&dir);

        let rename = format!(
            "format: markharness/knowledge-intent/v1\nmode: merge\n\nfeatures:\n  - uid: {feature_uid}\n    behaviors:\n      - uid: {behavior_uid}\n        id: recorded\n"
        );
        let doc = parse_intent(&rename).unwrap();
        let outcome = reconcile_creation(dir.path(), &doc).unwrap();
        assert_eq!(outcome.updated.len(), 2, "the Behavior and its Scenario");

        let events = registry::load_events_from_working_tree(
            dir.path(),
            EntityKind::Behavior,
            &behavior_uid,
        )
        .unwrap();
        assert_eq!(events.len(), 2);
        assert!(events.iter().any(|e| matches!(
            &e.mutation,
            IdentityMutation::Renamed { from_id, to_id } if from_id == "capture" && to_id == "recorded"
        )));

        // The Behavior's own file stays put, like a renamed Feature's, but
        // now declares the new id...
        let behavior = knowledge::parse_behavior(
            &std::fs::read_to_string(
                dir.path()
                    .join(".markharness/knowledge/features/todo-management/capture/behavior.yml"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(behavior.id, "recorded");
        // ...and its child no longer points at an id nothing carries.
        let scenario = knowledge::parse_scenario(
            &std::fs::read_to_string(dir.path().join(
                ".markharness/knowledge/features/todo-management/capture/empty-title/scenario.yml",
            ))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(scenario.behavior, "recorded");

        let repeated = reconcile_creation(dir.path(), &doc).unwrap();
        assert!(repeated.updated.is_empty(), "{repeated:?}");
        assert_eq!(
            registry::load_events_from_working_tree(
                dir.path(),
                EntityKind::Behavior,
                &behavior_uid
            )
            .unwrap()
            .len(),
            2,
            "a re-run must not record a second Renamed event"
        );
    }

    /// The same contract for a Scenario, whose file additionally moves —
    /// its path is derived from its own id.
    #[test]
    fn renaming_a_scenario_records_an_event_and_moves_its_file() {
        let dir = init_project();
        let (feature_uid, behavior_uid, scenario_uid) = rename_fixture(&dir);

        let rename = format!(
            "format: markharness/knowledge-intent/v1\nmode: merge\n\nfeatures:\n  - uid: {feature_uid}\n    behaviors:\n      - uid: {behavior_uid}\n        scenarios:\n          - uid: {scenario_uid}\n            id: blank-title\n"
        );
        let doc = parse_intent(&rename).unwrap();
        let outcome = reconcile_creation(dir.path(), &doc).unwrap();
        assert_eq!(outcome.updated.len(), 1);
        assert_eq!(
            outcome.updated[0].previous_path.as_deref(),
            Some(
                ".markharness/knowledge/features/todo-management/capture/empty-title/scenario.yml"
            )
        );

        let old_path = dir.path().join(
            ".markharness/knowledge/features/todo-management/capture/empty-title/scenario.yml",
        );
        let new_path = dir.path().join(
            ".markharness/knowledge/features/todo-management/capture/blank-title/scenario.yml",
        );
        assert!(!old_path.is_file());
        let scenario =
            knowledge::parse_scenario(&std::fs::read_to_string(&new_path).unwrap()).unwrap();
        assert_eq!(scenario.id, "blank-title");
        assert_eq!(scenario.uid, Some(scenario_uid.clone()));

        let events = registry::load_events_from_working_tree(
            dir.path(),
            EntityKind::Scenario,
            &scenario_uid,
        )
        .unwrap();
        assert_eq!(events.len(), 2);
        assert!(events.iter().any(|e| matches!(
            &e.mutation,
            IdentityMutation::Renamed { from_id, to_id } if from_id == "empty-title" && to_id == "blank-title"
        )));

        let repeated = reconcile_creation(dir.path(), &doc).unwrap();
        assert!(repeated.updated.is_empty(), "{repeated:?}");
    }

    /// Regression test for a bug this rename work uncovered: renaming a
    /// Feature left every child Behavior's `feature:` pointing at an id
    /// nothing carried any more, which the very next scope check reads as
    /// a mismatch. The rename must carry its children along.
    #[test]
    fn renaming_a_feature_updates_its_behaviors_back_reference() {
        let dir = init_project();
        let (feature_uid, behavior_uid, _) = rename_fixture(&dir);

        let rename = format!(
            "format: markharness/knowledge-intent/v1\nmode: merge\n\nfeatures:\n  - uid: {feature_uid}\n    id: task-management\n"
        );
        reconcile_creation(dir.path(), &parse_intent(&rename).unwrap()).unwrap();

        let behavior = knowledge::parse_behavior(
            &std::fs::read_to_string(
                dir.path()
                    .join(".markharness/knowledge/features/todo-management/capture/behavior.yml"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(behavior.feature, "task-management");

        // Proof that it matters: the renamed Feature's Behaviors are still
        // reachable for a follow-up patch, which the stale back-reference
        // would have rejected as `conflicting_scope`.
        let patch = format!(
            "format: markharness/knowledge-intent/v1\nmode: merge\n\nfeatures:\n  - uid: {feature_uid}\n    behaviors:\n      - uid: {behavior_uid}\n        label: Still reachable\n"
        );
        let outcome = reconcile_creation(dir.path(), &parse_intent(&patch).unwrap()).unwrap();
        assert_eq!(outcome.updated.len(), 1);
    }

    /// A rename is committed through the same single transaction as every
    /// other write, so a crash after the commit point must converge on the
    /// fully renamed state — identity event, the element's own file, its
    /// children's back-references and any move, all of it.
    #[test]
    fn a_rename_interrupted_after_its_commit_point_is_rolled_forward_in_full() {
        let dir = init_project();
        let (feature_uid, behavior_uid, scenario_uid) = rename_fixture(&dir);
        let rename = format!(
            "format: markharness/knowledge-intent/v1\nmode: merge\n\nfeatures:\n  - uid: {feature_uid}\n    behaviors:\n      - uid: {behavior_uid}\n        id: recorded\n        scenarios:\n          - uid: {scenario_uid}\n            id: blank-title\n"
        );
        let plan = build_plan(dir.path(), &parse_intent(&rename).unwrap()).unwrap();

        // Drive the protocol by hand and stop right after the commit point.
        let recorded_at = crate::time::iso8601_utc_now();
        let mut batch_events = Vec::new();
        push_renamed(
            dir.path(),
            &mut batch_events,
            EntityKind::Behavior,
            &behavior_uid,
            "capture",
            "recorded",
            &recorded_at,
        )
        .unwrap();
        let BehaviorOutcome::Updated {
            canonical,
            existing_path,
            ..
        } = &plan.behavior_updates[0]
        else {
            panic!("expected an Updated Behavior");
        };
        let ScenarioOutcome::Updated {
            canonical: scenario,
            existing_path: from,
            new_path: to,
            ..
        } = &plan.scenario_updates[0]
        else {
            panic!("expected an Updated Scenario");
        };
        let intent = recovery::begin_batch_with_payload(
            dir.path(),
            batch_events,
            Some(recovery::IntentPayload::KnowledgeReconcile {
                files: vec![pending_file(
                    dir.path(),
                    existing_path,
                    knowledge::serialize_behavior(canonical),
                )],
                moves: vec![recovery::PendingKnowledgeMove {
                    from_relative_path: relative_path_string(dir.path(), from),
                    to_relative_path: relative_path_string(dir.path(), to),
                    contents: knowledge::serialize_scenario(scenario),
                }],
            }),
        )
        .unwrap();
        recovery::commit_batch(dir.path(), &intent).unwrap();
        // Crash point: the event landed, no Knowledge file has moved yet.

        recovery::run_startup_recovery(dir.path(), |intent| {
            feature_ops::roll_forward(dir.path(), intent)
        })
        .unwrap();

        let behavior = knowledge::parse_behavior(
            &std::fs::read_to_string(
                dir.path()
                    .join(".markharness/knowledge/features/todo-management/capture/behavior.yml"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(behavior.id, "recorded");
        let moved = dir.path().join(
            ".markharness/knowledge/features/todo-management/capture/blank-title/scenario.yml",
        );
        assert!(moved.is_file(), "the Scenario move must have completed");
        assert!(
            !dir.path()
                .join(
                    ".markharness/knowledge/features/todo-management/capture/empty-title/scenario.yml"
                )
                .is_file()
        );
    }

    /// The happy path of a content-only reconcile must still go through
    /// the recovery protocol and clean up after itself — a staging entry
    /// left behind would block the next `--check`, and no staging entry at
    /// all would mean the writes bypassed the protocol entirely.
    #[test]
    fn a_content_only_reconcile_commits_through_recovery_and_leaves_no_staging_entry() {
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
        let created = reconcile_creation(dir.path(), &parse_intent(yaml).unwrap()).unwrap();
        let uid = created.created[0].uid.clone();

        let patch_yaml = format!(
            "format: markharness/knowledge-intent/v1\nmode: merge\n\nrequirements:\n  - uid: {uid}\n    label: Patched\n"
        );
        reconcile_creation(dir.path(), &parse_intent(&patch_yaml).unwrap()).unwrap();

        assert_eq!(
            label_of(
                &dir.path()
                    .join(".markharness/knowledge/requirements/todo/requirement.yml")
            ),
            "Patched"
        );
        assert!(
            !recovery::has_incomplete_operations(dir.path()).unwrap(),
            "the operation must finish its own staging entry"
        );
    }

    /// Stages the payload a content-only reconcile of two Requirements
    /// produces, without any identity event — the shape whose crash
    /// behaviour the reviewer flagged, since an earlier version wrote
    /// these files directly and left a half-updated tree behind.
    fn stage_two_label_patches(
        dir: &tempfile::TempDir,
        label: &str,
    ) -> (recovery::Intent, Vec<std::path::PathBuf>) {
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - id: first
    source: native
    label: First
    axis: []
  - id: second
    source: native
    label: Second
    axis: []
";
        reconcile_creation(dir.path(), &parse_intent(yaml).unwrap()).unwrap();

        let paths: Vec<std::path::PathBuf> = ["first", "second"]
            .iter()
            .map(|id| {
                dir.path().join(format!(
                    ".markharness/knowledge/requirements/{id}/requirement.yml"
                ))
            })
            .collect();
        let files = paths
            .iter()
            .map(|path| {
                let mut requirement =
                    knowledge::parse_requirement(&std::fs::read_to_string(path).unwrap()).unwrap();
                requirement.label = Some(label.to_string());
                recovery::PendingKnowledgeFile {
                    relative_path: relative_path_string(dir.path(), path),
                    contents: knowledge::serialize_requirement(&requirement),
                }
            })
            .collect();
        let intent = recovery::begin_batch_with_payload(
            dir.path(),
            Vec::new(),
            Some(recovery::IntentPayload::KnowledgeReconcile {
                files,
                moves: Vec::new(),
            }),
        )
        .unwrap();
        (intent, paths)
    }

    fn label_of(path: &std::path::Path) -> String {
        knowledge::parse_requirement(&std::fs::read_to_string(path).unwrap())
            .unwrap()
            .label
            .unwrap()
    }

    /// Pre-commit crash for a content-only operation: the staged intent
    /// exists but its commit marker never landed, so recovery must discard
    /// it and leave *both* Requirements at their old labels — no partial
    /// application.
    #[test]
    fn a_content_only_batch_that_never_reached_its_commit_marker_is_discarded() {
        let dir = init_project();
        let (_intent, paths) = stage_two_label_patches(&dir, "Patched");
        // Deliberately no `commit_batch`: this is the crash point.

        recovery::run_startup_recovery(dir.path(), |intent| {
            feature_ops::roll_forward(dir.path(), intent)
        })
        .unwrap();

        assert_eq!(label_of(&paths[0]), "First");
        assert_eq!(label_of(&paths[1]), "Second");
        assert!(!recovery::has_incomplete_operations(dir.path()).unwrap());
    }

    /// Post-commit crash for the same content-only operation: the marker
    /// landed, so recovery must roll *every* pending file forward. An
    /// operation that issues no identity event still converges on its new
    /// state rather than being silently dropped.
    #[test]
    fn a_content_only_batch_past_its_commit_marker_is_rolled_forward_in_full() {
        let dir = init_project();
        let (intent, paths) = stage_two_label_patches(&dir, "Patched");
        recovery::commit_batch(dir.path(), &intent).unwrap();
        // Deliberately no roll_forward/finish: this is the crash point.
        assert_eq!(label_of(&paths[0]), "First", "nothing written yet");

        recovery::run_startup_recovery(dir.path(), |intent| {
            feature_ops::roll_forward(dir.path(), intent)
        })
        .unwrap();

        assert_eq!(label_of(&paths[0]), "Patched");
        assert_eq!(label_of(&paths[1]), "Patched");
        assert!(!recovery::has_incomplete_operations(dir.path()).unwrap());
    }

    /// A crash *between* two of the payload's file writes — the multi-
    /// element boundary the reviewer asked about — must also converge:
    /// recovery replays the whole payload idempotently, finishing the
    /// files that never landed without disturbing the ones that did.
    #[test]
    fn recovery_finishes_a_multi_file_payload_interrupted_between_two_writes() {
        let dir = init_project();
        let (intent, paths) = stage_two_label_patches(&dir, "Patched");
        recovery::commit_batch(dir.path(), &intent).unwrap();
        let recovery::IntentPayload::KnowledgeReconcile { files, .. } =
            intent.caller_payload.as_ref().unwrap()
        else {
            panic!("expected a KnowledgeReconcile payload");
        };
        crate::fs_safety::replace_file(
            dir.path(),
            &dir.path().join(&files[0].relative_path),
            files[0].contents.as_bytes(),
        )
        .unwrap();
        assert_eq!(label_of(&paths[0]), "Patched");
        assert_eq!(label_of(&paths[1]), "Second", "the crash point");

        recovery::run_startup_recovery(dir.path(), |intent| {
            feature_ops::roll_forward(dir.path(), intent)
        })
        .unwrap();

        assert_eq!(label_of(&paths[0]), "Patched");
        assert_eq!(label_of(&paths[1]), "Patched");
    }

    /// A Scenario reparent is replayed as a move, so recovery has to cope
    /// with a crash in its own middle: the file already relocated but
    /// still holding pre-patch content. Replay must skip the rename it
    /// cannot redo and finish the content write, rather than failing on
    /// the missing source.
    #[test]
    fn recovery_finishes_a_scenario_move_interrupted_between_the_rename_and_the_rewrite() {
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
        reconcile_creation(dir.path(), &parse_intent(create_yaml).unwrap()).unwrap();
        let from = dir.path().join(
            ".markharness/knowledge/features/todo-management/capture/empty-title/scenario.yml",
        );
        let to = dir.path().join(
            ".markharness/knowledge/features/todo-management/review/empty-title/scenario.yml",
        );
        let mut scenario =
            knowledge::parse_scenario(&std::fs::read_to_string(&from).unwrap()).unwrap();
        scenario.behavior = "review".to_string();

        let intent = recovery::begin_batch_with_payload(
            dir.path(),
            Vec::new(),
            Some(recovery::IntentPayload::KnowledgeReconcile {
                files: Vec::new(),
                moves: vec![recovery::PendingKnowledgeMove {
                    from_relative_path: relative_path_string(dir.path(), &from),
                    to_relative_path: relative_path_string(dir.path(), &to),
                    contents: knowledge::serialize_scenario(&scenario),
                }],
            }),
        )
        .unwrap();
        recovery::commit_batch(dir.path(), &intent).unwrap();
        // The rename happened; the content rewrite did not.
        crate::fs_safety::rename_no_follow(dir.path(), &from, &to).unwrap();
        assert_eq!(
            knowledge::parse_scenario(&std::fs::read_to_string(&to).unwrap())
                .unwrap()
                .behavior,
            "capture",
            "the crash point: relocated but still pre-patch"
        );

        recovery::run_startup_recovery(dir.path(), |intent| {
            feature_ops::roll_forward(dir.path(), intent)
        })
        .unwrap();

        assert!(!from.is_file());
        assert_eq!(
            knowledge::parse_scenario(&std::fs::read_to_string(&to).unwrap())
                .unwrap()
                .behavior,
            "review"
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
            Some(recovery::IntentPayload::KnowledgeReconcile {
                files: vec![recovery::PendingKnowledgeFile {
                    relative_path: ".markharness/knowledge/requirements/todo/requirement.yml"
                        .to_string(),
                    contents: knowledge::serialize_requirement(&requirement),
                }],
                moves: Vec::new(),
            }),
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

    /// Regression test for the reviewer-flagged bug: `check_creation` used
    /// to call `recovery::run_startup_recovery`, which — on finding a
    /// leftover staging entry from an earlier crash whose identity event
    /// is already committed (as this test's own setup leaves it, matching
    /// the crash point in `a_subsequent_command_rolls_forward_pending_files_
    /// after_a_kill_right_after_commit`) — would itself roll it forward,
    /// writing the pending Knowledge file, before `--check` ever got to
    /// `build_plan`. `--check` must refuse instead: the pending Requirement
    /// file must stay unwritten and the staging entry itself untouched
    /// (still resolvable by a later real command), not silently resolved
    /// as a side effect of a read-only call.
    #[test]
    fn check_creation_refuses_rather_than_resolving_a_pending_crash_recovery() {
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
        let staged_intent = recovery::begin_batch_with_payload(
            dir.path(),
            vec![recovery::BatchEvent {
                entity_kind: EntityKind::Requirement,
                entity_uid: requirement_uid.clone(),
                identity_event_uid: event_uid.clone(),
                event_yaml: serde_yaml_ng::to_string(&event).unwrap(),
            }],
            Some(recovery::IntentPayload::KnowledgeReconcile {
                files: vec![recovery::PendingKnowledgeFile {
                    relative_path: ".markharness/knowledge/requirements/todo/requirement.yml"
                        .to_string(),
                    contents: knowledge::serialize_requirement(&requirement),
                }],
                moves: Vec::new(),
            }),
        )
        .unwrap();
        recovery::commit_batch(dir.path(), &staged_intent).unwrap();
        // Deliberately no roll_forward/finish: the crash point, exactly
        // like `a_subsequent_command_rolls_forward_pending_files_after_a_kill_right_after_commit`
        // — except this time the next call is `--check`, not a real run.

        let doc = parse_intent(
            "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - id: unrelated
    source: native
    label: An unrelated new Requirement
    axis: []
",
        )
        .unwrap();
        let err = check_creation(dir.path(), &doc).unwrap_err();
        assert!(matches!(err, ReconcileError::RecoveryPending));

        assert!(
            !dir.path()
                .join(".markharness/knowledge/requirements/todo/requirement.yml")
                .is_file(),
            "--check must not roll the pending Requirement file forward"
        );
        assert!(
            dir.path()
                .join(".markharness/.identity-staging")
                .join(&staged_intent.operation_id)
                .is_dir(),
            "--check must leave the staging entry itself for a real command to resolve"
        );
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
            Some(recovery::IntentPayload::KnowledgeReconcile {
                files: vec![recovery::PendingKnowledgeFile {
                    relative_path: ".markharness/knowledge/requirements/todo/requirement.yml"
                        .to_string(),
                    contents: knowledge::serialize_requirement(&requirement),
                }],
                moves: Vec::new(),
            }),
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

    /// ADR 0027 §3's "UIDなし、同じkind・scope・IDが存在しない → 新規" row,
    /// applied to a Scenario whose parent Behavior already exists: the most
    /// common incremental authoring step there is (the repository's own
    /// `examples/todo-minimal` evolves exactly this way between v1 and v2).
    /// The restated Requirement/Feature/Behavior match by id and content, so
    /// they report `unchanged` while only the added Scenario is created.
    #[test]
    fn adds_a_new_scenario_under_an_existing_behavior_and_leaves_its_siblings_unchanged() {
        let dir = init_project();
        let v1 = "\
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
        reconcile_creation(dir.path(), &parse_intent(v1).unwrap()).unwrap();

        let v2 = "\
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
          - id: max-length
            description: An over-long title cannot be added
            phases:
              - steps:
                  - action: Attempt to add a 201-character title
                results:
                  - No TODO is added
";
        let outcome = reconcile_creation(dir.path(), &parse_intent(v2).unwrap()).unwrap();

        assert_eq!(outcome.created.len(), 1, "only the added Scenario is new");
        assert_eq!(outcome.created[0].kind, EntityKind::Scenario);
        assert_eq!(outcome.created[0].id, "max-length");
        assert!(outcome.updated.is_empty());
        assert_eq!(
            outcome.unchanged.len(),
            3,
            "Feature, Behavior and the pre-existing Scenario"
        );

        let scenario = knowledge::parse_scenario(
            &std::fs::read_to_string(dir.path().join(
                ".markharness/knowledge/features/todo-management/add-todo/max-length/scenario.yml",
            ))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(scenario.behavior, "add-todo");
        assert!(scenario.uid.is_some());
    }

    /// The same ADR 0027 §3 "新規" row one level up: a Behavior (with its own
    /// Scenarios) added under a Feature that already exists.
    #[test]
    fn adds_a_new_behavior_with_its_scenarios_under_an_existing_feature() {
        let dir = init_project();
        let v1 = "\
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
        reconcile_creation(dir.path(), &parse_intent(v1).unwrap()).unwrap();

        let v2 = "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - id: todo-management
    label: TODO management
    axis: []
    behaviors:
      - id: remove-todo
        description: Remove a TODO
        scenarios:
          - id: last-item
            description: Removing the last remaining TODO empties the list
            phases:
              - steps:
                  - action: Remove the only TODO
                results:
                  - The list is empty
";
        let outcome = reconcile_creation(dir.path(), &parse_intent(v2).unwrap()).unwrap();

        assert_eq!(outcome.created.len(), 2, "the Behavior and its Scenario");
        assert_eq!(outcome.created[0].kind, EntityKind::Behavior);
        assert_eq!(outcome.created[0].id, "remove-todo");
        assert_eq!(outcome.created[1].kind, EntityKind::Scenario);
        assert_eq!(outcome.created[1].id, "last-item");
        assert_eq!(outcome.unchanged.len(), 1, "the Feature itself");

        let behavior =
            knowledge::parse_behavior(
                &std::fs::read_to_string(dir.path().join(
                    ".markharness/knowledge/features/todo-management/remove-todo/behavior.yml",
                ))
                .unwrap(),
            )
            .unwrap();
        assert_eq!(behavior.feature, "todo-management");
        assert!(behavior.uid.is_some());
        assert!(
            dir.path()
                .join(".markharness/knowledge/features/todo-management/add-todo/behavior.yml")
                .is_file(),
            "the pre-existing Behavior must survive untouched"
        );
    }

    /// ADR 0027 §3's reparent row, with a destination Behavior this same
    /// Intent creates: the Scenario keeps its UID and moves under the new
    /// Behavior's directory.
    #[test]
    fn reparents_an_existing_scenario_into_a_brand_new_behavior() {
        let dir = init_project();
        let v1 = "\
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
        let created = reconcile_creation(dir.path(), &parse_intent(v1).unwrap()).unwrap();
        let scenario_uid = created
            .created
            .iter()
            .find(|c| c.kind == EntityKind::Scenario)
            .unwrap()
            .uid
            .clone();

        let v2 = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - id: todo-management
    label: TODO management
    axis: []
    behaviors:
      - id: validate-todo
        description: Validate a TODO before adding it
        scenarios:
          - uid: {scenario_uid}
"
        );
        let outcome = reconcile_creation(dir.path(), &parse_intent(&v2).unwrap()).unwrap();

        assert_eq!(outcome.created.len(), 1, "the destination Behavior");
        assert_eq!(outcome.created[0].kind, EntityKind::Behavior);
        assert_eq!(outcome.updated.len(), 1, "the reparented Scenario");
        assert_eq!(outcome.updated[0].uid, scenario_uid);

        let moved = dir.path().join(
            ".markharness/knowledge/features/todo-management/validate-todo/empty-title/scenario.yml",
        );
        assert!(moved.is_file());
        let scenario =
            knowledge::parse_scenario(&std::fs::read_to_string(&moved).unwrap()).unwrap();
        assert_eq!(scenario.behavior, "validate-todo");
        assert_eq!(scenario.uid.as_deref(), Some(scenario_uid.as_str()));
        assert!(
            !dir.path()
                .join(
                    ".markharness/knowledge/features/todo-management/add-todo/empty-title/scenario.yml"
                )
                .is_file(),
            "the Scenario must not be left behind at its old path"
        );
    }
}
