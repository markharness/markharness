use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fs_safety::{remove_dir_all_no_follow, replace_file};
use crate::identity::{EntityKind, lock};

fn staging_root(root: &Path) -> PathBuf {
    root.join(crate::project_root::MARKHARNESS_DIR)
        .join(".identity-staging")
}

fn staging_dir(root: &Path, operation_id: &str) -> PathBuf {
    staging_root(root).join(operation_id)
}

fn intent_path(root: &Path, operation_id: &str) -> PathBuf {
    staging_dir(root, operation_id).join("intent.yml")
}

/// `.markharness/identity-events/<kind>/<uid>/<event_uid>.yml` — the
/// identity event's final location (design doc §4.1, §6.1). Writing to
/// this exact path is the operation's single logical commit point:
/// whether this file exists is the only thing that decides whether the
/// operation happened.
pub fn event_file_path(
    root: &Path,
    kind: EntityKind,
    entity_uid: &str,
    event_uid: &str,
) -> PathBuf {
    root.join(crate::project_root::MARKHARNESS_DIR)
        .join("identity-events")
        .join(kind.directory_segment())
        .join(entity_uid)
        .join(format!("{event_uid}.yml"))
}

/// Durable proof that an identity operation was attempted, written before
/// the commit point (design doc §6.1). Recovery uses `identity_event_uid`
/// to check whether the operation's event actually landed at
/// [`event_file_path`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intent {
    pub operation_id: String,
    pub entity_kind: EntityKind,
    pub entity_uid: String,
    pub identity_event_uid: String,
    /// All events belonging to one project-wide operation. Empty for the
    /// legacy/single-entity form. The first entry is the logical commit
    /// point; once it exists, recovery writes every remaining event and
    /// rolls all projections forward before exposing normal operation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub batch_events: Vec<BatchEvent>,
    /// Data a caller needs durably recorded *before* this operation's
    /// commit point, and available again to its own `roll_forward`
    /// callback both on the happy path and during crash recovery replay
    /// (design doc §6.1: the whole point of writing intent first is that
    /// nothing the operation depends on is only ever held in memory). Each
    /// caller feature gets its own [`IntentPayload`] variant rather than a
    /// bare untyped map, so `roll_forward` (there is only one per this
    /// module's caller, but nothing stops that from changing) is forced by
    /// the type system to match the exact variant it expects instead of
    /// assuming any non-empty payload must be its own — misreading another
    /// feature's payload as its own would otherwise fail silently or
    /// corrupt unrelated state rather than erroring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller_payload: Option<IntentPayload>,
}

