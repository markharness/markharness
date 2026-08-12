use std::io;
use std::path::Path;
use std::process::Command;

/// Whether a `TreeEntry` is a file (blob) or a directory (tree) object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    Blob,
    Tree,
}

/// One entry from `git ls-tree -r -t`: an object's path (relative to the
/// repo root), its content-addressed SHA, and whether it's a file or a
/// directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub path: String,
    pub sha: String,
    pub kind: ObjectKind,
}

fn run_git(root: &Path, args: &[&str]) -> io::Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Recursively lists blobs *and* directories (tree objects) under
/// `path_in_repo` (e.g. `"knowledge"`) as they existed at `git_ref` (a tag or
/// other revision), via `git ls-tree -r -t`. Simplified id resolution (§3.3
/// の非コミットキャッシュではなく、毎回の直接走査): callers derive an id
/// from `TreeEntry::path` themselves. Including tree entries lets callers
/// look up a directory's own content-addressed SHA (e.g. a Feature's whole
/// subtree) in the same single `git` invocation, rather than one process per
/// directory.
pub fn ls_tree_recursive(
    root: &Path,
    git_ref: &str,
    path_in_repo: &str,
) -> io::Result<Vec<TreeEntry>> {
    let raw = run_git(root, &["ls-tree", "-r", "-t", git_ref, "--", path_in_repo])?;
    let mut entries = Vec::new();
    for line in raw.lines() {
        // format: "<mode> <type> <sha>\t<path>"
        let Some((meta, path)) = line.split_once('\t') else {
            continue;
        };
        let mut fields = meta.split_whitespace();
        let _mode = fields.next();
        let obj_type = fields.next();
        let sha = fields.next();
        let kind = match obj_type {
            Some("blob") => ObjectKind::Blob,
            Some("tree") => ObjectKind::Tree,
            _ => continue,
        };
        if let Some(sha) = sha {
            entries.push(TreeEntry {
                path: path.to_string(),
                sha: sha.to_string(),
                kind,
            });
        }
    }
    Ok(entries)
}

/// The git tree object SHA of `path_in_repo` (e.g. `"knowledge"`) as it
/// existed at `git_ref`, via `git ls-tree` (a pathspec, not a `<rev>:<path>`
/// revision expression — the latter is always repo-root-relative regardless
/// of `-C root`, which breaks when `root` is a subdirectory of the repo
/// rather than its top level; `ls-tree`'s pathspec is `-C`-relative like any
/// other git subcommand argument, so it resolves correctly either way).
/// Returns `None` if the path did not exist at that ref (rather than an
/// error), so callers can fold a missing `knowledge/` into a stable cache
/// key (§3.3 cache_key's `tree_sha(knowledge/ 配下のGitツリーオブジェクトSHA)`
/// component).
pub fn tree_sha(root: &Path, git_ref: &str, path_in_repo: &str) -> io::Result<Option<String>> {
    let raw = run_git(root, &["ls-tree", git_ref, "--", path_in_repo])?;
    let Some(line) = raw.lines().next() else {
        return Ok(None);
    };
    // format: "<mode> <type> <sha>\t<path>"
    let Some((meta, _path)) = line.split_once('\t') else {
        return Ok(None);
    };
    Ok(meta.split_whitespace().nth(2).map(|sha| sha.to_string()))
}

/// Reads a blob's content by its content-addressed SHA (as returned in a
/// `TreeEntry` from `ls_tree_recursive`), via `git cat-file -p`. Used to
/// resolve a Feature's id from its `feature.yml` content (the `id:` field)
/// rather than its directory name (§3.3 path-independent id resolution).
/// Taking a SHA rather than a `<ref, path>` pair sidesteps the same
/// repo-root-relative path pitfall as `tree_sha` above, since a blob SHA
/// needs no path resolution at all.
pub fn show_blob_by_sha(root: &Path, sha: &str) -> io::Result<String> {
    run_git(root, &["cat-file", "-p", sha])
}

/// The parent commit SHAs of `commit`, in order (empty for a root commit,
/// one for a normal commit, two for a merge commit), via `git log --format=%P`.
/// Used by the §3.2 merge-base lineage audit to find a merge commit's P1/P2.
pub fn parents(root: &Path, commit: &str) -> io::Result<Vec<String>> {
    let raw = run_git(root, &["log", "-1", "--format=%P", commit])?;
    Ok(raw.split_whitespace().map(|s| s.to_string()).collect())
}

