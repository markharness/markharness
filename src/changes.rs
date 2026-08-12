use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::generate;
use crate::git;
use crate::id_cache::{self, FeatureVersion};
use crate::lineage::{self, LineageKind};

/// The kind of change a human attaches to a `ChangeEvent` after the fact
/// (§3.5): a specification change, a bug fix, a refactor (behavior
/// unchanged), or anything not covered by those three.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeType {
    SpecChange,
    BugFix,
    Refactor,
    Other,
}

/// One detected Feature change between two milestones (§3.5 ChangeEvent).
/// `change_type` is computed as `None` here and filled in afterwards by a
/// human via `markharness changes annotate` (per docs/cli-manual.md UC5, it
/// is not computed from the diff itself).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangeEvent {
    pub event_id: String,
    pub feature_id: String,
    pub from_milestone: String,
    pub to_milestone: String,
    pub from_tree_sha: Option<String>,
    pub to_tree_sha: Option<String>,
    pub impacted_testcases: Vec<String>,
    #[serde(default)]
    pub change_type: Option<ChangeType>,
    /// One entry per two-parent merge commit found in the
    /// `from_milestone..to_milestone` interval (§3.2) at which this Feature
    /// is a true divergence (both parents changed it differently from
    /// their `git merge-base`), oldest merge first. Empty otherwise,
    /// including the ordinary linear case covered by
    /// `from_tree_sha`/`to_tree_sha`.
    #[serde(default)]
    pub true_divergences: Vec<TrueDivergence>,
    /// `event_id`s of other `ChangeEvent`s that a human has recorded as
    /// part of the same logical change (§3.5). Purely additive and
    /// human-populated via `markharness changes annotate --related`;
    /// doesn't affect the per-Feature automatic computation in
    /// `compute_changes`.
    #[serde(default)]
    pub related_events: Vec<String>,
}

/// A single true-divergence merge recorded against a `ChangeEvent`: the
/// merge commit itself (auditable via `markharness changes lineage
/// --commit <merge_commit>` or `git show <merge_commit>`) and the two
/// parent tree SHAs `[tree(P1), tree(P2)]` that diverged.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrueDivergence {
    pub merge_commit: String,
    pub parent_tree_shas: [String; 2],
}

/// For each Feature id, `[tree(P1), tree(P2)]` when `merge_commit` is a
/// two-parent merge commit and the Feature is a true divergence per
/// `lineage::classify` (§3.2). Returns an empty map when `merge_commit`
/// isn't itself a two-parent commit (defensive; callers only pass merge
/// commits found by `find_merge_commits_in_interval`).
fn true_divergence_parent_tree_shas(
    root: &Path,
    merge_commit: &str,
    use_cache: bool,
) -> io::Result<BTreeMap<String, [String; 2]>> {
    let parents = git::parents(root, merge_commit)?;
    let [p1, p2] = parents.as_slice() else {
        return Ok(BTreeMap::new());
    };
    let base = git::merge_base(root, p1, p2)?;

    let base_versions = tree_sha_map(id_cache::resolve_feature_versions(root, &base, use_cache)?);
    let p1_versions = tree_sha_map(id_cache::resolve_feature_versions(root, p1, use_cache)?);
    let p2_versions = tree_sha_map(id_cache::resolve_feature_versions(root, p2, use_cache)?);

    let all_ids: BTreeSet<&String> = p1_versions.keys().chain(p2_versions.keys()).collect();

    let mut result = BTreeMap::new();
    for feature_id in all_ids {
        let base_sha = base_versions.get(feature_id);
        let p1_sha = p1_versions.get(feature_id);
        let p2_sha = p2_versions.get(feature_id);
        // `TrueDivergence` can also occur when one branch deleted the
        // Feature and the other changed it (`p1_sha`/`p2_sha` not both
        // `Some`): there are no two tree SHAs to record in that case, so
        // fall back to the ordinary `from_tree_sha`/`to_tree_sha`
        // representation instead of populating `true_divergences`.
        if let (Some(p1_sha), Some(p2_sha)) = (p1_sha, p2_sha)
            && lineage::classify(base_sha, Some(p1_sha), Some(p2_sha))
                == LineageKind::TrueDivergence
        {
            result.insert(feature_id.clone(), [p1_sha.clone(), p2_sha.clone()]);
        }
    }
    Ok(result)
}