/// A caller feature's own durably-persisted pre-commit data (see
/// `Intent::caller_payload`). One variant per feature that needs this —
/// add a new variant rather than reusing an existing one for an unrelated
/// purpose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntentPayload {
    /// `feature_ops::migrate_all`'s legacy (pre-migration) case identity —
    /// `case_id` -> a serialized `identity::migration_manifest::LegacySnapshot`
    /// — captured before `batch_events` ever touch the working tree, so it
    /// survives a crash between the commit point and the migration
    /// manifest being updated.
    IdentityMigration(std::collections::BTreeMap<String, String>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchEvent {
    pub entity_kind: EntityKind,
    pub entity_uid: String,
    pub identity_event_uid: String,
    pub event_yaml: String,
}

/// Step 1 (design doc §6.1): durably records intent before anything else
/// is written. Callers are expected to already hold `identity::lock`
/// (orchestration, not this module's concern — kept separate so the
/// staging mechanism stays testable without a lock in the loop).
pub fn begin(
    root: &Path,
    entity_kind: EntityKind,
    entity_uid: &str,
    identity_event_uid: &str,
) -> io::Result<Intent> {
    let intent = Intent {
        operation_id: ulid::Ulid::new().to_string(),
        entity_kind,
        entity_uid: entity_uid.to_string(),
        identity_event_uid: identity_event_uid.to_string(),
        batch_events: Vec::new(),
        caller_payload: None,
    };
    let yaml = serde_yaml_ng::to_string(&intent).map_err(io::Error::other)?;
    replace_file(
        root,
        &intent_path(root, &intent.operation_id),
        yaml.as_bytes(),
    )?;
    Ok(intent)
}

/// Records the complete plan for a multi-entity operation before its
/// logical commit point. This makes the operation recoverable as a whole.
pub fn begin_batch(root: &Path, batch_events: Vec<BatchEvent>) -> io::Result<Intent> {
    begin_batch_with_payload(root, batch_events, None)
}

/// Like [`begin_batch`], but also durably records `caller_payload`
/// (`Intent::caller_payload`) before the batch's logical commit point, so
/// it survives a crash between that commit point and the caller's own
/// post-commit work (e.g. `feature_ops::migrate_all` recording the
/// migration manifest).
pub fn begin_batch_with_payload(
    root: &Path,
    batch_events: Vec<BatchEvent>,
    caller_payload: Option<IntentPayload>,
) -> io::Result<Intent> {
    let first = batch_events
        .first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "batch must not be empty"))?;
    let intent = Intent {
        operation_id: ulid::Ulid::new().to_string(),
        entity_kind: first.entity_kind,
        entity_uid: first.entity_uid.clone(),
        identity_event_uid: first.identity_event_uid.clone(),
        batch_events,
        caller_payload,
    };
    let yaml = serde_yaml_ng::to_string(&intent).map_err(io::Error::other)?;
    replace_file(
        root,
        &intent_path(root, &intent.operation_id),
        yaml.as_bytes(),
    )?;
    Ok(intent)
}

/// Step 2, the single logical commit point: writes `event` to its final
/// location. `event`'s own `identity_event_uid` must match
/// `intent.identity_event_uid`; recovery's `is_committed` check depends on
/// this agreement.
pub fn commit(root: &Path, intent: &Intent, event_yaml: &str) -> io::Result<()> {
    replace_file(
        root,
        &event_file_path(
            root,
            intent.entity_kind,
            &intent.entity_uid,
            &intent.identity_event_uid,
        ),
        event_yaml.as_bytes(),
    )
}

/// Commits a batch in deterministic order. The first event is the single
/// logical commit point; the durable intent contains enough information
/// for startup recovery to finish every later event.
pub fn commit_batch(root: &Path, intent: &Intent) -> io::Result<()> {
    for event in &intent.batch_events {
        replace_file(
            root,
            &event_file_path(
                root,
                event.entity_kind,
                &event.entity_uid,
                &event.identity_event_uid,
            ),
            event.event_yaml.as_bytes(),
        )?;
    }
    Ok(())
}

/// Idempotently completes event writes after a committed batch was
/// interrupted. Must only be called after [`is_committed`] is true.
pub fn complete_batch_commits(root: &Path, intent: &Intent) -> io::Result<()> {
    for event in &intent.batch_events {
        let path = event_file_path(
            root,
            event.entity_kind,
            &event.entity_uid,
            &event.identity_event_uid,
        );
        if !path.is_file() {
            replace_file(root, &path, event.event_yaml.as_bytes())?;
        }
    }
    Ok(())
}

/// Whether `intent`'s event has reached its final location — the sole
/// criterion for "did this operation happen" (design doc §6.1).
pub fn is_committed(root: &Path, intent: &Intent) -> bool {
    event_file_path(
        root,
        intent.entity_kind,
        &intent.entity_uid,
        &intent.identity_event_uid,
    )
    .is_file()
}

/// Step 5: removes the staging directory, marking the operation
/// complete. Idempotent (a missing directory is not an error).
pub fn finish(root: &Path, intent: &Intent) -> io::Result<()> {
    remove_dir_all_no_follow(root, &staging_dir(root, &intent.operation_id))
}

/// What happened to one leftover staging entry during recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryOutcome {
    /// The intent's event never reached its final location: the
    /// operation never truly started (in the sense that matters), so
    /// nothing else needed rolling forward. The old state was already
    /// correct.
    Discarded { operation_id: String },
    /// The intent's event was already committed; `roll_forward` was
    /// invoked (idempotently) to bring Knowledge YAML and the Registry
    /// cache in line with it.
    RolledForward { operation_id: String },
}

