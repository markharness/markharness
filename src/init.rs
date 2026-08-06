use std::fs;
use std::io;
use std::path::Path;

const SUBDIRS: [&str; 3] = ["knowledge", "generated", "changes"];

pub fn run_init(root: &Path, force: bool) -> io::Result<()> {
    for name in SUBDIRS {
        let dir = root.join(name);
        if dir.exists() {
            if !force {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "{} already exists; pass --force to re-initialize",
                        dir.display()
                    ),
                ));
            }
            continue;
        }
        fs::create_dir_all(&dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_knowledge_generated_and_changes_directories_in_empty_dir() {
        let dir = tempfile::tempdir().unwrap();

        run_init(dir.path(), false).unwrap();

        assert!(dir.path().join("knowledge").is_dir());
        assert!(dir.path().join("generated").is_dir());
        assert!(dir.path().join("changes").is_dir());
    }

    #[test]
    fn errors_when_already_initialized_without_force() {
        let dir = tempfile::tempdir().unwrap();
        run_init(dir.path(), false).unwrap();

        let result = run_init(dir.path(), false);

        assert!(result.is_err());
    }

    #[test]
    fn force_reinit_preserves_existing_knowledge_files() {
        let dir = tempfile::tempdir().unwrap();
        run_init(dir.path(), false).unwrap();
        let marker = dir.path().join("knowledge").join("marker.txt");
        fs::write(&marker, "keep me").unwrap();

        run_init(dir.path(), true).unwrap();

        assert_eq!(fs::read_to_string(&marker).unwrap(), "keep me");
    }
}
