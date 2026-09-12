use std::fs;
use std::io;
use std::path::Path;

use crate::identity::{
    EntityKind, IdentityEvent, IdentityMutation, engine, knowledge_walk, marker,
    migration_manifest, recovery, registry,
};
use crate::knowledge;
use crate::time::iso8601_utc_now;

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

/// Why `sync_entity` refused to run, or failed partway.
#[derive(Debug)]
pub enum SyncError {
    /// The entity has no `uid` on record — nothing to sync.
    NotFound,
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
        engine::replay(entity_uid, &events).map_err(SyncError::ReplayFailed)?;

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
    /// ADR 0017 §1・§3: Feature files whose `requirement_uids` held a
    /// Requirement *display id* (the Issue #44 bug), paired with the
    /// resolved `requirement_uids` they should have instead. `migrate_all`
    /// re-reads each file fresh (rather than writing a snapshot serialized
    /// here, at plan time) once the identity-event batch above has fully
    /// committed — the Feature's *own* `uid` may only exist on disk after
    /// that point, and a stale in-memory snapshot from before would clobber
    /// it.
    feature_fixups: Vec<(std::path::PathBuf, Vec<String>)>,
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
        // A project that finished entity-uid issuance in an earlier run can
        // still have Features whose `requirement_uids` was never resolved
        // (Issue #44) — write those fixups even though there are no
        // identity events this round.
        write_feature_fixups(root, &plan.feature_fixups)?;
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
    // Written only after every Requirement this plan resolved against has
    // actually had its `uid:` rolled forward onto disk above, so a Feature
    // is never rewritten to reference a uid that isn't there yet.
    write_feature_fixups(root, &plan.feature_fixups)?;
    mark_uid_mode_if_fully_migrated(root)?;
    Ok(plan.report)
}

/// Writes each Feature file's resolved `requirement_uids` (ADR 0017 §1・§3,
/// Issue #44). Re-reads the file fresh and only overwrites this one field
/// (matching `knowledge_walk::write_id_and_uid`'s own parse/mutate/
/// serialize round trip for the same file), so a Feature's own `uid` —
/// possibly just rolled forward onto this exact file by `roll_forward`
/// above — is never clobbered by a stale plan-time snapshot.
fn write_feature_fixups(
    root: &Path,
    feature_fixups: &[(std::path::PathBuf, Vec<String>)],
) -> io::Result<()> {
    for (path, requirement_uids) in feature_fixups {
        let content = fs::read_to_string(path)?;
        let mut feature = knowledge::parse_feature(&content).map_err(io::Error::other)?;
        feature.requirement_uids = requirement_uids.clone();
        crate::fs_safety::replace_file(
            root,
            path,
            knowledge::serialize_feature(&feature).as_bytes(),
        )?;
    }
    Ok(())
}

