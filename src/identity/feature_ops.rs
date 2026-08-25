use std::fs;
use std::io;
use std::path::Path;

use crate::execution::iso8601_utc_now;
use crate::identity::{
    EntityKind, IdentityEvent, IdentityMutation, engine, knowledge_walk, marker,
    migration_manifest, recovery, registry,
};

/// Why `rename_id` refused to run, or failed partway (design doc §3, §9).
#[derive(Debug)]
pub enum RenameError {
    /// No `feature.yml` in the working tree has `id: <old_id>`.
    FeatureNotFound(String),
    /// The Feature has no `uid` yet — `identity migrate` (or equivalent
    /// issuance) must run first (design doc §12).
    NotMigrated(String),
    /// `new_id` already names a different Feature.
    NewIdAlreadyInUse(String),
    /// A concurrent identity operation is genuinely in progress
    /// (design doc §6.3) — the caller must retry later, not race it.
    OperationInProgress,
    /// Replaying the Feature's identity events failed (e.g. an
    /// unresolved branch divergence, design doc §7).
    ReplayFailed(engine::ReplayError),
    /// `feature.yml`'s `id:` disagrees with what replaying its identity
    /// events says the current id should be — the working tree and the
    /// identity event log have drifted apart (should not happen absent a
    /// manual edit; caller must reconcile before renaming).
    CurrentIdMismatch {
        expected: String,
        actual: String,
    },
    Io(io::Error),
}

impl From<io::Error> for RenameError {
    fn from(e: io::Error) -> Self {
        RenameError::Io(e)
    }
}

/// Brings the entity's Knowledge YAML and the Registry cache in line with
/// `intent`'s current replayed state (design doc §6.1 steps 3–4). Runs both
/// immediately after a fresh commit and, identically, from the startup
/// recovery scan after a crash — the only difference is which process
/// invocation calls it, which is exactly the point: this step must be
/// idempotent and safe to redo from just the identity event log. Kind-
/// generic (design doc §3.2): the only kind-specific part —
/// which struct to parse/serialize — lives in `knowledge_walk`.
fn roll_forward(root: &Path, intent: &recovery::Intent) -> io::Result<()> {
    recovery::complete_batch_commits(root, intent)?;
    if intent.batch_events.is_empty() {
        roll_forward_entity(root, intent.entity_kind, &intent.entity_uid)?;
    } else {
        for event in &intent.batch_events {
            roll_forward_entity(root, event.entity_kind, &event.entity_uid)?;
        }
    }
    // `migrate_all` is the only caller that ever sets `caller_payload`
    // (its cases' legacy, pre-migration snapshot identity, captured and
    // durably persisted *before* this intent's commit point — design doc
    // §6.1). Recording it here, rather than back in `migrate_all` itself,
    // is what makes it survive a crash between the commit point and the
    // manifest being updated: this function runs identically on the happy
    // path and from crash recovery replay (`run_startup_recovery`), so a
    // kill in that window is simply finished on the next run instead of
    // silently losing the legacy identity. Matching on the exact
    // `IdentityMigration` variant (rather than "any non-empty payload")
    // means a future, unrelated `IntentPayload` variant can never be
    // misread as this one.
    if let Some(recovery::IntentPayload::IdentityMigration(payload)) = &intent.caller_payload {
        let legacy_signatures =
            migration_manifest::LegacyCaseSignatures::from_durable_payload(payload)?;
        migration_manifest::record_new_case_uids(root, &legacy_signatures)?;
    }
    Ok(())
}

fn roll_forward_entity(root: &Path, entity_kind: EntityKind, entity_uid: &str) -> io::Result<()> {
    let result = registry::resolve_from_working_tree(root, entity_kind, entity_uid)?
        .map_err(|e| io::Error::other(format!("{e:?}")))?;

    // A migrate operation's root `Issued` event assigns a `uid` no
    // Knowledge file carries yet, so `find_by_uid` can't find it — fall
    // back to locating it by the id the event just issued/renamed to.
    let found = match knowledge_walk::find_by_uid(root, entity_kind, entity_uid)? {
        Some(found) => Some(found),
        None => knowledge_walk::find_by_id(root, entity_kind, &result.current_id)?,
    };

    if let Some(found) = found
        && (found.id != result.current_id || found.uid.as_deref() != Some(entity_uid))
    {
        knowledge_walk::write_id_and_uid(
            root,
            entity_kind,
            &found.path,
            &result.current_id,
            entity_uid,
        )?;
    }

    registry::invalidate(root, entity_kind, entity_uid)
}

/// `markharness feature rename-id <old> <new>` (design doc §3, §9):
/// changes a Feature's `id:` while preserving its `uid`, recording the
/// change as a `Renamed` identity event. Requires the Feature to already
/// have a `uid` (run `identity migrate` first if not).
pub fn rename_id(root: &Path, old_id: &str, new_id: &str) -> Result<(), RenameError> {
    // Reading the current Knowledge state, resolving `entity_uid`, and
    // replaying its events are not a single atomic filesystem operation,
    // so they must run *after* acquiring the lock, not before: a
    // concurrent identity mutation for this entity (or for whatever
    // Feature currently holds `new_id`) could otherwise land in the gap
    // between an unlocked read here and the commit below, letting this
    // call commit against a predecessor or id state that is already
    // stale by the time it runs. This is not limited to two concurrent
    // `rename_id` calls racing each other — any other identity operation
    // mutating the same entity while this read is unlocked has the same
    // effect. Reusing the exact lock `run_startup_recovery` itself
    // acquired (rather than releasing it and acquiring a fresh one) keeps
    // recovery and this check-and-commit as one continuous critical
    // section — see that function's own doc comment for why the gap
    // between two separate acquires matters too.
    let held_lock = match recovery::run_startup_recovery(root, |intent| roll_forward(root, intent))?
    {
        recovery::StartupRecovery::OperationInProgress => {
            return Err(RenameError::OperationInProgress);
        }
        recovery::StartupRecovery::Ready { lock, .. } => lock,
    };
    let outcome = (|| {
        let Some(found) = knowledge_walk::find_by_id(root, EntityKind::Feature, old_id)? else {
            return Err(RenameError::FeatureNotFound(old_id.to_string()));
        };
        let Some(entity_uid) = found.uid.clone() else {
            return Err(RenameError::NotMigrated(old_id.to_string()));
        };
        if old_id != new_id
            && knowledge_walk::find_by_id(root, EntityKind::Feature, new_id)?.is_some()
        {
            return Err(RenameError::NewIdAlreadyInUse(new_id.to_string()));
        }

        let replay_result =
            registry::resolve_from_working_tree(root, EntityKind::Feature, &entity_uid)?
                .map_err(RenameError::ReplayFailed)?;
        if replay_result.current_id != old_id {
            return Err(RenameError::CurrentIdMismatch {
                expected: old_id.to_string(),
                actual: replay_result.current_id,
            });
        }

        commit_rename(
            root,
            &entity_uid,
            old_id,
            new_id,
            &replay_result.current_head_event_uid,
        )
    })();
    held_lock.release()?;
    outcome
}

/// Shared commit body for every identity operation that appends exactly
/// one new `IdentityEvent`: generate its uid, stage it via
/// `recovery::begin`, serialize and durably `recovery::commit` it, then
/// `roll_forward` the affected Knowledge file and `recovery::finish` the
/// intent. Every one of `rename_id`, `resolve_divergence`, `release_id`,
/// `retire_entity`, `restore_entity`, and `reissue_entity` differs only in
/// which `entity_uid` it commits under, what predecessor(s) the new event
/// names, and which `IdentityMutation` it carries — this factors that
/// mechanical sequence out once so those six call sites don't each
/// reimplement it. Callers convert the `io::Error` this returns into their
/// own error type via `?` and `From<io::Error>`.
fn commit_single_event(
    root: &Path,
    kind: EntityKind,
    entity_uid: &str,
    previous_identity_event_uid: Option<String>,
    previous_identity_event_uids: Vec<String>,
    mutation: IdentityMutation,
) -> io::Result<()> {
    let event_uid = ulid::Ulid::new().to_string();
    let intent = recovery::begin(root, kind, entity_uid, &event_uid)?;

    let event = IdentityEvent {
        identity_event_uid: event_uid,
        entity_uid: entity_uid.to_string(),
        entity_kind: kind,
        previous_identity_event_uid,
        previous_identity_event_uids,
        recorded_at: iso8601_utc_now(),
        mutation,
    };
    let event_yaml = serde_yaml_ng::to_string(&event).map_err(io::Error::other)?;
    recovery::commit(root, &intent, &event_yaml)?;

    roll_forward(root, &intent)?;
    recovery::finish(root, &intent)?;
    Ok(())
}

fn commit_rename(
    root: &Path,
    entity_uid: &str,
    old_id: &str,
    new_id: &str,
    current_head_event_uid: &str,
) -> Result<(), RenameError> {
    commit_single_event(
        root,
        EntityKind::Feature,
        entity_uid,
        Some(current_head_event_uid.to_string()),
        Vec::new(),
        IdentityMutation::Renamed {
            from_id: old_id.to_string(),
            to_id: new_id.to_string(),
        },
    )?;
    Ok(())
}

/// Why `resolve_divergence` refused to run, or failed partway (design doc §7).
#[derive(Debug)]
pub enum ResolveError {
    /// The entity has no branch divergence to resolve right now (either
    /// replay already succeeds, or it fails for a reason other than
    /// `AmbiguousDivergence`).
    NoDivergence,
    /// `keep_event_uid` does not name one of the actual divergent heads.
    NotADivergentHead {
        keep_event_uid: String,
        divergent_head_uids: Vec<String>,
    },
    OperationInProgress,
    Io(io::Error),
}

impl From<io::Error> for ResolveError {
    fn from(e: io::Error) -> Self {
        ResolveError::Io(e)
    }
}

/// `markharness identity resolve <kind> <uid> --keep <event-uid>`
/// (design doc §7): explicitly joins a branch divergence — two identity
/// events that both extend the same predecessor — by appending a
/// `Resolved` event naming which head's outcome survives. Never invoked
/// automatically (no merge driver, design doc §7's Q5 rationale).
pub fn resolve_divergence(
    root: &Path,
    kind: EntityKind,
    entity_uid: &str,
    keep_event_uid: &str,
) -> Result<(), ResolveError> {
    // Same TOCTOU concern as `rename_id`: reading events and replaying them
    // to find the divergent heads must happen under the lock, not before
    // it, or a concurrent mutation (e.g. another `resolve` racing this one,
    // or any other identity operation for this entity) could change which
    // heads are actually divergent between this read and the commit below.
    // Reusing the same lock `run_startup_recovery` acquired keeps recovery
    // and this check-and-commit as one continuous critical section.
    let held_lock = match recovery::run_startup_recovery(root, |intent| roll_forward(root, intent))?
    {
        recovery::StartupRecovery::OperationInProgress => {
            return Err(ResolveError::OperationInProgress);
        }
        recovery::StartupRecovery::Ready { lock, .. } => lock,
    };
    let outcome = (|| {
        let events = registry::load_events_from_working_tree(root, kind, entity_uid)?;
        let divergent_head_uids = match engine::replay(entity_uid, &events) {
            Err(engine::ReplayError::AmbiguousDivergence {
                divergent_head_uids,
            }) => divergent_head_uids,
            _ => return Err(ResolveError::NoDivergence),
        };
        if !divergent_head_uids.iter().any(|uid| uid == keep_event_uid) {
            return Err(ResolveError::NotADivergentHead {
                keep_event_uid: keep_event_uid.to_string(),
                divergent_head_uids,
            });
        }

        commit_resolution(root, kind, entity_uid, keep_event_uid, &divergent_head_uids)
    })();
    held_lock.release()?;
    outcome
}

fn commit_resolution(
    root: &Path,
    kind: EntityKind,
    entity_uid: &str,
    keep_event_uid: &str,
    divergent_head_uids: &[String],
) -> Result<(), ResolveError> {
    commit_single_event(
        root,
        kind,
        entity_uid,
        None,
        divergent_head_uids.to_vec(),
        IdentityMutation::Resolved {
            winning_event_uid: keep_event_uid.to_string(),
        },
    )?;
    Ok(())
}

/// Why `release_id` refused to run, or failed partway (design doc §9).
#[derive(Debug)]
pub enum ReleaseError {
    /// The entity is not currently `retired` — `release` only lifts the
    /// reuse reservation on a retired UID's old ids (design doc §2, §9).
    NotRetired,
    /// `released_id` never appears in this entity's `id_history`, so
    /// there is no reservation of it to lift.
    IdNeverUsedByThisEntity {
        released_id: String,
    },
    OperationInProgress,
    ReplayFailed(engine::ReplayError),
    Io(io::Error),
}

impl From<io::Error> for ReleaseError {
    fn from(e: io::Error) -> Self {
        ReleaseError::Io(e)
    }
}