/// The startup recovery scan (design doc §6.1): for every leftover
/// `.markharness/.identity-staging/<operation-id>/`, either discards it
/// (event never committed) or calls `roll_forward` once and then removes
/// it (event committed). `roll_forward` is supplied by the caller because
/// what "roll forward" means — which Knowledge YAML to rewrite, from
/// which replay result — is entity-kind-specific domain logic (Phase 2+),
/// not something this generic module knows.
///
/// Ordinary commands must call this (after acquiring `identity::lock`,
/// per design doc §6.3) before doing anything else, so an interrupted
/// operation is never left implicitly half-applied.
pub fn recover_incomplete_operations<F>(
    root: &Path,
    mut roll_forward: F,
) -> io::Result<Vec<RecoveryOutcome>>
where
    F: FnMut(&Intent) -> io::Result<()>,
{
    let dir = staging_root(root);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let mut operation_ids: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && let Some(name) = entry.file_name().to_str()
        {
            operation_ids.push(name.to_string());
        }
    }
    operation_ids.sort();

    let mut outcomes = Vec::new();
    for operation_id in operation_ids {
        let path = intent_path(root, &operation_id);
        let Ok(contents) = fs::read_to_string(&path) else {
            // No readable intent.yml (e.g. staging dir created but the
            // intent write itself never completed): nothing was ever
            // committed under this operation id, so it is safe to discard.
            remove_dir_all_no_follow(root, &staging_dir(root, &operation_id))?;
            outcomes.push(RecoveryOutcome::Discarded { operation_id });
            continue;
        };
        let intent: Intent = serde_yaml_ng::from_str(&contents).map_err(io::Error::other)?;

        if is_committed(root, &intent) {
            roll_forward(&intent)?;
            finish(root, &intent)?;
            outcomes.push(RecoveryOutcome::RolledForward { operation_id });
        } else {
            finish(root, &intent)?;
            outcomes.push(RecoveryOutcome::Discarded { operation_id });
        }
    }
    Ok(outcomes)
}

/// The combined startup check every ordinary command runs before doing
/// anything else (design doc §6.3): acquire the identity lock, then roll
/// forward or discard any leftover staging entries. A *live* lock (an
/// operation genuinely running concurrently, or another process's own
/// startup recovery already in flight) is left in place and reported so
/// the caller can refuse to proceed rather than racing it.
///
/// `recover_incomplete_operations`'s own doc comment states its contract
/// as running "after acquiring `identity::lock`," which is exactly what
/// this function does on its caller's behalf.
///
/// Unlike an earlier version of this function, the lock is **not**
/// released before returning on success — it is handed back to the caller
/// inside `StartupRecovery::Ready`, and the caller must go on to use that
/// exact same lock for its own check-and-commit rather than releasing it
/// and acquiring a fresh one. Releasing and reacquiring left a real gap: a
/// *different* process could commit an event and crash mid-operation
/// (post-commit, pre-roll-forward) in the window between this recovery
/// scan finishing and the caller's own separate acquire, and the caller
/// would then read that inconsistent intermediate state without this
/// function's own recovery logic ever having had a chance to notice and
/// fix it — because it ran too early, before the crash even happened.
/// Keeping recovery and the caller's operation inside one continuous lock
/// hold closes that gap: nothing else can commit anything for this
/// project between this scan and the caller's own read.
///
/// Because `lock::IdentityLock` is now backed by the OS's own advisory
/// file lock rather than a plain file's presence (see that module's doc
/// comment for why), there is no separate "is the existing lock merely a
/// stale leftover from a crashed process" question to answer here at all
/// — a crash releases the OS lock as part of the crashed process exiting,
/// so a fresh `acquire` right after a crash simply succeeds immediately.
/// `acquire` failing with `WouldBlock` means exactly one thing: a *live*
/// holder; any other error (permission denied, a read-only filesystem, a
/// malformed lock path, ...) is a genuine failure this function must
/// propagate rather than misreport as mere lock contention.
pub fn run_startup_recovery<F>(root: &Path, roll_forward: F) -> io::Result<StartupRecovery>
where
    F: FnMut(&Intent) -> io::Result<()>,
{
    let held_lock = match lock::IdentityLock::acquire(root) {
        Ok(held_lock) => held_lock,
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
            return Ok(StartupRecovery::OperationInProgress);
        }
        Err(e) => return Err(e),
    };
    let outcomes = recover_incomplete_operations(root, roll_forward)?;
    Ok(StartupRecovery::Ready {
        outcomes,
        lock: held_lock,
    })
}

