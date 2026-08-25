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

/// Rejects a revision-like argument that would be interpreted as a Git
/// option rather than a revision (CWE-88 argument injection, e.g.
/// `--output=<path>` causing `git log` to write to an attacker-chosen path).
/// Callers must run this on every externally sourced revision/ref before
/// splicing it into a git argv.
fn reject_option_like(value: &str) -> io::Result<()> {
    if value.starts_with('-') {
        return Err(io::Error::other(format!(
            "refusing to pass option-like value '{value}' as a git revision"
        )));
    }
    Ok(())
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
    reject_option_like(git_ref)?;
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
    reject_option_like(git_ref)?;
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

/// The git blob SHA `path_in_repo` would be given if committed right now
/// from the working tree, via `git hash-object` — the same content filters
/// (e.g. line-ending normalization) `git add` would apply, applied without
/// writing anything to the object database (no `-w`). This is git's own
/// real object identity for a file's current content, computed the same
/// way whether `root` is the top of the repository or a linked worktree
/// (`add_detached_worktree`) checked out from it — used by
/// `identity::migration_manifest` to capture a legacy (pre-migration)
/// snapshot's file identity without needing to write it to the object
/// database or touch the index/staging area.
pub fn hash_object(root: &Path, path_in_repo: &str) -> io::Result<String> {
    let raw = run_git(root, &["hash-object", "--", path_in_repo])?;
    Ok(raw.trim().to_string())
}

/// The git tree SHA `path_in_repo` (e.g. `.markharness/knowledge`,
/// repo-relative) would have if the *working tree* were committed right
/// now — the real tree object `git commit` would create for that exact
/// subtree, including any uncommitted edits. Computed via a disposable
/// temporary index (`GIT_INDEX_FILE`) populated from the working tree with
/// `git add -A`, then `git write-tree --prefix=path_in_repo` against that
/// temporary index, so the repository's real staging area is never
/// touched. Unlike [`tree_sha`], which reads a *committed* ref, this
/// reflects the working tree as it stands right now — used by
/// `identity::migration_manifest::capture_case_signatures` to capture
/// `.markharness/knowledge`'s legacy (pre-migration) snapshot identity
/// before `feature_ops::migrate_all` writes anything, which is not
/// guaranteed to be committed yet. `write-tree` may write ordinary,
/// content-addressed loose objects to the object database as a side
/// effect — the same objects git would create if this content were
/// actually committed — but never mutates the repository's real index or
/// `HEAD`. Requires `root` to already be a git repository.
pub fn write_tree_prefix(root: &Path, path_in_repo: &str) -> io::Result<String> {
    // The path must not exist yet: git treats a zero-byte file as a
    // corrupt index ("index file smaller than expected"), but happily
    // creates a fresh one at a path that doesn't exist at all.
    let temp_dir = tempfile::tempdir()?;
    let index_path = temp_dir.path().join("index");
    let add_status = Command::new("git")
        .arg("-C")
        .arg(root)
        .env("GIT_INDEX_FILE", &index_path)
        .args(["add", "-A", "--", path_in_repo])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if !add_status.success() {
        return Err(io::Error::other(format!(
            "git add -A -- {path_in_repo} failed while building a temporary index"
        )));
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .env("GIT_INDEX_FILE", index_path)
        .args(["write-tree", &format!("--prefix={path_in_repo}")])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "git write-tree --prefix={path_in_repo} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// The parent commit SHAs of `commit`, in order (empty for a root commit,
/// one for a normal commit, two for a merge commit), via `git log --format=%P`.
/// Used by the §3.2 merge-base lineage audit to find a merge commit's P1/P2.
pub fn parents(root: &Path, commit: &str) -> io::Result<Vec<String>> {
    reject_option_like(commit)?;
    let raw = run_git(root, &["log", "-1", "--format=%P", commit])?;
    Ok(raw.split_whitespace().map(|s| s.to_string()).collect())
}

/// The best common ancestor commit of `a` and `b`, via `git merge-base`.
/// Used by the §3.2 merge-base lineage audit to find the merge base B a
/// merge commit's two parents diverged from.
pub fn merge_base(root: &Path, a: &str, b: &str) -> io::Result<String> {
    reject_option_like(a)?;
    reject_option_like(b)?;
    let raw = run_git(root, &["merge-base", a, b])?;
    Ok(raw.trim().to_string())
}

/// All two-parent merge commits in `from..to`, oldest first.
pub fn merge_commits_between(root: &Path, from: &str, to: &str) -> io::Result<Vec<String>> {
    reject_option_like(from)?;
    reject_option_like(to)?;
    let range = format!("{from}..{to}");
    let raw = run_git(
        root,
        &[
            "rev-list",
            "--parents",
            "--ancestry-path",
            "--reverse",
            &range,
        ],
    )?;
    Ok(raw
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let commit = parts.next()?;
            (parts.count() == 2).then(|| commit.to_string())
        })
        .collect())
}

/// Materializes `git_ref` as a detached worktree at `destination`.
pub fn add_detached_worktree(root: &Path, destination: &Path, git_ref: &str) -> io::Result<()> {
    reject_option_like(git_ref)?;
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["worktree", "add", "--detach", "-q"])
        .arg(destination)
        .arg(git_ref)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "git worktree add failed for ref {git_ref}"
        )))
    }
}

