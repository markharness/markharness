use std::io;
use std::path::Path;
use std::process::Command;

/// One entry from `git ls-tree -r`: a blob's path (relative to the repo
/// root) and its content-addressed SHA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub path: String,
    pub blob_sha: String,
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

/// Recursively lists blobs under `path_in_repo` (e.g. `"knowledge"`) as they
/// existed at `git_ref` (a tag or other revision), via `git ls-tree -r`.
/// Simplified id resolution (§3.3 の非コミットキャッシュではなく、毎回の直接走査):
/// callers derive an id from `TreeEntry::path` themselves.
pub fn ls_tree_recursive(
    root: &Path,
    git_ref: &str,
    path_in_repo: &str,
) -> io::Result<Vec<TreeEntry>> {
    let raw = run_git(root, &["ls-tree", "-r", git_ref, "--", path_in_repo])?;
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
        if obj_type != Some("blob") {
            continue;
        }
        if let Some(sha) = sha {
            entries.push(TreeEntry {
                path: path.to_string(),
                blob_sha: sha.to_string(),
            });
        }
    }
    Ok(entries)
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
        run_git(dir.path(), &["init", "-q"]).unwrap();
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

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "knowledge/req/feat/feature.yml");
        assert_eq!(entries[0].blob_sha.len(), 40);
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
    fn blob_sha_changes_when_file_content_changes_across_tags() {
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

        assert_ne!(at_m1[0].blob_sha, at_m2[0].blob_sha);
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
