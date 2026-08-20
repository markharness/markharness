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
    let first = batch_events
        .first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "batch must not be empty"))?;
    let intent = Intent {
        operation_id: ulid::Ulid::new().to_string(),
        entity_kind: first.entity_kind,
        entity_uid: first.entity_uid.clone(),
        identity_event_uid: first.identity_event_uid.clone(),
        batch_events,
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
/// anything else (design doc §6.3): clear a stale lock left by a crashed
/// operation, then roll forward or discard any leftover staging entries.
/// A *live* lock (an operation genuinely running concurrently) is left in
/// place and reported so the caller can refuse to proceed rather than
/// racing it.
pub fn run_startup_recovery<F>(root: &Path, roll_forward: F) -> io::Result<StartupRecovery>
where
    F: FnMut(&Intent) -> io::Result<()>,
{
    let cleared_stale_lock = lock::clear_if_stale(root)?;
    if !cleared_stale_lock && lock_is_present(root)? {
        return Ok(StartupRecovery::OperationInProgress);
    }
    let outcomes = recover_incomplete_operations(root, roll_forward)?;
    Ok(StartupRecovery::Recovered(outcomes))
}

fn lock_is_present(root: &Path) -> io::Result<bool> {
    match fs::metadata(
        root.join(crate::project_root::MARKHARNESS_DIR)
            .join(".identity.lock"),
    ) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum StartupRecovery {
    /// A lock owned by a still-running process is held; the caller must
    /// not proceed (design doc §6.3: "通常コマンドは...recoveryを完了する
    /// まで通常処理を行わない", which for a genuinely concurrent operation
    /// means refusing rather than racing it).
    OperationInProgress,
    Recovered(Vec<RecoveryOutcome>),
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
        assert!(matches!(
            result,
            StartupRecovery::Recovered(outcomes) if matches!(outcomes.as_slice(), [RecoveryOutcome::RolledForward { .. }])
        ));
    }

    #[test]
    fn startup_recovery_clears_a_stale_lock_and_still_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let lock_dir = dir.path().join(".markharness");
        fs::create_dir_all(&lock_dir).unwrap();
        fs::write(lock_dir.join(".identity.lock"), "999999999").unwrap();
        begin(dir.path(), EntityKind::Feature, "uid-1", "event-1").unwrap();

        let result = run_startup_recovery(dir.path(), |_| Ok(())).unwrap();
        assert!(!lock_dir.join(".identity.lock").exists());
        assert!(matches!(result, StartupRecovery::Recovered(_)));
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

        assert_eq!(result, StartupRecovery::OperationInProgress);
        assert_eq!(roll_forward_calls, 0);
        held_lock.release().unwrap();
    }
}