/// `markharness identity release <kind> <uid> <old-id>` (design doc §9):
/// explicitly lifts the reuse reservation on `released_id`, an id
/// formerly held by a now-`retired` entity, so a *different* entity may
/// be issued that id afterward. Runs with no confirmation flag, matching
/// `rename-id` (design doc §9's Q9 rationale: the command itself plus its
/// Git diff and identity event are the audit trail).
pub fn release_id(
    root: &Path,
    kind: EntityKind,
    entity_uid: &str,
    released_id: &str,
) -> Result<(), ReleaseError> {
    // Same TOCTOU concern as `rename_id`/`resolve_divergence`: reading and
    // replaying events to check `Retired` status and `id_history` must
    // happen under the lock, or a concurrent mutation for this entity
    // (e.g. a `restore` landing between this read and the commit below)
    // could make the check stale by the time `commit_release` runs.
    // Reusing the same lock `run_startup_recovery` acquired keeps recovery
    // and this check-and-commit as one continuous critical section.
    let held_lock = match recovery::run_startup_recovery(root, |intent| roll_forward(root, intent))?
    {
        recovery::StartupRecovery::OperationInProgress => {
            return Err(ReleaseError::OperationInProgress);
        }
        recovery::StartupRecovery::Ready { lock, .. } => lock,
    };
    let outcome = (|| {
        let events = registry::load_events_from_working_tree(root, kind, entity_uid)?;
        let replay_result =
            engine::replay(entity_uid, &events).map_err(ReleaseError::ReplayFailed)?;
        if replay_result.status != engine::Status::Retired {
            return Err(ReleaseError::NotRetired);
        }
        if !replay_result
            .id_history
            .iter()
            .any(|entry| entry.id == released_id)
        {
            return Err(ReleaseError::IdNeverUsedByThisEntity {
                released_id: released_id.to_string(),
            });
        }

        commit_release(
            root,
            kind,
            entity_uid,
            released_id,
            &replay_result.current_head_event_uid,
        )
    })();
    held_lock.release()?;
    outcome
}

fn commit_release(
    root: &Path,
    kind: EntityKind,
    entity_uid: &str,
    released_id: &str,
    current_head_event_uid: &str,
) -> Result<(), ReleaseError> {
    commit_single_event(
        root,
        kind,
        entity_uid,
        Some(current_head_event_uid.to_string()),
        Vec::new(),
        IdentityMutation::Released {
            released_id: released_id.to_string(),
        },
    )?;
    Ok(())
}

/// Why `retire_entity` refused to run, or failed partway (design doc §2,
/// §4.2: retirement is "triggered by deleting the Knowledge element").
#[derive(Debug)]
pub enum RetireError {
    /// The entity has no `uid` yet — nothing to retire.
    NotFound,
    /// The Knowledge element still exists in the working tree. Retirement
    /// records that an element was *removed*; deleting the file is the
    /// user's action, this command only records it.
    StillPresent,
    /// The entity is already `retired`.
    AlreadyRetired,
    OperationInProgress,
    ReplayFailed(engine::ReplayError),
    Io(io::Error),
}

impl From<io::Error> for RetireError {
    fn from(e: io::Error) -> Self {
        RetireError::Io(e)
    }
}

/// `markharness identity retire <kind> <uid>` (design doc §2, §4.2): records
/// that a Knowledge element the caller has already deleted from the working
/// tree is retired, appending a `Retired` identity event. Never deletes a
/// file itself and never runs automatically (no filesystem watcher) — the
/// deletion is the user's own action; this command only records it, the
/// same division of responsibility as `rename-id` recording a rename the
/// user already decided on.
pub fn retire_entity(root: &Path, kind: EntityKind, entity_uid: &str) -> Result<(), RetireError> {
    // The read-replay-validate sequence below is not a single atomic
    // filesystem operation, so it must run *after* acquiring the lock, not
    // before: a concurrent `retire` (or any other identity mutation for
    // this entity) could otherwise commit a `Retired` event referencing
    // the same predecessor this call already read as the current head,
    // producing an unintended branch divergence instead of the single
    // linear `retired` transition ADR 0013 requires. Locking only around
    // the commit step is not enough by itself. Reusing the same lock
    // `run_startup_recovery` acquired keeps recovery and this
    // check-and-commit as one continuous critical section.
    let held_lock = match recovery::run_startup_recovery(root, |intent| roll_forward(root, intent))?
    {
        recovery::StartupRecovery::OperationInProgress => {
            return Err(RetireError::OperationInProgress);
        }
        recovery::StartupRecovery::Ready { lock, .. } => lock,
    };
    let outcome = (|| {
        let events = registry::load_events_from_working_tree(root, kind, entity_uid)?;
        if events.is_empty() {
            return Err(RetireError::NotFound);
        }
        let replay_result =
            engine::replay(entity_uid, &events).map_err(RetireError::ReplayFailed)?;
        if replay_result.status == engine::Status::Retired {
            return Err(RetireError::AlreadyRetired);
        }
        if knowledge_walk::find_by_uid(root, kind, entity_uid)?.is_some() {
            return Err(RetireError::StillPresent);
        }

        commit_retire(
            root,
            kind,
            entity_uid,
            &replay_result.current_head_event_uid,
        )
    })();
    held_lock.release()?;
    outcome
}

fn commit_retire(
    root: &Path,
    kind: EntityKind,
    entity_uid: &str,
    current_head_event_uid: &str,
) -> Result<(), RetireError> {
    commit_single_event(
        root,
        kind,
        entity_uid,
        Some(current_head_event_uid.to_string()),
        Vec::new(),
        IdentityMutation::Retired,
    )?;
    Ok(())
}

/// Why `restore_entity` refused to run, or failed partway (design doc §2).
#[derive(Debug)]
pub enum RestoreError {
    /// The entity has no `uid` yet.
    NotFound,
    /// The entity is not currently `retired` — nothing to restore.
    NotRetired,
    OperationInProgress,
    ReplayFailed(engine::ReplayError),
    Io(io::Error),
}

impl From<io::Error> for RestoreError {
    fn from(e: io::Error) -> Self {
        RestoreError::Io(e)
    }
}

/// `markharness identity restore <kind> <uid>` (design doc §2): reverses a
/// previous `retire_entity`, appending a `Restored` identity event that
/// flips the entity's status back to `Active`. Does not recreate the
/// Knowledge element itself. If the caller recreates the file *before*
/// calling `restore`, this function's own roll-forward step fills its
/// `uid:` back in immediately. If the file is recreated *afterward*
/// instead, nothing automatically notices — call `sync_entity` (`identity
/// sync`) once the file exists again to fill its `uid:` in on demand; that
/// is the one operation guaranteed to work for this regardless of order
/// and for every `EntityKind` (unlike `rename_id`, which only exists for
/// Feature and requires the file to already carry a `uid:`).
pub fn restore_entity(root: &Path, kind: EntityKind, entity_uid: &str) -> Result<(), RestoreError> {
    // Same TOCTOU concern as `retire_entity` above: the lock must wrap the
    // read-replay-validate sequence, not just the commit, or two
    // concurrent `restore` calls (or a `restore` racing another mutation
    // for the same entity) could both read the same current head and each
    // commit a `Restored` event against it — an unintended branch
    // divergence instead of the single linear transition ADR 0013 requires.
    // Reusing the same lock `run_startup_recovery` acquired keeps recovery
    // and this check-and-commit as one continuous critical section.
    let held_lock = match recovery::run_startup_recovery(root, |intent| roll_forward(root, intent))?
    {
        recovery::StartupRecovery::OperationInProgress => {
            return Err(RestoreError::OperationInProgress);
        }
        recovery::StartupRecovery::Ready { lock, .. } => lock,
    };
    let outcome = (|| {
        let events = registry::load_events_from_working_tree(root, kind, entity_uid)?;
        if events.is_empty() {
            return Err(RestoreError::NotFound);
        }
        let replay_result =
            engine::replay(entity_uid, &events).map_err(RestoreError::ReplayFailed)?;
        if replay_result.status != engine::Status::Retired {
            return Err(RestoreError::NotRetired);
        }

        commit_restore(
            root,
            kind,
            entity_uid,
            &replay_result.current_head_event_uid,
        )
    })();
    held_lock.release()?;
    outcome
}

fn commit_restore(
    root: &Path,
    kind: EntityKind,
    entity_uid: &str,
    current_head_event_uid: &str,
) -> Result<(), RestoreError> {
    commit_single_event(
        root,
        kind,
        entity_uid,
        Some(current_head_event_uid.to_string()),
        Vec::new(),
        IdentityMutation::Restored,
    )?;
    Ok(())
}

/// A fresh identity issued by `reissue_entity`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReissuedEntity {
    pub uid: String,
    pub source_uid: Option<String>,
}

/// Why `reissue_entity` refused to run, or failed partway (design doc's
/// "copy、import、repository統合の規則").
#[derive(Debug)]
pub enum ReissueError {
    /// No `kind` element with this `id` exists in the working tree.
    NotFound(String),
    /// `id` has not been explicitly `release`d from the element's current
    /// `uid:` in this project's local identity event log. ADR 0013: "Once
    /// an ID has been issued to a UID, it cannot be assigned to another UID
    /// unless an explicit `release` event lifts that reservation." Merely
    /// retiring the old UID is not enough — its former ids stay reserved
    /// until `identity release` runs. Run `identity retire` followed by
    /// `identity release <kind> <old-uid> <id>` first.
    SourceIdNotReleased {
        source_uid: String,
        id: String,
    },
    /// `id` is not held by the Knowledge file's own `uid:` (it may have
    /// none at all — a copy/import or hand-edited recreation), but some
    /// *other* locally known UID of this `kind` still reserves it: `id`
    /// appears in that UID's identity event log (as an `issued`,
    /// `reissued`, or `renamed`-to id) with no matching `released` event.
    /// ADR 0013's reservation rule is keyed on the id itself, not on which
    /// Knowledge file currently claims it, so this repository-wide check
    /// closes the gap a uid-less recreation would otherwise slip through.
    IdReservedByAnotherUid {
        holder_uid: String,
        id: String,
    },
    OperationInProgress,
    ReplayFailed(engine::ReplayError),
    Io(io::Error),
}

impl From<io::Error> for ReissueError {
    fn from(e: io::Error) -> Self {
        ReissueError::Io(e)
    }
}

/// `markharness identity reissue <kind> <id>` (ADR 0013's copy/import/
/// repository-integration rules): assigns a *brand-new* `uid` to the
/// Knowledge element currently named `id`, recording a root `Reissued`
/// identity event — deliberately not continuing whatever identity history
/// the element's current `uid:` (if any) names, since a genuine reissue
/// means this project is treating the element as a distinct entity rather
/// than the same one continuing under a carried-over UID (e.g. a copy from
/// another repository that should not share identity with its source, or
/// one side of a UID collision discovered while integrating two
/// repositories). The element's previous `uid:` value, if it had one, is
/// recorded as `source_uid` purely for audit provenance — it is never
/// resolved as an entity in this project (design doc's `Reissued` variant).
///
/// The reservation check below always scans every *locally known* UID of
/// this `kind` (`find_unreleased_reservation_holder`), regardless of
/// whether the Knowledge file itself currently carries a `uid:`. Checking
/// only the file's own `uid:` when it has one is not sufficient: a `uid:`
/// copied in from elsewhere (no local event log for it at all — e.g. a
/// literal copy/import) trivially has nothing to check, which would
/// silently skip the scan and let a still-reserved (retired but not
/// `release`d) claim held by some *other*, genuinely local UID slip
/// through. The scan naturally covers the "no `uid:` at all" case the same
/// way, since a uid-less file simply has no self-check to skip in the
/// first place.
///
/// The scan determines "reserved" from a *causally ordered* walk of each
/// candidate UID's event log (`is_id_reserved_by`, via
/// `engine::causal_order`), not from an unordered "does a matching event
/// exist anywhere" scan — a UID can legitimately release an id and later
/// reclaim it (e.g. retire → release → restore → retire again without a
/// second release), and only the *most recent* claim/release for that
/// specific id determines whether it is currently reserved.
///
/// The whole check-then-commit sequence runs while holding
/// `lock::IdentityLock`, not just the commit: reading every candidate UID's
/// event log to decide "reserved or not" is not a single atomic
/// filesystem operation, and another identity command committing a
/// conflicting mutation (e.g. a concurrent `release`) in the gap between an
/// un-locked check and the commit would let two `reissue` calls (or a
/// `reissue` racing a `release`) both observe the pre-mutation state and
/// disagree with what actually lands on disk. Holding the lock for the
/// full duration instead makes a concurrent identity command fail fast
/// with `OperationInProgress` rather than racing.
pub fn reissue_entity(
    root: &Path,
    kind: EntityKind,
    id: &str,
) -> Result<ReissuedEntity, ReissueError> {
    // Reusing the same lock `run_startup_recovery` acquired keeps recovery
    // and this check-and-commit as one continuous critical section.
    let held_lock = match recovery::run_startup_recovery(root, |intent| roll_forward(root, intent))?
    {
        recovery::StartupRecovery::OperationInProgress => {
            return Err(ReissueError::OperationInProgress);
        }
        recovery::StartupRecovery::Ready { lock, .. } => lock,
    };
    let outcome = (|| {
        let Some(found) = knowledge_walk::find_by_id(root, kind, id)? else {
            return Err(ReissueError::NotFound(id.to_string()));
        };
        let source_uid = found.uid.clone();

        // Always scan every *locally known* UID of this `kind` — not just
        // `source_uid` — for an unreleased reservation on `id`. Checking
        // only `source_uid` when the Knowledge file happens to carry a
        // `uid:` was a gap: a `uid:` copied in from elsewhere (no local
        // event log of its own) trivially "passes" a source_uid-only
        // check, silently skipping the repository-wide scan and letting
        // reissue bypass a reservation genuinely held by some *other*
        // locally known UID. `source_uid`, when it does have a local
        // event log, is simply one of the candidates this scan considers.
        if let Some(holder_uid) = find_unreleased_reservation_holder(root, kind, id)? {
            if Some(&holder_uid) == source_uid.as_ref() {
                return Err(ReissueError::SourceIdNotReleased {
                    source_uid: holder_uid,
                    id: id.to_string(),
                });
            }
            return Err(ReissueError::IdReservedByAnotherUid {
                holder_uid,
                id: id.to_string(),
            });
        }

        commit_reissue(root, kind, id, source_uid)
    })();
    held_lock.release()?;
    outcome
}

