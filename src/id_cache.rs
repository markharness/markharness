use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::git::{self, ObjectKind};

/// A Feature's id, its directory path, and the git tree SHA of its whole
/// subtree (feature.yml + behavior/condition/expected files below it) at
/// some git ref. Simplified id resolution (§3.3 の非コミットキャッシュの本格
/// 実装ではなく、現行の「id = ディレクトリ名」というパス安定な前提に基づく
/// 簡易版)。Using the directory's tree SHA rather than feature.yml's own
/// blob SHA means Condition/Behavior/ExpectedResult changes are captured
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

/// Extracts a Feature id from a `knowledge/<requirement>/<feature>/feature.yml`
/// blob's path (the feature directory's name), matching how `feature_id`
/// doubles as a stable directory name elsewhere in this codebase. Returns
/// the id together with the feature directory's own path (the blob's parent),
/// which is looked up as a tree entry in the same listing.
fn feature_dir_from_feature_yml_path(path: &str) -> Option<(String, &str)> {
    let dir_path = path.strip_suffix("/feature.yml")?;
    let id = dir_path.rsplit('/').next()?;
    Some((id.to_string(), dir_path))
}

/// Resolves every Feature's directory tree SHA at `git_ref` by walking
/// `knowledge/` with a single `git ls-tree -r -t` (§3.3 の簡易版; 毎回直接
/// 走査するか、`use_cache` が true なら `.markharness-cache/` を読み書きす
/// る)。Discovers Feature directories via their `feature.yml` blob, then
/// looks up that directory's own tree entry in the same listing — no
/// per-Feature git subprocess needed.
pub fn resolve_feature_versions(
    root: &Path,
    git_ref: &str,
    use_cache: bool,
) -> io::Result<Vec<FeatureVersion>> {
    if use_cache
        && let Ok(cached) = fs::read_to_string(cache_path(root, git_ref))
        && let Ok(entries) = serde_json::from_str::<Vec<FeatureVersion>>(&cached)
    {
        return Ok(entries);
    }

    let tree_entries = git::ls_tree_recursive(root, git_ref, "knowledge")?;

    let mut features: Vec<FeatureVersion> = Vec::new();
    for entry in &tree_entries {
        if entry.kind != ObjectKind::Blob {
            continue;
        }
        let Some((id, dir_path)) = feature_dir_from_feature_yml_path(&entry.path) else {
            continue;
        };
        let Some(dir_entry) = tree_entries
            .iter()
            .find(|e| e.kind == ObjectKind::Tree && e.path == dir_path)
        else {
            continue;
        };
        features.push(FeatureVersion {
            id,
            path: dir_path.to_string(),
            tree_sha: dir_entry.sha.clone(),
        });
    }
    features.sort_by(|a, b| a.id.cmp(&b.id));

    if use_cache {
        let dir = cache_dir(root);
        fs::create_dir_all(&dir)?;
        let json = serde_json::to_string(&features).map_err(io::Error::other)?;
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