#[derive(Debug)]
pub enum StartupRecovery {
    /// A lock owned by a still-running process is held; the caller must
    /// not proceed (design doc §6.3: "通常コマンドは...recoveryを完了する
    /// まで通常処理を行わない", which for a genuinely concurrent operation
    /// means refusing rather than racing it).
    OperationInProgress,
    /// Recovery completed (possibly finding nothing to do) and the lock it
    /// was acquired under is handed back here, still held. Callers must
    /// use this exact `lock` for their own subsequent check-and-commit —
    /// see this function's own doc comment for why releasing it and
    /// acquiring a fresh one instead would reopen the gap this design
    /// closes — and are responsible for releasing it themselves once
    /// their own operation-specific work is done.
    Ready {
        outcomes: Vec<RecoveryOutcome>,
        lock: lock::IdentityLock,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_writes_a_readable_intent_file() {
        let dir = tempfile::tempdir().unwrap();
        let intent = begin(dir.path(), EntityKind::Feature, "uid-1", "event-1").unwrap();
        assert!(intent_path(dir.path(), &intent.operation_id).is_file());
        assert_eq!(intent.entity_uid, "uid-1");
        assert_eq!(intent.identity_event_uid, "event-1");
    }

    #[test]
    fn is_committed_is_false_before_commit_and_true_after() {
        let dir = tempfile::tempdir().unwrap();
        let intent = begin(dir.path(), EntityKind::Feature, "uid-1", "event-1").unwrap();
        assert!(!is_committed(dir.path(), &intent));

        commit(dir.path(), &intent, "identity_event_uid: event-1\n").unwrap();
        assert!(is_committed(dir.path(), &intent));
    }

    #[test]
    fn finish_removes_the_staging_directory_but_not_the_committed_event() {
        let dir = tempfile::tempdir().unwrap();
        let intent = begin(dir.path(), EntityKind::Feature, "uid-1", "event-1").unwrap();
        commit(dir.path(), &intent, "identity_event_uid: event-1\n").unwrap();

        finish(dir.path(), &intent).unwrap();

        assert!(!staging_dir(dir.path(), &intent.operation_id).exists());
        assert!(is_committed(dir.path(), &intent));
    }

    #[test]
    fn recovery_is_a_no_op_when_no_staging_directory_exists() {
        let dir = tempfile::tempdir().unwrap();
        let outcomes = recover_incomplete_operations(dir.path(), |_| Ok(())).unwrap();
        assert!(outcomes.is_empty());
    }

    /// Simulates a process kill before the commit point (design doc
    /// §6.1): `begin` ran, but `commit` never did. Recovery must discard
    /// without ever calling `roll_forward`.
    #[test]
    fn recovery_discards_an_operation_whose_event_never_committed() {
        let dir = tempfile::tempdir().unwrap();
        begin(dir.path(), EntityKind::Feature, "uid-1", "event-1").unwrap();

        let mut roll_forward_calls = 0;
        let outcomes = recover_incomplete_operations(dir.path(), |_| {
            roll_forward_calls += 1;
            Ok(())
        })
        .unwrap();

        assert_eq!(roll_forward_calls, 0);
        assert!(matches!(
            outcomes.as_slice(),
            [RecoveryOutcome::Discarded { .. }]
        ));
        assert!(
            staging_root(dir.path())
                .read_dir()
                .unwrap()
                .next()
                .is_none()
        );
    }

    /// Simulates a process kill after the commit point but before
    /// `finish`: recovery must roll forward exactly once and then clean
    /// up the staging directory.
    #[test]
    fn recovery_rolls_forward_an_operation_whose_event_was_committed() {
        let dir = tempfile::tempdir().unwrap();
        let intent = begin(dir.path(), EntityKind::Feature, "uid-1", "event-1").unwrap();
        commit(dir.path(), &intent, "identity_event_uid: event-1\n").unwrap();
        // Deliberately no `finish` call: this is the crash point being simulated.

        let mut rolled_forward_for: Vec<String> = Vec::new();
        let outcomes = recover_incomplete_operations(dir.path(), |intent| {
            rolled_forward_for.push(intent.entity_uid.clone());
            Ok(())
        })
        .unwrap();

        assert_eq!(rolled_forward_for, vec!["uid-1".to_string()]);
        assert!(matches!(
            outcomes.as_slice(),
            [RecoveryOutcome::RolledForward { .. }]
        ));
        assert!(is_committed(dir.path(), &intent));
        assert!(
            staging_root(dir.path())
                .read_dir()
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[test]
    fn recovery_handles_multiple_leftover_operations_independently() {
        let dir = tempfile::tempdir().unwrap();
        let committed = begin(dir.path(), EntityKind::Feature, "uid-committed", "event-a").unwrap();
        commit(dir.path(), &committed, "identity_event_uid: event-a\n").unwrap();
        begin(
            dir.path(),
            EntityKind::Feature,
            "uid-uncommitted",
            "event-b",
        )
        .unwrap();

        let mut rolled_forward_for: Vec<String> = Vec::new();
        let outcomes = recover_incomplete_operations(dir.path(), |intent| {
            rolled_forward_for.push(intent.entity_uid.clone());
            Ok(())
        })
        .unwrap();

        assert_eq!(outcomes.len(), 2);
        assert_eq!(rolled_forward_for, vec!["uid-committed".to_string()]);
    }

    /// The reviewer-requested kill/restart boundary: `caller_payload` must
    /// be readable by `roll_forward` during crash recovery exactly as it
    /// was at `begin_batch_with_payload` time — a process kill right after
    /// the batch's commit point (simulated here by never calling `finish`)
    /// must not lose it.
    #[test]
    fn caller_payload_survives_recovery_after_a_kill_right_after_the_commit_point() {
        let dir = tempfile::tempdir().unwrap();
        let events = vec![BatchEvent {
            entity_kind: EntityKind::Feature,
            entity_uid: "uid-a".to_string(),
            identity_event_uid: "event-a".to_string(),
            event_yaml: "identity_event_uid: event-a\n".to_string(),
        }];
        let mut payload_map = std::collections::BTreeMap::new();
        payload_map.insert("tc-case-1".to_string(), "legacy-snapshot-yaml".to_string());
        let payload = Some(IntentPayload::IdentityMigration(payload_map));
        let intent = begin_batch_with_payload(dir.path(), events, payload.clone()).unwrap();
        commit(dir.path(), &intent, "identity_event_uid: event-a\n").unwrap();
        // Deliberately no `finish` call: this is the crash point being simulated.

        let mut seen_payload = None;
        recover_incomplete_operations(dir.path(), |intent| {
            seen_payload = Some(intent.caller_payload.clone());
            Ok(())
        })
        .unwrap();

        assert_eq!(seen_payload, Some(payload));
    }

    #[test]
    fn recovery_completes_every_event_in_one_committed_batch() {
        let dir = tempfile::tempdir().unwrap();
        let events = vec![
            BatchEvent {
                entity_kind: EntityKind::Feature,
                entity_uid: "uid-a".to_string(),
                identity_event_uid: "event-a".to_string(),
                event_yaml: "identity_event_uid: event-a\n".to_string(),
            },
            BatchEvent {
                entity_kind: EntityKind::Feature,
                entity_uid: "uid-b".to_string(),
                identity_event_uid: "event-b".to_string(),
                event_yaml: "identity_event_uid: event-b\n".to_string(),
            },
        ];
        let intent = begin_batch(dir.path(), events).unwrap();
        // Simulate a crash immediately after the first event established
        // the batch's logical commit point.
        commit(dir.path(), &intent, "identity_event_uid: event-a\n").unwrap();

        recover_incomplete_operations(dir.path(), |intent| {
            complete_batch_commits(dir.path(), intent)
        })
        .unwrap();

        assert!(event_file_path(dir.path(), EntityKind::Feature, "uid-a", "event-a").is_file());
        assert!(event_file_path(dir.path(), EntityKind::Feature, "uid-b", "event-b").is_file());
    }

    /// Recovery itself must be safe to interrupt and rerun (design doc
    /// §6.3): rerunning after a fully successful recovery finds nothing
    /// left to do.
    #[test]
    fn rerunning_recovery_after_a_clean_recovery_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let intent = begin(dir.path(), EntityKind::Feature, "uid-1", "event-1").unwrap();
        commit(dir.path(), &intent, "identity_event_uid: event-1\n").unwrap();
        recover_incomplete_operations(dir.path(), |_| Ok(())).unwrap();

        let outcomes = recover_incomplete_operations(dir.path(), |_| Ok(())).unwrap();
        assert!(outcomes.is_empty());
    }

    #[test]
    fn startup_recovery_rolls_forward_when_no_lock_is_held() {
        let dir = tempfile::tempdir().unwrap();
        let intent = begin(dir.path(), EntityKind::Feature, "uid-1", "event-1").unwrap();
        commit(dir.path(), &intent, "identity_event_uid: event-1\n").unwrap();

        let result = run_startup_recovery(dir.path(), |_| Ok(())).unwrap();
        match result {
            StartupRecovery::Ready { outcomes, lock } => {
                assert!(matches!(
                    outcomes.as_slice(),
                    [RecoveryOutcome::RolledForward { .. }]
                ));
                lock.release().unwrap();
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn startup_recovery_recovers_despite_a_leftover_lock_file_from_a_crashed_process() {
        let dir = tempfile::tempdir().unwrap();
        let lock_dir = dir.path().join(".markharness");
        fs::create_dir_all(&lock_dir).unwrap();
        // A crashed process's leftover: the file itself survives, but
        // nothing at the OS level still holds a lock on it (the crash
        // released that automatically) — `lock::IdentityLock` no longer
        // needs to inspect this content at all to know it's safe to
        // acquire fresh.
        fs::write(lock_dir.join(".identity.lock"), "999999999").unwrap();
        begin(dir.path(), EntityKind::Feature, "uid-1", "event-1").unwrap();

        let result = run_startup_recovery(dir.path(), |_| Ok(())).unwrap();
        match result {
            StartupRecovery::Ready { lock, .. } => lock.release().unwrap(),
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    /// Regression test for the handoff gap an earlier design had:
    /// `run_startup_recovery` used to release its lock before returning,
    /// so a caller finishing recovery and going on to its own
    /// check-and-commit had to acquire a *second*, separate lock — leaving
    /// a real window in between where a different process could commit an
    /// event and crash (post-commit, pre-roll-forward) without this
    /// recovery scan ever getting a chance to notice and fix it, since it
    /// had already run and finished before that crash even happened.
    ///
    /// Simulates process A completing recovery and then still being mid
    /// way through its own operation-specific work (the lock from `Ready`
    /// is not released yet) and process B trying to start up at that exact
    /// moment: B's own `acquire` must be refused with `WouldBlock` for the
    /// *entire* time A holds the lock — not just during A's recovery scan
    /// — and only succeed once A actually finishes and releases.
    #[test]
    fn recovery_lock_stays_held_for_caller_until_it_finishes_its_own_operation() {
        let dir = tempfile::tempdir().unwrap();

        let a_lock = match run_startup_recovery(dir.path(), |_| Ok(())).unwrap() {
            StartupRecovery::Ready { lock, .. } => lock,
            other => panic!("expected Ready, got {other:?}"),
        };

        // Process B attempting to start up (or acquire the lock directly
        // for its own operation) while A is still mid-operation.
        let b_attempt = lock::IdentityLock::acquire(dir.path());
        assert!(
            matches!(&b_attempt, Err(e) if e.kind() == io::ErrorKind::WouldBlock),
            "B must be refused while A still holds the handed-off lock, got {b_attempt:?}"
        );

        // A finishes its own operation-specific work and only now releases.
        a_lock.release().unwrap();

        // B can finally proceed.
        let b_lock = lock::IdentityLock::acquire(dir.path())
            .expect("B must be able to acquire once A has released");
        b_lock.release().unwrap();
    }

    #[test]
    fn startup_recovery_refuses_to_proceed_while_a_live_operation_holds_the_lock() {
        let dir = tempfile::tempdir().unwrap();
        let held_lock = lock::IdentityLock::acquire(dir.path()).unwrap();

        let mut roll_forward_calls = 0;
        let result = run_startup_recovery(dir.path(), |_| {
            roll_forward_calls += 1;
            Ok(())
        })
        .unwrap();

        assert!(matches!(result, StartupRecovery::OperationInProgress));
        assert_eq!(roll_forward_calls, 0);
        held_lock.release().unwrap();
    }

    /// Regression test: `run_startup_recovery` used to call
    /// `recover_incomplete_operations` — which writes Knowledge
    /// projections via the caller's `roll_forward` — without holding any
    /// lock of its own around it. Two processes starting up after the same
    /// crash could both roll forward the same leftover committed intent
    /// concurrently. Real OS threads, barrier-started together, all racing
    /// to recover the same leftover-lock-file-plus-leftover-intent state
    /// (the `.identity.lock` content here is a crashed process's leftover
    /// *file*, per `lock::IdentityLock`'s own doc comment — nothing at the
    /// OS level actually holds it) must let exactly one of them actually
    /// invoke `roll_forward` — never zero (the leftover must still get
    /// recovered by *someone*), never more than one (that would be the
    /// double-application this test guards against).
    #[test]
    fn concurrent_startup_recovery_never_rolls_forward_twice_for_the_same_stale_lock() {
        let dir = tempfile::tempdir().unwrap();
        let lock_dir = dir.path().join(".markharness");
        fs::create_dir_all(&lock_dir).unwrap();
        fs::write(lock_dir.join(".identity.lock"), "999999999").unwrap();
        let intent = begin(dir.path(), EntityKind::Feature, "uid-1", "event-1").unwrap();
        commit(dir.path(), &intent, "identity_event_uid: event-1\n").unwrap();

        let root = dir.path();
        let roll_forward_calls = std::sync::atomic::AtomicUsize::new(0);
        const ATTEMPTS: usize = 8;
        let barrier = std::sync::Barrier::new(ATTEMPTS);
        let results: Vec<io::Result<StartupRecovery>> = std::thread::scope(|scope| {
            let barrier = &barrier;
            let roll_forward_calls = &roll_forward_calls;
            let handles: Vec<_> = (0..ATTEMPTS)
                .map(|_| {
                    scope.spawn(move || {
                        barrier.wait();
                        run_startup_recovery(root, |_| {
                            roll_forward_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            Ok(())
                        })
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });

        assert_eq!(
            roll_forward_calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "exactly one concurrent startup recovery attempt must actually roll forward"
        );
        // Every attempt must end cleanly one way or another — either it
        // recovered (possibly finding nothing left, if it ran after the
        // winner already finished) or it correctly backed off as
        // `OperationInProgress`; a queue-free, fail-fast lock (design doc
        // §6, Q6) makes both legitimate, so this doesn't pin down exactly
        // how many land in each bucket — only that none of them error out.
        assert!(
            results.iter().all(|r| r.is_ok()),
            "no concurrent startup recovery attempt should fail outright: {results:?}"
        );
        for result in results {
            if let Ok(StartupRecovery::Ready { lock, .. }) = result {
                lock.release().unwrap();
            }
        }
    }
}