/// All two-parent merge commits in the `from_milestone..to_milestone`
/// interval, oldest first (`--reverse`, matching `generate.rs`'s
/// deterministic-ordering convention: `git rev-list` without it yields
/// newest-first).
fn find_merge_commits_in_interval(
    root: &Path,
    from_milestone: &str,
    to_milestone: &str,
) -> io::Result<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "rev-list",
            "--parents",
            "--ancestry-path",
            "--reverse",
            &format!("{from_milestone}..{to_milestone}"),
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "git rev-list failed for {from_milestone}..{to_milestone}: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let mut merge_commits = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut parts = line.split_whitespace();
        let Some(commit) = parts.next() else {
            continue;
        };
        let parent_count = parts.count();
        if parent_count == 2 {
            merge_commits.push(commit.to_string());
        }
    }

    Ok(merge_commits)
}

fn tree_sha_map(versions: Vec<FeatureVersion>) -> BTreeMap<String, String> {
    versions.into_iter().map(|v| (v.id, v.tree_sha)).collect()
}

/// Maps each Feature id to the `case_id`s of testcases generated from it,
/// using the *current* `knowledge/` working tree as the structural
/// generation graph (§3.2(A): `CONDITION`→`TESTCASE`, does not need version
/// history — only the version-history side, `derived_from`, does). Legacy
/// behavior, opted into via `compute_changes`'s `use_current_tree`: recomputing
/// the same past `from_milestone..to_milestone` interval later can yield a
/// different `impacted_testcases` set as the working tree keeps changing.
fn impacted_testcases_by_feature(root: &Path) -> io::Result<BTreeMap<String, Vec<String>>> {
    testcases_by_feature(generate::generate_testcases(&root.join("knowledge"))?)
}

/// Maps each Feature id to the `case_id`s of testcases generated from it, as
/// `knowledge/` existed at `milestone` (a git tag), independent of the
/// current working tree. This is `compute_changes`'s default: recomputing a
/// past `from_milestone..to_milestone` interval later always yields the same
/// `impacted_testcases`, because it's derived from `to_milestone`'s
/// committed tree rather than whatever `knowledge/` looks like right now.
///
/// `knowledge/` at `milestone` is materialized into a temporary `git
/// worktree` (rather than reading each blob individually) so the existing
/// `generate::generate_testcases` filesystem-walking logic can be reused
/// unchanged.
fn historical_testcases_by_feature(
    root: &Path,
    milestone: &str,
) -> io::Result<BTreeMap<String, Vec<String>>> {
    let tmp = tempfile::tempdir()?;
    let worktree_path = tmp.path().join("worktree");
    let add_status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["worktree", "add", "--detach", "-q"])
        .arg(&worktree_path)
        .arg(milestone)
        .status()?;
    if !add_status.success() {
        return Err(io::Error::other(format!(
            "git worktree add failed for milestone {milestone}"
        )));
    }

    let testcases = generate::generate_testcases(&worktree_path.join("knowledge"));

    // Best-effort cleanup: an orphaned worktree under a soon-to-be-deleted
    // temp dir doesn't leak data, but `git worktree remove` keeps `git
    // worktree list` clean. Not fatal if it fails.
    let _ = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["worktree", "remove", "--force"])
        .arg(&worktree_path)
        .status();

    testcases_by_feature(testcases?)
}

