use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::fs_safety::{remove_dir_all_no_follow, replace_file};
use crate::git::{self, ObjectKind};
use crate::knowledge;

/// A Feature's id, its directory path, and the git tree SHA of its whole
/// subtree (feature.yml + behavior/condition/expected files below it) at
/// some git ref. The id is read from `feature.yml`'s `id:` field, not the
/// directory name, so it survives directory renames (§3.3 path-independent
/// id resolution). Using the directory's tree SHA rather than feature.yml's
/// own blob SHA means Condition/Behavior/ExpectedResult changes are captured
/// even when feature.yml itself is untouched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureVersion {
    pub id: String,
    pub path: String,
    pub tree_sha: String,
    /// The Feature's immutable identity (ADR 0013), when it has one.
    /// `None` for a Feature that predates `identity migrate` or a project
    /// that hasn't adopted the identity model at all — consumers must
    /// treat that as "no uid-based identity available yet", not as a
    /// missing/corrupt value.
    #[serde(default)]
    pub uid: Option<String>,
}

fn cache_dir(root: &Path) -> PathBuf {
    root.join(".markharness-cache")
}

fn sanitize_ref(git_ref: &str) -> String {
    git_ref
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn cache_path(root: &Path, git_ref: &str) -> PathBuf {
    cache_dir(root).join(format!("{}.json", sanitize_ref(git_ref)))
}

/// Bumped when the rules for which fields feed the cache key (not their
/// values) change; §3.3 `canonicalization_rule_version`.
const CANONICALIZATION_RULE_VERSION: &str = "1";
/// Bumped when the on-disk cache file's own JSON shape changes; §3.3
/// `id_index_schema_version`.
const ID_INDEX_SCHEMA_VERSION: &str = "1";

/// A content-addressable cache key (§3.3): `knowledge/`'s tree SHA at
/// `git_ref` plus three version tags. Any of the four changing invalidates
/// the cache on read — see `resolve_feature_versions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CacheKey {
    tree_sha: Option<String>,
    canonicalization_rule_version: String,
    id_index_schema_version: String,
    tool_version: String,
}

