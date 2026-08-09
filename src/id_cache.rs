use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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
        tree_sha: git::tree_sha(root, git_ref, "knowledge")?,
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
/// show`), not the directory name, so that renaming a Feature's directory
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

    let tree_entries = git::ls_tree_recursive(root, git_ref, "knowledge")?;

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
        let content = git::show_blob(root, git_ref, &entry.path)?;
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
            },
        );
    }
    let features: Vec<FeatureVersion> = by_id.into_values().collect();

    if let Some(current_key) = current_key {
        let dir = cache_dir(root);
        fs::create_dir_all(&dir)?;
        let cache_file = CacheFile {
            key: current_key,
            entries: features.clone(),
        };
        let json = serde_json::to_string(&cache_file).map_err(io::Error::other)?;
        fs::write(cache_path(root, git_ref), json)?;
    }

    Ok(features)
}

/// `markharness cache rebuild`: discards `.markharness-cache/` outright,
/// letting the next `changes compute` recompute lazily (§UC7, cache rebuild
/// は全削除のみで即時再計算はしない設計).
pub fn rebuild_cache(root: &Path) -> io::Result<()> {
    let dir = cache_dir(root);
    match fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
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
        let feature_dir = dir
            .path()
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

        let old_dir = dir.path().join("knowledge/controls/player-jump");
        let new_dir = dir.path().join("knowledge/controls/player-jump-renamed");
        fs::rename(&old_dir, &new_dir).unwrap();
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "rename directory"]);
        run_git(dir.path(), &["tag", "m2"]);

        let versions = resolve_feature_versions(dir.path(), "m2", false).unwrap();

        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].id, "player-jump");
        assert_eq!(versions[0].path, "knowledge/controls/player-jump-renamed");
    }

    /// Two Feature directories whose `feature.yml` both declare the same
    /// `id:` must be rejected rather than silently collapsed into one entry
    /// (or worse, non-deterministically overwriting each other).
    #[test]
    fn errors_when_two_feature_directories_declare_the_same_id() {
        let dir = init_repo_with_feature("player-jump", "controls");

        let dup_dir = dir.path().join("knowledge/controls/player-jump-duplicate");
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
        assert_eq!(versions[0].path, "knowledge/controls/player-jump");
        assert_eq!(versions[0].tree_sha.len(), 40);
    }

    #[test]
    fn without_cache_recomputes_and_reflects_new_commits() {
        let dir = init_repo_with_feature("player-jump", "controls");
        let first = resolve_feature_versions(dir.path(), "m1", false).unwrap();

        fs::write(
            dir.path()
                .join("knowledge/controls/player-jump/feature.yml"),
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

        let behavior_dir = dir.path().join("knowledge/controls/player-jump/jump");
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
                .join("knowledge/controls/player-jump/feature.yml"),
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
