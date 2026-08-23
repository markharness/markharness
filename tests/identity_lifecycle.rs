#![allow(clippy::disallowed_methods)]
//! End-to-end coverage for ADR 0013's Feature vertical slice (Phase 2):
//! migrate -> rename -> branch divergence -> resolve -> crash recovery, all
//! against one project, exercising the public `markharness::identity` API
//! the same way a real caller (the CLI) would chain these operations.

use std::fs;
use std::path::Path;

use markharness::identity::{
    self, EntityKind, IdentityEvent, IdentityMutation, engine, recovery, registry,
};
use markharness::knowledge::{self, Feature};

fn write_feature(root: &Path, requirement: &str, feature: &str, id: &str, uid: Option<&str>) {
    let dir = root
        .join(".markharness/knowledge")
        .join(requirement)
        .join(feature);
    fs::create_dir_all(&dir).unwrap();
    let value = Feature {
        id: id.to_string(),
        requirement: requirement.to_string(),
        label: id.to_string(),
        axis: Vec::new(),
        description: None,
        forked_from: None,
        uid: uid.map(str::to_string),
    };
    fs::write(
        dir.join("feature.yml"),
        knowledge::serialize_feature(&value),
    )
    .unwrap();
}

#[test]
fn feature_lifecycle_migrates_renames_resolves_divergence_and_recovers_from_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    markharness::init::run_init(dir.path()).unwrap();
    fs::create_dir_all(dir.path().join(".markharness/knowledge/controls")).unwrap();
    fs::write(
        dir.path()
            .join(".markharness/knowledge/controls/requirement.yml"),
        "id: controls\nlabel: controls\naxis: []\n",
    )
    .unwrap();
    write_feature(dir.path(), "controls", "player-jump", "player-jump", None);

    // 1. migrate: the Feature has no uid yet (nor does the Requirement
    // above it — `identity migrate` now covers every EntityKind, so both
    // get migrated in this one operation).
    let migrate_report = identity::migrate_entities(dir.path()).unwrap();
    assert_eq!(migrate_report.migrated.len(), 2);
    let feature_migration = migrate_report
        .migrated
        .iter()
        .find(|m| m.kind == EntityKind::Feature)
        .unwrap();
    let uid = feature_migration.uid.clone();
    assert_eq!(feature_migration.id, "player-jump");

    // Re-running migrate is a no-op now (idempotent, incremental migration).
    let second_migrate = identity::migrate_entities(dir.path()).unwrap();
    assert!(second_migrate.migrated.is_empty());

    // 2. rename: the uid survives an id change.
    identity::rename_id(dir.path(), "player-jump", "player-double-jump").unwrap();
    let content = fs::read_to_string(
        dir.path()
            .join(".markharness/knowledge/controls/player-jump/feature.yml"),
    )
    .unwrap();
    let feature: Feature = knowledge::parse_feature(&content).unwrap();
    assert_eq!(feature.id, "player-double-jump");
    assert_eq!(feature.uid, Some(uid.clone()));

    // 3. branch divergence: two independent identity events extend the
    // same predecessor (as two branches would, absent a merge driver —
    // design doc §7). Simulated directly at the identity-event-log level,
    // the same shape `git merge` would produce.
    let events =
        registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, &uid).unwrap();
    let head_event_uid = events
        .iter()
        .find(|e| matches!(&e.mutation, IdentityMutation::Renamed { .. }))
        .unwrap()
        .identity_event_uid
        .clone();
    let branch_a = ulid::Ulid::new().to_string();
    let branch_b = ulid::Ulid::new().to_string();
    for (event_uid, to_id) in [(&branch_a, "jump-attack"), (&branch_b, "double-jump-v2")] {
        let events_dir = dir
            .path()
            .join(".markharness/identity-events/features")
            .join(&uid);
        fs::create_dir_all(&events_dir).unwrap();
        let event = IdentityEvent {
            identity_event_uid: event_uid.clone(),
            entity_uid: uid.clone(),
            entity_kind: EntityKind::Feature,
            previous_identity_event_uid: Some(head_event_uid.clone()),
            previous_identity_event_uids: Vec::new(),
            recorded_at: "2026-08-20T03:00:00Z".to_string(),
            mutation: IdentityMutation::Renamed {
                from_id: "player-double-jump".to_string(),
                to_id: to_id.to_string(),
            },
        };
        fs::write(
            events_dir.join(format!("{event_uid}.yml")),
            serde_yaml_ng::to_string(&event).unwrap(),
        )
        .unwrap();
    }

    let events =
        registry::load_events_from_working_tree(dir.path(), EntityKind::Feature, &uid).unwrap();
    assert!(matches!(
        engine::replay(&uid, &events),
        Err(engine::ReplayError::AmbiguousDivergence { .. })
    ));

    // 4. resolve: pick branch_a as the surviving head.
    identity::resolve_divergence(dir.path(), EntityKind::Feature, &uid, &branch_a).unwrap();
    let content = fs::read_to_string(
        dir.path()
            .join(".markharness/knowledge/controls/player-jump/feature.yml"),
    )
    .unwrap();
    let feature: Feature = knowledge::parse_feature(&content).unwrap();
    assert_eq!(feature.id, "jump-attack");
    assert_eq!(feature.uid, Some(uid.clone()));

    // 5. crash recovery: commit a further rename's identity event but stop
    // short of rolling `feature.yml` forward or finishing the intent —
    // exactly where a process kill between commit-point and roll-forward
    // would leave things (design doc §6.1/§6.3).
    let replay_result = registry::resolve_from_working_tree(dir.path(), EntityKind::Feature, &uid)
        .unwrap()
        .unwrap();
    let crash_event_uid = ulid::Ulid::new().to_string();
    let intent = recovery::begin(dir.path(), EntityKind::Feature, &uid, &crash_event_uid).unwrap();
    let crash_event = IdentityEvent {
        identity_event_uid: crash_event_uid.clone(),
        entity_uid: uid.clone(),
        entity_kind: EntityKind::Feature,
        previous_identity_event_uid: Some(replay_result.current_head_event_uid.clone()),
        previous_identity_event_uids: Vec::new(),
        recorded_at: "2026-08-20T04:00:00Z".to_string(),
        mutation: IdentityMutation::Renamed {
            from_id: "jump-attack".to_string(),
            to_id: "jump-attack-v2".to_string(),
        },
    };
    recovery::commit(
        dir.path(),
        &intent,
        &serde_yaml_ng::to_string(&crash_event).unwrap(),
    )
    .unwrap();
    // Deliberately no roll_forward/finish: this is the simulated crash point.

    let stale = fs::read_to_string(
        dir.path()
            .join(".markharness/knowledge/controls/player-jump/feature.yml"),
    )
    .unwrap();
    assert!(
        stale.contains("id: jump-attack\n"),
        "feature.yml should still show the pre-crash id until recovery runs, got: {stale}"
    );

    // A later, unrelated command (any identity operation) must run startup
    // recovery first and finish the interrupted rename before doing its
    // own work — never fail or leave the crash unresolved.
    identity::migrate_entities(dir.path()).unwrap();

    let recovered = fs::read_to_string(
        dir.path()
            .join(".markharness/knowledge/controls/player-jump/feature.yml"),
    )
    .unwrap();
    let feature: Feature = knowledge::parse_feature(&recovered).unwrap();
    assert_eq!(feature.id, "jump-attack-v2");
    assert_eq!(feature.uid, Some(uid));
}