/// Whether `holder_uid`'s identity event log currently reserves `id`,
/// determined by walking its causally ordered events (root to head) while
/// tracking two things together: the running `current_id` (exactly as
/// `engine::replay` computes it) and a `reserved` flag for `id`
/// specifically. `Issued`/`Reissued` naming `id`, or a `Renamed` whose
/// `to_id` is `id`, set `reserved`; a later `Released { released_id }`
/// matching `id` clears it. Order matters both because a UID can release an
/// id and legitimately reclaim it later (so "some `Released` event exists
/// anywhere in the log" is not sufficient — only the *most recent* touch of
/// this id decides it), and because `Restored` implicitly reclaims whatever
/// the entity's `current_id` is at that point without emitting a fresh
/// claim event of its own (ADR 0013: restoring the same UID is the one
/// exception to needing an explicit `release`) — so `Restored` must also be
/// treated as re-asserting `reserved` when `current_id == id`, or a
/// retire → release → restore → retire-again sequence with no second
/// release would be wrongly read as still released.
fn is_id_reserved_by(
    root: &Path,
    kind: EntityKind,
    holder_uid: &str,
    id: &str,
) -> Result<bool, ReissueError> {
    let events = registry::load_events_from_working_tree(root, kind, holder_uid)?;
    if events.is_empty() {
        return Ok(false);
    }
    let chain = engine::causal_order(&events).map_err(ReissueError::ReplayFailed)?;
    let mut current_id: Option<&str> = None;
    let mut reserved = false;
    for event in &chain {
        match &event.mutation {
            IdentityMutation::Issued { id: event_id }
            | IdentityMutation::Reissued { id: event_id, .. } => {
                current_id = Some(event_id.as_str());
                if event_id == id {
                    reserved = true;
                }
            }
            IdentityMutation::Renamed { to_id, .. } => {
                current_id = Some(to_id.as_str());
                if to_id == id {
                    reserved = true;
                }
            }
            IdentityMutation::Restored => {
                if current_id == Some(id) {
                    reserved = true;
                }
            }
            IdentityMutation::Released { released_id } if released_id == id => {
                reserved = false;
            }
            IdentityMutation::Retired
            | IdentityMutation::Released { .. }
            | IdentityMutation::Resolved { .. } => {}
        }
    }
    Ok(reserved)
}

/// Scans every locally known UID of `kind` (other than the one already
/// checked by `reissue_entity`'s own-file lookup) for a still-reserved,
/// unreleased claim on `id`, per `is_id_reserved_by`. Returns the first
/// offending UID found, or `None` if no local UID reserves `id`.
fn find_unreleased_reservation_holder(
    root: &Path,
    kind: EntityKind,
    id: &str,
) -> Result<Option<String>, ReissueError> {
    let events_root = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("identity-events")
        .join(kind.directory_segment());
    let entries = match fs::read_dir(&events_root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(ReissueError::Io(e)),
    };
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let holder_uid = entry.file_name().to_string_lossy().into_owned();
        if is_id_reserved_by(root, kind, &holder_uid, id)? {
            return Ok(Some(holder_uid));
        }
    }
    Ok(None)
}

fn commit_reissue(
    root: &Path,
    kind: EntityKind,
    id: &str,
    source_uid: Option<String>,
) -> Result<ReissuedEntity, ReissueError> {
    let entity_uid = ulid::Ulid::new().to_string();
    commit_single_event(
        root,
        kind,
        &entity_uid,
        None,
        Vec::new(),
        IdentityMutation::Reissued {
            id: id.to_string(),
            source_uid: source_uid.clone(),
        },
    )?;
    Ok(ReissuedEntity {
        uid: entity_uid,
        source_uid,
    })
}

/// Why `sync_entity` refused to run, or failed partway.
#[derive(Debug)]
pub enum SyncError {
    /// The entity has no `uid` on record — nothing to sync.
    NotFound,
    /// The entity is not currently `Active` (i.e. it's `Retired`).
    /// Writing its `uid:` into a Knowledge file without a `Restored` event
    /// first would resurrect a deleted element's on-disk presence while
    /// its identity event log still says retired — run `identity restore`
    /// before `sync` for a retired entity.
    NotActive(engine::Status),
    OperationInProgress,
    ReplayFailed(engine::ReplayError),
    Io(io::Error),
}

impl From<io::Error> for SyncError {
    fn from(e: io::Error) -> Self {
        SyncError::Io(e)
    }
}

/// `markharness identity sync <kind> <uid>`: writes `entity_uid`'s current
/// replayed `id` into whatever Knowledge file currently carries that id,
/// filling in its `uid:` if missing. This is the general-purpose form of
/// the fallback-by-id resync `roll_forward_entity` already performs as a
/// side effect of every other identity operation (migrate, rename, retire,
/// restore, ...) — exposed directly for the case a Knowledge file is
/// (re)created outside of, or after, one of those operations (e.g.
/// recreating a `restore_entity`d element's file *after* calling
/// `restore`, rather than before) and so never went through that
/// side-effect. Every `EntityKind` is supported, not just Feature (unlike
/// `rename-id`, which requires an existing `uid:` on the file and so can't
/// be repurposed as a general resync for a still-uid-less file). Writes
/// nothing else and creates no identity event — purely re-derives file
/// state from the already-durable event log, so it needs no crash-recovery
/// batching of its own.
pub fn sync_entity(root: &Path, kind: EntityKind, entity_uid: &str) -> Result<(), SyncError> {
    // Same TOCTOU concern as the other operations above: checking `Active`
    // status must happen under the lock, or a concurrent `retire` landing
    // between this read and the write below could let `sync` still write
    // `uid:` back into a Knowledge file for an entity that is `Retired` by
    // the time the write actually happens — exactly the resurrection
    // `NotActive` exists to prevent. Reusing the same lock
    // `run_startup_recovery` acquired keeps recovery and this
    // check-and-write as one continuous critical section.
    let held_lock = match recovery::run_startup_recovery(root, |intent| roll_forward(root, intent))?
    {
        recovery::StartupRecovery::OperationInProgress => {
            return Err(SyncError::OperationInProgress);
        }
        recovery::StartupRecovery::Ready { lock, .. } => lock,
    };
    let outcome = (|| {
        let events = registry::load_events_from_working_tree(root, kind, entity_uid)?;
        if events.is_empty() {
            return Err(SyncError::NotFound);
        }
        let replay_result = engine::replay(entity_uid, &events).map_err(SyncError::ReplayFailed)?;
        if replay_result.status != engine::Status::Active {
            return Err(SyncError::NotActive(replay_result.status));
        }

        roll_forward_entity(root, kind, entity_uid).map_err(SyncError::from)
    })();
    held_lock.release()?;
    outcome
}

/// One Knowledge element `identity migrate` newly assigned a `uid` to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigratedEntity {
    pub kind: EntityKind,
    pub id: String,
    pub uid: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MigrateReport {
    pub migrated: Vec<MigratedEntity>,
    pub conflicts: Vec<String>,
    pub changed_files: Vec<String>,
}

struct MigrationPlan {
    report: MigrateReport,
    events: Vec<recovery::BatchEvent>,
}

/// Why `migrate_entities` refused to run, or failed partway.
#[derive(Debug)]
pub enum MigrateError {
    /// A concurrent identity operation is genuinely in progress
    /// (design doc §6.3) — the caller must retry later, not race it.
    OperationInProgress,
    Conflicts(Vec<String>),
    Io(io::Error),
}

impl From<io::Error> for MigrateError {
    fn from(e: io::Error) -> Self {
        MigrateError::Io(e)
    }
}

/// `markharness identity migrate` (design doc §12, §13 Phase 4: all five
/// Knowledge element kinds): assigns a fresh `uid` to every element in the
/// working tree that doesn't have one yet, recording each as a root
/// `Issued` identity event. Idempotent — an element that already has a
/// `uid` is left untouched, so re-running after copy/import/hand-editing
/// introduces new uid-less elements is safe.
pub fn migrate_entities(root: &Path) -> Result<MigrateReport, MigrateError> {
    // Reusing the same lock `run_startup_recovery` acquired keeps recovery
    // and the migration itself as one continuous critical section.
    let held_lock = match recovery::run_startup_recovery(root, |intent| roll_forward(root, intent))?
    {
        recovery::StartupRecovery::OperationInProgress => {
            return Err(MigrateError::OperationInProgress);
        }
        recovery::StartupRecovery::Ready { lock, .. } => lock,
    };
    let outcome = migrate_all(root);
    held_lock.release()?;
    outcome
}

fn migrate_all(root: &Path) -> Result<MigrateReport, MigrateError> {
    // Captured *before* anything below writes a single byte: this is the
    // legacy (pre-migration) identity this run's new manifest entries, if
    // any, must be keyed on — see `LegacySnapshot`'s doc comment for why
    // recomputing it from the post-migration tree instead would be wrong.
    let legacy_signatures = migration_manifest::capture_case_signatures(root)?;

    let plan = build_migration_plan(root)?;
    if !plan.report.conflicts.is_empty() {
        return Err(MigrateError::Conflicts(plan.report.conflicts));
    }
    if plan.events.is_empty() {
        // Nothing for this run to migrate at the entity level, but a
        // case's `case_uid` can still become newly computable purely
        // because an *earlier* run finished its last missing element —
        // recorded directly, since there is no risky write in progress
        // this round for `legacy_signatures` to protect against.
        migration_manifest::record_new_case_uids(root, &legacy_signatures)?;
        mark_uid_mode_if_fully_migrated(root)?;
        return Ok(plan.report);
    }
    // `legacy_signatures` is durably written into the intent here —
    // *before* `commit_batch` below reaches this operation's logical
    // commit point — so `roll_forward` (identically on the happy path and
    // from crash recovery, see its own doc comment) can still record the
    // correct migration manifest entries even if the process is killed
    // between the commit point and this call returning.
    let intent = recovery::begin_batch_with_payload(
        root,
        plan.events,
        Some(recovery::IntentPayload::IdentityMigration(
            legacy_signatures.to_durable_payload()?,
        )),
    )?;
    recovery::commit_batch(root, &intent)?;
    roll_forward(root, &intent)?;
    recovery::finish(root, &intent)?;
    mark_uid_mode_if_fully_migrated(root)?;
    Ok(plan.report)
}

/// The schema version 2 public cutover (design doc §13 Phase 5, ADR 0013
/// 「移行」節): once every element of all five `EntityKind`s carries a
/// `uid`, flips `config.toml`'s `[identity]` marker to `mode = "uid"` — the
/// single authoritative flag consumers use to decide whether uid-less
/// elements are a legitimate pre-migration state or a data-integrity
/// violation. Re-scans the working tree directly rather than trusting the
/// migration plan just committed, so it stays correct even if entities
/// appeared between planning and commit. Idempotent — cheap enough to call
/// after every `migrate_entities` run, including no-op ones.
fn mark_uid_mode_if_fully_migrated(root: &Path) -> io::Result<()> {
    for kind in EntityKind::ALL {
        if knowledge_walk::list_entities(root, kind)?
            .iter()
            .any(|entity| entity.uid.is_none())
        {
            return Ok(());
        }
    }
    marker::mark_uid_mode(root)
}

/// Computes the exact UID assignments a migration would make, across all
/// five `EntityKind`s, without writing Knowledge, identity events, locks,
/// or staging state.
pub fn plan_migration(root: &Path) -> Result<MigrateReport, MigrateError> {
    Ok(build_migration_plan(root)?.report)
}