fn testcases_by_feature(
    testcases: Vec<generate::TestCase>,
) -> io::Result<BTreeMap<String, Vec<String>>> {
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
/// `to_milestone` (two git tags) by comparing each Feature's directory tree
/// SHA at each tag (§3.2〜3.4 の簡易版; マイルストーン=引数のtag名をそのまま
/// 使用)。Using the whole directory's tree SHA rather than just
/// `feature.yml`'s blob SHA means Condition/Behavior/ExpectedResult changes
/// are detected even when `feature.yml` itself is untouched.
///
/// `impacted_testcases` is derived from `to_milestone`'s tree by default
/// (`use_current_tree: false`), so recomputing the same past interval later
/// is deterministic. Pass `use_current_tree: true` to opt into the legacy
/// behavior of reading the current `knowledge/` working tree instead (see
/// `impacted_testcases_by_feature` / `historical_testcases_by_feature`).
pub fn compute_changes(
    root: &Path,
    from_milestone: &str,
    to_milestone: &str,
    use_cache: bool,
    use_current_tree: bool,
) -> io::Result<Vec<ChangeEvent>> {
    let from_versions = tree_sha_map(id_cache::resolve_feature_versions(
        root,
        from_milestone,
        use_cache,
    )?);
    let to_versions = tree_sha_map(id_cache::resolve_feature_versions(
        root,
        to_milestone,
        use_cache,
    )?);
    let impacted = if use_current_tree {
        impacted_testcases_by_feature(root)?
    } else {
        historical_testcases_by_feature(root, to_milestone)?
    };
    let merge_commits = find_merge_commits_in_interval(root, from_milestone, to_milestone)?;
    let mut true_divergences_by_feature: BTreeMap<String, Vec<TrueDivergence>> = BTreeMap::new();
    for merge_commit in &merge_commits {
        let divergences = true_divergence_parent_tree_shas(root, merge_commit, use_cache)?;
        for (feature_id, parent_tree_shas) in divergences {
            true_divergences_by_feature
                .entry(feature_id)
                .or_default()
                .push(TrueDivergence {
                    merge_commit: merge_commit.clone(),
                    parent_tree_shas,
                });
        }
    }

    let all_ids: BTreeSet<&String> = from_versions.keys().chain(to_versions.keys()).collect();

    let mut events = Vec::new();
    for feature_id in all_ids {
        let from_tree_sha = from_versions.get(feature_id).cloned();
        let to_tree_sha = to_versions.get(feature_id).cloned();
        if from_tree_sha == to_tree_sha {
            continue;
        }
        let true_divergences = true_divergences_by_feature
            .get(feature_id)
            .cloned()
            .unwrap_or_default();
        events.push(ChangeEvent {
            event_id: format!("{feature_id}--{from_milestone}--{to_milestone}"),
            feature_id: feature_id.clone(),
            from_milestone: from_milestone.to_string(),
            to_milestone: to_milestone.to_string(),
            from_tree_sha,
            to_tree_sha,
            impacted_testcases: impacted.get(feature_id).cloned().unwrap_or_default(),
            change_type: None,
            true_divergences,
            related_events: Vec::new(),
        });
    }

    Ok(events)
}

pub fn serialize_changes(events: &[ChangeEvent]) -> String {
    serde_yaml_ng::to_string(events).expect("ChangeEvent serialization is infallible")
}

/// Reads `changes/<milestone>.yaml` (the ChangeEvents whose `to_milestone`
/// is `milestone`, written by `compute_changes`/`serialize_changes`).
/// Returns an empty list if the file doesn't exist, rather than an error.
pub fn read_changes(root: &Path, milestone: &str) -> io::Result<Vec<ChangeEvent>> {
    let path = root.join("changes").join(format!("{milestone}.yaml"));
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path)?;
    serde_yaml_ng::from_str(&content).map_err(io::Error::other)
}

/// Why `markharness changes annotate` failed to set a `change_type` or
/// `related_events`.
#[derive(Debug)]
pub enum AnnotateError {
    /// No event with this `event_id` exists under `changes/`. Carries the
    /// offending id: for `annotate_related_events` this may be either the
    /// target `event_id` or one of the `--related` ids.
    NotFound(String),
    Io(io::Error),
}

impl From<io::Error> for AnnotateError {
    fn from(e: io::Error) -> Self {
        AnnotateError::Io(e)
    }
}

/// Sets `change_type` on the `ChangeEvent` identified by `event_id`,
/// searching every `changes/*.yaml` file (event ids are unique but a
/// caller need not know which milestone interval an event belongs to), and
/// rewrites that file in place (§3.5: `change_type` is filled in by a human
/// after `compute_changes`, not computed).
pub fn annotate_change_type(
    root: &Path,
    event_id: &str,
    change_type: ChangeType,
) -> Result<(), AnnotateError> {
    for path in changes_yaml_paths(root)? {
        let content = fs::read_to_string(&path)?;
        let mut events: Vec<ChangeEvent> =
            serde_yaml_ng::from_str(&content).map_err(io::Error::other)?;
        let Some(event) = events.iter_mut().find(|e| e.event_id == event_id) else {
            continue;
        };
        event.change_type = Some(change_type);
        fs::write(&path, serialize_changes(&events))?;
        return Ok(());
    }

    Err(AnnotateError::NotFound(event_id.to_string()))
}