/// The best common ancestor commit of `a` and `b`, via `git merge-base`.
/// Used by the §3.2 merge-base lineage audit to find the merge base B a
/// merge commit's two parents diverged from.
pub fn merge_base(root: &Path, a: &str, b: &str) -> io::Result<String> {
    let raw = run_git(root, &["merge-base", a, b])?;
    Ok(raw.trim().to_string())
}

/// The committer date (ISO 8601) of the commit `git_ref` points at, used to
/// order milestones by recency (§4.2).
pub fn commit_date(root: &Path, git_ref: &str) -> io::Result<String> {
    let raw = run_git(root, &["log", "-1", "--format=%cI", git_ref])?;
    Ok(raw.trim().to_string())
}

/// Reads a git notes entry under `notes_ref` for `git_ref`. Returns `None`
/// when no note exists yet (rather than treating it as an error).
pub fn notes_show(root: &Path, notes_ref: &str, git_ref: &str) -> io::Result<Option<String>> {
    match run_git(root, &["notes", "--ref", notes_ref, "show", git_ref]) {
        Ok(content) => Ok(Some(content)),
        Err(_) => Ok(None),
    }
}

/// Whether `tag` exists in `root`'s repository. A missing tag is a normal
/// `false` result, not an error (unlike `run_git`, which treats any non-zero
/// git exit status as an `io::Error`).
pub fn tag_exists(root: &Path, tag: &str) -> io::Result<bool> {
    let ref_name = format!("refs/tags/{tag}");
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--verify", "--quiet", &ref_name])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    Ok(status.success())
}