/// Best-effort removal used when cleaning up temporary worktrees.
pub fn remove_worktree(root: &Path, worktree: &Path) -> io::Result<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["worktree", "remove", "--force"])
        .arg(worktree)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("git worktree remove failed"))
    }
}

/// The committer date (ISO 8601) of the commit `git_ref` points at, used to
/// order milestones by recency (§4.2).
pub fn commit_date(root: &Path, git_ref: &str) -> io::Result<String> {
    reject_option_like(git_ref)?;
    let raw = run_git(root, &["log", "-1", "--format=%cI", git_ref])?;
    Ok(raw.trim().to_string())
}

/// Whether `ancestor` is an ancestor of (or the same commit as) `descendant`
/// in `root`'s history, via `git merge-base --is-ancestor`. Used to break
/// committer-date ties when ordering milestones by recency (§4.2): two
/// milestones tagged within the same wall-clock second must still be
/// ordered by actual history rather than by string-comparing equal dates.
pub fn is_ancestor(root: &Path, ancestor: &str, descendant: &str) -> io::Result<bool> {
    reject_option_like(ancestor)?;
    reject_option_like(descendant)?;
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    Ok(status.success())
}

/// Reads a git notes entry under `notes_ref` for `git_ref`. Returns `None`
/// when no note exists yet (rather than treating it as an error).
pub fn notes_show(root: &Path, notes_ref: &str, git_ref: &str) -> io::Result<Option<String>> {
    reject_option_like(git_ref)?;
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
    reject_option_like(git_ref)?;
    run_git(
        root,
        &[
            "notes", "--ref", notes_ref, "add", "-f", "-m", message, git_ref,
        ],
    )?;
    Ok(())
}

/// One line of `git diff --name-status`'s output: whether a path was
/// added, deleted, or had its content modified between the two commits
/// compared. Rename detection is explicitly disabled (`--no-renames`) — a
/// path that disappears at one commit and reappears at another under a
/// different name (or with sufficiently similar content) must be reported
/// as a plain delete, not folded away as a single `R100` rename line this
/// parser doesn't understand, so `identity::audit`'s append-only check
/// sees it. Simply omitting `-M`/`--find-renames` is not enough: a
/// repository (or the user's global config) with `diff.renames` enabled
/// turns rename detection on by default even without either flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffStatus {
    Added,
    Deleted,
    Modified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffEntry {
    pub status: DiffStatus,
    pub path: String,
}