/// The UID-mode public cutover (design doc §13 Phase 5, ADR 0013 「移行」
/// 節): once every element of all five `EntityKind`s carries a `uid`,
/// flips `config.toml`'s `[identity]` marker to `mode = "uid"` — the
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
        return Ok(MigrationPlan {
            report,
            events,
            feature_fixups: Vec::new(),
        });
    }

    // Every Requirement's `uid`, by its current display `id` — either
    // already assigned, or the fresh one Pass 2 below issues in this same
    // run. Resolves Feature.requirement_uids entries (Pass 3) regardless of
    // whether the referenced Requirement was migrated in an earlier run or
    // this one.
    let mut requirement_uid_by_id: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();

    // Pass 2: generate one root `Issued` event per uid-less element, all
    // sharing `recorded_at` (design doc §12: one operation, one honest
    // "tracking began here" timestamp).
    for (kind, entities) in per_kind_entities {
        for entity in entities {
            if entity.uid.is_some() {
                if kind == EntityKind::Requirement {
                    requirement_uid_by_id
                        .insert(entity.id.clone(), entity.uid.clone().expect("checked Some"));
                }
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
            if kind == EntityKind::Requirement {
                requirement_uid_by_id.insert(entity.id.clone(), entity_uid.clone());
            }
            report.migrated.push(MigratedEntity {
                kind,
                id: entity.id,
                uid: entity_uid,
            });
        }
    }

    // Pass 3 (ADR 0017 §1・§3, Issue #44): resolve every Feature's
    // `requirement_uids` — an entry may still hold a Requirement *display
    // id* (the bug) rather than its `uid`. An entry that matches neither a
    // known Requirement uid nor a known Requirement id is a dangling
    // reference: reject the whole migrate rather than silently rewrite it
    // to something wrong or drop it.
    let known_requirement_uids: std::collections::BTreeSet<&String> =
        requirement_uid_by_id.values().collect();
    let mut feature_fixups = Vec::new();
    for found in knowledge_walk::list_entities(root, EntityKind::Feature)? {
        let content = fs::read_to_string(&found.path)?;
        let feature = knowledge::parse_feature(&content).map_err(io::Error::other)?;
        let mut changed = false;
        let mut resolved = Vec::with_capacity(feature.requirement_uids.len());
        for entry in &feature.requirement_uids {
            if known_requirement_uids.contains(entry) {
                resolved.push(entry.clone());
            } else if let Some(uid) = requirement_uid_by_id.get(entry) {
                resolved.push(uid.clone());
                changed = true;
            } else {
                report.conflicts.push(format!(
                    "{}: requirement_uids references unknown requirement '{entry}'",
                    relative_path_string(root, &found.path)
                ));
            }
        }
        if changed && report.conflicts.is_empty() {
            feature_fixups.push((found.path, resolved));
        }
    }
    if !report.conflicts.is_empty() {
        return Ok(MigrationPlan {
            report,
            events: Vec::new(),
            feature_fixups: Vec::new(),
        });
    }

    Ok(MigrationPlan {
        report,
        events,
        feature_fixups,
    })
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
                .join(".markharness/knowledge/features/player-jump"),
        )
        .unwrap();
        fs::create_dir_all(
            dir.path()
                .join(".markharness/knowledge/requirements/controls"),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/requirements/controls/requirement.yml"),
            // Already migrated, so Feature-focused tests calling
            // `migrate_entities`/`plan_migration` see only the Feature(s)
            // they set up themselves — multi-kind migration itself is
            // covered separately below.
            "id: controls\nsource: native\nlabel: controls\naxis: []\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FR0\n",
        )
        .unwrap();
        dir
    }

    fn write_feature(dir: &Path, id: &str, uid: Option<&str>) {
        let feature = Feature {
            id: id.to_string(),
            requirement_uids: vec!["controls".to_string()],
            label: id.to_string(),
            axis: Vec::new(),
            description: None,
            forked_from: None,
            uid: uid.map(str::to_string),
        };
        fs::write(
            dir.join(".markharness/knowledge/features/player-jump/feature.yml"),
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
                .join(".markharness/knowledge/features/player-jump/feature.yml"),
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
                .join(".markharness/knowledge/features/other-feature"),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/other-feature/feature.yml"),
            "id: task-management\nrequirement_uids: [controls]\nlabel: task-management\naxis: []\n",
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
                .join(".markharness/knowledge/features/player-jump/feature.yml"),
        )
        .unwrap();
        assert!(stale.contains("id: todo-management"));

        // A later `rename-id` call for an unrelated purpose (here: renaming
        // it right back) first runs startup recovery, which must roll the
        // interrupted operation forward before doing anything else.
        rename_id(dir.path(), "task-management", "todo-management-v2").unwrap();

        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/features/player-jump/feature.yml"),
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
                .join(".markharness/knowledge/features/player-jump/feature.yml"),
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

    #[test]
    fn sync_entity_refuses_an_unknown_uid() {
        let dir = init_project();

        let err =
            sync_entity(dir.path(), EntityKind::Feature, "01UNKNOWN0000000000000000").unwrap_err();

        assert!(matches!(err, SyncError::NotFound));
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
                .join(".markharness/knowledge/features/player-jump/feature.yml"),
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

    /// Issue #44: `write_feature`'s Feature always carries the pre-fix bug
    /// (`requirement_uids: ["controls"]`, a Requirement *display id*, not
    /// its uid). `init_project`'s "controls" Requirement already has a uid
    /// (`01ARZ3NDEKTSV4RRFFQ69G5FR0`) — migrating the Feature must resolve
    /// its `requirement_uids` to that real uid, not leave the display id in
    /// place.
    #[test]
    fn migrate_entities_resolves_a_features_display_id_requirement_reference_to_its_real_uid() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", None);

        migrate_entities(dir.path()).unwrap();

        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/features/player-jump/feature.yml"),
        )
        .unwrap();
        let feature: Feature = knowledge::parse_feature(&content).unwrap();
        assert_eq!(
            feature.requirement_uids,
            vec!["01ARZ3NDEKTSV4RRFFQ69G5FR0".to_string()]
        );
    }

    /// Same fixup, but for a Feature that was already fully migrated
    /// (already has its own `uid`) *before* this fix existed — the
    /// real-world Issue #44 scenario. `migrate_entities` must still repair
    /// its `requirement_uids` even though there is no uid-less entity left
    /// to issue an event for.
    #[test]
    fn migrate_entities_resolves_requirement_references_on_an_already_migrated_feature() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", Some(UID));
        issue_uid(dir.path(), UID, "todo-management");

        let report = migrate_entities(dir.path()).unwrap();

        assert!(
            report.migrated.is_empty(),
            "no entity-level uid issuance expected, got {report:?}"
        );
        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/features/player-jump/feature.yml"),
        )
        .unwrap();
        let feature: Feature = knowledge::parse_feature(&content).unwrap();
        assert_eq!(
            feature.requirement_uids,
            vec!["01ARZ3NDEKTSV4RRFFQ69G5FR0".to_string()]
        );
    }

    /// A `requirement_uids` entry matching neither a known Requirement uid
    /// nor a known Requirement display id is a dangling reference —
    /// `migrate_entities` must reject the whole run rather than silently
    /// drop or misresolve it, and must not write anything.
    #[test]
    fn migrate_entities_rejects_a_feature_referencing_an_unknown_requirement() {
        let dir = init_project();
        fs::create_dir_all(dir.path().join(".markharness/knowledge/features/orphan")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/orphan/feature.yml"),
            "id: orphan\nrequirement_uids: [no-such-requirement]\nlabel: orphan\naxis: []\n",
        )
        .unwrap();

        let result = migrate_entities(dir.path());

        assert!(matches!(result, Err(MigrateError::Conflicts(_))));
        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/features/orphan/feature.yml"),
        )
        .unwrap();
        assert!(
            content.contains("requirement_uids: [no-such-requirement]"),
            "the unresolved file must be left untouched, got: {content}"
        );
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
                .join(".markharness/knowledge/features/player-jump/feature.yml"),
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
        fs::create_dir_all(dir.path().join(".markharness/knowledge/features/second")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/second/feature.yml"),
            "id: second-feature\nrequirement_uids: [controls]\nlabel: second-feature\naxis: []\n",
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
                .join(".markharness/knowledge/features/player-jump/feature.yml"),
        )
        .unwrap();
        assert!(!content.contains("uid:"));
        assert!(!dir.path().join(".markharness/identity-events").exists());
    }

    #[test]
    fn plan_feature_migration_reports_duplicate_ids_without_writing() {
        let dir = init_project();
        write_feature(dir.path(), "todo-management", None);
        fs::create_dir_all(dir.path().join(".markharness/knowledge/features/second")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/second/feature.yml"),
            "id: todo-management\nrequirement_uids: [controls]\nlabel: duplicate\naxis: []\n",
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
        fs::create_dir_all(dir.path().join(".markharness/knowledge/features/second")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/second/feature.yml"),
            "id: todo-management\nrequirement_uids: [controls]\nlabel: duplicate\naxis: []\n",
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
        fs::create_dir_all(dir.path().join(".markharness/knowledge/features/second")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/second/feature.yml"),
            "id: second-feature\nrequirement_uids: [controls]\nlabel: second-feature\naxis: []\n",
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
            .join(".markharness/knowledge/features/feature/behavior/scenario");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(dir.path().join(".markharness/knowledge/requirements/req")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/requirements/req/requirement.yml"),
            "id: req\nsource: native\nlabel: req\naxis: []\n",
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/feature/feature.yml"),
            "id: feature\nrequirement_uids: [req]\nlabel: feature\naxis: []\n",
        )
        .unwrap();
        fs::write(
            base.parent().unwrap().join("behavior.yml"),
            "id: behavior\nfeature: feature\nlabel: behavior\naxis: []\ndescription: |\n  d\nprocedures: {}\n",
        )
        .unwrap();
        fs::write(
            base.join("scenario.yml"),
            "id: scenario\nbehavior: behavior\nlabel: scenario\ndescription: |\n  d\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"Confirmed.\"\n",
        )
        .unwrap();
        dir
    }

    /// Step 30: `identity migrate` must cover all four `EntityKind`s in a
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
        assert_eq!(report.migrated.len(), 4);
    }

    /// The batch is genuinely one operation: every migrated element,
    /// across all four kinds, shares the same `recorded_at`.
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
                .join(".markharness/knowledge/features/feature/behavior/behavior.yml"),
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
                .join(".markharness/knowledge/features/feature/feature.yml"),
            "id: req\nrequirement_uids: [req]\nlabel: feature\naxis: []\n",
        )
        .unwrap();

        let report = migrate_entities(dir.path()).unwrap();

        assert!(report.conflicts.is_empty());
        assert_eq!(report.migrated.len(), 4);
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
                .join(".markharness/knowledge/features/feature/other-behavior"),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/feature/other-behavior/behavior.yml"),
            "id: behavior\nfeature: feature\nlabel: other\naxis: []\ndescription: |\n  d\nprocedures: {}\n",
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
                .join(".markharness/knowledge/features/feature/other-behavior"),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/feature/other-behavior/behavior.yml"),
            "id: behavior\nfeature: feature\nlabel: other\naxis: []\ndescription: |\n  d\nprocedures: {}\n",
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
                .join(".markharness/knowledge/features/feature/behavior/behavior.yml"),
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
                .join(".markharness/knowledge/features/feature/behavior/behavior.yml"),
        )
        .unwrap();
        assert!(
            content.contains("id: behavior-a"),
            "expected behavior.yml to reflect the resolved rename, got: {content}"
        );
    }

    /// Step 31: a single `migrate_entities` call that completes every one
    /// of a case's three contributing elements at once must also record
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
            "tc-feature-behavior-scenario"
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