fn changes_yaml_paths(root: &Path) -> io::Result<Vec<PathBuf>> {
    let changes_dir = root.join("changes");
    let mut entries: Vec<PathBuf> = fs::read_dir(&changes_dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "yaml"))
        .collect();
    entries.sort();
    Ok(entries)
}

/// Checks that `event_id` and every id in `related_ids` exist as an
/// `event_id` somewhere under `changes/*.yaml`, without writing anything.
/// Shared by `annotate_related_events` (which re-checks right before it
/// writes) and by callers that want to validate a `--related` id *before*
/// running an unrelated write (e.g. `changes annotate --type ... --related
/// ...`, so a typo'd `--related` id can't leave `change_type` written while
/// `related_events` isn't — see `markharness changes annotate`'s CLI
/// dispatch).
pub fn validate_annotate_ids(
    root: &Path,
    event_id: &str,
    related_ids: &[String],
) -> Result<(), AnnotateError> {
    let mut known_ids: BTreeSet<String> = BTreeSet::new();
    for path in changes_yaml_paths(root)? {
        let content = fs::read_to_string(&path)?;
        let events: Vec<ChangeEvent> =
            serde_yaml_ng::from_str(&content).map_err(io::Error::other)?;
        known_ids.extend(events.into_iter().map(|e| e.event_id));
    }

    if !known_ids.contains(event_id) {
        return Err(AnnotateError::NotFound(event_id.to_string()));
    }
    for related_id in related_ids {
        if !known_ids.contains(related_id) {
            return Err(AnnotateError::NotFound(related_id.clone()));
        }
    }
    Ok(())
}