/// Every added/deleted/modified path under `path_in_repo` between `from`
/// and `to`, via `git diff --name-status`. Used by `identity::audit` to
/// find, commit pair by commit pair, exactly which identity event files
/// changed — far cheaper than re-listing the whole tree at every commit.
pub fn diff_name_status(
    root: &Path,
    from: &str,
    to: &str,
    path_in_repo: &str,
) -> io::Result<Vec<DiffEntry>> {
    reject_option_like(from)?;
    reject_option_like(to)?;
    let raw = run_git(
        root,
        &[
            "diff",
            "--no-renames",
            "--name-status",
            from,
            to,
            "--",
            path_in_repo,
        ],
    )?;
    let mut entries = Vec::new();
    for line in raw.lines() {
        let mut parts = line.splitn(2, '\t');
        let status = match parts.next() {
            Some("A") => DiffStatus::Added,
            Some("D") => DiffStatus::Deleted,
            Some("M") => DiffStatus::Modified,
            _ => continue,
        };
        if let Some(path) = parts.next() {
            entries.push(DiffEntry {
                status,
                path: path.to_string(),
            });
        }
    }
    Ok(entries)
}

/// Every commit from the repository root up to `git_ref`, following only
/// first parents (i.e. skipping commits that only ever existed on a
/// branch that was merged in), oldest first. Used by `identity::audit` to
/// walk exactly the linear history that ended up on `git_ref`, matching
/// what a `git log` on that branch would show.
pub fn first_parent_history(root: &Path, git_ref: &str) -> io::Result<Vec<String>> {
    reject_option_like(git_ref)?;
    let raw = run_git(root, &["log", "--first-parent", "--format=%H", git_ref])?;
    let mut commits: Vec<String> = raw.lines().map(|s| s.to_string()).collect();
    commits.reverse();
    Ok(commits)
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
        run_git(dir.path(), &["config", "core.autocrlf", "false"]).unwrap();
        dir
    }

    fn commit_all(dir: &Path, message: &str) {
        run_git(dir, &["add", "-A"]).unwrap();
        run_git(dir, &["commit", "-q", "-m", message]).unwrap();
    }

    #[test]
    fn parents_rejects_option_like_revision_instead_of_writing_output_file() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        let victim = dir.path().join("victim.txt");
        let malicious = format!("--output={}", victim.display());

        let result = parents(dir.path(), &malicious);

        assert!(result.is_err());
        assert!(
            !victim.exists(),
            "git must not have been invoked with the option-like revision"
        );
    }

    #[test]
    fn commit_date_rejects_option_like_revision_instead_of_writing_output_file() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        let victim = dir.path().join("victim.txt");
        let malicious = format!("--output={}", victim.display());

        let result = commit_date(dir.path(), &malicious);

        assert!(result.is_err());
        assert!(!victim.exists());
    }

    #[test]
    fn merge_base_rejects_option_like_revision() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        let victim = dir.path().join("victim.txt");
        let malicious = format!("--output={}", victim.display());

        let result = merge_base(dir.path(), &malicious, "HEAD");
        assert!(result.is_err());

        let result = merge_base(dir.path(), "HEAD", &malicious);
        assert!(result.is_err());
        assert!(!victim.exists());
    }

    #[test]
    fn tree_sha_rejects_option_like_revision() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        let victim = dir.path().join("victim.txt");
        let malicious = format!("--output={}", victim.display());

        let result = tree_sha(dir.path(), &malicious, "README.md");

        assert!(result.is_err());
        assert!(!victim.exists());
    }

    #[test]
    fn ls_tree_recursive_rejects_option_like_revision() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        let victim = dir.path().join("victim.txt");
        let malicious = format!("--output={}", victim.display());

        let result = ls_tree_recursive(dir.path(), &malicious, "README.md");

        assert!(result.is_err());
        assert!(!victim.exists());
    }

    #[test]
    fn notes_show_rejects_option_like_revision() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        let victim = dir.path().join("victim.txt");
        let malicious = format!("--output={}", victim.display());

        let result = notes_show(dir.path(), "refs/notes/commits", &malicious);

        assert!(result.is_err());
        assert!(!victim.exists());
    }

    #[test]
    fn parents_still_works_for_normal_tag_short_sha_and_head() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]).unwrap();

        assert!(parents(dir.path(), "m1").is_ok());
        assert!(parents(dir.path(), "HEAD").is_ok());
    }

    #[test]
    fn ls_tree_recursive_lists_blobs_under_path_at_ref() {
        let dir = init_repo();
        fs::create_dir_all(dir.path().join(".markharness/knowledge/req/feat")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/req/feat/feature.yml"),
            "id: feat\n",
        )
        .unwrap();
        commit_all(dir.path(), "add feature");
        run_git(dir.path(), &["tag", "m1"]).unwrap();

        let entries = ls_tree_recursive(
            dir.path(),
            "m1",
            crate::project_root::KNOWLEDGE_PATH_IN_REPO,
        )
        .unwrap();
        let blobs: Vec<_> = entries
            .iter()
            .filter(|e| e.kind == ObjectKind::Blob)
            .collect();

        assert_eq!(blobs.len(), 1);
        assert_eq!(blobs[0].path, ".markharness/knowledge/req/feat/feature.yml");
        assert_eq!(blobs[0].sha.len(), 40);
    }

    #[test]
    fn ls_tree_recursive_also_lists_tree_entries_for_directories() {
        let dir = init_repo();
        fs::create_dir_all(dir.path().join(".markharness/knowledge/req/feat")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/req/feat/feature.yml"),
            "id: feat\n",
        )
        .unwrap();
        commit_all(dir.path(), "add feature");
        run_git(dir.path(), &["tag", "m1"]).unwrap();

        let entries = ls_tree_recursive(
            dir.path(),
            "m1",
            crate::project_root::KNOWLEDGE_PATH_IN_REPO,
        )
        .unwrap();
        let feat_dir = entries
            .iter()
            .find(|e| e.kind == ObjectKind::Tree && e.path == ".markharness/knowledge/req/feat")
            .expect("expected a tree entry for .markharness/knowledge/req/feat");

        assert_eq!(feat_dir.sha.len(), 40);
    }

    #[test]
    fn ls_tree_recursive_returns_empty_when_path_absent_at_ref() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]).unwrap();

        let entries = ls_tree_recursive(
            dir.path(),
            "m1",
            crate::project_root::KNOWLEDGE_PATH_IN_REPO,
        )
        .unwrap();

        assert!(entries.is_empty());
    }

    #[test]
    fn blob_entry_sha_changes_when_file_content_changes_across_tags() {
        let dir = init_repo();
        fs::create_dir_all(dir.path().join(".markharness/knowledge/req/feat")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/req/feat/feature.yml"),
            "id: feat\nlabel: v1\n",
        )
        .unwrap();
        commit_all(dir.path(), "v1");
        run_git(dir.path(), &["tag", "m1"]).unwrap();

        fs::write(
            dir.path()
                .join(".markharness/knowledge/req/feat/feature.yml"),
            "id: feat\nlabel: v2\n",
        )
        .unwrap();
        commit_all(dir.path(), "v2");
        run_git(dir.path(), &["tag", "m2"]).unwrap();

        let at_m1 = ls_tree_recursive(
            dir.path(),
            "m1",
            crate::project_root::KNOWLEDGE_PATH_IN_REPO,
        )
        .unwrap();
        let at_m2 = ls_tree_recursive(
            dir.path(),
            "m2",
            crate::project_root::KNOWLEDGE_PATH_IN_REPO,
        )
        .unwrap();
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
        fs::create_dir_all(dir.path().join(".markharness/knowledge/req/feat")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/req/feat/feature.yml"),
            "id: feat\n",
        )
        .unwrap();
        commit_all(dir.path(), "add feature");
        run_git(dir.path(), &["tag", "m1"]).unwrap();

        let sha = tree_sha(
            dir.path(),
            "m1",
            crate::project_root::KNOWLEDGE_PATH_IN_REPO,
        )
        .unwrap();

        assert_eq!(sha.map(|s| s.len()), Some(40));
    }

    #[test]
    fn tree_sha_returns_none_when_path_absent_at_ref() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]).unwrap();

        let sha = tree_sha(
            dir.path(),
            "m1",
            crate::project_root::KNOWLEDGE_PATH_IN_REPO,
        )
        .unwrap();

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
        fs::create_dir_all(repo.path().join("sub/.markharness/knowledge/req/feat")).unwrap();
        fs::write(
            repo.path()
                .join("sub/.markharness/knowledge/req/feat/feature.yml"),
            "id: feat\n",
        )
        .unwrap();
        commit_all(repo.path(), "add feature");
        run_git(repo.path(), &["tag", "t1"]).unwrap();

        let sub_root = repo.path().join("sub");
        let sha = tree_sha(&sub_root, "t1", crate::project_root::KNOWLEDGE_PATH_IN_REPO).unwrap();

        assert!(sha.is_some());
    }

    #[test]
    fn show_blob_by_sha_returns_content_of_the_blob() {
        let dir = init_repo();
        fs::create_dir_all(dir.path().join(".markharness/knowledge/req/feat")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/req/feat/feature.yml"),
            "id: feat\nlabel: v1\n",
        )
        .unwrap();
        commit_all(dir.path(), "add feature");
        run_git(dir.path(), &["tag", "m1"]).unwrap();
        let entries = ls_tree_recursive(
            dir.path(),
            "m1",
            crate::project_root::KNOWLEDGE_PATH_IN_REPO,
        )
        .unwrap();
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
        fs::create_dir_all(repo.path().join("sub/.markharness/knowledge/req/feat")).unwrap();
        fs::write(
            repo.path()
                .join("sub/.markharness/knowledge/req/feat/feature.yml"),
            "id: feat\n",
        )
        .unwrap();
        commit_all(repo.path(), "add feature");
        run_git(repo.path(), &["tag", "t1"]).unwrap();
        let sub_root = repo.path().join("sub");
        let entries =
            ls_tree_recursive(&sub_root, "t1", crate::project_root::KNOWLEDGE_PATH_IN_REPO)
                .unwrap();
        let blob = entries.iter().find(|e| e.kind == ObjectKind::Blob).unwrap();

        let content = show_blob_by_sha(&sub_root, &blob.sha).unwrap();

        assert_eq!(content, "id: feat\n");
    }

    #[test]
    fn hash_object_matches_the_blob_sha_git_itself_records_once_committed() {
        let dir = init_repo();
        fs::create_dir_all(dir.path().join(".markharness/knowledge/req/feat")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/req/feat/feature.yml"),
            "id: feat\nlabel: v1\n",
        )
        .unwrap();

        let before_commit =
            hash_object(dir.path(), ".markharness/knowledge/req/feat/feature.yml").unwrap();

        commit_all(dir.path(), "add feature");
        run_git(dir.path(), &["tag", "m1"]).unwrap();
        let entries = ls_tree_recursive(
            dir.path(),
            "m1",
            crate::project_root::KNOWLEDGE_PATH_IN_REPO,
        )
        .unwrap();
        let committed_sha = entries
            .iter()
            .find(|e| e.kind == ObjectKind::Blob)
            .unwrap()
            .sha
            .clone();

        assert_eq!(before_commit, committed_sha);
        assert_eq!(before_commit.len(), 40);
    }

    #[test]
    fn hash_object_does_not_write_to_the_object_database() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        fs::write(dir.path().join("untracked.txt"), "not committed\n").unwrap();

        let sha = hash_object(dir.path(), "untracked.txt").unwrap();

        // `git cat-file -e` fails if the object was never written (no `-w`
        // was passed to `hash-object`), proving this was a pure computation.
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["cat-file", "-e", &sha])
            .status()
            .unwrap();
        assert!(!status.success());
    }

    #[test]
    fn write_tree_prefix_reflects_uncommitted_working_tree_content() {
        let dir = init_repo();
        fs::create_dir_all(dir.path().join(".markharness/knowledge/req/feat")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/req/feat/feature.yml"),
            "id: feat\nlabel: v1\n",
        )
        .unwrap();
        commit_all(dir.path(), "v1");

        let before_edit =
            write_tree_prefix(dir.path(), crate::project_root::KNOWLEDGE_PATH_IN_REPO).unwrap();

        // Uncommitted edit: the tree SHA must change without a commit.
        fs::write(
            dir.path()
                .join(".markharness/knowledge/req/feat/feature.yml"),
            "id: feat\nlabel: v2\n",
        )
        .unwrap();
        let after_edit =
            write_tree_prefix(dir.path(), crate::project_root::KNOWLEDGE_PATH_IN_REPO).unwrap();

        assert_eq!(before_edit.len(), 40);
        assert_ne!(before_edit, after_edit);
    }

    #[test]
    fn write_tree_prefix_matches_the_committed_tree_sha_once_actually_committed() {
        let dir = init_repo();
        fs::create_dir_all(dir.path().join(".markharness/knowledge/req/feat")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/req/feat/feature.yml"),
            "id: feat\n",
        )
        .unwrap();

        let live =
            write_tree_prefix(dir.path(), crate::project_root::KNOWLEDGE_PATH_IN_REPO).unwrap();
        commit_all(dir.path(), "add feature");
        run_git(dir.path(), &["tag", "m1"]).unwrap();
        let committed = tree_sha(
            dir.path(),
            "m1",
            crate::project_root::KNOWLEDGE_PATH_IN_REPO,
        )
        .unwrap()
        .unwrap();

        assert_eq!(live, committed);
    }

    #[test]
    fn write_tree_prefix_does_not_touch_the_real_staging_area() {
        let dir = init_repo();
        fs::create_dir_all(dir.path().join(".markharness/knowledge")).unwrap();
        fs::write(
            dir.path().join(".markharness/knowledge/untracked.yml"),
            "id: x\n",
        )
        .unwrap();

        write_tree_prefix(dir.path(), crate::project_root::KNOWLEDGE_PATH_IN_REPO).unwrap();

        let status = run_git(dir.path(), &["status", "--porcelain"]).unwrap();
        assert!(
            status.contains("??"),
            "the file must still show as untracked in the real index, got: {status}"
        );
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

    #[test]
    fn is_ancestor_true_for_an_earlier_commit_on_the_same_branch() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "first");
        run_git(dir.path(), &["tag", "m1"]).unwrap();
        fs::write(dir.path().join("README.md"), "world\n").unwrap();
        commit_all(dir.path(), "second");
        run_git(dir.path(), &["tag", "m2"]).unwrap();

        assert!(is_ancestor(dir.path(), "m1", "m2").unwrap());
        assert!(!is_ancestor(dir.path(), "m2", "m1").unwrap());
    }

    #[test]
    fn is_ancestor_rejects_option_like_revision() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        let victim = dir.path().join("victim.txt");
        let malicious = format!("--output={}", victim.display());

        let result = is_ancestor(dir.path(), &malicious, "HEAD");
        assert!(result.is_err());

        let result = is_ancestor(dir.path(), "HEAD", &malicious);
        assert!(result.is_err());
        assert!(!victim.exists());
    }

    #[test]
    fn diff_name_status_reports_added_deleted_and_modified_paths() {
        let dir = init_repo();
        fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        fs::write(dir.path().join("b.txt"), "keep\n").unwrap();
        commit_all(dir.path(), "first");
        let from = run_git(dir.path(), &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string();

        fs::write(dir.path().join("a.txt"), "two\n").unwrap();
        fs::remove_file(dir.path().join("b.txt")).unwrap();
        fs::write(dir.path().join("c.txt"), "new\n").unwrap();
        commit_all(dir.path(), "second");
        let to = run_git(dir.path(), &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string();

        let mut entries = diff_name_status(dir.path(), &from, &to, ".").unwrap();
        entries.sort_by(|a, b| a.path.cmp(&b.path));

        assert_eq!(
            entries,
            vec![
                DiffEntry {
                    status: DiffStatus::Modified,
                    path: "a.txt".to_string()
                },
                DiffEntry {
                    status: DiffStatus::Deleted,
                    path: "b.txt".to_string()
                },
                DiffEntry {
                    status: DiffStatus::Added,
                    path: "c.txt".to_string()
                },
            ]
        );
    }

    /// If the repository (or the user's global config) has
    /// `diff.renames` enabled, plain `git diff --name-status` collapses a
    /// delete+add pair with similar content into a single `R100` line
    /// instead of separate `D`/`A` lines — invisible to `identity::audit`,
    /// which only understands `A`/`D`/`M`. `diff_name_status` must force
    /// rename detection off regardless of that config, so a genuine
    /// deletion is never silently hidden behind a detected "rename".
    #[test]
    fn diff_name_status_never_folds_a_delete_and_add_into_a_rename_even_with_diff_renames_enabled()
    {
        let dir = init_repo();
        run_git(dir.path(), &["config", "diff.renames", "true"]).unwrap();
        let content = "identical content that is long enough for git's \
            similarity heuristic to treat this as a rename by default\n"
            .repeat(5);
        fs::write(dir.path().join("old_name.txt"), &content).unwrap();
        commit_all(dir.path(), "first");
        let from = run_git(dir.path(), &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string();

        fs::remove_file(dir.path().join("old_name.txt")).unwrap();
        fs::write(dir.path().join("new_name.txt"), &content).unwrap();
        commit_all(dir.path(), "second");
        let to = run_git(dir.path(), &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string();

        let mut entries = diff_name_status(dir.path(), &from, &to, ".").unwrap();
        entries.sort_by(|a, b| a.path.cmp(&b.path));

        assert_eq!(
            entries,
            vec![
                DiffEntry {
                    status: DiffStatus::Added,
                    path: "new_name.txt".to_string()
                },
                DiffEntry {
                    status: DiffStatus::Deleted,
                    path: "old_name.txt".to_string()
                },
            ]
        );
    }

    #[test]
    fn diff_name_status_rejects_option_like_revisions() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        let victim = dir.path().join("victim.txt");
        let malicious = format!("--output={}", victim.display());

        let result = diff_name_status(dir.path(), &malicious, "HEAD", ".");
        assert!(result.is_err());
        assert!(!victim.exists());
    }

    #[test]
    fn first_parent_history_returns_commits_oldest_first() {
        let dir = init_repo();
        fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        commit_all(dir.path(), "first");
        let first = run_git(dir.path(), &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string();
        fs::write(dir.path().join("a.txt"), "two\n").unwrap();
        commit_all(dir.path(), "second");
        let second = run_git(dir.path(), &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string();

        let history = first_parent_history(dir.path(), "HEAD").unwrap();

        assert_eq!(history, vec![first, second]);
    }

    #[test]
    fn first_parent_history_skips_commits_only_reachable_via_a_merged_side_branch() {
        let dir = init_repo();
        fs::write(dir.path().join("a.txt"), "base\n").unwrap();
        commit_all(dir.path(), "base");
        let base = run_git(dir.path(), &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string();

        run_git(dir.path(), &["checkout", "-q", "-b", "side"]).unwrap();
        fs::write(dir.path().join("b.txt"), "side\n").unwrap();
        commit_all(dir.path(), "side commit");

        run_git(dir.path(), &["checkout", "-q", "main"]).unwrap();
        fs::write(dir.path().join("c.txt"), "main\n").unwrap();
        commit_all(dir.path(), "main commit");
        run_git(
            dir.path(),
            &["merge", "-q", "--no-ff", "side", "-m", "merge"],
        )
        .unwrap();
        let merge = run_git(dir.path(), &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string();

        let history = first_parent_history(dir.path(), "HEAD").unwrap();

        assert_eq!(
            history.len(),
            3,
            "expected base, main commit, merge only: {history:?}"
        );
        assert_eq!(history[0], base);
        assert_eq!(history[2], merge);
    }
}
