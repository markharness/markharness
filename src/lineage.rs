use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

use crate::git;
use crate::id_cache::{self, FeatureVersion};

/// How a Feature's tree SHA behaved across a merge commit's two parents,
/// relative to their merge-base (§3.2). The audit-only secondary lineage
/// tool: independent of `changes compute`'s milestone-boundary comparison,
/// and does not write to `changes/*.yaml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineageKind {
    /// Only one parent diverged from the merge-base; a normal linear change.
    Linear,
    /// Both parents diverged from the merge-base *and* from each other: a
    /// true branch divergence. `derived_from` would need to record both
    /// parent tree SHAs (§3.2, not yet persisted anywhere — audit-only).
    TrueDivergence,
    /// Both parents carry the same tree SHA (including the case where
    /// neither diverged from the merge-base at all); treated as one parent.
    SingleParent,
}

/// One Feature's lineage classification at a specific merge commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureLineage {
    pub feature_id: String,
    pub base_tree_sha: Option<String>,
    pub parent1_tree_sha: Option<String>,
    pub parent2_tree_sha: Option<String>,
    pub kind: LineageKind,
}

pub(crate) fn classify(
    base: Option<&String>,
    p1: Option<&String>,
    p2: Option<&String>,
) -> LineageKind {
    if p1 == p2 {
        LineageKind::SingleParent
    } else if p1 == base || p2 == base {
        LineageKind::Linear
    } else {
        LineageKind::TrueDivergence
    }
}

fn tree_sha_map(versions: Vec<FeatureVersion>) -> BTreeMap<String, String> {
    versions.into_iter().map(|v| (v.id, v.tree_sha)).collect()
}

/// §3.2's ancestor-search lineage reconstruction for a single merge commit:
/// finds its two parents (P1/P2) and their merge-base (B) via `git
/// merge-base`, then classifies every Feature id seen at any of the three
/// points. Errors if `merge_commit` does not have exactly two parents (i.e.
/// is not a merge commit).
pub fn compute_lineage(root: &Path, merge_commit: &str) -> io::Result<Vec<FeatureLineage>> {
    let parents = git::parents(root, merge_commit)?;
    if parents.len() != 2 {
        return Err(io::Error::other(format!(
            "'{merge_commit}' is not a merge commit (expected 2 parents, found {})",
            parents.len()
        )));
    }
    let (p1, p2) = (&parents[0], &parents[1]);
    let base = git::merge_base(root, p1, p2)?;

    let base_versions = tree_sha_map(id_cache::resolve_feature_versions(root, &base, false)?);
    let p1_versions = tree_sha_map(id_cache::resolve_feature_versions(root, p1, false)?);
    let p2_versions = tree_sha_map(id_cache::resolve_feature_versions(root, p2, false)?);

    let all_ids: BTreeSet<&String> = base_versions
        .keys()
        .chain(p1_versions.keys())
        .chain(p2_versions.keys())
        .collect();

    let mut result = Vec::new();
    for feature_id in all_ids {
        let base_sha = base_versions.get(feature_id);
        let p1_sha = p1_versions.get(feature_id);
        let p2_sha = p2_versions.get(feature_id);
        result.push(FeatureLineage {
            feature_id: feature_id.clone(),
            base_tree_sha: base_sha.cloned(),
            parent1_tree_sha: p1_sha.cloned(),
            parent2_tree_sha: p2_sha.cloned(),
            kind: classify(base_sha, p1_sha, p2_sha),
        });
    }

    Ok(result)
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
        run_git(dir.path(), &["init", "-q", "-b", "main"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        dir
    }

    fn write_feature(root: &Path, label: &str) {
        let dir = root.join("knowledge/controls/player-jump");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            root.join("knowledge/controls/requirement.yml"),
            "id: controls\nlabel: controls\naxis: []\n",
        )
        .unwrap();
        fs::write(
            dir.join("feature.yml"),
            format!("id: player-jump\nrequirement: controls\nlabel: {label}\naxis: []\n"),
        )
        .unwrap();
    }

    fn commit_all(root: &Path, message: &str) {
        run_git(root, &["add", "-A"]);
        run_git(root, &["commit", "-q", "-m", message]);
    }

    #[test]
    fn errors_when_commit_is_not_a_merge_commit() {
        let dir = init_repo();
        write_feature(dir.path(), "v1");
        commit_all(dir.path(), "v1");

        let result = compute_lineage(dir.path(), "HEAD");

        assert!(result.is_err());
    }

    #[test]
    fn classifies_as_linear_when_only_one_branch_changed_the_feature() {
        let dir = init_repo();
        write_feature(dir.path(), "base");
        commit_all(dir.path(), "base");
        run_git(dir.path(), &["branch", "feature"]);

        // main changes the Feature; `feature` branch does not.
        write_feature(dir.path(), "changed-on-main");
        commit_all(dir.path(), "on main");

        run_git(dir.path(), &["checkout", "-q", "feature"]);
        fs::write(dir.path().join("unrelated.txt"), "x\n").unwrap();
        commit_all(dir.path(), "unrelated change on feature");

        run_git(dir.path(), &["checkout", "-q", "main"]);
        run_git(
            dir.path(),
            &["merge", "--no-ff", "-q", "-m", "merge", "feature"],
        );

        let lineage = compute_lineage(dir.path(), "HEAD").unwrap();
        let entry = lineage
            .iter()
            .find(|l| l.feature_id == "player-jump")
            .unwrap();

        assert_eq!(entry.kind, LineageKind::Linear);
    }

    #[test]
    fn classifies_as_true_divergence_when_both_branches_changed_the_feature_differently() {
        let dir = init_repo();
        write_feature(dir.path(), "base");
        commit_all(dir.path(), "base");
        run_git(dir.path(), &["branch", "feature"]);

        write_feature(dir.path(), "changed-on-main");
        commit_all(dir.path(), "on main");

        run_git(dir.path(), &["checkout", "-q", "feature"]);
        write_feature(dir.path(), "changed-on-feature");
        commit_all(dir.path(), "on feature");

        run_git(dir.path(), &["checkout", "-q", "main"]);
        run_git(
            dir.path(),
            &[
                "merge", "-q", "-m", "merge", "-X", "ours", "--no-ff", "feature",
            ],
        );

        let lineage = compute_lineage(dir.path(), "HEAD").unwrap();
        let entry = lineage
            .iter()
            .find(|l| l.feature_id == "player-jump")
            .unwrap();

        assert_eq!(entry.kind, LineageKind::TrueDivergence);
    }

    #[test]
    fn classifies_as_single_parent_when_neither_branch_changed_the_feature() {
        let dir = init_repo();
        write_feature(dir.path(), "base");
        commit_all(dir.path(), "base");
        run_git(dir.path(), &["branch", "feature"]);

        fs::write(dir.path().join("main-only.txt"), "x\n").unwrap();
        commit_all(dir.path(), "unrelated on main");

        run_git(dir.path(), &["checkout", "-q", "feature"]);
        fs::write(dir.path().join("feature-only.txt"), "x\n").unwrap();
        commit_all(dir.path(), "unrelated on feature");

        run_git(dir.path(), &["checkout", "-q", "main"]);
        run_git(
            dir.path(),
            &["merge", "--no-ff", "-q", "-m", "merge", "feature"],
        );

        let lineage = compute_lineage(dir.path(), "HEAD").unwrap();
        let entry = lineage
            .iter()
            .find(|l| l.feature_id == "player-jump")
            .unwrap();

        assert_eq!(entry.kind, LineageKind::SingleParent);
    }
}