/// Appends `related_ids` to `related_events` on the `ChangeEvent`
/// identified by `event_id` (§3.5: purely additive, human-recorded
/// cross-references between ChangeEvents; doesn't affect the automatic
/// per-Feature computation). Searches every `changes/*.yaml` file like
/// `annotate_change_type`. Every id in `related_ids` must itself exist as
/// an `event_id` somewhere under `changes/` (`validate_annotate_ids`),
/// checked up front so a partial write never happens because of a typo'd
/// `--related` id.
pub fn annotate_related_events(
    root: &Path,
    event_id: &str,
    related_ids: &[String],
) -> Result<(), AnnotateError> {
    validate_annotate_ids(root, event_id, related_ids)?;

    for path in changes_yaml_paths(root)? {
        let content = fs::read_to_string(&path)?;
        let mut events: Vec<ChangeEvent> =
            serde_yaml_ng::from_str(&content).map_err(io::Error::other)?;
        let Some(event) = events.iter_mut().find(|e| e.event_id == event_id) else {
            continue;
        };
        event.related_events.extend(related_ids.iter().cloned());
        fs::write(&path, serialize_changes(&events))?;
        return Ok(());
    }

    unreachable!("event_id was validated above, so it must be in some file's events")
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

        let events = compute_changes(dir.path(), "m1", "m2", false, false).unwrap();

        assert!(events.is_empty());
    }

    #[test]
    fn reports_changed_event_with_impacted_testcases_when_feature_tree_sha_differs() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");

        write_full_chain(dir.path(), "v2");
        commit_and_tag(dir.path(), "v2", "m2");

        let events = compute_changes(dir.path(), "m1", "m2", false, false).unwrap();

        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.feature_id, "player-jump");
        assert_eq!(event.event_id, "player-jump--m1--m2");
        assert!(event.from_tree_sha.is_some());
        assert!(event.to_tree_sha.is_some());
        assert_ne!(event.from_tree_sha, event.to_tree_sha);
        assert_eq!(event.impacted_testcases, vec!["tc-ground-001".to_string()]);
    }

    #[test]
    fn impacted_testcases_default_to_the_to_milestone_tree_ignoring_later_working_tree_changes() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");
        write_full_chain(dir.path(), "v2");
        commit_and_tag(dir.path(), "v2", "m2");

        // Simulate a later, uncommitted addition to the working tree made
        // after m2 was tagged: a second Condition/ExpectedResult under the
        // same Feature. Recomputing the m1..m2 interval later must not pick
        // this up under the default (historical) mode.
        let air = dir.path().join("knowledge/controls/player-jump/jump/air");
        fs::create_dir_all(&air).unwrap();
        fs::write(
            air.join("condition.yml"),
            "id: air\nbehavior: jump\nlabel: air\ndescription: |\n  Jump in the air.\n",
        )
        .unwrap();
        fs::create_dir_all(air.join("expected")).unwrap();
        fs::write(
            air.join("expected/001.yml"),
            "id: air-001\ncondition: air\ndescription: |\n  jumps safely\n",
        )
        .unwrap();

        let events = compute_changes(dir.path(), "m1", "m2", false, false).unwrap();

        let event = events
            .iter()
            .find(|e| e.feature_id == "player-jump")
            .unwrap();
        assert_eq!(event.impacted_testcases, vec!["tc-ground-001".to_string()]);
    }

    #[test]
    fn impacted_testcases_use_the_current_working_tree_when_use_current_tree_is_set() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");
        write_full_chain(dir.path(), "v2");
        commit_and_tag(dir.path(), "v2", "m2");

        // Same later, uncommitted addition as the historical-mode test
        // above, but this time the opt-in current-tree mode must reflect it
        // (legacy behavior, preserved for backward compatibility).
        let air = dir.path().join("knowledge/controls/player-jump/jump/air");
        fs::create_dir_all(&air).unwrap();
        fs::write(
            air.join("condition.yml"),
            "id: air\nbehavior: jump\nlabel: air\ndescription: |\n  Jump in the air.\n",
        )
        .unwrap();
        fs::create_dir_all(air.join("expected")).unwrap();
        fs::write(
            air.join("expected/001.yml"),
            "id: air-001\ncondition: air\ndescription: |\n  jumps safely\n",
        )
        .unwrap();

        let events = compute_changes(dir.path(), "m1", "m2", false, true).unwrap();

        let event = events
            .iter()
            .find(|e| e.feature_id == "player-jump")
            .unwrap();
        let mut impacted = event.impacted_testcases.clone();
        impacted.sort();
        assert_eq!(
            impacted,
            vec!["tc-air-001".to_string(), "tc-ground-001".to_string()]
        );
    }

    #[test]
    fn reports_changed_event_when_only_a_condition_file_changes_and_feature_yml_is_untouched() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");

        // Only the Condition's description changes; feature.yml is
        // byte-for-byte identical between m1 and m2.
        fs::write(
            dir.path()
                .join("knowledge/controls/player-jump/jump/ground/condition.yml"),
            "id: ground\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from a moving platform.\n",
        )
        .unwrap();
        commit_and_tag(dir.path(), "condition change", "m2");

        let events = compute_changes(dir.path(), "m1", "m2", false, false).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].feature_id, "player-jump");
    }

    #[test]
    fn reports_added_event_when_feature_did_not_exist_at_from_milestone() {
        let dir = init_repo();
        fs::create_dir_all(dir.path().join("knowledge")).unwrap();
        fs::write(dir.path().join("README.md"), "empty\n").unwrap();
        commit_and_tag(dir.path(), "empty", "m1");

        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "add feature", "m2");

        let events = compute_changes(dir.path(), "m1", "m2", false, false).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].from_tree_sha, None);
        assert!(events[0].to_tree_sha.is_some());
    }

    #[test]
    fn reports_removed_event_when_feature_no_longer_exists_at_to_milestone() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");

        fs::remove_dir_all(dir.path().join("knowledge/controls")).unwrap();
        commit_and_tag(dir.path(), "remove feature", "m2");

        let events = compute_changes(dir.path(), "m1", "m2", false, false).unwrap();

        assert_eq!(events.len(), 1);
        assert!(events[0].from_tree_sha.is_some());
        assert_eq!(events[0].to_tree_sha, None);
        assert!(events[0].impacted_testcases.is_empty());
    }

    #[test]
    fn compute_changes_leaves_change_type_as_none() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");
        write_full_chain(dir.path(), "v2");
        commit_and_tag(dir.path(), "v2", "m2");

        let events = compute_changes(dir.path(), "m1", "m2", false, false).unwrap();

        assert_eq!(events[0].change_type, None);
    }

    #[test]
    fn change_type_serializes_as_snake_case() {
        let events = vec![ChangeEvent {
            event_id: "player-jump--m1--m2".to_string(),
            feature_id: "player-jump".to_string(),
            from_milestone: "m1".to_string(),
            to_milestone: "m2".to_string(),
            from_tree_sha: Some("aaa".to_string()),
            to_tree_sha: Some("bbb".to_string()),
            impacted_testcases: vec!["tc-ground-001".to_string()],
            change_type: Some(ChangeType::SpecChange),
            true_divergences: Vec::new(),
            related_events: Vec::new(),
        }];

        let yaml = serialize_changes(&events);

        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(parsed[0]["change_type"].as_str(), Some("spec_change"));
    }

    /// `changes/*.yaml` files written before `change_type` existed have no
    /// such key; reading them must not fail (`#[serde(default)]`).
    #[test]
    fn read_changes_defaults_change_type_to_none_for_files_written_before_the_field_existed() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("changes")).unwrap();
        fs::write(
            dir.path().join("changes/m2.yaml"),
            "- event_id: player-jump--m1--m2\n  feature_id: player-jump\n  from_milestone: m1\n  to_milestone: m2\n  from_tree_sha: aaa\n  to_tree_sha: bbb\n  impacted_testcases: [tc-ground-001]\n",
        )
        .unwrap();

        let read = read_changes(dir.path(), "m2").unwrap();

        assert_eq!(read[0].change_type, None);
    }

    #[test]
    fn serialize_changes_produces_valid_yaml() {
        let events = vec![ChangeEvent {
            event_id: "player-jump--m1--m2".to_string(),
            feature_id: "player-jump".to_string(),
            from_milestone: "m1".to_string(),
            to_milestone: "m2".to_string(),
            from_tree_sha: Some("aaa".to_string()),
            to_tree_sha: Some("bbb".to_string()),
            impacted_testcases: vec!["tc-ground-001".to_string()],
            change_type: None,
            true_divergences: Vec::new(),
            related_events: Vec::new(),
        }];

        let yaml = serialize_changes(&events);

        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(parsed[0]["feature_id"].as_str(), Some("player-jump"));
    }

    #[test]
    fn read_changes_returns_events_written_by_serialize_changes() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("changes")).unwrap();
        let events = vec![ChangeEvent {
            event_id: "player-jump--m1--m2".to_string(),
            feature_id: "player-jump".to_string(),
            from_milestone: "m1".to_string(),
            to_milestone: "m2".to_string(),
            from_tree_sha: Some("aaa".to_string()),
            to_tree_sha: Some("bbb".to_string()),
            impacted_testcases: vec!["tc-ground-001".to_string()],
            change_type: None,
            true_divergences: Vec::new(),
            related_events: Vec::new(),
        }];
        fs::write(
            dir.path().join("changes/m2.yaml"),
            serialize_changes(&events),
        )
        .unwrap();

        let read = read_changes(dir.path(), "m2").unwrap();

        assert_eq!(read, events);
    }

    #[test]
    fn read_changes_returns_empty_when_file_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();

        let read = read_changes(dir.path(), "m2").unwrap();

        assert!(read.is_empty());
    }

    fn sample_event(event_id: &str) -> ChangeEvent {
        ChangeEvent {
            event_id: event_id.to_string(),
            feature_id: "player-jump".to_string(),
            from_milestone: "m1".to_string(),
            to_milestone: "m2".to_string(),
            from_tree_sha: Some("aaa".to_string()),
            to_tree_sha: Some("bbb".to_string()),
            impacted_testcases: vec!["tc-ground-001".to_string()],
            change_type: None,
            true_divergences: Vec::new(),
            related_events: Vec::new(),
        }
    }

    #[test]
    fn annotate_change_type_sets_the_field_on_the_matching_event() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("changes")).unwrap();
        fs::write(
            dir.path().join("changes/m2.yaml"),
            serialize_changes(&[sample_event("player-jump--m1--m2")]),
        )
        .unwrap();

        annotate_change_type(dir.path(), "player-jump--m1--m2", ChangeType::BugFix).unwrap();

        let events = read_changes(dir.path(), "m2").unwrap();
        assert_eq!(events[0].change_type, Some(ChangeType::BugFix));
    }

    #[test]
    fn annotate_change_type_preserves_other_events_in_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("changes")).unwrap();
        fs::write(
            dir.path().join("changes/m2.yaml"),
            serialize_changes(&[
                sample_event("player-jump--m1--m2"),
                sample_event("other-feature--m1--m2"),
            ]),
        )
        .unwrap();

        annotate_change_type(dir.path(), "player-jump--m1--m2", ChangeType::Refactor).unwrap();

        let events = read_changes(dir.path(), "m2").unwrap();
        let untouched = events
            .iter()
            .find(|e| e.event_id == "other-feature--m1--m2")
            .unwrap();
        assert_eq!(untouched.change_type, None);
    }

    #[test]
    fn annotate_change_type_searches_across_multiple_changes_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("changes")).unwrap();
        fs::write(
            dir.path().join("changes/m2.yaml"),
            serialize_changes(&[sample_event("player-jump--m1--m2")]),
        )
        .unwrap();
        fs::write(
            dir.path().join("changes/m3.yaml"),
            serialize_changes(&[sample_event("player-jump--m2--m3")]),
        )
        .unwrap();

        annotate_change_type(dir.path(), "player-jump--m2--m3", ChangeType::Other).unwrap();

        let events = read_changes(dir.path(), "m3").unwrap();
        assert_eq!(events[0].change_type, Some(ChangeType::Other));
    }

    #[test]
    fn records_both_parent_tree_shas_when_to_milestone_is_a_true_divergence_merge_commit() {
        let dir = init_repo();
        write_full_chain(dir.path(), "base");
        commit_and_tag(dir.path(), "base", "m1");
        run_git(dir.path(), &["branch", "feature"]);

        write_full_chain(dir.path(), "changed-on-main");
        commit_and_tag(dir.path(), "on main", "main-tip");

        run_git(dir.path(), &["checkout", "-q", "feature"]);
        write_full_chain(dir.path(), "changed-on-feature");
        commit_and_tag(dir.path(), "on feature", "feature-tip");

        run_git(dir.path(), &["checkout", "-q", "main"]);
        run_git(
            dir.path(),
            &[
                "merge", "-q", "-m", "merge", "-X", "ours", "--no-ff", "feature",
            ],
        );
        run_git(dir.path(), &["tag", "m2"]);

        let events = compute_changes(dir.path(), "m1", "m2", false, false).unwrap();

        let event = events
            .iter()
            .find(|e| e.feature_id == "player-jump")
            .unwrap();
        assert_eq!(event.true_divergences.len(), 1);
        let parent_tree_shas = &event.true_divergences[0].parent_tree_shas;
        assert_ne!(parent_tree_shas[0], parent_tree_shas[1]);
    }

    /// Regression: `lineage::classify` returns `TrueDivergence` not only when
    /// both parents changed a Feature differently, but also when one branch
    /// *deleted* the Feature and the other changed it (base=Some, one
    /// parent=None, other parent=Some(!=base) — neither equals the other nor
    /// the base). `true_divergence_parent_tree_shas` must not assume both
    /// parent tree SHAs are `Some` in that case.
    #[test]
    fn does_not_panic_when_true_divergence_involves_a_feature_deleted_on_one_branch() {
        let dir = init_repo();
        write_full_chain(dir.path(), "base");
        commit_and_tag(dir.path(), "base", "m1");
        run_git(dir.path(), &["branch", "feature"]);

        run_git(dir.path(), &["rm", "-rq", "knowledge/controls/player-jump"]);
        commit_and_tag(dir.path(), "delete on main", "main-tip");

        run_git(dir.path(), &["checkout", "-q", "feature"]);
        write_full_chain(dir.path(), "changed-on-feature");
        commit_and_tag(dir.path(), "change on feature", "feature-tip");

        run_git(dir.path(), &["checkout", "-q", "main"]);
        // A modify/delete conflict isn't auto-resolved by `-X ours`/`-X
        // theirs`; resolve it manually by keeping the feature branch's
        // (modified, surviving) version, matching a maintainer resolving a
        // real conflict in favor of the change rather than the deletion.
        let merge_status = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["merge", "--no-ff", "-q", "-m", "merge", "feature"])
            .status()
            .unwrap();
        assert!(!merge_status.success(), "expected a merge conflict");
        run_git(
            dir.path(),
            &[
                "checkout",
                "feature",
                "--",
                "knowledge/controls/player-jump",
            ],
        );
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "--no-edit"]);
        run_git(dir.path(), &["tag", "m2"]);

        let events = compute_changes(dir.path(), "m1", "m2", false, false).unwrap();

        let event = events
            .iter()
            .find(|e| e.feature_id == "player-jump")
            .unwrap();
        assert!(event.true_divergences.is_empty());
    }

    #[test]
    fn leaves_true_divergences_empty_when_to_milestone_is_not_a_merge_commit() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");
        write_full_chain(dir.path(), "v2");
        commit_and_tag(dir.path(), "v2", "m2");

        let events = compute_changes(dir.path(), "m1", "m2", false, false).unwrap();

        assert!(events[0].true_divergences.is_empty());
    }

    #[test]
    fn annotate_change_type_errors_when_event_id_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("changes")).unwrap();
        fs::write(
            dir.path().join("changes/m2.yaml"),
            serialize_changes(&[sample_event("player-jump--m1--m2")]),
        )
        .unwrap();

        let result = annotate_change_type(dir.path(), "no-such-event", ChangeType::Other);

        assert!(matches!(result, Err(AnnotateError::NotFound(id)) if id == "no-such-event"));
    }

    #[test]
    fn related_events_defaults_to_empty_and_round_trips_through_yaml() {
        let mut event = sample_event("player-jump--m1--m2");
        assert!(event.related_events.is_empty());
        event.related_events = vec!["other-feature--m1--m2".to_string()];

        let yaml = serialize_changes(&[event]);
        let parsed: Vec<ChangeEvent> = serde_yaml_ng::from_str(&yaml).unwrap();

        assert_eq!(
            parsed[0].related_events,
            vec!["other-feature--m1--m2".to_string()]
        );
    }

    /// `changes/*.yaml` files written before `related_events` existed have
    /// no such key; reading them must not fail (`#[serde(default)]`).
    #[test]
    fn read_changes_defaults_related_events_to_empty_for_files_written_before_the_field_existed() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("changes")).unwrap();
        fs::write(
            dir.path().join("changes/m2.yaml"),
            "- event_id: player-jump--m1--m2\n  feature_id: player-jump\n  from_milestone: m1\n  to_milestone: m2\n  from_tree_sha: aaa\n  to_tree_sha: bbb\n  impacted_testcases: [tc-ground-001]\n",
        )
        .unwrap();

        let read = read_changes(dir.path(), "m2").unwrap();

        assert!(read[0].related_events.is_empty());
    }

    #[test]
    fn annotate_related_events_appends_ids_on_the_matching_event() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("changes")).unwrap();
        fs::write(
            dir.path().join("changes/m2.yaml"),
            serialize_changes(&[
                sample_event("player-jump--m1--m2"),
                sample_event("other-feature--m1--m2"),
            ]),
        )
        .unwrap();

        annotate_related_events(
            dir.path(),
            "player-jump--m1--m2",
            &["other-feature--m1--m2".to_string()],
        )
        .unwrap();

        let events = read_changes(dir.path(), "m2").unwrap();
        let annotated = events
            .iter()
            .find(|e| e.event_id == "player-jump--m1--m2")
            .unwrap();
        assert_eq!(
            annotated.related_events,
            vec!["other-feature--m1--m2".to_string()]
        );
        let untouched = events
            .iter()
            .find(|e| e.event_id == "other-feature--m1--m2")
            .unwrap();
        assert!(untouched.related_events.is_empty());
    }

    #[test]
    fn annotate_related_events_errors_when_the_target_event_id_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("changes")).unwrap();
        fs::write(
            dir.path().join("changes/m2.yaml"),
            serialize_changes(&[sample_event("player-jump--m1--m2")]),
        )
        .unwrap();

        let result = annotate_related_events(dir.path(), "no-such-event", &[]);

        assert!(matches!(result, Err(AnnotateError::NotFound(id)) if id == "no-such-event"));
    }

    #[test]
    fn annotate_related_events_errors_when_a_related_event_id_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("changes")).unwrap();
        fs::write(
            dir.path().join("changes/m2.yaml"),
            serialize_changes(&[sample_event("player-jump--m1--m2")]),
        )
        .unwrap();

        let result = annotate_related_events(
            dir.path(),
            "player-jump--m1--m2",
            &["no-such-event".to_string()],
        );

        assert!(matches!(result, Err(AnnotateError::NotFound(id)) if id == "no-such-event"));
        let events = read_changes(dir.path(), "m2").unwrap();
        assert!(events[0].related_events.is_empty());
    }
}
