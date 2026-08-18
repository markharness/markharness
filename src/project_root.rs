use std::io;
use std::path::{Path, PathBuf};

/// markharness が管理するディレクトリ群(knowledge/axes/generated/executions/
/// changes/schema)を集約する名前空間。既存プロジェクトへの導入時にトップ
/// レベルの汎用的な名前(`knowledge/`、`schema/`等)が既存ディレクトリと衝突
/// するのを避けるため、単一のドットディレクトリ配下にまとめる。
/// 中身は `.markharness-cache/`(id解決キャッシュ)を除きコミット対象。
pub const MARKHARNESS_DIR: &str = ".markharness";

/// `init`(`src/init.rs`)が作成するプロジェクトルートの目印(`MARKHARNESS_DIR`
/// 配下)。ADR 0012 により、ルート直下の独立ファイルだった `.markharness.toml`
/// から `MARKHARNESS_DIR` 名前空間内に移した(ADR 0011 が謳う「`.markharness/`
/// が markharness の占有する唯一のトップレベル名」という方針と、ルート直下の
/// 別ファイルが矛盾していたため)。`MARKHARNESS_DIR` との合成は `Path::join`
/// ではなく文字列リテラルで持つ(`Path::join`は`const`文脈で使えないため)。
pub const MARKER_FILE: &str = ".markharness/config.toml";

/// `knowledge/`(`MARKHARNESS_DIR` 配下)へのリポジトリ相対パス。`git ls-tree`
/// 等の pathspec は `Path::join` を使わず生文字列で渡すため、`MARKHARNESS_DIR`
/// と別に持つ。値を変更する際は両者を一致させること(Rust の `const` は
/// 文字列連結ができないため手動同期が必要)。
pub const KNOWLEDGE_PATH_IN_REPO: &str = ".markharness/knowledge";

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
/// どちらの場合も、対象ディレクトリに `MARKER_FILE` が存在しなければ
/// `markharness init` を促すエラーを返す。
///
/// `cargo --manifest-path` は指定した manifest の実在を検証してから使う。
/// `--dir` も同様に、明示パスであっても検証なしに信頼はしない(存在しない/
/// 未初期化のディレクトリを渡すと、以前は後続処理内の分かりにくい汎用的な
/// ファイルI/Oエラーとして失敗していた)。
///
/// `--dir` を絶対パス化するのは `git -C` や `cargo --manifest-path` と同じ
/// 一般的な作法に合わせるため。相対パスのまま保持すると、内部で独自に
/// 絶対パス化する処理(例: `tempfile::Builder::tempdir_in` は相対な base を
/// `env::current_dir()` に結合してから使う)との間でパスの絶対/相対が食い違い、
/// `fs_safety::ensure_no_symlink_ancestor` の `strip_prefix` 比較が失敗する。
pub fn resolve(explicit: Option<PathBuf>, cwd: &Path) -> io::Result<PathBuf> {
    match explicit {
        Some(dir) => {
            let root = cwd.join(dir);
            if root.join(MARKER_FILE).is_file() {
                Ok(root)
            } else {
                Err(not_found_error(&root))
            }
        }
        None => find_root(cwd).ok_or_else(|| not_found_error(cwd)),
    }
}

fn not_found_error(dir: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "no markharness project found in '{}' or any parent directory; run `markharness init` first",
            dir.display()
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `MARKER_FILE` は `MARKHARNESS_DIR` 配下の多段パスなので、テストで
    /// 書き込む際は親ディレクトリを先に作る必要がある。
    fn write_marker(root: &Path) {
        let path = root.join(MARKER_FILE);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "schema_version = 1\n").unwrap();
    }

    #[test]
    fn markharness_dir_is_a_single_dot_directory_namespace() {
        assert_eq!(MARKHARNESS_DIR, ".markharness");
    }

    #[test]
    fn knowledge_path_in_repo_stays_in_sync_with_markharness_dir() {
        assert_eq!(
            KNOWLEDGE_PATH_IN_REPO,
            format!("{MARKHARNESS_DIR}/knowledge")
        );
    }

    #[test]
    fn marker_file_stays_in_sync_with_markharness_dir() {
        assert_eq!(MARKER_FILE, format!("{MARKHARNESS_DIR}/config.toml"));
    }

    #[test]
    fn resolve_returns_explicit_dir_without_searching_when_it_has_a_marker() {
        let dir = tempfile::tempdir().unwrap();
        let explicit = dir.path().join("some/explicit/path");
        write_marker(&explicit);

        let resolved = resolve(Some(PathBuf::from("some/explicit/path")), dir.path()).unwrap();

        assert_eq!(resolved, explicit);
    }

    #[test]
    fn resolve_errors_when_explicit_dir_has_no_marker() {
        // Like `cargo --manifest-path`, an explicit `--dir` is validated
        // before being trusted rather than passed through as-is; otherwise a
        // non-existent or uninitialized directory fails later with an
        // unrelated, confusing filesystem error instead of this clear one.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("not-initialized")).unwrap();

        let err = resolve(Some(PathBuf::from("not-initialized")), dir.path()).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(err.to_string().contains("markharness init"));
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
        write_marker(&dir.path().join("nested/dir"));

        let resolved = resolve(Some(PathBuf::from("nested/dir")), dir.path()).unwrap();

        assert!(resolved.is_absolute());
        assert_eq!(resolved, dir.path().join("nested/dir"));
    }

    #[test]
    fn resolve_leaves_an_absolute_explicit_dir_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let explicit = dir.path().join("already/absolute");
        write_marker(&explicit);

        let resolved = resolve(Some(explicit.clone()), dir.path()).unwrap();

        assert_eq!(resolved, explicit);
    }

    #[test]
    fn resolve_finds_root_from_cwd_when_dir_omitted() {
        let dir = tempfile::tempdir().unwrap();
        write_marker(dir.path());
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
        write_marker(dir.path());

        let found = find_root(dir.path());

        assert_eq!(found, Some(dir.path().to_path_buf()));
    }

    #[test]
    fn finds_root_in_an_ancestor_directory_when_start_is_nested_inside_it() {
        let dir = tempfile::tempdir().unwrap();
        write_marker(dir.path());
        let nested = dir.path().join("knowledge/req/feat");
        std::fs::create_dir_all(&nested).unwrap();

        let found = find_root(&nested);

        assert_eq!(found, Some(dir.path().to_path_buf()));
    }

    #[test]
    fn prefers_the_nearest_marker_over_an_outer_one_for_nested_projects() {
        let outer = tempfile::tempdir().unwrap();
        write_marker(outer.path());
        let inner = outer.path().join("sub-project");
        std::fs::create_dir_all(&inner).unwrap();
        write_marker(&inner);
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
