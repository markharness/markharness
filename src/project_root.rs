use std::io;
use std::path::{Path, PathBuf};

/// `init`(`src/init.rs`)が作成するプロジェクトルートの目印。
pub const MARKER_FILE: &str = ".markharness.toml";

/// `start` からファイルシステムルートまで遡り、`MARKER_FILE` を持つ
/// 最も近い祖先ディレクトリ(nested projectでは最も内側のプロジェクト)を返す。
pub fn find_root(start: &Path) -> Option<PathBuf> {
    let mut current = start;
    loop {
        if current.join(MARKER_FILE).is_file() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

/// `--dir` が明示されていれば(探索はせず)`cwd` を基準に絶対パス化して
/// ルートとして使い、省略されていれば `cwd` から `find_root` で上位探索する。
/// どちらでもルートが定まらない場合は `markharness init` を促すエラーを返す。
///
/// `--dir` を絶対パス化するのは `git -C` や `cargo --manifest-path` と同じ
/// 一般的な作法に合わせるため。相対パスのまま保持すると、内部で独自に
/// 絶対パス化する処理(例: `tempfile::Builder::tempdir_in` は相対な base を
/// `env::current_dir()` に結合してから使う)との間でパスの絶対/相対が食い違い、
/// `fs_safety::ensure_no_symlink_ancestor` の `strip_prefix` 比較が失敗する。
pub fn resolve(explicit: Option<PathBuf>, cwd: &Path) -> io::Result<PathBuf> {
    match explicit {
        Some(dir) => Ok(cwd.join(dir)),
        None => find_root(cwd).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "no markharness project found in '{}' or any parent directory; run `markharness init` first",
                    cwd.display()
                ),
            )
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_returns_explicit_dir_without_searching() {
        let dir = tempfile::tempdir().unwrap();

        let resolved = resolve(Some(PathBuf::from("some/explicit/path")), dir.path()).unwrap();

        assert_eq!(resolved, dir.path().join("some/explicit/path"));
    }

    #[test]
    fn resolve_joins_a_relative_explicit_dir_with_cwd() {
        // A relative `--dir` must be resolved against `cwd` immediately,
        // matching the convention of `git -C`, `cargo --manifest-path`, etc.
        // Otherwise code downstream that independently absolutizes paths
        // (e.g. `tempfile::Builder::tempdir_in`, which joins a relative base
        // onto `env::current_dir()`) ends up comparing an absolute path
        // against this still-relative root and fails a `strip_prefix` check.
        let dir = tempfile::tempdir().unwrap();

        let resolved = resolve(Some(PathBuf::from("nested/dir")), dir.path()).unwrap();

        assert!(resolved.is_absolute());
        assert_eq!(resolved, dir.path().join("nested/dir"));
    }

    #[test]
    fn resolve_leaves_an_absolute_explicit_dir_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let explicit = dir.path().join("already/absolute");

        let resolved = resolve(Some(explicit.clone()), dir.path()).unwrap();

        assert_eq!(resolved, explicit);
    }

    #[test]
    fn resolve_finds_root_from_cwd_when_dir_omitted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MARKER_FILE), "schema_version = 1\n").unwrap();
        let nested = dir.path().join("knowledge");
        std::fs::create_dir_all(&nested).unwrap();

        let resolved = resolve(None, &nested).unwrap();

        assert_eq!(resolved, dir.path().to_path_buf());
    }

    #[test]
    fn resolve_errors_with_guidance_when_no_project_found() {
        let dir = tempfile::tempdir().unwrap();

        let err = resolve(None, dir.path()).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(err.to_string().contains("markharness init"));
    }

    #[test]
    fn finds_root_when_marker_is_in_the_start_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MARKER_FILE), "schema_version = 1\n").unwrap();

        let found = find_root(dir.path());

        assert_eq!(found, Some(dir.path().to_path_buf()));
    }

    #[test]
    fn finds_root_in_an_ancestor_directory_when_start_is_nested_inside_it() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MARKER_FILE), "schema_version = 1\n").unwrap();
        let nested = dir.path().join("knowledge/req/feat");
        std::fs::create_dir_all(&nested).unwrap();

        let found = find_root(&nested);

        assert_eq!(found, Some(dir.path().to_path_buf()));
    }

    #[test]
    fn prefers_the_nearest_marker_over_an_outer_one_for_nested_projects() {
        let outer = tempfile::tempdir().unwrap();
        std::fs::write(outer.path().join(MARKER_FILE), "schema_version = 1\n").unwrap();
        let inner = outer.path().join("sub-project");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join(MARKER_FILE), "schema_version = 1\n").unwrap();
        let start = inner.join("knowledge");
        std::fs::create_dir_all(&start).unwrap();

        let found = find_root(&start);

        assert_eq!(found, Some(inner));
    }

    #[test]
    fn returns_none_when_no_ancestor_has_a_marker() {
        let dir = tempfile::tempdir().unwrap();

        let found = find_root(dir.path());

        assert_eq!(found, None);
    }
}
