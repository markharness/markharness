use std::fs;
use std::io;
use std::path::Path;

use crate::fs_safety::replace_file;

/// UC1〜UC8を支える物理ディレクトリ構成(論文 §3.5)。
/// knowledge=UC1/UC1b, axes=UC1, generated=UC2/UC3, executions=UC4,
/// changes=UC5/UC6, schema=UC7。
/// UC8(既存ツールからのインポート)は専用ディレクトリを持たず knowledge/ に書き込む。
const SUBDIRS: [&str; 6] = [
    "knowledge",
    "axes",
    "generated",
    "executions",
    "changes",
    "schema",
];

/// `markharness init` が管理する .gitignore エントリ。
/// .markharness-cache/ は id解決キャッシュ(§3.3)で非コミット・毎プロジェクト再構築のため対象。
const GITIGNORE_ENTRIES: [&str; 1] = [".markharness-cache/"];

pub fn run_init(root: &Path) -> io::Result<()> {
    for name in SUBDIRS {
        let dir = root.join(name);
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
        }
    }
    ensure_gitignore(root)?;
    ensure_default_schemas(root)?;
    ensure_gitkeep_in_empty_dirs(root)?;
    Ok(())
}

/// `git` never tracks an empty directory, so a bare `mkdir` (as `run_init`
/// above does for `SUBDIRS`) is invisible to `git add -A` and does not
/// survive a commit/clone round trip. Drop a placeholder file into every
/// subdirectory that is still empty after the rest of `init` has run, so
/// the full UC1-UC8 directory layout is actually committable.
fn ensure_gitkeep_in_empty_dirs(root: &Path) -> io::Result<()> {
    for name in SUBDIRS {
        let dir = root.join(name);
        let is_empty = fs::read_dir(&dir)?.next().is_none();
        if is_empty {
            replace_file(root, &dir.join(".gitkeep"), b"")?;
        }
    }
    Ok(())
}

/// Populates `schema/` with the default JSON Schema files (§3.5/§3.6) used
/// by `markharness validate`, without overwriting a file a project has
/// already customized.
fn ensure_default_schemas(root: &Path) -> io::Result<()> {
    let schema_dir = root.join("schema");
    for (name, content) in crate::schema::DEFAULT_SCHEMA_FILES {
        let path = schema_dir.join(name);
        if !path.exists() {
            replace_file(root, &path, content.as_bytes())?;
        }
    }
    Ok(())
}

fn ensure_gitignore(root: &Path) -> io::Result<()> {
    let path = root.join(".gitignore");
    let existing = fs::read_to_string(&path).unwrap_or_default();

    let missing: Vec<&str> = GITIGNORE_ENTRIES
        .into_iter()
        .filter(|entry| !existing.lines().any(|line| line == *entry))
        .collect();

    if missing.is_empty() {
        return Ok(());
    }

    let mut updated = existing.clone();
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    if !updated.is_empty() {
        updated.push('\n');
    }
    updated.push_str("# markharness init\n");
    for entry in missing {
        updated.push_str(entry);
        updated.push('\n');
    }

    replace_file(root, &path, updated.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_all_uc1_to_uc8_directories_in_empty_dir() {
        let dir = tempfile::tempdir().unwrap();

        run_init(dir.path()).unwrap();

        for name in SUBDIRS {
            assert!(dir.path().join(name).is_dir(), "{name} should be created");
        }
    }

    #[test]
    fn creates_missing_directories_when_some_already_exist() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("knowledge")).unwrap();
        let marker = dir.path().join("knowledge").join("marker.txt");
        fs::write(&marker, "keep me").unwrap();

        run_init(dir.path()).unwrap();

        for name in SUBDIRS {
            assert!(dir.path().join(name).is_dir(), "{name} should be created");
        }
        assert_eq!(fs::read_to_string(&marker).unwrap(), "keep me");
    }

    #[test]
    fn is_idempotent_when_already_fully_initialized() {
        let dir = tempfile::tempdir().unwrap();
        run_init(dir.path()).unwrap();

        let result = run_init(dir.path());

        assert!(result.is_ok());
        for name in SUBDIRS {
            assert!(dir.path().join(name).is_dir());
        }
    }

    #[test]
    fn creates_gitignore_with_cache_entry_when_missing() {
        let dir = tempfile::tempdir().unwrap();

        run_init(dir.path()).unwrap();

        let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(content.contains(".markharness-cache/"));
    }

    #[test]
    fn appends_missing_entry_to_existing_gitignore_without_removing_content() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "node_modules/\n").unwrap();

        run_init(dir.path()).unwrap();

        let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(content.contains("node_modules/"));
        assert!(content.contains(".markharness-cache/"));
    }

    #[test]
    fn populates_schema_dir_with_default_schema_files() {
        let dir = tempfile::tempdir().unwrap();

        run_init(dir.path()).unwrap();

        for (name, _) in crate::schema::DEFAULT_SCHEMA_FILES {
            assert!(
                dir.path().join("schema").join(name).is_file(),
                "{name} should be created under schema/"
            );
        }
    }

    #[test]
    fn does_not_overwrite_a_customized_schema_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("schema")).unwrap();
        fs::write(
            dir.path().join("schema/feature.schema.json"),
            "custom content",
        )
        .unwrap();

        run_init(dir.path()).unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join("schema/feature.schema.json")).unwrap(),
            "custom content"
        );
    }

    /// `git` does not track empty directories, so a plain `mkdir` for
    /// `knowledge/`, `axes/`, `generated/`, `executions/`, `changes/` is
    /// invisible to `git add -A` and never survives a commit/clone round
    /// trip. `init` must leave something committable behind in every
    /// directory it creates that isn't otherwise populated (`schema/` gets
    /// real files from `ensure_default_schemas`, so it needs no placeholder).
    #[test]
    fn creates_gitkeep_placeholder_in_directories_that_would_otherwise_be_empty_so_git_can_track_them()
     {
        let dir = tempfile::tempdir().unwrap();

        run_init(dir.path()).unwrap();

        for name in ["knowledge", "axes", "generated", "executions", "changes"] {
            assert!(
                dir.path().join(name).join(".gitkeep").is_file(),
                "{name} should contain a .gitkeep placeholder"
            );
        }
        assert!(
            !dir.path().join("schema").join(".gitkeep").exists(),
            "schema/ already has real files and needs no placeholder"
        );
    }

    #[test]
    fn does_not_add_gitkeep_to_a_directory_that_already_has_content() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("knowledge/req/feat")).unwrap();
        fs::write(
            dir.path().join("knowledge/req/feat/feature.yml"),
            "id: feat\n",
        )
        .unwrap();

        run_init(dir.path()).unwrap();

        assert!(!dir.path().join("knowledge").join(".gitkeep").exists());
    }

    #[test]
    fn does_not_duplicate_entry_when_already_present() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(".gitignore"),
            "# markharness init\n.markharness-cache/\n",
        )
        .unwrap();

        run_init(dir.path()).unwrap();

        let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        let occurrences = content.matches(".markharness-cache/").count();
        assert_eq!(occurrences, 1);
    }
}
