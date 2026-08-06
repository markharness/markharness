use std::fs;
use std::io;
use std::path::Path;

/// UC1〜UC8を支える物理ディレクトリ構成(論文 §3.5)。
/// knowledge=UC1/UC1b, axes=UC1, generated=UC2/UC3, executions=UC4,
/// changes=UC5/UC6, schema=UC7, tools=UC2/UC5/UC6/UC7。
/// UC8(既存ツールからのインポート)は専用ディレクトリを持たず knowledge/ に書き込む。
const SUBDIRS: [&str; 7] = [
    "knowledge",
    "axes",
    "generated",
    "executions",
    "changes",
    "schema",
    "tools",
];

pub fn run_init(root: &Path) -> io::Result<()> {
    for name in SUBDIRS {
        let dir = root.join(name);
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
        }
    }
    Ok(())
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
}
