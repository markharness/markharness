use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::execution::iso8601_utc_now;
use crate::fs_safety::replace_file;
use crate::identity::{
    EntityKind, IdentityEvent, IdentityMutation, engine, lock, recovery, registry,
};
use crate::knowledge::{self, Feature};

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

struct FoundFeature {
    path: PathBuf,
    feature: Feature,
}

fn sorted_subdirs(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut dirs: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };
    dirs.sort();
    Ok(dirs)
}

/// Walks `knowledge/<requirement>/<feature>/feature.yml` in the working
/// tree (not a committed ref — this drives mutating commands, which act
/// before anything is committed) looking for the first Feature matching
/// `predicate`.
fn find_feature_where(
    root: &Path,
    predicate: impl Fn(&Feature) -> bool,
) -> io::Result<Option<FoundFeature>> {
    let knowledge_root = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("knowledge");
    for requirement_dir in sorted_subdirs(&knowledge_root)? {
        for feature_dir in sorted_subdirs(&requirement_dir)? {
            let feature_path = feature_dir.join("feature.yml");
            if !feature_path.is_file() {
                continue;
            }
            let content = fs::read_to_string(&feature_path)?;
            if let Ok(feature) = knowledge::parse_feature(&content)
                && predicate(&feature)
            {
                return Ok(Some(FoundFeature {
                    path: feature_path,
                    feature,
                }));
            }
        }
    }
    Ok(None)
}

fn find_feature_by_id(root: &Path, id: &str) -> io::Result<Option<FoundFeature>> {
    find_feature_where(root, |feature| feature.id == id)
}

fn find_feature_by_uid(root: &Path, uid: &str) -> io::Result<Option<FoundFeature>> {
    find_feature_where(root, |feature| feature.uid.as_deref() == Some(uid))
}

/// Brings `feature.yml` and the Registry cache in line with `intent`'s
/// entity's current replayed state (design doc §6.1 steps 3–4). Runs both
/// immediately after a fresh commit and, identically, from the startup
/// recovery scan after a crash — the only difference is which process
/// invocation calls it, which is exactly the point: this step must be
/// idempotent and safe to redo from just the identity event log.
fn roll_forward(root: &Path, intent: &recovery::Intent) -> io::Result<()> {
    let result = registry::resolve_from_working_tree(root, intent.entity_kind, &intent.entity_uid)?
        .map_err(|e| io::Error::other(format!("{e:?}")))?;

    if let Some(found) = find_feature_by_uid(root, &intent.entity_uid)?
        && found.feature.id != result.current_id
    {
        let updated = Feature {
            id: result.current_id,
            ..found.feature
        };
        replace_file(
            root,
            &found.path,
            knowledge::serialize_feature(&updated).as_bytes(),
        )?;
    }

    registry::invalidate(root, intent.entity_kind, &intent.entity_uid)
}