/// Overwrites (or creates) the git notes entry under `notes_ref` for `git_ref`.
pub fn notes_add(root: &Path, notes_ref: &str, git_ref: &str, message: &str) -> io::Result<()> {
    run_git(
        root,
        &[
            "notes", "--ref", notes_ref, "add", "-f", "-m", message, git_ref,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "-q", "-b", "main"]).unwrap();
        run_git(dir.path(), &["config", "user.email", "test@example.com"]).unwrap();
        run_git(dir.path(), &["config", "user.name", "Test"]).unwrap();
        dir
    }

    fn commit_all(dir: &Path, message: &str) {
        run_git(dir, &["add", "-A"]).unwrap();
        run_git(dir, &["commit", "-q", "-m", message]).unwrap();
    }

    #[test]
    fn ls_tree_recursive_lists_blobs_under_path_at_ref() {
        let dir = init_repo();
        fs::create_dir_all(dir.path().join("knowledge/req/feat")).unwrap();
        fs::write(
            dir.path().join("knowledge/req/feat/feature.yml"),
            "id: feat\n",
        )
        .unwrap();
        commit_all(dir.path(), "add feature");
        run_git(dir.path(), &["tag", "m1"]).unwrap();

        let entries = ls_tree_recursive(dir.path(), "m1", "knowledge").unwrap();
        let blobs: Vec<_> = entries
            .iter()
            .filter(|e| e.kind == ObjectKind::Blob)
            .collect();

        assert_eq!(blobs.len(), 1);
        assert_eq!(blobs[0].path, "knowledge/req/feat/feature.yml");
        assert_eq!(blobs[0].sha.len(), 40);
    }

    #[test]
    fn ls_tree_recursive_also_lists_tree_entries_for_directories() {
        let dir = init_repo();
        fs::create_dir_all(dir.path().join("knowledge/req/feat")).unwrap();
        fs::write(
            dir.path().join("knowledge/req/feat/feature.yml"),
            "id: feat\n",
        )
        .unwrap();
        commit_all(dir.path(), "add feature");
        run_git(dir.path(), &["tag", "m1"]).unwrap();

        let entries = ls_tree_recursive(dir.path(), "m1", "knowledge").unwrap();
        let feat_dir = entries
            .iter()
            .find(|e| e.kind == ObjectKind::Tree && e.path == "knowledge/req/feat")
            .expect("expected a tree entry for knowledge/req/feat");

        assert_eq!(feat_dir.sha.len(), 40);
    }

    #[test]
    fn ls_tree_recursive_returns_empty_when_path_absent_at_ref() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]).unwrap();

        let entries = ls_tree_recursive(dir.path(), "m1", "knowledge").unwrap();

        assert!(entries.is_empty());
    }

    #[test]
    fn blob_entry_sha_changes_when_file_content_changes_across_tags() {
        let dir = init_repo();
        fs::create_dir_all(dir.path().join("knowledge/req/feat")).unwrap();
        fs::write(
            dir.path().join("knowledge/req/feat/feature.yml"),
            "id: feat\nlabel: v1\n",
        )
        .unwrap();
        commit_all(dir.path(), "v1");
        run_git(dir.path(), &["tag", "m1"]).unwrap();

        fs::write(
            dir.path().join("knowledge/req/feat/feature.yml"),
            "id: feat\nlabel: v2\n",
        )
        .unwrap();
        commit_all(dir.path(), "v2");
        run_git(dir.path(), &["tag", "m2"]).unwrap();

        let at_m1 = ls_tree_recursive(dir.path(), "m1", "knowledge").unwrap();
        let at_m2 = ls_tree_recursive(dir.path(), "m2", "knowledge").unwrap();
        let blob_at = |entries: &[TreeEntry]| {
            entries
                .iter()
                .find(|e| e.kind == ObjectKind::Blob)
                .unwrap()
                .sha
                .clone()
        };

        assert_ne!(blob_at(&at_m1), blob_at(&at_m2));
    }

    #[test]
    fn tree_sha_returns_the_tree_object_sha_for_an_existing_path_at_ref() {
        let dir = init_repo();
        fs::create_dir_all(dir.path().join("knowledge/req/feat")).unwrap();
        fs::write(
            dir.path().join("knowledge/req/feat/feature.yml"),
            "id: feat\n",
        )
        .unwrap();
        commit_all(dir.path(), "add feature");
        run_git(dir.path(), &["tag", "m1"]).unwrap();

        let sha = tree_sha(dir.path(), "m1", "knowledge").unwrap();

        assert_eq!(sha.map(|s| s.len()), Some(40));
    }

    #[test]
    fn tree_sha_returns_none_when_path_absent_at_ref() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]).unwrap();

        let sha = tree_sha(dir.path(), "m1", "knowledge").unwrap();

        assert_eq!(sha, None);
    }

    /// Regression test for a project directory (`root`) that is a
    /// subdirectory of the git repository rather than its top level (a
    /// supported layout, e.g. `markharness init --dir docs` inside a larger
    /// product repo). Before this fix, `tree_sha` built a `<rev>:<path>`
    /// revision expression, which git always resolves repo-root-relative
    /// regardless of `-C root`, so it looked for `knowledge` at the repo
    /// root and missed `sub/knowledge`. `ls-tree`'s pathspec argument does
    /// not have this problem: it resolves relative to `-C root` like any
    /// other git subcommand argument.
    #[test]
    fn tree_sha_resolves_path_when_root_is_a_subdirectory_of_the_repo() {
        let repo = init_repo();
        fs::create_dir_all(repo.path().join("sub/knowledge/req/feat")).unwrap();
        fs::write(
            repo.path().join("sub/knowledge/req/feat/feature.yml"),
            "id: feat\n",
        )
        .unwrap();
        commit_all(repo.path(), "add feature");
        run_git(repo.path(), &["tag", "t1"]).unwrap();

        let sub_root = repo.path().join("sub");
        let sha = tree_sha(&sub_root, "t1", "knowledge").unwrap();

        assert!(sha.is_some());
    }

    #[test]
    fn show_blob_by_sha_returns_content_of_the_blob() {
        let dir = init_repo();
        fs::create_dir_all(dir.path().join("knowledge/req/feat")).unwrap();
        fs::write(
            dir.path().join("knowledge/req/feat/feature.yml"),
            "id: feat\nlabel: v1\n",
        )
        .unwrap();
        commit_all(dir.path(), "add feature");
        run_git(dir.path(), &["tag", "m1"]).unwrap();
        let entries = ls_tree_recursive(dir.path(), "m1", "knowledge").unwrap();
        let blob = entries.iter().find(|e| e.kind == ObjectKind::Blob).unwrap();

        let content = show_blob_by_sha(dir.path(), &blob.sha).unwrap();

        assert_eq!(content, "id: feat\nlabel: v1\n");
    }

    /// A blob SHA is content-addressed and needs no path resolution, so
    /// `show_blob_by_sha` is unaffected by whether `root` is the repo's top
    /// level or a subdirectory of it — unlike the old path-based lookup this
    /// replaced (see `tree_sha_resolves_path_when_root_is_a_subdirectory_of_the_repo`).
    #[test]
    fn show_blob_by_sha_works_when_root_is_a_subdirectory_of_the_repo() {
        let repo = init_repo();
        fs::create_dir_all(repo.path().join("sub/knowledge/req/feat")).unwrap();
        fs::write(
            repo.path().join("sub/knowledge/req/feat/feature.yml"),
            "id: feat\n",
        )
        .unwrap();
        commit_all(repo.path(), "add feature");
        run_git(repo.path(), &["tag", "t1"]).unwrap();
        let sub_root = repo.path().join("sub");
        let entries = ls_tree_recursive(&sub_root, "t1", "knowledge").unwrap();
        let blob = entries.iter().find(|e| e.kind == ObjectKind::Blob).unwrap();

        let content = show_blob_by_sha(&sub_root, &blob.sha).unwrap();

        assert_eq!(content, "id: feat\n");
    }

    #[test]
    fn parents_returns_empty_for_a_root_commit() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");

        let parents = parents(dir.path(), "HEAD").unwrap();

        assert!(parents.is_empty());
    }

    #[test]
    fn parents_returns_one_sha_for_a_normal_commit() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "first");
        fs::write(dir.path().join("README.md"), "world\n").unwrap();
        commit_all(dir.path(), "second");

        let parents = parents(dir.path(), "HEAD").unwrap();

        assert_eq!(parents.len(), 1);
        assert_eq!(parents[0].len(), 40);
    }

    #[test]
    fn parents_returns_two_shas_for_a_merge_commit() {
        let dir = init_repo();
        fs::write(dir.path().join("base.txt"), "base\n").unwrap();
        commit_all(dir.path(), "base");
        run_git(dir.path(), &["branch", "feature"]).unwrap();

        fs::write(dir.path().join("main.txt"), "main\n").unwrap();
        commit_all(dir.path(), "on main");

        run_git(dir.path(), &["checkout", "-q", "feature"]).unwrap();
        fs::write(dir.path().join("feature.txt"), "feature\n").unwrap();
        commit_all(dir.path(), "on feature");

        run_git(dir.path(), &["checkout", "-q", "main"]).unwrap();
        run_git(
            dir.path(),
            &["merge", "--no-ff", "-q", "-m", "merge feature", "feature"],
        )
        .unwrap();

        let parents = parents(dir.path(), "HEAD").unwrap();

        assert_eq!(parents.len(), 2);
    }

    #[test]
    fn merge_base_finds_the_common_ancestor_of_two_diverged_branches() {
        let dir = init_repo();
        fs::write(dir.path().join("base.txt"), "base\n").unwrap();
        commit_all(dir.path(), "base");
        let base_sha = run_git(dir.path(), &["rev-parse", "HEAD"]).unwrap();
        run_git(dir.path(), &["branch", "feature"]).unwrap();

        fs::write(dir.path().join("main.txt"), "main\n").unwrap();
        commit_all(dir.path(), "on main");

        run_git(dir.path(), &["checkout", "-q", "feature"]).unwrap();
        fs::write(dir.path().join("feature.txt"), "feature\n").unwrap();
        commit_all(dir.path(), "on feature");

        let base = merge_base(dir.path(), "main", "feature").unwrap();

        assert_eq!(base, base_sha.trim());
    }

    #[test]
    fn commit_date_returns_iso8601_committer_date() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]).unwrap();

        let date = commit_date(dir.path(), "m1").unwrap();

        // ISO 8601 with an explicit offset, e.g. 2026-08-08T12:34:56+09:00
        assert!(date.contains('T'), "expected ISO8601 date, got: {date}");
    }

    #[test]
    fn notes_show_returns_none_when_no_note_exists() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");

        let note = notes_show(dir.path(), "markharness-backfill", "HEAD").unwrap();

        assert_eq!(note, None);
    }

    #[test]
    fn notes_add_then_show_roundtrips_the_message() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");

        notes_add(dir.path(), "markharness-backfill", "HEAD", "done").unwrap();
        let note = notes_show(dir.path(), "markharness-backfill", "HEAD").unwrap();

        assert_eq!(note, Some("done\n".to_string()));
    }

    #[test]
    fn tag_exists_returns_true_for_an_existing_tag() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]).unwrap();

        assert!(tag_exists(dir.path(), "m1").unwrap());
    }

    #[test]
    fn tag_exists_returns_false_for_a_missing_tag() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");

        assert!(!tag_exists(dir.path(), "nope").unwrap());
    }
}
