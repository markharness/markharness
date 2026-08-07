use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

use crate::generate;
use crate::id_cache::{self, FeatureBlob};

/// One detected Feature change between two milestones (§3.5 ChangeEvent).
/// `change_type` is intentionally absent: per docs/cli-manual.md UC5, it is
/// filled in by a human afterwards, not computed here.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ChangeEvent {
    pub event_id: String,
    pub feature_id: String,
    pub from_milestone: String,
    pub to_milestone: String,
    pub from_blob: Option<String>,
    pub to_blob: Option<String>,
    pub impacted_testcases: Vec<String>,
}

fn blob_map(blobs: Vec<FeatureBlob>) -> BTreeMap<String, String> {
    blobs.into_iter().map(|b| (b.id, b.blob_sha)).collect()
}

/// Maps each Feature id to the `case_id`s of testcases generated from it,
/// using the *current* `knowledge/` working tree as the structural
/// generation graph (§3.2(A): `CONDITION`→`TESTCASE`, does not need version
/// history — only the version-history side, `derived_from`, does).
fn impacted_testcases_by_feature(root: &Path) -> io::Result<BTreeMap<String, Vec<String>>> {
    let testcases = generate::generate_testcases(&root.join("knowledge"))?;
    let mut by_feature: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for testcase in testcases {
        by_feature
            .entry(testcase.generated_from.feature.clone())
            .or_default()
            .push(testcase.case_id);
    }
    Ok(by_feature)
}

/// Computes `derived_from`-style change events between `from_milestone` and
/// `to_milestone` (two git tags) by comparing each Feature's blob SHA at
/// each tag (§3.2〜3.4 の簡易版; マイルストーン=引数のtag名をそのまま使用)。
pub fn compute_changes(
    root: &Path,
    from_milestone: &str,
    to_milestone: &str,
    use_cache: bool,
) -> io::Result<Vec<ChangeEvent>> {
    let from_blobs = blob_map(id_cache::resolve_feature_blobs(
        root,
        from_milestone,
        use_cache,
    )?);
    let to_blobs = blob_map(id_cache::resolve_feature_blobs(
        root,
        to_milestone,
        use_cache,
    )?);
    let impacted = impacted_testcases_by_feature(root)?;

    let all_ids: BTreeSet<&String> = from_blobs.keys().chain(to_blobs.keys()).collect();

    let mut events = Vec::new();
    for feature_id in all_ids {
        let from_blob = from_blobs.get(feature_id).cloned();
        let to_blob = to_blobs.get(feature_id).cloned();
        if from_blob == to_blob {
            continue;
        }
        events.push(ChangeEvent {
            event_id: format!("{feature_id}--{from_milestone}--{to_milestone}"),
            feature_id: feature_id.clone(),
            from_milestone: from_milestone.to_string(),
            to_milestone: to_milestone.to_string(),
            from_blob,
            to_blob,
            impacted_testcases: impacted.get(feature_id).cloned().unwrap_or_default(),
        });
    }

    Ok(events)
}

pub fn serialize_changes(events: &[ChangeEvent]) -> String {
    serde_yaml_ng::to_string(events).expect("ChangeEvent serialization is infallible")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn run_git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        dir
    }

    fn write_full_chain(root: &Path, label: &str) {
        let base = root.join("knowledge/controls/player-jump/jump/ground");
        fs::create_dir_all(&base).unwrap();
        fs::write(
            root.join("knowledge/controls/requirement.yml"),
            "id: controls\nlabel: controls\naxis: [gameplay]\n",
        )
        .unwrap();
        fs::write(
            root.join("knowledge/controls/player-jump/feature.yml"),
            format!("id: player-jump\nrequirement: controls\nlabel: {label}\naxis: [gameplay]\n"),
        )
        .unwrap();
        fs::write(
            root.join("knowledge/controls/player-jump/jump/behavior.yml"),
            "id: jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\n",
        )
        .unwrap();
        fs::write(
            base.join("condition.yml"),
            "id: ground\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from the ground.\n",
        )
        .unwrap();
        fs::create_dir_all(base.join("expected")).unwrap();
        fs::write(
            base.join("expected/001.yml"),
            "id: ground-001\ncondition: ground\ndescription: |\n  lands safely\n",
        )
        .unwrap();
    }

    fn commit_and_tag(root: &Path, message: &str, tag: &str) {
        run_git(root, &["add", "-A"]);
        run_git(root, &["commit", "-q", "-m", message]);
        run_git(root, &["tag", tag]);
    }

    #[test]
    fn reports_no_events_when_nothing_changed_between_milestones() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");
        run_git(dir.path(), &["tag", "m2"]);

        let events = compute_changes(dir.path(), "m1", "m2", false).unwrap();

        assert!(events.is_empty());
    }

    #[test]
    fn reports_changed_event_with_impacted_testcases_when_feature_blob_differs() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");

        write_full_chain(dir.path(), "v2");
        commit_and_tag(dir.path(), "v2", "m2");

        let events = compute_changes(dir.path(), "m1", "m2", false).unwrap();

        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.feature_id, "player-jump");
        assert_eq!(event.event_id, "player-jump--m1--m2");
        assert!(event.from_blob.is_some());
        assert!(event.to_blob.is_some());
        assert_ne!(event.from_blob, event.to_blob);
        assert_eq!(event.impacted_testcases, vec!["tc-ground-001".to_string()]);
    }

    #[test]
    fn reports_added_event_when_feature_did_not_exist_at_from_milestone() {
        let dir = init_repo();
        fs::create_dir_all(dir.path().join("knowledge")).unwrap();
        fs::write(dir.path().join("README.md"), "empty\n").unwrap();
        commit_and_tag(dir.path(), "empty", "m1");

        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "add feature", "m2");

        let events = compute_changes(dir.path(), "m1", "m2", false).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].from_blob, None);
        assert!(events[0].to_blob.is_some());
    }

    #[test]
    fn reports_removed_event_when_feature_no_longer_exists_at_to_milestone() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");

        fs::remove_dir_all(dir.path().join("knowledge/controls")).unwrap();
        commit_and_tag(dir.path(), "remove feature", "m2");

        let events = compute_changes(dir.path(), "m1", "m2", false).unwrap();

        assert_eq!(events.len(), 1);
        assert!(events[0].from_blob.is_some());
        assert_eq!(events[0].to_blob, None);
        assert!(events[0].impacted_testcases.is_empty());
    }

    #[test]
    fn serialize_changes_produces_valid_yaml() {
        let events = vec![ChangeEvent {
            event_id: "player-jump--m1--m2".to_string(),
            feature_id: "player-jump".to_string(),
            from_milestone: "m1".to_string(),
            to_milestone: "m2".to_string(),
            from_blob: Some("aaa".to_string()),
            to_blob: Some("bbb".to_string()),
            impacted_testcases: vec!["tc-ground-001".to_string()],
        }];

        let yaml = serialize_changes(&events);

        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(parsed[0]["feature_id"].as_str(), Some("player-jump"));
    }
}