fn relative_path_string(root: &Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn build_migration_plan(root: &Path) -> Result<MigrationPlan, MigrateError> {
    let mut report = MigrateReport::default();
    let mut events = Vec::new();
    let recorded_at = iso8601_utc_now();

    // Pass 1: enumerate every kind's elements and detect id/uid conflicts
    // *before* generating any event — a conflict in one kind must not let
    // another kind's migration proceed halfway (design doc §12's
    // all-or-nothing batch).
    let mut per_kind_entities = Vec::new();
    for kind in EntityKind::ALL {
        let entities = knowledge_walk::list_entities(root, kind)?;

        let mut ids: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        let mut uids: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for entity in &entities {
            let relative_path = relative_path_string(root, &entity.path);
            ids.entry(entity.id.clone())
                .or_default()
                .push(relative_path.clone());
            if let Some(uid) = &entity.uid {
                uids.entry(uid.clone()).or_default().push(relative_path);
            }
        }
        for (id, paths) in ids.into_iter().filter(|(_, paths)| paths.len() > 1) {
            report.conflicts.push(format!(
                "duplicate {} id '{id}': {}",
                kind.as_str(),
                paths.join(", ")
            ));
        }
        for (uid, paths) in uids.into_iter().filter(|(_, paths)| paths.len() > 1) {
            report.conflicts.push(format!(
                "duplicate {} uid '{uid}': {}",
                kind.as_str(),
                paths.join(", ")
            ));
        }

        per_kind_entities.push((kind, entities));
    }
    if !report.conflicts.is_empty() {
        return Ok(MigrationPlan { report, events });
    }

    // Pass 2: generate one root `Issued` event per uid-less element, all
    // sharing `recorded_at` (design doc §12: one operation, one honest
    // "tracking began here" timestamp).
    for (kind, entities) in per_kind_entities {
        for entity in entities {
            if entity.uid.is_some() {
                continue;
            }
            let entity_uid = ulid::Ulid::new().to_string();
            let event_uid = ulid::Ulid::new().to_string();
            let event = IdentityEvent {
                identity_event_uid: event_uid.clone(),
                entity_uid: entity_uid.clone(),
                entity_kind: kind,
                previous_identity_event_uid: None,
                previous_identity_event_uids: Vec::new(),
                recorded_at: recorded_at.clone(),
                mutation: IdentityMutation::Issued {
                    id: entity.id.clone(),
                },
            };
            events.push(recovery::BatchEvent {
                entity_kind: kind,
                entity_uid: entity_uid.clone(),
                identity_event_uid: event_uid.clone(),
                event_yaml: serde_yaml_ng::to_string(&event).map_err(io::Error::other)?,
            });
            report
                .changed_files
                .push(relative_path_string(root, &entity.path));
            report.changed_files.push(relative_path_string(
                root,
                &recovery::event_file_path(root, kind, &entity_uid, &event_uid),
            ));
            report.migrated.push(MigratedEntity {
                kind,
                id: entity.id,
                uid: entity_uid,
            });
        }
    }
    Ok(MigrationPlan { report, events })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::{self, Feature};
    use std::fs;

    /// `migrate_entities` -> `migration_manifest::capture_case_signatures`
    /// -> `git::write_tree_prefix` requires an actual git repository (it
    /// builds a real tree object via a temporary index) — every fixture
    /// used by a test that calls `migrate_entities` needs this.
    fn init_git_repo(dir: &Path) {
        let status = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .status()
                .unwrap()
        };
        assert!(status(&["init", "-q"]).success());
        assert!(status(&["config", "user.email", "test@example.com"]).success());
        assert!(status(&["config", "user.name", "Test"]).success());
        assert!(status(&["config", "core.autocrlf", "false"]).success());
    }

    fn init_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        fs::create_dir_all(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump"),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/controls/requirement.yml"),
            // Already migrated, so Feature-focused tests calling
            // `migrate_entities`/`plan_migration` see only the Feature(s)
            // they set up themselves — multi-kind migration itself is
            // covered separately below.
            "id: controls\nlabel: controls\naxis: []\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FR0\n",
        )
        .unwrap();
        dir
    }

    fn write_feature(dir: &Path, id: &str, uid: Option<&str>) {
        let feature = Feature {
            id: id.to_string(),
            requirement: "controls".to_string(),
            label: id.to_string(),
            axis: Vec::new(),
            description: None,
            forked_from: None,
            uid: uid.map(str::to_string),
        };
        fs::write(
            dir.join(".markharness/knowledge/controls/player-jump/feature.yml"),
            knowledge::serialize_feature(&feature),
        )
        .unwrap();
    }

    fn issue_uid(dir: &Path, uid: &str, id: &str) {
        let events_dir = dir.join(".markharness/identity-events/features").join(uid);
        fs::create_dir_all(&events_dir).unwrap();
        let event = IdentityEvent {
            identity_event_uid: "01ARZ3NDEKTSV4RRFFQ69G5FE0".to_string(),
            entity_uid: uid.to_string(),
            entity_kind: EntityKind::Feature,
            previous_identity_event_uid: None,
            previous_identity_event_uids: Vec::new(),
            recorded_at: "2026-08-20T00:00:00Z".to_string(),
            mutation: IdentityMutation::Issued { id: id.to_string() },
        };
        fs::write(
            events_dir.join("01ARZ3NDEKTSV4RRFFQ69G5FE0.yml"),
            serde_yaml_ng::to_string(&event).unwrap(),
        )
        .unwrap();
    }

    const UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    #[test]
    fn renames_a_migrated_feature_and_updates_feature_yml() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");

        rename_id(dir.path(), "todo-management", "task-management").unwrap();

        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        let feature: Feature = knowledge::parse_feature(&content).unwrap();
        assert_eq!(feature.id, "task-management");
        assert_eq!(feature.uid, Some(UID.to_string()));
    }

    #[test]
    fn renames_write_a_renamed_identity_event() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");

        rename_id(dir.path(), "todo-management", "task-management").unwrap();

        let events =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, UID).unwrap();
        assert_eq!(events.len(), 2);
        assert!(events.iter().any(|e| matches!(
            &e.mutation,
            IdentityMutation::Renamed { from_id, to_id }
                if from_id == "todo-management" && to_id == "task-management"
        )));
    }

    #[test]
    fn rename_fails_when_feature_has_no_uid_yet() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", None);

        let err = rename_id(dir.path(), "todo-management", "task-management").unwrap_err();
        assert!(matches!(err, RenameError::NotMigrated(id) if id == "todo-management"));
    }

    #[test]
    fn rename_fails_when_old_id_does_not_exist() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");

        let err = rename_id(dir.path(), "does-not-exist", "task-management").unwrap_err();
        assert!(matches!(err, RenameError::FeatureNotFound(id) if id == "does-not-exist"));
    }

    #[test]
    fn rename_fails_when_new_id_is_already_used_by_another_feature() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");
        fs::create_dir_all(
            dir.path()
                .join(".markharness/knowledge/controls/other-feature"),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/controls/other-feature/feature.yml"),
            "id: task-management\nrequirement: controls\nlabel: task-management\naxis: []\n",
        )
        .unwrap();

        let err = rename_id(dir.path(), "todo-management", "task-management").unwrap_err();
        assert!(matches!(err, RenameError::NewIdAlreadyInUse(id) if id == "task-management"));
    }

    /// Simulates recovering from a crash that happened after the identity
    /// event committed but before `feature.yml` was rolled forward: the
    /// next `rename_id` call (or any command doing startup recovery) must
    /// finish the job rather than leaving `feature.yml` stale forever.
    #[test]
    fn a_subsequent_command_rolls_forward_an_interrupted_rename() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");

        let intent = recovery::begin(
            dir.path(),
            EntityKind::Feature,
            UID,
            "01ARZ3NDEKTSV4RRFFQ69G5FE1",
        )
        .unwrap();
        let event = IdentityEvent {
            identity_event_uid: "01ARZ3NDEKTSV4RRFFQ69G5FE1".to_string(),
            entity_uid: UID.to_string(),
            entity_kind: EntityKind::Feature,
            previous_identity_event_uid: Some("01ARZ3NDEKTSV4RRFFQ69G5FE0".to_string()),
            previous_identity_event_uids: Vec::new(),
            recorded_at: "2026-08-20T01:00:00Z".to_string(),
            mutation: IdentityMutation::Renamed {
                from_id: "todo-management".to_string(),
                to_id: "task-management".to_string(),
            },
        };
        recovery::commit(
            dir.path(),
            &intent,
            &serde_yaml_ng::to_string(&event).unwrap(),
        )
        .unwrap();
        // Deliberately no roll_forward/finish: this is the crash point.

        // feature.yml still says the old id until recovery runs.
        let stale = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        assert!(stale.contains("id: todo-management"));

        // A later `rename-id` call for an unrelated purpose (here: renaming
        // it right back) first runs startup recovery, which must roll the
        // interrupted operation forward before doing anything else.
        rename_id(dir.path(), "task-management", "todo-management-v2").unwrap();

        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        assert!(content.contains("id: todo-management-v2"));
    }

    fn write_event(dir: &Path, uid: &str, event: &IdentityEvent) {
        let events_dir = dir.join(".markharness/identity-events/features").join(uid);
        fs::create_dir_all(&events_dir).unwrap();
        fs::write(
            events_dir.join(format!("{}.yml", event.identity_event_uid)),
            serde_yaml_ng::to_string(event).unwrap(),
        )
        .unwrap();
    }

    fn divergent_project() -> (tempfile::TempDir, String, String) {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");
        let branch_a = "01ARZ3NDEKTSV4RRFFQ69G5FE1A".to_string();
        let branch_b = "01ARZ3NDEKTSV4RRFFQ69G5FE1B".to_string();
        write_event(
            dir.path(),
            UID,
            &IdentityEvent {
                identity_event_uid: branch_a.clone(),
                entity_uid: UID.to_string(),
                entity_kind: EntityKind::Feature,
                previous_identity_event_uid: Some("01ARZ3NDEKTSV4RRFFQ69G5FE0".to_string()),
                previous_identity_event_uids: Vec::new(),
                recorded_at: "2026-08-20T01:00:00Z".to_string(),
                mutation: IdentityMutation::Renamed {
                    from_id: "todo-management".to_string(),
                    to_id: "task-management".to_string(),
                },
            },
        );
        write_event(
            dir.path(),
            UID,
            &IdentityEvent {
                identity_event_uid: branch_b.clone(),
                entity_uid: UID.to_string(),
                entity_kind: EntityKind::Feature,
                previous_identity_event_uid: Some("01ARZ3NDEKTSV4RRFFQ69G5FE0".to_string()),
                previous_identity_event_uids: Vec::new(),
                recorded_at: "2026-08-20T01:00:01Z".to_string(),
                mutation: IdentityMutation::Renamed {
                    from_id: "todo-management".to_string(),
                    to_id: "work-management".to_string(),
                },
            },
        );
        (dir, branch_a, branch_b)
    }

    #[test]
    fn resolve_divergence_picks_the_kept_head_and_updates_feature_yml() {
        let (dir, branch_a, _branch_b) = divergent_project();

        resolve_divergence(dir.path(), EntityKind::Feature, UID, &branch_a).unwrap();

        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        assert!(content.contains("id: task-management"));

        let result = registry::resolve_from_working_tree(dir.path(), EntityKind::Feature, UID)
            .unwrap()
            .unwrap();
        assert_eq!(result.current_id, "task-management");
    }

    #[test]
    fn resolve_divergence_fails_when_there_is_no_divergence() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");

        let err =
            resolve_divergence(dir.path(), EntityKind::Feature, UID, "some-event").unwrap_err();
        assert!(matches!(err, ResolveError::NoDivergence));
    }

    #[test]
    fn resolve_divergence_fails_when_keep_event_is_not_a_divergent_head() {
        let (dir, _branch_a, _branch_b) = divergent_project();

        let err = resolve_divergence(dir.path(), EntityKind::Feature, UID, "not-a-real-event")
            .unwrap_err();
        assert!(matches!(err, ResolveError::NotADivergentHead { .. }));
    }

    fn retired_project() -> tempfile::TempDir {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");
        write_event(
            dir.path(),
            UID,
            &IdentityEvent {
                identity_event_uid: "01ARZ3NDEKTSV4RRFFQ69G5FE1".to_string(),
                entity_uid: UID.to_string(),
                entity_kind: EntityKind::Feature,
                previous_identity_event_uid: Some("01ARZ3NDEKTSV4RRFFQ69G5FE0".to_string()),
                previous_identity_event_uids: Vec::new(),
                recorded_at: "2026-08-20T02:00:00Z".to_string(),
                mutation: IdentityMutation::Retired,
            },
        );
        dir
    }

    #[test]
    fn release_id_succeeds_for_a_retired_entitys_former_id() {
        let dir = retired_project();

        release_id(dir.path(), EntityKind::Feature, UID, "todo-management").unwrap();

        let events =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, UID).unwrap();
        assert!(events.iter().any(|e| matches!(
            &e.mutation,
            IdentityMutation::Released { released_id } if released_id == "todo-management"
        )));
    }

    #[test]
    fn release_id_fails_when_entity_is_not_retired() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");

        let err = release_id(dir.path(), EntityKind::Feature, UID, "todo-management").unwrap_err();
        assert!(matches!(err, ReleaseError::NotRetired));
    }

    #[test]
    fn release_id_fails_when_id_was_never_used_by_this_entity() {
        let dir = retired_project();

        let err = release_id(dir.path(), EntityKind::Feature, UID, "never-used-id").unwrap_err();
        assert!(
            matches!(err, ReleaseError::IdNeverUsedByThisEntity { released_id } if released_id == "never-used-id")
        );
    }

    /// Regression test for a *cross-operation* TOCTOU race, distinct from
    /// the same-operation races already covered by
    /// `retire_entity_never_double_commits_under_concurrent_calls` and
    /// friends: `restore_entity` and `release_id` are two *different*
    /// operations that can both legally run against the same retired
    /// entity at once (`release_id` doesn't require `Active`/`Retired` to
    /// stay put — only that it *was* `Retired` at the moment it commits).
    /// If either read its current head event *before* acquiring the lock
    /// (as both originally did), the two calls could both capture the same
    /// stale head, then each commit a new event against it — an
    /// unintended branch divergence (two children of the same parent) —
    /// instead of the second call correctly chaining onto whatever the
    /// first one just committed. Real OS threads, barrier-started
    /// together, racing `restore` against `release` on the same entity.
    #[test]
    fn restore_and_release_never_diverge_under_concurrent_calls() {
        let dir = retired_project();
        let root = dir.path();

        let barrier = std::sync::Barrier::new(2);
        let (restore_result, release_result) = std::thread::scope(|scope| {
            let barrier = &barrier;
            let restore_handle = scope.spawn(move || {
                barrier.wait();
                restore_entity(root, EntityKind::Feature, UID)
            });
            let release_handle = scope.spawn(move || {
                barrier.wait();
                release_id(root, EntityKind::Feature, UID, "todo-management")
            });
            (
                restore_handle.join().unwrap(),
                release_handle.join().unwrap(),
            )
        });

        // `IdentityLock` is fail-fast, not a queue (design doc §6, Q6): if
        // `release` wins the race for the lock, `restore`'s own attempt to
        // acquire it can be refused outright with `OperationInProgress`
        // (caught by `run_startup_recovery`) or a plain lock-contention
        // `Io` error, rather than blocking until `release` finishes. That
        // refusal is the intended, correct behavior — not a bug — so only
        // require it be a *clean* refusal, never some other logical error
        // (e.g. `NotRetired`) that would indicate `restore` read stale
        // state and proceeded on it anyway.
        assert!(
            matches!(
                restore_result,
                Ok(()) | Err(RestoreError::OperationInProgress) | Err(RestoreError::Io(_))
            ),
            "restore must either succeed or fail with a clean lock-contention \
             error, not {restore_result:?}"
        );
        // Unlike `restore`, `release`'s precondition (`Retired`) is *not*
        // invariant under the other call: if `restore` happens to run to
        // completion entirely before `release` even starts (fully
        // serialized — a legitimate, correct interleaving under a
        // fail-fast, non-queuing lock, not a bug), `release` will
        // correctly observe `Active` and be refused with `NotRetired`,
        // not a lock-contention error. Excluding that case would make this
        // test spuriously fail whenever the scheduler happens to serialize
        // the two calls that way, which is a real, expected outcome, not
        // CI flakiness to paper over.
        assert!(
            matches!(
                release_result,
                Ok(())
                    | Err(ReleaseError::OperationInProgress)
                    | Err(ReleaseError::Io(_))
                    | Err(ReleaseError::NotRetired)
            ),
            "release must either succeed or fail with a clean lock-contention \
             or already-restored error, not {release_result:?}"
        );
        let events =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, UID).unwrap();
        let replay_result = engine::replay(UID, &events).expect(
            "no branch divergence: concurrent restore/release must not both commit against \
             the same stale head",
        );
        // `restore` only actually flips status if it won the lock race;
        // losing it with a clean contention error leaves the entity
        // `Retired`, which is a correct outcome too (the caller is
        // expected to retry).
        if restore_result.is_ok() {
            assert_eq!(replay_result.status, engine::Status::Active);
        } else {
            assert_eq!(replay_result.status, engine::Status::Retired);
        }
    }

    /// Cross-operation-kind coverage beyond `restore`×`release` above: a
    /// `retire` and a `reissue` racing each other on *unrelated* entities
    /// must both converge cleanly — proving the recovery-lock handoff
    /// (`run_startup_recovery` handing its own lock straight to the
    /// caller's check-and-commit, rather than releasing and letting the
    /// caller reacquire separately) correctly serializes different
    /// operation kinds contending for the same project-wide lock, not just
    /// two calls to the same function.
    #[test]
    fn retire_and_reissue_on_unrelated_entities_never_corrupt_each_other_under_concurrent_calls() {
        let dir = init_project();
        // Entity A: ready to retire (its Knowledge file already deleted,
        // as `retire_entity` requires).
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");
        fs::remove_file(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();

        // Entity B: an unrelated, uid-less Feature ready for `reissue`
        // (no reservation on its id to bump into).
        let other_feature = Feature {
            id: "unrelated-feature".to_string(),
            requirement: "controls".to_string(),
            label: "unrelated-feature".to_string(),
            axis: Vec::new(),
            description: None,
            forked_from: None,
            uid: None,
        };
        fs::create_dir_all(dir.path().join(".markharness/knowledge/controls/unrelated")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/controls/unrelated/feature.yml"),
            knowledge::serialize_feature(&other_feature),
        )
        .unwrap();

        let root = dir.path();
        let barrier = std::sync::Barrier::new(2);
        let (retire_result, reissue_result) = std::thread::scope(|scope| {
            let barrier = &barrier;
            let retire_handle = scope.spawn(move || {
                barrier.wait();
                retire_entity(root, EntityKind::Feature, UID)
            });
            let reissue_handle = scope.spawn(move || {
                barrier.wait();
                reissue_entity(root, EntityKind::Feature, "unrelated-feature")
            });
            (
                retire_handle.join().unwrap(),
                reissue_handle.join().unwrap(),
            )
        });

        // `IdentityLock` is a single, project-wide, fail-fast (not
        // queuing) lock, so a two-way race for it almost always leaves
        // exactly one side refused with a clean contention error rather
        // than both succeeding serially — neither operation's own
        // precondition actually depends on the other (they touch
        // unrelated entities), but losing the lock race is still a
        // legitimate outcome, not a bug. What must hold regardless of who
        // wins is: no corruption — no clean, non-contention error, and no
        // branch divergence in either entity's event log.
        assert!(
            matches!(
                retire_result,
                Ok(()) | Err(RetireError::OperationInProgress) | Err(RetireError::Io(_))
            ),
            "retire must either succeed or fail with a clean lock-contention error, not {retire_result:?}"
        );
        assert!(
            matches!(
                reissue_result,
                Ok(_) | Err(ReissueError::OperationInProgress) | Err(ReissueError::Io(_))
            ),
            "reissue must either succeed or fail with a clean lock-contention error, not {reissue_result:?}"
        );

        let a_events =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, UID).unwrap();
        let a_replay = engine::replay(UID, &a_events)
            .expect("no branch divergence on entity A from the concurrent reissue on entity B");
        assert_eq!(
            a_replay.status,
            if retire_result.is_ok() {
                engine::Status::Retired
            } else {
                engine::Status::Active
            }
        );

        if let Ok(reissued) = &reissue_result {
            let b_events = registry::load_events_from_working_tree(
                dir.path(),
                EntityKind::Feature,
                &reissued.uid,
            )
            .unwrap();
            let b_replay = engine::replay(&reissued.uid, &b_events)
                .expect("no branch divergence on entity B from the concurrent retire on entity A");
            assert_eq!(b_replay.current_id, "unrelated-feature");
        }
    }

    #[test]
    fn retire_entity_appends_a_retired_event_once_the_knowledge_file_is_gone() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");
        fs::remove_file(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();

        retire_entity(dir.path(), EntityKind::Feature, UID).unwrap();

        let events =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, UID).unwrap();
        assert!(
            events
                .iter()
                .any(|e| matches!(&e.mutation, IdentityMutation::Retired))
        );
        let replay_result = engine::replay(UID, &events).unwrap();
        assert_eq!(replay_result.status, engine::Status::Retired);
    }

    #[test]
    fn retire_entity_refuses_while_the_knowledge_file_still_exists() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");

        let err = retire_entity(dir.path(), EntityKind::Feature, UID).unwrap_err();

        assert!(matches!(err, RetireError::StillPresent));
    }

    #[test]
    fn retire_entity_refuses_an_already_retired_entity() {
        let dir = retired_project();

        let err = retire_entity(dir.path(), EntityKind::Feature, UID).unwrap_err();

        assert!(matches!(err, RetireError::AlreadyRetired));
    }

    #[test]
    fn retire_entity_refuses_an_unknown_uid() {
        let dir = init_project();

        let err = retire_entity(dir.path(), EntityKind::Feature, "01UNKNOWN0000000000000000")
            .unwrap_err();

        assert!(matches!(err, RetireError::NotFound));
    }

    /// Regression test for a check-then-act race: reading events, replaying
    /// them, and checking the Knowledge file's absence are not a single
    /// atomic filesystem operation, so if that happened *before*
    /// `IdentityLock` were acquired (as it originally did), two concurrent
    /// `retire` calls for the same entity could both read the same current
    /// head and each commit a `Retired` event against it — an unintended
    /// branch divergence rather than the single linear `retired`
    /// transition ADR 0013 requires (the second call should instead be
    /// refused as `AlreadyRetired`). Running the whole check-then-commit
    /// sequence under the lock must serialize this: real OS threads,
    /// barrier-started together, contending to retire the same entity.
    #[test]
    fn retire_entity_never_double_commits_under_concurrent_calls() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");
        fs::remove_file(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        let root = dir.path();

        const ATTEMPTS: usize = 8;
        let barrier = std::sync::Barrier::new(ATTEMPTS);
        let successes = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..ATTEMPTS)
                .map(|_| {
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        retire_entity(root, EntityKind::Feature, UID)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .filter(Result::is_ok)
                .count()
        });

        assert_eq!(
            successes, 1,
            "exactly one concurrent retire attempt for the same entity must succeed"
        );
        let events =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, UID).unwrap();
        let replay_result = engine::replay(UID, &events)
            .expect("no branch divergence: exactly one Retired event must exist");
        assert_eq!(replay_result.status, engine::Status::Retired);
    }

    /// Simulates a process kill right after `retire_entity`'s logical
    /// commit point (the identity event is durably committed, but
    /// `roll_forward`/`finish` never ran) and verifies the shared recovery
    /// machinery (`run_startup_recovery`) converges the entity to
    /// `Retired` on the next run, the same guarantee already exercised for
    /// `migrate_entities`'s batch path and for a `Renamed` event in
    /// `tests/identity_lifecycle.rs`.
    #[test]
    fn retire_entity_recovers_after_a_kill_right_after_the_commit_point() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");
        fs::remove_file(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();

        let events =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, UID).unwrap();
        let replay_result = engine::replay(UID, &events).unwrap();
        let event_uid = ulid::Ulid::new().to_string();
        let intent = recovery::begin(dir.path(), EntityKind::Feature, UID, &event_uid).unwrap();
        let event = IdentityEvent {
            identity_event_uid: event_uid,
            entity_uid: UID.to_string(),
            entity_kind: EntityKind::Feature,
            previous_identity_event_uid: Some(replay_result.current_head_event_uid.clone()),
            previous_identity_event_uids: Vec::new(),
            recorded_at: iso8601_utc_now(),
            mutation: IdentityMutation::Retired,
        };
        let event_yaml = serde_yaml_ng::to_string(&event).unwrap();
        recovery::commit(dir.path(), &intent, &event_yaml).unwrap();
        // Deliberately no roll_forward/finish call: this is the crash point.

        match recovery::run_startup_recovery(dir.path(), |intent| roll_forward(dir.path(), intent))
            .unwrap()
        {
            recovery::StartupRecovery::Ready { lock, .. } => lock.release().unwrap(),
            other => panic!("expected recovery to replay the interrupted intent, got {other:?}"),
        }

        let events =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, UID).unwrap();
        let replay_result = engine::replay(UID, &events).unwrap();
        assert_eq!(replay_result.status, engine::Status::Retired);
    }

    /// The other crash boundary for a single-event operation: the intent
    /// is staged, but the event itself is never written (`commit` never
    /// ran). Recovery must discard the leftover intent and leave the
    /// entity exactly as it was before the attempted retire — not
    /// half-apply it.
    #[test]
    fn retire_entity_discards_an_uncommitted_intent_and_leaves_the_entity_unchanged() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");
        fs::remove_file(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        let events_before =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, UID).unwrap();

        let event_uid = ulid::Ulid::new().to_string();
        recovery::begin(dir.path(), EntityKind::Feature, UID, &event_uid).unwrap();
        // Deliberately no `commit`/`roll_forward`/`finish` call: this is
        // the pre-commit crash point.

        match recovery::run_startup_recovery(dir.path(), |intent| roll_forward(dir.path(), intent))
            .unwrap()
        {
            recovery::StartupRecovery::Ready { lock, .. } => lock.release().unwrap(),
            other => panic!("expected recovery to discard the uncommitted intent, got {other:?}"),
        }

        let events_after =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, UID).unwrap();
        assert_eq!(
            events_after, events_before,
            "an uncommitted retire attempt must leave the entity's event log untouched"
        );
        let replay_result = engine::replay(UID, &events_after).unwrap();
        assert_eq!(replay_result.status, engine::Status::Active);
    }

    #[test]
    fn restore_entity_flips_a_retired_entity_back_to_active() {
        let dir = retired_project();

        restore_entity(dir.path(), EntityKind::Feature, UID).unwrap();

        let events =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, UID).unwrap();
        let replay_result = engine::replay(UID, &events).unwrap();
        assert_eq!(replay_result.status, engine::Status::Active);
    }

    #[test]
    fn restore_entity_refuses_an_entity_that_is_not_retired() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");

        let err = restore_entity(dir.path(), EntityKind::Feature, UID).unwrap_err();

        assert!(matches!(err, RestoreError::NotRetired));
    }

    /// Same TOCTOU concern and fix as
    /// `retire_entity_never_double_commits_under_concurrent_calls`, for
    /// `restore_entity`: real OS threads, barrier-started together,
    /// contending to restore the same retired entity.
    #[test]
    fn restore_entity_never_double_commits_under_concurrent_calls() {
        let dir = retired_project();
        let root = dir.path();

        const ATTEMPTS: usize = 8;
        let barrier = std::sync::Barrier::new(ATTEMPTS);
        let successes = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..ATTEMPTS)
                .map(|_| {
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        restore_entity(root, EntityKind::Feature, UID)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .filter(Result::is_ok)
                .count()
        });

        assert_eq!(
            successes, 1,
            "exactly one concurrent restore attempt for the same entity must succeed"
        );
        let events =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, UID).unwrap();
        let replay_result = engine::replay(UID, &events)
            .expect("no branch divergence: exactly one Restored event must exist");
        assert_eq!(replay_result.status, engine::Status::Active);
    }

    /// Same crash-recovery guarantee as
    /// `retire_entity_recovers_after_a_kill_right_after_the_commit_point`,
    /// for a `Restored` event.
    #[test]
    fn restore_entity_recovers_after_a_kill_right_after_the_commit_point() {
        let dir = retired_project();

        let events =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, UID).unwrap();
        let replay_result = engine::replay(UID, &events).unwrap();
        assert_eq!(replay_result.status, engine::Status::Retired);
        let event_uid = ulid::Ulid::new().to_string();
        let intent = recovery::begin(dir.path(), EntityKind::Feature, UID, &event_uid).unwrap();
        let event = IdentityEvent {
            identity_event_uid: event_uid,
            entity_uid: UID.to_string(),
            entity_kind: EntityKind::Feature,
            previous_identity_event_uid: Some(replay_result.current_head_event_uid.clone()),
            previous_identity_event_uids: Vec::new(),
            recorded_at: iso8601_utc_now(),
            mutation: IdentityMutation::Restored,
        };
        let event_yaml = serde_yaml_ng::to_string(&event).unwrap();
        recovery::commit(dir.path(), &intent, &event_yaml).unwrap();
        // Deliberately no roll_forward/finish call: this is the crash point.

        match recovery::run_startup_recovery(dir.path(), |intent| roll_forward(dir.path(), intent))
            .unwrap()
        {
            recovery::StartupRecovery::Ready { lock, .. } => lock.release().unwrap(),
            other => panic!("expected recovery to replay the interrupted intent, got {other:?}"),
        }

        let events =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, UID).unwrap();
        let replay_result = engine::replay(UID, &events).unwrap();
        assert_eq!(replay_result.status, engine::Status::Active);
    }

    /// Pre-commit crash boundary for `restore`, mirroring
    /// `retire_entity_discards_an_uncommitted_intent_and_leaves_the_entity_unchanged`.
    #[test]
    fn restore_entity_discards_an_uncommitted_intent_and_leaves_the_entity_unchanged() {
        let dir = retired_project();
        let events_before =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, UID).unwrap();

        let event_uid = ulid::Ulid::new().to_string();
        recovery::begin(dir.path(), EntityKind::Feature, UID, &event_uid).unwrap();
        // Deliberately no `commit`/`roll_forward`/`finish` call: this is
        // the pre-commit crash point.

        match recovery::run_startup_recovery(dir.path(), |intent| roll_forward(dir.path(), intent))
            .unwrap()
        {
            recovery::StartupRecovery::Ready { lock, .. } => lock.release().unwrap(),
            other => panic!("expected recovery to discard the uncommitted intent, got {other:?}"),
        }

        let events_after =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, UID).unwrap();
        assert_eq!(
            events_after, events_before,
            "an uncommitted restore attempt must leave the entity's event log untouched"
        );
        let replay_result = engine::replay(UID, &events_after).unwrap();
        assert_eq!(replay_result.status, engine::Status::Retired);
    }

    /// If the Knowledge file is re-created (uid-less, same id) before
    /// `restore_entity` runs, `restore`'s own roll-forward step folds the
    /// `uid:` back into it — the same fallback-by-id mechanism `identity
    /// migrate` uses to fill in a fresh element's `uid:`.
    #[test]
    fn restore_entity_resyncs_a_knowledge_file_recreated_before_it_runs() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");
        fs::remove_file(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        retire_entity(dir.path(), EntityKind::Feature, UID).unwrap();
        write_feature(dir.path(), "todo-management", None);

        restore_entity(dir.path(), EntityKind::Feature, UID).unwrap();

        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        let feature: Feature = knowledge::parse_feature(&content).unwrap();
        assert_eq!(feature.uid.as_deref(), Some(UID));
    }

    /// `sync_entity` must work for every `EntityKind`, not just Feature —
    /// Requirement/Behavior/Condition/ExpectedResult have no `rename-id`
    /// equivalent to fall back on, so without a general sync operation a
    /// file recreated *after* `restore_entity` (rather than before) could
    /// never have its `uid:` filled back in for these kinds at all.
    #[test]
    fn sync_entity_resyncs_a_non_feature_kind_file_recreated_after_restore() {
        let dir = init_project();
        const REQ_UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FQ1";
        fs::create_dir_all(dir.path().join(".markharness/knowledge/req-x")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/req-x/requirement.yml"),
            format!("id: req-x\nlabel: req-x\naxis: []\nuid: {REQ_UID}\n"),
        )
        .unwrap();
        let events_dir = dir
            .path()
            .join(".markharness/identity-events/requirements")
            .join(REQ_UID);
        fs::create_dir_all(&events_dir).unwrap();
        let issued = IdentityEvent {
            identity_event_uid: "01ARZ3NDEKTSV4RRFFQ69G5FQ0".to_string(),
            entity_uid: REQ_UID.to_string(),
            entity_kind: EntityKind::Requirement,
            previous_identity_event_uid: None,
            previous_identity_event_uids: Vec::new(),
            recorded_at: "2026-08-20T00:00:00Z".to_string(),
            mutation: IdentityMutation::Issued {
                id: "req-x".to_string(),
            },
        };
        fs::write(
            events_dir.join("01ARZ3NDEKTSV4RRFFQ69G5FQ0.yml"),
            serde_yaml_ng::to_string(&issued).unwrap(),
        )
        .unwrap();

        fs::remove_file(
            dir.path()
                .join(".markharness/knowledge/req-x/requirement.yml"),
        )
        .unwrap();
        retire_entity(dir.path(), EntityKind::Requirement, REQ_UID).unwrap();
        restore_entity(dir.path(), EntityKind::Requirement, REQ_UID).unwrap();
        // Recreate the file *after* restore — the file has no `uid:` yet,
        // and (for this kind) no other command could ever fill it in.
        fs::write(
            dir.path()
                .join(".markharness/knowledge/req-x/requirement.yml"),
            "id: req-x\nlabel: req-x\naxis: []\n",
        )
        .unwrap();

        sync_entity(dir.path(), EntityKind::Requirement, REQ_UID).unwrap();

        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/req-x/requirement.yml"),
        )
        .unwrap();
        let requirement: knowledge::Requirement = knowledge::parse_requirement(&content).unwrap();
        assert_eq!(requirement.uid.as_deref(), Some(REQ_UID));
    }

    #[test]
    fn sync_entity_refuses_an_unknown_uid() {
        let dir = init_project();

        let err =
            sync_entity(dir.path(), EntityKind::Feature, "01UNKNOWN0000000000000000").unwrap_err();

        assert!(matches!(err, SyncError::NotFound));
    }

    /// A `Retired` entity's Knowledge element must not reappear via `sync`
    /// alone: without a `Restored` event, writing its `uid:` back into a
    /// same-id file the caller (re)created would resurrect a deleted
    /// element's presence on disk while its identity event log still says
    /// `retired`.
    #[test]
    fn sync_entity_refuses_a_retired_entity() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");
        fs::remove_file(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        retire_entity(dir.path(), EntityKind::Feature, UID).unwrap();
        write_feature(dir.path(), "todo-management", None);

        let err = sync_entity(dir.path(), EntityKind::Feature, UID).unwrap_err();

        assert!(matches!(err, SyncError::NotActive(engine::Status::Retired)));
        // The refused sync must not have written the uid back in.
        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        let feature: Feature = knowledge::parse_feature(&content).unwrap();
        assert_eq!(feature.uid, None);
    }

    #[test]
    fn reissue_entity_assigns_a_brand_new_uid_and_records_the_source_uid() {
        let dir = init_project();
        // A `uid:` copied in from another repository as-is: no local
        // `.markharness/identity-events/` entry for it, so it has no live
        // local identity for `reissue` to protect (ADR 0013's actual
        // copy/import scenario — contrast with
        // `reissue_entity_refuses_when_the_current_uid_has_a_live_local_identity`,
        // where the uid *does* have a live local identity and must be
        // retired first).
        let foreign_uid = "01FOREIGN00000000000000000";
        write_feature(dir.path(), "todo-management", Some(foreign_uid));

        let reissued = reissue_entity(dir.path(), EntityKind::Feature, "todo-management").unwrap();

        assert_ne!(reissued.uid, foreign_uid);
        assert_eq!(reissued.source_uid.as_deref(), Some(foreign_uid));
        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        let feature: Feature = knowledge::parse_feature(&content).unwrap();
        assert_eq!(feature.uid.as_deref(), Some(reissued.uid.as_str()));
        let events =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, &reissued.uid)
                .unwrap();
        assert!(events.iter().any(|e| matches!(
            &e.mutation,
            IdentityMutation::Reissued { id, source_uid }
                if id == "todo-management" && source_uid.as_deref() == Some(foreign_uid)
        )));
        // The foreign uid never had a local event log, and still doesn't —
        // reissue does not fabricate one.
        let old_events =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, foreign_uid)
                .unwrap();
        assert!(old_events.is_empty());
    }

    /// Regression test: a Knowledge file *does* carry a `uid:` (a foreign
    /// one copied in from elsewhere, with no local event log of its own —
    /// same setup as
    /// `reissue_entity_assigns_a_brand_new_uid_and_records_the_source_uid`),
    /// but some *other*, genuinely local UID still holds an unreleased
    /// reservation on the same id. Checking only the file's own `uid:`
    /// would find nothing to object to (the foreign uid has no log) and
    /// wrongly let the reissue through — the repository-wide scan must run
    /// unconditionally, not only when the file is uid-less.
    #[test]
    fn reissue_entity_refuses_when_another_local_uid_reserves_the_id_even_though_the_file_has_a_uid()
     {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");
        fs::remove_file(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        retire_entity(dir.path(), EntityKind::Feature, UID).unwrap();
        // Recreated with a *different*, foreign uid (no local event log of
        // its own) rather than uid-less — `UID`'s unreleased reservation
        // must still block the reissue.
        let foreign_uid = "01FOREIGN00000000000000000";
        write_feature(dir.path(), "todo-management", Some(foreign_uid));

        let err = reissue_entity(dir.path(), EntityKind::Feature, "todo-management").unwrap_err();

        assert!(matches!(
            err,
            ReissueError::IdReservedByAnotherUid { holder_uid, id }
                if holder_uid == UID && id == "todo-management"
        ));
    }

    #[test]
    fn reissue_entity_works_on_a_uid_less_element_and_records_no_source_uid() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", None);

        let reissued = reissue_entity(dir.path(), EntityKind::Feature, "todo-management").unwrap();

        assert_eq!(reissued.source_uid, None);
        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        let feature: Feature = knowledge::parse_feature(&content).unwrap();
        assert_eq!(feature.uid.as_deref(), Some(reissued.uid.as_str()));
    }

    #[test]
    fn reissue_entity_refuses_an_unknown_id() {
        let dir = init_project();

        let err = reissue_entity(dir.path(), EntityKind::Feature, "no-such-id").unwrap_err();

        assert!(matches!(err, ReissueError::NotFound(id) if id == "no-such-id"));
    }

    /// Same crash-recovery guarantee as
    /// `retire_entity_recovers_after_a_kill_right_after_the_commit_point`,
    /// for a root `Reissued` event (no predecessor, unlike retire/restore).
    #[test]
    fn reissue_entity_recovers_after_a_kill_right_after_the_commit_point() {
        let dir = init_project();
        let foreign_uid = "01FOREIGN00000000000000000";
        write_feature(dir.path(), "todo-management", Some(foreign_uid));

        let new_uid = ulid::Ulid::new().to_string();
        let event_uid = ulid::Ulid::new().to_string();
        let intent =
            recovery::begin(dir.path(), EntityKind::Feature, &new_uid, &event_uid).unwrap();
        let event = IdentityEvent {
            identity_event_uid: event_uid,
            entity_uid: new_uid.clone(),
            entity_kind: EntityKind::Feature,
            previous_identity_event_uid: None,
            previous_identity_event_uids: Vec::new(),
            recorded_at: iso8601_utc_now(),
            mutation: IdentityMutation::Reissued {
                id: "todo-management".to_string(),
                source_uid: Some(foreign_uid.to_string()),
            },
        };
        let event_yaml = serde_yaml_ng::to_string(&event).unwrap();
        recovery::commit(dir.path(), &intent, &event_yaml).unwrap();
        // Deliberately no roll_forward/finish call: this is the crash point.

        match recovery::run_startup_recovery(dir.path(), |intent| roll_forward(dir.path(), intent))
            .unwrap()
        {
            recovery::StartupRecovery::Ready { lock, .. } => lock.release().unwrap(),
            other => panic!("expected recovery to replay the interrupted intent, got {other:?}"),
        }

        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        let feature: Feature = knowledge::parse_feature(&content).unwrap();
        assert_eq!(feature.uid.as_deref(), Some(new_uid.as_str()));
    }

    /// Pre-commit crash boundary for `reissue`, mirroring
    /// `retire_entity_discards_an_uncommitted_intent_and_leaves_the_entity_unchanged`.
    /// Unlike retire/restore, a reissue's event is a *root* for a
    /// brand-new `entity_uid` — recovery discarding it before commit must
    /// leave that uid with no event log at all, and the Knowledge file
    /// untouched (still carrying whatever `uid:` it had before).
    #[test]
    fn reissue_entity_discards_an_uncommitted_intent_and_leaves_the_entity_unchanged() {
        let dir = init_project();
        let foreign_uid = "01FOREIGN00000000000000000";
        write_feature(dir.path(), "todo-management", Some(foreign_uid));

        let new_uid = ulid::Ulid::new().to_string();
        let event_uid = ulid::Ulid::new().to_string();
        recovery::begin(dir.path(), EntityKind::Feature, &new_uid, &event_uid).unwrap();
        // Deliberately no `commit`/`roll_forward`/`finish` call: this is
        // the pre-commit crash point.

        match recovery::run_startup_recovery(dir.path(), |intent| roll_forward(dir.path(), intent))
            .unwrap()
        {
            recovery::StartupRecovery::Ready { lock, .. } => lock.release().unwrap(),
            other => panic!("expected recovery to discard the uncommitted intent, got {other:?}"),
        }

        let new_uid_events =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, &new_uid)
                .unwrap();
        assert!(
            new_uid_events.is_empty(),
            "an uncommitted reissue must never leave a partial event log for the new uid"
        );
        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        let feature: Feature = knowledge::parse_feature(&content).unwrap();
        assert_eq!(
            feature.uid.as_deref(),
            Some(foreign_uid),
            "an uncommitted reissue must leave the Knowledge file's uid untouched"
        );
    }

    /// ADR 0013: "UIDs and IDs must each be unique within one snapshot,"
    /// and reassigning an id away from an active UID requires an explicit
    /// retire + release, not a silent reissue. If the element's current
    /// `uid:` still has a live, non-retired local identity, `reissue`
    /// must refuse rather than leave that old UID's event log claiming
    /// "active" for an id the Knowledge file no longer points at.
    #[test]
    fn reissue_entity_refuses_when_the_current_uid_has_a_live_local_identity() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");

        let err = reissue_entity(dir.path(), EntityKind::Feature, "todo-management").unwrap_err();

        assert!(matches!(
            err,
            ReissueError::SourceIdNotReleased { source_uid, id }
                if source_uid == UID && id == "todo-management"
        ));
        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        let feature: Feature = knowledge::parse_feature(&content).unwrap();
        assert_eq!(
            feature.uid.as_deref(),
            Some(UID),
            "a refused reissue must not have touched the Knowledge file"
        );
    }

    /// ADR 0013: "Once an ID has been issued to a UID, it cannot be
    /// assigned to another UID unless an explicit `release` event lifts
    /// that reservation" — retiring the old UID alone is not enough, the
    /// id itself stays reserved.
    #[test]
    fn reissue_entity_refuses_when_retired_but_not_released() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");
        fs::remove_file(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        retire_entity(dir.path(), EntityKind::Feature, UID).unwrap();
        // Recreated still carrying the old (retired, not yet released) uid
        // — e.g. a hand-edit or a stale copy brought it back.
        write_feature(dir.path(), "todo-management", Some(UID));

        let err = reissue_entity(dir.path(), EntityKind::Feature, "todo-management").unwrap_err();

        assert!(matches!(
            err,
            ReissueError::SourceIdNotReleased { source_uid, id }
                if source_uid == UID && id == "todo-management"
        ));
    }

    /// Once the old UID is retired *and* the id explicitly released (the
    /// ADR-mandated path), reissuing the same id must succeed.
    #[test]
    fn reissue_entity_succeeds_once_the_id_is_released() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");
        fs::remove_file(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        retire_entity(dir.path(), EntityKind::Feature, UID).unwrap();
        release_id(dir.path(), EntityKind::Feature, UID, "todo-management").unwrap();
        write_feature(dir.path(), "todo-management", Some(UID));

        let reissued = reissue_entity(dir.path(), EntityKind::Feature, "todo-management").unwrap();

        assert_ne!(reissued.uid, UID);
        assert_eq!(reissued.source_uid.as_deref(), Some(UID));
    }

    /// A uid-less recreation (no residual `uid:` field at all) has no
    /// `source_uid` to check against, so it is never subject to the
    /// released-id requirement — this is the ordinary "element was
    /// deleted, retired, and a genuinely fresh replacement was created"
    /// path, not a reissue-over-a-reservation.
    /// A uid-less recreation has no `uid:` field for `reissue_entity`'s
    /// own-file lookup to check, but the id is still reserved by `UID`'s
    /// event log (retired, never released) — the repository-wide scan
    /// must catch this even though the Knowledge file itself carries no
    /// evidence of the reservation. Regression test for the gap flagged by
    /// the Codex adversarial review: a uid-less recreation must not bypass
    /// another UID's still-live reservation.
    #[test]
    fn reissue_entity_refuses_a_uid_less_recreation_that_bypasses_anothers_reservation() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");
        fs::remove_file(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        retire_entity(dir.path(), EntityKind::Feature, UID).unwrap();
        write_feature(dir.path(), "todo-management", None);

        let err = reissue_entity(dir.path(), EntityKind::Feature, "todo-management").unwrap_err();

        assert!(matches!(
            err,
            ReissueError::IdReservedByAnotherUid { holder_uid, id }
                if holder_uid == UID && id == "todo-management"
        ));
    }

    /// Once the reserving UID's id is explicitly released, the same
    /// uid-less recreation must succeed.
    #[test]
    fn reissue_entity_succeeds_for_a_uid_less_recreation_once_the_reservation_is_released() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");
        fs::remove_file(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        retire_entity(dir.path(), EntityKind::Feature, UID).unwrap();
        release_id(dir.path(), EntityKind::Feature, UID, "todo-management").unwrap();
        write_feature(dir.path(), "todo-management", None);

        let reissued = reissue_entity(dir.path(), EntityKind::Feature, "todo-management").unwrap();

        assert_ne!(reissued.uid, UID);
        assert_eq!(reissued.source_uid, None);
    }

    /// Regression test for an event-ordering bug in the reservation check:
    /// an unordered "does a matching `Released` event exist anywhere in
    /// the log" scan would treat this id as released just because *some*
    /// `Released` event for it exists, even though `UID` reclaimed the id
    /// (via `restore`, ADR 0013's one exception to needing an explicit
    /// `release`) and retired *again* afterward with no second release.
    /// The most recent touch of the id must decide reservation, not "any"
    /// touch.
    #[test]
    fn reissue_entity_refuses_a_reservation_reclaimed_by_restore_after_an_earlier_release() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");
        fs::remove_file(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        retire_entity(dir.path(), EntityKind::Feature, UID).unwrap();
        release_id(dir.path(), EntityKind::Feature, UID, "todo-management").unwrap();
        // `UID` reclaims "todo-management" via restore (no new claim event
        // is recorded for the id — `restore` alone is the ADR-sanctioned
        // exception), then is retired *again* without a second release.
        restore_entity(dir.path(), EntityKind::Feature, UID).unwrap();
        retire_entity(dir.path(), EntityKind::Feature, UID).unwrap();
        write_feature(dir.path(), "todo-management", None);

        let err = reissue_entity(dir.path(), EntityKind::Feature, "todo-management").unwrap_err();

        assert!(matches!(
            err,
            ReissueError::IdReservedByAnotherUid { holder_uid, id }
                if holder_uid == UID && id == "todo-management"
        ));
    }

    /// Same reclaim-then-retire-again sequence, but releasing a *second*
    /// time before reissuing must succeed — proving the fix distinguishes
    /// "released, matching the most recent claim" from "released at some
    /// point in the past."
    #[test]
    fn reissue_entity_succeeds_once_a_reclaimed_reservation_is_released_again() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");
        fs::remove_file(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        retire_entity(dir.path(), EntityKind::Feature, UID).unwrap();
        release_id(dir.path(), EntityKind::Feature, UID, "todo-management").unwrap();
        restore_entity(dir.path(), EntityKind::Feature, UID).unwrap();
        retire_entity(dir.path(), EntityKind::Feature, UID).unwrap();
        release_id(dir.path(), EntityKind::Feature, UID, "todo-management").unwrap();
        write_feature(dir.path(), "todo-management", None);

        let reissued = reissue_entity(dir.path(), EntityKind::Feature, "todo-management").unwrap();

        assert_ne!(reissued.uid, UID);
        assert_eq!(reissued.source_uid, None);
    }

    /// Regression test for a check-then-act race: reading every candidate
    /// UID's reservation state is not a single atomic filesystem
    /// operation, so if that read happened *before* `IdentityLock` were
    /// acquired (as it originally did), two concurrent `reissue` calls for
    /// the same unreserved id could both observe "not reserved," both
    /// eventually acquire the lock in turn, and both commit a root
    /// `Reissued` event — leaving two different UIDs claiming the same id.
    /// Running every check under the lock (acquired up front, held through
    /// the commit) must serialize this: real OS threads, barrier-started
    /// together, contending for the same uid-less, unreserved id.
    #[test]
    fn reissue_entity_never_double_commits_under_concurrent_calls_for_the_same_id() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", None);
        let root = dir.path();

        const ATTEMPTS: usize = 8;
        let barrier = std::sync::Barrier::new(ATTEMPTS);
        let successes = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..ATTEMPTS)
                .map(|_| {
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        reissue_entity(root, EntityKind::Feature, "todo-management")
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .filter(Result::is_ok)
                .count()
        });

        assert_eq!(
            successes, 1,
            "exactly one concurrent reissue attempt for the same id must succeed"
        );
    }

    #[test]
    fn migrate_features_assigns_a_uid_and_writes_an_issued_event() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", None);

        let report = migrate_entities(dir.path()).unwrap();

        assert_eq!(report.migrated.len(), 1);
        assert_eq!(report.migrated[0].id, "todo-management");
        assert!(report.conflicts.is_empty());
        assert_eq!(report.changed_files.len(), 2);
        let assigned_uid = report.migrated[0].uid.clone();

        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        let feature: Feature = knowledge::parse_feature(&content).unwrap();
        assert_eq!(feature.uid, Some(assigned_uid.clone()));

        let events =
            registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, &assigned_uid)
                .unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].mutation,
            IdentityMutation::Issued { id } if id == "todo-management"
        ));
    }

    #[test]
    fn migrate_features_is_idempotent_and_skips_an_already_migrated_feature() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");

        let report = migrate_entities(dir.path()).unwrap();

        assert!(report.migrated.is_empty());
        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        let feature: Feature = knowledge::parse_feature(&content).unwrap();
        assert_eq!(feature.uid, Some(UID.to_string()));
    }

    #[test]
    fn migrate_features_only_touches_features_without_a_uid_when_mixed() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");
        fs::create_dir_all(dir.path().join(".markharness/knowledge/controls/second")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/controls/second/feature.yml"),
            "id: second-feature\nrequirement: controls\nlabel: second-feature\naxis: []\n",
        )
        .unwrap();

        let report = migrate_entities(dir.path()).unwrap();

        assert_eq!(report.migrated.len(), 1);
        assert_eq!(report.migrated[0].id, "second-feature");
    }

    #[test]
    fn plan_feature_migration_reports_uids_without_modifying_the_project() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", None);

        let report = plan_migration(dir.path()).unwrap();

        assert_eq!(report.migrated.len(), 1);
        assert_eq!(report.migrated[0].id, "todo-management");
        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        )
        .unwrap();
        assert!(!content.contains("uid:"));
        assert!(!dir.path().join(".markharness/identity-events").exists());
    }

    #[test]
    fn plan_feature_migration_reports_duplicate_ids_without_writing() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", None);
        fs::create_dir_all(dir.path().join(".markharness/knowledge/controls/second")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/controls/second/feature.yml"),
            "id: todo-management\nrequirement: controls\nlabel: duplicate\naxis: []\n",
        )
        .unwrap();

        let report = plan_migration(dir.path()).unwrap();

        assert!(report.migrated.is_empty());
        assert_eq!(report.conflicts.len(), 1);
        assert!(report.changed_files.is_empty());
        assert!(!dir.path().join(".markharness/identity-events").exists());
    }

    #[test]
    fn migrate_features_rejects_conflicts_before_the_batch_commit_point() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", None);
        fs::create_dir_all(dir.path().join(".markharness/knowledge/controls/second")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/controls/second/feature.yml"),
            "id: todo-management\nrequirement: controls\nlabel: duplicate\naxis: []\n",
        )
        .unwrap();

        let result = migrate_entities(dir.path());

        assert!(matches!(result, Err(MigrateError::Conflicts(_))));
        assert!(!dir.path().join(".markharness/identity-events").exists());
        assert!(!dir.path().join(".markharness/.identity-staging").exists());
    }

    #[test]
    fn one_migration_records_one_operation_timestamp_for_every_feature() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", None);
        fs::create_dir_all(dir.path().join(".markharness/knowledge/controls/second")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/controls/second/feature.yml"),
            "id: second-feature\nrequirement: controls\nlabel: second-feature\naxis: []\n",
        )
        .unwrap();

        let report = migrate_entities(dir.path()).unwrap();
        let recorded_at: std::collections::BTreeSet<String> = report
            .migrated
            .iter()
            .map(|migrated| {
                registry::load_events_from_working_tree(
                    dir.path(),
                    EntityKind::Feature,
                    &migrated.uid,
                )
                .unwrap()[0]
                    .recorded_at
                    .clone()
            })
            .collect();

        assert_eq!(recorded_at.len(), 1);
    }

    /// A working tree with one uid-less element of every kind:
    /// req -> feature -> behavior -> condition -> expected/001.yml.
    fn full_tree_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        let base = dir
            .path()
            .join(".markharness/knowledge/req/feature/behavior/condition");
        fs::create_dir_all(base.join("expected")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/req/requirement.yml"),
            "id: req\nlabel: req\naxis: []\n",
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/req/feature/feature.yml"),
            "id: feature\nrequirement: req\nlabel: feature\naxis: []\n",
        )
        .unwrap();
        fs::write(
            base.parent().unwrap().join("behavior.yml"),
            "id: behavior\nfeature: feature\nlabel: behavior\naxis: []\ndescription: |\n  d\n",
        )
        .unwrap();
        fs::write(
            base.join("condition.yml"),
            "id: condition\nbehavior: behavior\nlabel: condition\ndescription: |\n  d\n",
        )
        .unwrap();
        fs::write(
            base.join("expected/001.yml"),
            "id: condition-001\ncondition: condition\ndescription: |\n  d\n",
        )
        .unwrap();
        dir
    }

    /// Step 30: `identity migrate` must cover all five `EntityKind`s in a
    /// single operation, not just Feature.
    #[test]
    fn migrate_entities_assigns_a_uid_to_every_kind_in_one_operation() {
        let dir = full_tree_project();

        let report = migrate_entities(dir.path()).unwrap();

        let kinds: std::collections::BTreeSet<EntityKind> =
            report.migrated.iter().map(|m| m.kind).collect();
        assert_eq!(
            kinds,
            EntityKind::ALL.into_iter().collect(),
            "expected every EntityKind to be migrated, got {report:?}"
        );
        assert_eq!(report.migrated.len(), 5);
    }

    /// The batch is genuinely one operation: every migrated element,
    /// across all five kinds, shares the same `recorded_at`.
    #[test]
    fn migrate_entities_shares_one_recorded_at_across_every_kind() {
        let dir = full_tree_project();

        let report = migrate_entities(dir.path()).unwrap();

        let recorded_at: std::collections::BTreeSet<String> = report
            .migrated
            .iter()
            .map(|migrated| {
                registry::load_events_from_working_tree(dir.path(), migrated.kind, &migrated.uid)
                    .unwrap()[0]
                    .recorded_at
                    .clone()
            })
            .collect();

        assert_eq!(recorded_at.len(), 1);
    }

    /// Migrating a Behavior must write its `uid:` into `behavior.yml`
    /// specifically (not silently succeed while touching nothing, and not
    /// misfire onto some other kind's file).
    #[test]
    fn migrate_entities_writes_uid_into_the_behavior_yml_itself() {
        let dir = full_tree_project();

        let report = migrate_entities(dir.path()).unwrap();

        let behavior_migration = report
            .migrated
            .iter()
            .find(|m| m.kind == EntityKind::Behavior)
            .unwrap();
        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/req/feature/behavior/behavior.yml"),
        )
        .unwrap();
        let behavior: knowledge::Behavior = knowledge::parse_behavior(&content).unwrap();
        assert_eq!(behavior.uid, Some(behavior_migration.uid.clone()));
        assert_eq!(behavior.feature, "feature", "other fields must survive");
    }

    /// Duplicate ids in *different* kinds (e.g. a Requirement and a
    /// Feature both named "shared") are not a conflict — only a
    /// duplicate within the same kind is.
    #[test]
    fn migrate_entities_does_not_treat_the_same_id_in_different_kinds_as_a_conflict() {
        let dir = full_tree_project();
        // Reuse "req"'s id for the Feature too: allowed, different kinds.
        fs::write(
            dir.path()
                .join(".markharness/knowledge/req/feature/feature.yml"),
            "id: req\nrequirement: req\nlabel: feature\naxis: []\n",
        )
        .unwrap();

        let report = migrate_entities(dir.path()).unwrap();

        assert!(report.conflicts.is_empty());
        assert_eq!(report.migrated.len(), 5);
    }

    /// Step 34 (design doc §13 Phase 5): once every element of every kind
    /// carries a `uid`, `migrate_entities` must flip the project marker to
    /// `mode = "uid"` in the same operation, since that marker — not a
    /// count of migrated Features — is what future consumers check.
    #[test]
    fn migrate_entities_marks_uid_mode_once_every_kind_is_fully_migrated() {
        let dir = full_tree_project();
        assert!(!marker::is_uid_mode(dir.path()).unwrap());

        migrate_entities(dir.path()).unwrap();

        assert!(marker::is_uid_mode(dir.path()).unwrap());
    }

    /// A partial migration (e.g. only some kinds fully processed, or a
    /// project with a still-unmigrated element added after cutover) must
    /// not flip the marker — mode = "uid" is an all-five-kinds guarantee.
    #[test]
    fn migrate_entities_does_not_mark_uid_mode_while_conflicts_block_the_run() {
        let dir = full_tree_project();
        fs::create_dir_all(
            dir.path()
                .join(".markharness/knowledge/req/feature/other-behavior"),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/req/feature/other-behavior/behavior.yml"),
            "id: behavior\nfeature: feature\nlabel: other\naxis: []\ndescription: |\n  d\n",
        )
        .unwrap();

        let result = migrate_entities(dir.path());

        assert!(matches!(result, Err(MigrateError::Conflicts(_))));
        assert!(!marker::is_uid_mode(dir.path()).unwrap());
    }

    /// Re-running `migrate_entities` on an already fully migrated project
    /// (the common no-op path) must still leave the marker set — the
    /// no-events branch has its own call site for
    /// `mark_uid_mode_if_fully_migrated`.
    #[test]
    fn migrate_entities_keeps_uid_mode_marked_on_a_no_op_rerun() {
        let dir = full_tree_project();
        migrate_entities(dir.path()).unwrap();
        assert!(marker::is_uid_mode(dir.path()).unwrap());

        let report = migrate_entities(dir.path()).unwrap();

        assert!(report.migrated.is_empty());
        assert!(marker::is_uid_mode(dir.path()).unwrap());
    }

    /// A duplicate id *within* one kind (two Behaviors both named
    /// "behavior") must still be rejected, generalizing the earlier
    /// Feature-only conflict check to every kind.
    #[test]
    fn migrate_entities_rejects_a_duplicate_id_within_a_single_kind() {
        let dir = full_tree_project();
        fs::create_dir_all(
            dir.path()
                .join(".markharness/knowledge/req/feature/other-behavior"),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/req/feature/other-behavior/behavior.yml"),
            "id: behavior\nfeature: feature\nlabel: other\naxis: []\ndescription: |\n  d\n",
        )
        .unwrap();

        let result = migrate_entities(dir.path());

        assert!(
            matches!(result, Err(MigrateError::Conflicts(ref c)) if c.iter().any(|m| m.contains("behavior")))
        );
    }

    /// Regression: before `roll_forward_entity` was generalized past
    /// Feature, `resolve_divergence` for a non-Feature kind would silently
    /// fail to update its Knowledge YAML (the Feature-only lookup simply
    /// never found the Behavior, so nothing was written — no error, no
    /// effect). This must actually update `behavior.yml` now.
    #[test]
    fn resolve_divergence_updates_a_behavior_not_just_a_feature() {
        let dir = full_tree_project();
        const UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FBH";
        let events_dir = dir
            .path()
            .join(".markharness/identity-events/behaviors")
            .join(UID);
        fs::create_dir_all(&events_dir).unwrap();
        let root_event = IdentityEvent {
            identity_event_uid: "01ARZ3NDEKTSV4RRFFQ69G5FB0".to_string(),
            entity_uid: UID.to_string(),
            entity_kind: EntityKind::Behavior,
            previous_identity_event_uid: None,
            previous_identity_event_uids: Vec::new(),
            recorded_at: "2026-08-21T00:00:00Z".to_string(),
            mutation: IdentityMutation::Issued {
                id: "behavior".to_string(),
            },
        };
        fs::write(
            events_dir.join("01ARZ3NDEKTSV4RRFFQ69G5FB0.yml"),
            serde_yaml_ng::to_string(&root_event).unwrap(),
        )
        .unwrap();
        knowledge_walk::write_id_and_uid(
            dir.path(),
            EntityKind::Behavior,
            &dir.path()
                .join(".markharness/knowledge/req/feature/behavior/behavior.yml"),
            "behavior",
            UID,
        )
        .unwrap();
        let branch_a = IdentityEvent {
            identity_event_uid: "01ARZ3NDEKTSV4RRFFQ69G5FB1".to_string(),
            entity_uid: UID.to_string(),
            entity_kind: EntityKind::Behavior,
            previous_identity_event_uid: Some("01ARZ3NDEKTSV4RRFFQ69G5FB0".to_string()),
            previous_identity_event_uids: Vec::new(),
            recorded_at: "2026-08-21T00:01:00Z".to_string(),
            mutation: IdentityMutation::Renamed {
                from_id: "behavior".to_string(),
                to_id: "behavior-a".to_string(),
            },
        };
        let branch_b = IdentityEvent {
            identity_event_uid: "01ARZ3NDEKTSV4RRFFQ69G5FB2".to_string(),
            entity_uid: UID.to_string(),
            entity_kind: EntityKind::Behavior,
            previous_identity_event_uid: Some("01ARZ3NDEKTSV4RRFFQ69G5FB0".to_string()),
            previous_identity_event_uids: Vec::new(),
            recorded_at: "2026-08-21T00:01:01Z".to_string(),
            mutation: IdentityMutation::Renamed {
                from_id: "behavior".to_string(),
                to_id: "behavior-b".to_string(),
            },
        };
        fs::write(
            events_dir.join("01ARZ3NDEKTSV4RRFFQ69G5FB1.yml"),
            serde_yaml_ng::to_string(&branch_a).unwrap(),
        )
        .unwrap();
        fs::write(
            events_dir.join("01ARZ3NDEKTSV4RRFFQ69G5FB2.yml"),
            serde_yaml_ng::to_string(&branch_b).unwrap(),
        )
        .unwrap();

        resolve_divergence(
            dir.path(),
            EntityKind::Behavior,
            UID,
            "01ARZ3NDEKTSV4RRFFQ69G5FB1",
        )
        .unwrap();

        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/req/feature/behavior/behavior.yml"),
        )
        .unwrap();
        assert!(
            content.contains("id: behavior-a"),
            "expected behavior.yml to reflect the resolved rename, got: {content}"
        );
    }

    /// Step 31: a single `migrate_entities` call that completes every one
    /// of a case's five contributing elements at once must also record
    /// that case's `legacy_case_id` -> `case_uid` mapping in the
    /// migration manifest — not require a separate command.
    #[test]
    fn migrate_entities_records_the_migration_manifest_once_a_case_is_fully_migrated() {
        let dir = full_tree_project();

        migrate_entities(dir.path()).unwrap();

        let manifest = crate::identity::migration_manifest::read(dir.path()).unwrap();
        assert_eq!(manifest.entries.len(), 1);
        assert_eq!(
            manifest.entries[0].legacy_case_id,
            "tc-req-feature-behavior-condition"
        );
    }

    /// The reviewer's kill/restart finding: a process killed *after*
    /// `migrate_all`'s batch reaches its logical commit point (uid-issuing
    /// events durably written) but *before* the migration manifest is
    /// updated must not lose the case's legacy (pre-migration) identity.
    /// Recovery on the next run must record the manifest using the exact
    /// legacy snapshot captured before the crash — not a signature
    /// recomputed from the working tree, which by the time recovery runs
    /// is already fully migrated (uid lines present) and so would produce
    /// a different, wrongly "legacy"-labeled snapshot.
    #[test]
    fn migrate_entities_recovers_the_correct_legacy_manifest_entry_after_a_kill_right_after_the_commit_point()
     {
        let dir = full_tree_project();

        // Replicates `migrate_all` up through its logical commit point,
        // deliberately stopping there (no `roll_forward`/`finish`) to
        // simulate a process kill in exactly the window the finding
        // describes.
        let legacy_signatures = migration_manifest::capture_case_signatures(dir.path()).unwrap();
        let plan = build_migration_plan(dir.path()).unwrap();
        assert!(plan.report.conflicts.is_empty());
        assert!(
            !plan.events.is_empty(),
            "a fresh, fully uid-less project must have real entity-level work to migrate"
        );
        let intent = recovery::begin_batch_with_payload(
            dir.path(),
            plan.events,
            Some(recovery::IntentPayload::IdentityMigration(
                legacy_signatures.to_durable_payload().unwrap(),
            )),
        )
        .unwrap();
        recovery::commit_batch(dir.path(), &intent).unwrap();
        // Deliberately no roll_forward/finish call: this is the crash point.

        // "Restart": the public entry point must detect the leftover
        // intent, replay it via startup recovery, and finish normally.
        let report = migrate_entities(dir.path()).unwrap();
        assert!(
            report.migrated.is_empty(),
            "every entity was already committed by the simulated crash; nothing left for this \
             call to migrate at the entity level"
        );

        let manifest = migration_manifest::read(dir.path()).unwrap();
        assert_eq!(
            manifest.entries.len(),
            1,
            "recovery must record exactly the one legacy identity captured before the crash, \
             not also let the post-recovery migrate_all call record a second, spuriously \
             \"legacy\" entry from the by-then-already-migrated working tree: {manifest:?}"
        );
        let recovered_entry = &manifest.entries[0];

        // The recovered entry's legacy snapshot must be exactly what was
        // captured *before* the simulated crash — reconstructed here from
        // that same pre-crash capture's own durable payload, not recomputed.
        let legacy_payload = legacy_signatures.to_durable_payload().unwrap();
        let expected_legacy_snapshot: migration_manifest::LegacySnapshot =
            serde_yaml_ng::from_str(&legacy_payload[&recovered_entry.legacy_case_id]).unwrap();
        assert_eq!(recovered_entry.legacy_snapshot, expected_legacy_snapshot);

        // And it must differ from what a *fresh* capture of the now-migrated
        // (uid-bearing) working tree would produce — proving recovery used
        // the durably-persisted pre-crash snapshot, not a recomputation.
        let post_migration_signatures =
            migration_manifest::capture_case_signatures(dir.path()).unwrap();
        let post_migration_payload = post_migration_signatures.to_durable_payload().unwrap();
        let post_migration_snapshot: migration_manifest::LegacySnapshot =
            serde_yaml_ng::from_str(&post_migration_payload[&recovered_entry.legacy_case_id])
                .unwrap();
        assert_ne!(
            recovered_entry.legacy_snapshot, post_migration_snapshot,
            "the recovered entry must keep the true pre-migration legacy identity, not a \
             signature recomputed after uids were already written"
        );
    }
}