fn compute_cache_key(root: &Path, git_ref: &str) -> io::Result<CacheKey> {
    Ok(CacheKey {
        tree_sha: git::tree_sha(root, git_ref, crate::project_root::KNOWLEDGE_PATH_IN_REPO)?,
        canonicalization_rule_version: CANONICALIZATION_RULE_VERSION.to_string(),
        id_index_schema_version: ID_INDEX_SCHEMA_VERSION.to_string(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheFile {
    key: CacheKey,
    entries: Vec<FeatureVersion>,
}

/// Returns the feature directory's own path (the blob's parent) for a
/// `knowledge/<requirement>/<feature>/feature.yml` blob's path.
fn feature_dir_from_feature_yml_path(path: &str) -> Option<&str> {
    path.strip_suffix("/feature.yml")
}

/// Resolves every Feature's directory tree SHA at `git_ref` by walking
/// `knowledge/` with a single `git ls-tree -r -t`. When `use_cache` is true,
/// consults `.markharness-cache/` first, keyed by a content-addressable
/// `CacheKey` (§3.3: `knowledge/`'s tree SHA at `git_ref` + rule/schema/tool
/// version tags) — a stored cache whose key no longer matches the current
/// one is silently discarded and recomputed, rather than trusted or
/// requiring a manual `cache rebuild`. Discovers Feature directories via
/// their `feature.yml` blob, then looks up that directory's own tree entry
/// in the same listing — no per-Feature git subprocess needed for the tree
/// SHA. The id itself is read from `feature.yml`'s `id:` field (via `git
/// cat-file -p`, keyed by the blob SHA the same `ls-tree` listing already
/// gave us), not the directory name, so that renaming a Feature's directory
/// while keeping `id:` stable does not change its identity (§3.3
/// path-independent id resolution). Two Feature directories resolving to the
/// same id is an error rather than a silently dropped duplicate.
pub fn resolve_feature_versions(
    root: &Path,
    git_ref: &str,
    use_cache: bool,
) -> io::Result<Vec<FeatureVersion>> {
    let current_key = if use_cache {
        Some(compute_cache_key(root, git_ref)?)
    } else {
        None
    };

    if let Some(current_key) = &current_key
        && let Ok(cached) = fs::read_to_string(cache_path(root, git_ref))
        && let Ok(cache_file) = serde_json::from_str::<CacheFile>(&cached)
        && &cache_file.key == current_key
    {
        return Ok(cache_file.entries);
    }

    let tree_entries =
        git::ls_tree_recursive(root, git_ref, crate::project_root::KNOWLEDGE_PATH_IN_REPO)?;

    let mut by_id: BTreeMap<String, FeatureVersion> = BTreeMap::new();
    for entry in &tree_entries {
        if entry.kind != ObjectKind::Blob {
            continue;
        }
        let Some(dir_path) = feature_dir_from_feature_yml_path(&entry.path) else {
            continue;
        };
        let Some(dir_entry) = tree_entries
            .iter()
            .find(|e| e.kind == ObjectKind::Tree && e.path == dir_path)
        else {
            continue;
        };
        let content = git::show_blob_by_sha(root, &entry.sha)?;
        let feature = knowledge::parse_feature(&content).map_err(io::Error::other)?;

        if let Some(existing) = by_id.get(&feature.id) {
            return Err(io::Error::other(format!(
                "duplicate Feature id '{}' at {}: found at both '{}' and '{}'",
                feature.id, git_ref, existing.path, dir_path
            )));
        }
        by_id.insert(
            feature.id.clone(),
            FeatureVersion {
                id: feature.id,
                path: dir_path.to_string(),
                tree_sha: dir_entry.sha.clone(),
                uid: feature.uid,
            },
        );
    }
    let features: Vec<FeatureVersion> = by_id.into_values().collect();

    if let Some(current_key) = current_key {
        let path = cache_path(root, git_ref);
        let cache_file = CacheFile {
            key: current_key,
            entries: features.clone(),
        };
        let json = serde_json::to_string(&cache_file).map_err(io::Error::other)?;
        replace_file(root, &path, json.as_bytes())?;
    }

    Ok(features)
}

/// A Behavior's or Condition's id, its directory path, its subtree's tree
/// SHA at some git ref, and the directory path of the Feature it belongs
/// to. Used only to narrow `impacted_testcases` when `--granularity
/// behavior`/`condition` is requested (issue #15). Unlike `FeatureVersion`
/// this carries no `uid`: Behavior/Condition rename tracking across
/// milestones is explicitly out of scope for that option — it only narrows
/// the candidate set for a Feature already known (via `FeatureVersion`) to
/// have changed, so it doesn't need its own identity/rename model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubunitVersion {
    pub id: String,
    pub path: String,
    pub tree_sha: String,
    pub parent_feature_dir: String,
    /// The canonical `id:` (not directory name — same path-independence
    /// concern as Feature ids, §3.3) of the Behavior this Condition sits
    /// under. `None` for a `resolve_behavior_versions` result, where it
    /// doesn't apply. `changes.rs` needs this to build the same
    /// `(feature_id, behavior_id, condition_id)` key `generate.rs` writes
    /// into `TestCase.generated_from` — the Condition directory's own
    /// parent directory *name* isn't guaranteed to equal the Behavior's
    /// `id:` field.
    pub parent_behavior_id: Option<String>,
}

/// Behavior directories sit directly under their Feature directory
/// (`<feature>/<behavior>/`).
fn feature_dir_of_behavior_dir(behavior_dir: &str) -> Option<&str> {
    behavior_dir.rsplit_once('/').map(|(parent, _)| parent)
}

/// Condition directories sit one level below their Behavior directory
/// (`<feature>/<behavior>/<condition>/`).
fn feature_dir_of_condition_dir(condition_dir: &str) -> Option<&str> {
    let (behavior_dir, _) = condition_dir.rsplit_once('/')?;
    behavior_dir.rsplit_once('/').map(|(parent, _)| parent)
}

/// Shared implementation behind `resolve_behavior_versions` and
/// `resolve_condition_versions`: the same "marker file → parent directory's
/// tree SHA" pattern `resolve_feature_versions` uses, generalized to an
/// arbitrary marker filename and directory depth.
///
/// Duplicate ids are only an error *among immediate siblings* (the marker
/// directory's own parent — the Feature dir for a Behavior, the Behavior
/// dir for a Condition), not project- or Feature-wide: `docs/ja/cli-manual.md`
/// 1.2節's interactive `knowledge add` flow only checks for reuse within the
/// currently selected Feature/Behavior. So two different Features may
/// legitimately each have e.g. a `validate` Behavior, and two different
/// Behaviors under the *same* Feature may legitimately each have e.g. an
/// `empty-input` Condition — `feature_dir_of` (used only to tag each
/// resolved version with the Feature it ultimately belongs to, for
/// `changes.rs`'s per-Feature narrowing) must not be used as the
/// uniqueness scope for Conditions, which sit one level deeper than that.
fn resolve_marker_versions(
    root: &Path,
    git_ref: &str,
    marker_file: &str,
    feature_dir_of: impl Fn(&str) -> Option<&str>,
    parse_id: impl Fn(&str) -> Result<String, serde_yaml_ng::Error>,
    parent_behavior_id_of: impl Fn(&str) -> Option<String>,
) -> io::Result<Vec<SubunitVersion>> {
    let tree_entries =
        git::ls_tree_recursive(root, git_ref, crate::project_root::KNOWLEDGE_PATH_IN_REPO)?;

    let marker_suffix = format!("/{marker_file}");
    let mut by_key: BTreeMap<(String, String), SubunitVersion> = BTreeMap::new();
    for entry in &tree_entries {
        if entry.kind != ObjectKind::Blob {
            continue;
        }
        let Some(dir_path) = entry.path.strip_suffix(&marker_suffix) else {
            continue;
        };
        let Some(feature_dir) = feature_dir_of(dir_path) else {
            continue;
        };
        let Some(sibling_scope) = dir_path.rsplit_once('/').map(|(parent, _)| parent) else {
            continue;
        };
        let Some(dir_entry) = tree_entries
            .iter()
            .find(|e| e.kind == ObjectKind::Tree && e.path == dir_path)
        else {
            continue;
        };
        let content = git::show_blob_by_sha(root, &entry.sha)?;
        let id = parse_id(&content).map_err(io::Error::other)?;

        let key = (sibling_scope.to_string(), id.clone());
        if let Some(existing) = by_key.get(&key) {
            return Err(io::Error::other(format!(
                "duplicate id '{id}' under '{sibling_scope}' at {git_ref}: found at both '{}' and '{dir_path}'",
                existing.path
            )));
        }
        by_key.insert(
            key,
            SubunitVersion {
                id,
                path: dir_path.to_string(),
                tree_sha: dir_entry.sha.clone(),
                parent_feature_dir: feature_dir.to_string(),
                parent_behavior_id: parent_behavior_id_of(dir_path),
            },
        );
    }
    Ok(by_key.into_values().collect())
}

/// Resolves every Behavior's directory tree SHA at `git_ref`, for narrowing
/// `impacted_testcases` at `--granularity behavior` (issue #15). Mirrors
/// `resolve_feature_versions`'s pattern one level deeper. Not cached:
/// unlike Feature resolution (on the hot path of every `changes compute`),
/// this only runs when a non-default `--granularity` is requested.
pub fn resolve_behavior_versions(root: &Path, git_ref: &str) -> io::Result<Vec<SubunitVersion>> {
    resolve_marker_versions(
        root,
        git_ref,
        "behavior.yml",
        feature_dir_of_behavior_dir,
        |content| knowledge::parse_behavior(content).map(|b| b.id),
        |_behavior_dir| None,
    )
}

/// Same as `resolve_behavior_versions`, one level deeper, for
/// `--granularity condition`. Also resolves each Condition's parent
/// Behavior's canonical `id:` (`SubunitVersion::parent_behavior_id`) by
/// scanning the same `ls-tree` listing for `behavior.yml` blobs — a second
/// pass over already-fetched data, no extra `git` subprocess.
pub fn resolve_condition_versions(root: &Path, git_ref: &str) -> io::Result<Vec<SubunitVersion>> {
    let tree_entries =
        git::ls_tree_recursive(root, git_ref, crate::project_root::KNOWLEDGE_PATH_IN_REPO)?;

    let mut behavior_id_by_dir: BTreeMap<String, String> = BTreeMap::new();
    for entry in &tree_entries {
        if entry.kind != ObjectKind::Blob {
            continue;
        }
        let Some(behavior_dir) = behavior_dir_from_behavior_yml_path(&entry.path) else {
            continue;
        };
        let content = git::show_blob_by_sha(root, &entry.sha)?;
        let behavior = knowledge::parse_behavior(&content).map_err(io::Error::other)?;
        behavior_id_by_dir.insert(behavior_dir.to_string(), behavior.id);
    }

    resolve_marker_versions(
        root,
        git_ref,
        "condition.yml",
        feature_dir_of_condition_dir,
        |content| knowledge::parse_condition(content).map(|c| c.id),
        |condition_dir| {
            let (behavior_dir, _) = condition_dir.rsplit_once('/')?;
            behavior_id_by_dir.get(behavior_dir).cloned()
        },
    )
}

/// Returns a `behavior.yml` blob path's own directory (the Behavior's
/// directory itself, not its parent Feature — distinct from
/// `feature_dir_of_behavior_dir`, which goes one level further).
fn behavior_dir_from_behavior_yml_path(behavior_yml_path: &str) -> Option<&str> {
    behavior_yml_path.strip_suffix("/behavior.yml")
}

/// `markharness cache rebuild`: discards `.markharness-cache/` outright,
/// letting the next `changes compute` recompute lazily (§UC7, cache rebuild
/// は全削除のみで即時再計算はしない設計).
pub fn rebuild_cache(root: &Path) -> io::Result<()> {
    remove_dir_all_no_follow(root, &cache_dir(root))
}

/// A Feature's identity key (ADR 0013, design doc §2): its `uid` when it
/// has one, else its `id`. Un-migrated Features (no `uid` anywhere in the
/// project) therefore compare exactly as before ADR 0013 — the mixed-mode
/// fallback for projects that haven't run `identity migrate`. Shared by
/// every consumer that must match a Feature across two refs (`changes.rs`,
/// `lineage.rs`, and others as they adopt the identity model), so they
/// never independently reinvent (and risk diverging on) this rule.
///
/// A Feature whose `uid` is present on only one side of a comparison (the
/// migration-boundary case: it was migrated *during* the interval) is not
/// specially reconciled here — it appears once under its old `id`-keyed
/// identity and once under its `uid`-keyed identity, exactly like a
/// delete-then-add. Resolving that boundary is `identity migrate`'s
/// migration manifest (design doc §12, Phase 4), not this function.
pub fn identity_key(version: &FeatureVersion) -> String {
    version.uid.clone().unwrap_or_else(|| version.id.clone())
}

pub fn by_identity_key(versions: Vec<FeatureVersion>) -> BTreeMap<String, FeatureVersion> {
    versions
        .into_iter()
        .map(|v| (identity_key(&v), v))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn init_repo_with_feature(feature_id: &str, requirement_id: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        run_git(dir.path(), &["config", "core.autocrlf", "false"]);
        let feature_dir = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge")
            .join(requirement_id)
            .join(feature_id);
        fs::create_dir_all(&feature_dir).unwrap();
        fs::write(
            feature_dir.join("feature.yml"),
            format!(
                "id: {feature_id}\nrequirement: {requirement_id}\nlabel: {feature_id}\naxis: []\n"
            ),
        )
        .unwrap();
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "add feature"]);
        run_git(dir.path(), &["tag", "m1"]);
        dir
    }

    /// Regression test for the id_cache.rs (dir name) vs generate.rs
    /// (feature.yml's `id:` field) split identified in §3.3: renaming a
    /// Feature directory while keeping `id:` stable must still resolve to
    /// the same id (path-independence, the minimal §3.3 rename-tracking).
    #[test]
    fn resolves_feature_id_from_yaml_id_field_even_after_directory_rename() {
        let dir = init_repo_with_feature("player-jump", "controls");

        let old_dir = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump");
        let new_dir = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump-renamed");
        fs::rename(&old_dir, &new_dir).unwrap();
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "rename directory"]);
        run_git(dir.path(), &["tag", "m2"]);

        let versions = resolve_feature_versions(dir.path(), "m2", false).unwrap();

        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].id, "player-jump");
        assert_eq!(
            versions[0].path,
            ".markharness/knowledge/controls/player-jump-renamed"
        );
    }

    /// Two Feature directories whose `feature.yml` both declare the same
    /// `id:` must be rejected rather than silently collapsed into one entry
    /// (or worse, non-deterministically overwriting each other).
    #[test]
    fn errors_when_two_feature_directories_declare_the_same_id() {
        let dir = init_repo_with_feature("player-jump", "controls");

        let dup_dir = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump-duplicate");
        fs::create_dir_all(&dup_dir).unwrap();
        fs::write(
            dup_dir.join("feature.yml"),
            "id: player-jump\nrequirement: controls\nlabel: dup\naxis: []\n",
        )
        .unwrap();
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "duplicate id"]);
        run_git(dir.path(), &["tag", "m2"]);

        let result = resolve_feature_versions(dir.path(), "m2", false);

        assert!(result.is_err());
    }

    #[test]
    fn resolves_feature_id_and_tree_sha_at_ref() {
        let dir = init_repo_with_feature("player-jump", "controls");

        let versions = resolve_feature_versions(dir.path(), "m1", false).unwrap();

        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].id, "player-jump");
        assert_eq!(
            versions[0].path,
            ".markharness/knowledge/controls/player-jump"
        );
        assert_eq!(versions[0].tree_sha.len(), 40);
    }

    #[test]
    fn resolves_feature_id_without_uid_as_none() {
        let dir = init_repo_with_feature("player-jump", "controls");

        let versions = resolve_feature_versions(dir.path(), "m1", false).unwrap();

        assert_eq!(versions[0].uid, None);
    }

    /// ADR 0013: once a Feature has been issued a `uid`, `id_cache` must
    /// surface it so downstream consumers can adopt uid-based identity.
    #[test]
    fn resolves_feature_uid_when_present() {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        run_git(dir.path(), &["config", "core.autocrlf", "false"]);
        let feature_dir = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge")
            .join("controls")
            .join("player-jump");
        fs::create_dir_all(&feature_dir).unwrap();
        fs::write(
            feature_dir.join("feature.yml"),
            "id: player-jump\nrequirement: controls\nlabel: player-jump\naxis: []\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n",
        )
        .unwrap();
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "add feature"]);
        run_git(dir.path(), &["tag", "m1"]);

        let versions = resolve_feature_versions(dir.path(), "m1", false).unwrap();

        assert_eq!(
            versions[0].uid,
            Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string())
        );
    }

    #[test]
    fn without_cache_recomputes_and_reflects_new_commits() {
        let dir = init_repo_with_feature("player-jump", "controls");
        let first = resolve_feature_versions(dir.path(), "m1", false).unwrap();

        fs::write(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
            "id: player-jump\nrequirement: controls\nlabel: player-jump\naxis: [gameplay]\n",
        )
        .unwrap();
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "update"]);
        run_git(dir.path(), &["tag", "m2"]);

        let second = resolve_feature_versions(dir.path(), "m2", false).unwrap();

        assert_ne!(first[0].tree_sha, second[0].tree_sha);
    }

    /// The regression test motivating this design: a Condition file added
    /// under a Feature, with feature.yml itself left untouched, must still
    /// change the Feature's version identifier — otherwise `changes compute`
    /// silently misses the change (the bug this module fixes).
    #[test]
    fn tree_sha_changes_when_a_condition_is_added_without_touching_feature_yml() {
        let dir = init_repo_with_feature("player-jump", "controls");
        let first = resolve_feature_versions(dir.path(), "m1", false).unwrap();

        let behavior_dir = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/jump");
        fs::create_dir_all(&behavior_dir).unwrap();
        fs::write(
            behavior_dir.join("behavior.yml"),
            "id: jump\nfeature: player-jump\nlabel: jump\naxis: []\ndescription: |\n  Player presses jump.\n",
        )
        .unwrap();
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "add behavior"]);
        run_git(dir.path(), &["tag", "m2"]);

        let second = resolve_feature_versions(dir.path(), "m2", false).unwrap();

        assert_ne!(first[0].tree_sha, second[0].tree_sha);
    }

    fn write_behavior(dir: &Path, feature_path: &str, behavior_id: &str) -> PathBuf {
        let behavior_dir = dir
            .join(".markharness/knowledge")
            .join(feature_path)
            .join(behavior_id);
        fs::create_dir_all(&behavior_dir).unwrap();
        fs::write(
            behavior_dir.join("behavior.yml"),
            format!(
                "id: {behavior_id}\nfeature: player-jump\nlabel: {behavior_id}\naxis: []\ndescription: |\n  desc.\n"
            ),
        )
        .unwrap();
        behavior_dir
    }

    fn write_condition(behavior_dir: &Path, condition_id: &str) -> PathBuf {
        let condition_dir = behavior_dir.join(condition_id);
        fs::create_dir_all(&condition_dir).unwrap();
        fs::write(
            condition_dir.join("condition.yml"),
            format!("id: {condition_id}\nbehavior: jump\nlabel: {condition_id}\ndescription: |\n  desc.\n"),
        )
        .unwrap();
        condition_dir
    }

    #[test]
    fn resolve_behavior_versions_resolves_id_path_and_tree_sha() {
        let dir = init_repo_with_feature("player-jump", "controls");
        write_behavior(dir.path(), "controls/player-jump", "jump");
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "add behavior"]);
        run_git(dir.path(), &["tag", "m2"]);

        let versions = resolve_behavior_versions(dir.path(), "m2").unwrap();

        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].id, "jump");
        assert_eq!(
            versions[0].path,
            ".markharness/knowledge/controls/player-jump/jump"
        );
        assert_eq!(
            versions[0].parent_feature_dir,
            ".markharness/knowledge/controls/player-jump"
        );
    }

    #[test]
    fn resolve_behavior_versions_tree_sha_changes_when_only_that_behavior_is_edited() {
        let dir = init_repo_with_feature("player-jump", "controls");
        write_behavior(dir.path(), "controls/player-jump", "jump");
        let jump2_dir = write_behavior(dir.path(), "controls/player-jump", "duck");
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "add behaviors"]);
        run_git(dir.path(), &["tag", "m1b"]);
        let first = resolve_behavior_versions(dir.path(), "m1b").unwrap();

        fs::write(
            jump2_dir.join("behavior.yml"),
            "id: duck\nfeature: player-jump\nlabel: duck\naxis: []\ndescription: |\n  edited.\n",
        )
        .unwrap();
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "edit duck"]);
        run_git(dir.path(), &["tag", "m2b"]);
        let second = resolve_behavior_versions(dir.path(), "m2b").unwrap();

        let jump_before = first.iter().find(|v| v.id == "jump").unwrap();
        let jump_after = second.iter().find(|v| v.id == "jump").unwrap();
        let duck_before = first.iter().find(|v| v.id == "duck").unwrap();
        let duck_after = second.iter().find(|v| v.id == "duck").unwrap();
        assert_eq!(jump_before.tree_sha, jump_after.tree_sha);
        assert_ne!(duck_before.tree_sha, duck_after.tree_sha);
    }

    #[test]
    fn resolve_behavior_versions_errors_on_duplicate_id_within_the_same_feature() {
        let dir = init_repo_with_feature("player-jump", "controls");
        write_behavior(dir.path(), "controls/player-jump", "jump");
        let dup_dir = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/jump-dup");
        fs::create_dir_all(&dup_dir).unwrap();
        fs::write(
            dup_dir.join("behavior.yml"),
            "id: jump\nfeature: player-jump\nlabel: jump\naxis: []\ndescription: |\n  dup.\n",
        )
        .unwrap();
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "dup behavior"]);
        run_git(dir.path(), &["tag", "m2"]);

        let result = resolve_behavior_versions(dir.path(), "m2");

        assert!(result.is_err());
    }

    /// Two different Features may each have a Behavior with the same id
    /// (only sibling-scoped uniqueness is required, per the interactive
    /// `knowledge add` flow's reuse check — `docs/ja/cli-manual.md` 1.2節).
    #[test]
    fn resolve_behavior_versions_allows_the_same_id_under_different_features() {
        let dir = init_repo_with_feature("player-jump", "controls");
        write_behavior(dir.path(), "controls/player-jump", "validate");
        let other_feature_dir = dir
            .path()
            .join(".markharness/knowledge/controls/other-feature");
        fs::create_dir_all(&other_feature_dir).unwrap();
        fs::write(
            other_feature_dir.join("feature.yml"),
            "id: other-feature\nrequirement: controls\nlabel: other-feature\naxis: []\n",
        )
        .unwrap();
        write_behavior(dir.path(), "controls/other-feature", "validate");
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "add behaviors"]);
        run_git(dir.path(), &["tag", "m2"]);

        let versions = resolve_behavior_versions(dir.path(), "m2").unwrap();

        assert_eq!(versions.iter().filter(|v| v.id == "validate").count(), 2);
    }

    #[test]
    fn resolve_condition_versions_resolves_id_path_and_parent_feature_dir() {
        let dir = init_repo_with_feature("player-jump", "controls");
        let behavior_dir = write_behavior(dir.path(), "controls/player-jump", "jump");
        write_condition(&behavior_dir, "ground");
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "add condition"]);
        run_git(dir.path(), &["tag", "m2"]);

        let versions = resolve_condition_versions(dir.path(), "m2").unwrap();

        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].id, "ground");
        assert_eq!(
            versions[0].path,
            ".markharness/knowledge/controls/player-jump/jump/ground"
        );
        assert_eq!(
            versions[0].parent_feature_dir,
            ".markharness/knowledge/controls/player-jump"
        );
    }

    /// Regression test for the sibling-scope bug: Condition ids must only
    /// be checked for uniqueness within their own Behavior, not across the
    /// whole Feature — two different Behaviors under the same Feature
    /// legitimately reusing a Condition id (e.g. both `add-task` and
    /// `edit-task` having an `empty-input` Condition) must not error.
    #[test]
    fn resolve_condition_versions_allows_the_same_id_under_different_behaviors_of_the_same_feature()
    {
        let dir = init_repo_with_feature("player-jump", "controls");
        let jump_dir = write_behavior(dir.path(), "controls/player-jump", "jump");
        write_condition(&jump_dir, "empty-input");
        let duck_dir = write_behavior(dir.path(), "controls/player-jump", "duck");
        write_condition(&duck_dir, "empty-input");
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "add conditions"]);
        run_git(dir.path(), &["tag", "m2"]);

        let versions = resolve_condition_versions(dir.path(), "m2").unwrap();

        assert_eq!(versions.iter().filter(|v| v.id == "empty-input").count(), 2);
    }

    #[test]
    fn resolve_condition_versions_errors_on_duplicate_id_within_the_same_behavior() {
        let dir = init_repo_with_feature("player-jump", "controls");
        let jump_dir = write_behavior(dir.path(), "controls/player-jump", "jump");
        write_condition(&jump_dir, "ground");
        let dup_dir = jump_dir.join("ground-dup");
        fs::create_dir_all(&dup_dir).unwrap();
        fs::write(
            dup_dir.join("condition.yml"),
            "id: ground\nbehavior: jump\nlabel: ground\ndescription: |\n  dup.\n",
        )
        .unwrap();
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "dup condition"]);
        run_git(dir.path(), &["tag", "m2"]);

        let result = resolve_condition_versions(dir.path(), "m2");

        assert!(result.is_err());
    }

    #[cfg(unix)]
    fn link_dir(link: &Path, target: &Path) {
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[cfg(windows)]
    fn link_dir(link: &Path, target: &Path) {
        let status = Command::new("cmd")
            .args(["/c", "mklink", "/j"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "mklink /j failed");
    }

    #[test]
    fn refuses_to_write_the_cache_through_a_symlinked_cache_dir() {
        let dir = init_repo_with_feature("player-jump", "controls");
        let outside = tempfile::tempdir().unwrap();
        link_dir(&dir.path().join(".markharness-cache"), outside.path());

        let result = resolve_feature_versions(dir.path(), "m1", true);

        assert!(
            result.is_err(),
            "expected resolve_feature_versions to refuse a symlinked cache dir"
        );
        assert!(!outside.path().join("m1.json").exists());
    }

    #[test]
    fn with_cache_writes_and_reuses_cache_file() {
        let dir = init_repo_with_feature("player-jump", "controls");

        let first = resolve_feature_versions(dir.path(), "m1", true).unwrap();
        assert!(
            dir.path()
                .join(".markharness-cache")
                .join("m1.json")
                .is_file()
        );

        // Tamper with the working tree without committing/tagging again:
        // a cached call must not see this, proving it read the cache file
        // rather than recomputing via `git ls-tree`.
        fs::write(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
            "id: player-jump\nrequirement: controls\nlabel: player-jump\naxis: [gameplay]\n",
        )
        .unwrap();

        let second = resolve_feature_versions(dir.path(), "m1", true).unwrap();

        assert_eq!(first, second);
    }

    /// §3.3: a cache file whose stored key no longer matches the current
    /// one (here: a stale `tool_version`, standing in for any of the four
    /// key components going out of date — e.g. a `tool_version` bump between
    /// CLI releases) must be silently discarded and recomputed, not
    /// trusted — no manual `cache rebuild` required.
    #[test]
    fn stale_cache_key_is_silently_recomputed_instead_of_trusted() {
        let dir = init_repo_with_feature("player-jump", "controls");
        resolve_feature_versions(dir.path(), "m1", true).unwrap();

        let cache_path = dir.path().join(".markharness-cache").join("m1.json");
        let mut cache_file: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&cache_path).unwrap()).unwrap();
        cache_file["key"]["tool_version"] = serde_json::json!("stale-version");
        cache_file["entries"] = serde_json::json!([
            {"id": "bogus", "path": "bogus", "tree_sha": "bogus"}
        ]);
        fs::write(&cache_path, serde_json::to_string(&cache_file).unwrap()).unwrap();

        let versions = resolve_feature_versions(dir.path(), "m1", true).unwrap();

        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].id, "player-jump");
    }

    /// §3.3 破棄条件2: `canonicalization_rule_version`(正規化ルール自体の
    /// 改訂)が不一致の場合も、`tool_version`と同様に静かに再計算される
    /// ことを個別に検証する(この定数は本テスト作成時点でまだ"1"から
    /// 改訂されたことがなく、実際の改訂運用は未検証のままである旨を
    /// 論文§3.3・Future Workに明記している)。
    #[test]
    fn stale_canonicalization_rule_version_is_silently_recomputed_instead_of_trusted() {
        let dir = init_repo_with_feature("player-jump", "controls");
        resolve_feature_versions(dir.path(), "m1", true).unwrap();

        let cache_path = dir.path().join(".markharness-cache").join("m1.json");
        let mut cache_file: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&cache_path).unwrap()).unwrap();
        cache_file["key"]["canonicalization_rule_version"] = serde_json::json!("0");
        cache_file["entries"] = serde_json::json!([
            {"id": "bogus", "path": "bogus", "tree_sha": "bogus"}
        ]);
        fs::write(&cache_path, serde_json::to_string(&cache_file).unwrap()).unwrap();

        let versions = resolve_feature_versions(dir.path(), "m1", true).unwrap();

        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].id, "player-jump");
    }

    /// §3.3 破棄条件3: `id_index_schema_version`(id-indexのフォーマット
    /// 自体の改訂)が不一致の場合も同様に検証する。
    #[test]
    fn stale_id_index_schema_version_is_silently_recomputed_instead_of_trusted() {
        let dir = init_repo_with_feature("player-jump", "controls");
        resolve_feature_versions(dir.path(), "m1", true).unwrap();

        let cache_path = dir.path().join(".markharness-cache").join("m1.json");
        let mut cache_file: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&cache_path).unwrap()).unwrap();
        cache_file["key"]["id_index_schema_version"] = serde_json::json!("0");
        cache_file["entries"] = serde_json::json!([
            {"id": "bogus", "path": "bogus", "tree_sha": "bogus"}
        ]);
        fs::write(&cache_path, serde_json::to_string(&cache_file).unwrap()).unwrap();

        let versions = resolve_feature_versions(dir.path(), "m1", true).unwrap();

        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].id, "player-jump");
    }

    #[test]
    fn rebuild_cache_removes_the_cache_directory() {
        let dir = init_repo_with_feature("player-jump", "controls");
        resolve_feature_versions(dir.path(), "m1", true).unwrap();
        assert!(dir.path().join(".markharness-cache").is_dir());

        rebuild_cache(dir.path()).unwrap();

        assert!(!dir.path().join(".markharness-cache").exists());
    }

    #[test]
    fn rebuild_cache_is_a_no_op_when_cache_dir_missing() {
        let dir = tempfile::tempdir().unwrap();

        let result = rebuild_cache(dir.path());

        assert!(result.is_ok());
    }
}