/// `markharness feature rename-id <old> <new>` (design doc §3, §9):
/// changes a Feature's `id:` while preserving its `uid`, recording the
/// change as a `Renamed` identity event. Requires the Feature to already
/// have a `uid` (run `identity migrate` first if not).
pub fn rename_id(root: &Path, old_id: &str, new_id: &str) -> Result<(), RenameError> {
    match recovery::run_startup_recovery(root, |intent| roll_forward(root, intent))? {
        recovery::StartupRecovery::OperationInProgress => {
            return Err(RenameError::OperationInProgress);
        }
        recovery::StartupRecovery::Recovered(_) => {}
    }

    let Some(found) = find_feature_by_id(root, old_id)? else {
        return Err(RenameError::FeatureNotFound(old_id.to_string()));
    };
    let Some(entity_uid) = found.feature.uid.clone() else {
        return Err(RenameError::NotMigrated(old_id.to_string()));
    };
    if old_id != new_id && find_feature_by_id(root, new_id)?.is_some() {
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

    let held_lock = lock::IdentityLock::acquire(root)?;
    let outcome = commit_rename(
        root,
        &entity_uid,
        old_id,
        new_id,
        &replay_result.current_head_event_uid,
    );
    held_lock.release()?;
    outcome
}

fn commit_rename(
    root: &Path,
    entity_uid: &str,
    old_id: &str,
    new_id: &str,
    current_head_event_uid: &str,
) -> Result<(), RenameError> {
    let event_uid = ulid::Ulid::new().to_string();
    let intent = recovery::begin(root, EntityKind::Feature, entity_uid, &event_uid)?;

    let event = IdentityEvent {
        identity_event_uid: event_uid,
        entity_uid: entity_uid.to_string(),
        entity_kind: EntityKind::Feature,
        previous_identity_event_uid: Some(current_head_event_uid.to_string()),
        previous_identity_event_uids: Vec::new(),
        recorded_at: iso8601_utc_now(),
        mutation: IdentityMutation::Renamed {
            from_id: old_id.to_string(),
            to_id: new_id.to_string(),
        },
    };
    let event_yaml = serde_yaml_ng::to_string(&event).map_err(io::Error::other)?;
    recovery::commit(root, &intent, &event_yaml)?;

    roll_forward(root, &intent)?;
    recovery::finish(root, &intent)?;
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
    match recovery::run_startup_recovery(root, |intent| roll_forward(root, intent))? {
        recovery::StartupRecovery::OperationInProgress => {
            return Err(ResolveError::OperationInProgress);
        }
        recovery::StartupRecovery::Recovered(_) => {}
    }

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

    let held_lock = lock::IdentityLock::acquire(root)?;
    let outcome = commit_resolution(root, kind, entity_uid, keep_event_uid, &divergent_head_uids);
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
    let event_uid = ulid::Ulid::new().to_string();
    let intent = recovery::begin(root, kind, entity_uid, &event_uid)?;

    let event = IdentityEvent {
        identity_event_uid: event_uid,
        entity_uid: entity_uid.to_string(),
        entity_kind: kind,
        previous_identity_event_uid: None,
        previous_identity_event_uids: divergent_head_uids.to_vec(),
        recorded_at: iso8601_utc_now(),
        mutation: IdentityMutation::Resolved {
            winning_event_uid: keep_event_uid.to_string(),
        },
    };
    let event_yaml = serde_yaml_ng::to_string(&event).map_err(io::Error::other)?;
    recovery::commit(root, &intent, &event_yaml)?;

    roll_forward(root, &intent)?;
    recovery::finish(root, &intent)?;
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
    match recovery::run_startup_recovery(root, |intent| roll_forward(root, intent))? {
        recovery::StartupRecovery::OperationInProgress => {
            return Err(ReleaseError::OperationInProgress);
        }
        recovery::StartupRecovery::Recovered(_) => {}
    }

    let events = registry::load_events_from_working_tree(root, kind, entity_uid)?;
    let replay_result = engine::replay(entity_uid, &events).map_err(ReleaseError::ReplayFailed)?;
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

    let held_lock = lock::IdentityLock::acquire(root)?;
    let outcome = commit_release(
        root,
        kind,
        entity_uid,
        released_id,
        &replay_result.current_head_event_uid,
    );
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
    let event_uid = ulid::Ulid::new().to_string();
    let intent = recovery::begin(root, kind, entity_uid, &event_uid)?;

    let event = IdentityEvent {
        identity_event_uid: event_uid,
        entity_uid: entity_uid.to_string(),
        entity_kind: kind,
        previous_identity_event_uid: Some(current_head_event_uid.to_string()),
        previous_identity_event_uids: Vec::new(),
        recorded_at: iso8601_utc_now(),
        mutation: IdentityMutation::Released {
            released_id: released_id.to_string(),
        },
    };
    let event_yaml = serde_yaml_ng::to_string(&event).map_err(io::Error::other)?;
    recovery::commit(root, &intent, &event_yaml)?;

    roll_forward(root, &intent)?;
    recovery::finish(root, &intent)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn init_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump"),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/controls/requirement.yml"),
            "id: controls\nlabel: controls\naxis: []\n",
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
}
