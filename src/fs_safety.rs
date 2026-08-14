use std::fs;
use std::io;
use std::path::Path;

/// Rejects `target` if any directory between `root` (exclusive) and `target`
/// (exclusive) is a symlink. On Windows this also catches directory
/// junctions, since `FileType::is_symlink()` reports true for both. Ancestors
/// that don't exist yet are not an error, since managed directories
/// (`knowledge/`, `generated/`, `executions/`, `changes/`,
/// `.markharness-cache/`) are frequently created on demand.
///
/// Guards against a malicious repository placing a link where a managed
/// directory is expected, so that a later `remove_dir_all`/`write`/`rename`
/// doesn't silently follow the link outside `root`.
pub fn ensure_no_symlink_ancestor(root: &Path, target: &Path) -> io::Result<()> {
    let relative = target.strip_prefix(root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not inside {}", target.display(), root.display()),
        )
    })?;

    let mut current = root.to_path_buf();
    let mut ancestor_components: Vec<_> = relative.components().collect();
    ancestor_components.pop();
    for component in ancestor_components {
        current.push(component);
        if let Ok(metadata) = fs::symlink_metadata(&current)
            && metadata.file_type().is_symlink()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("refusing to follow symlink ancestor: {}", current.display()),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn link_dir(link: &Path, target: &Path) {
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[cfg(windows)]
    fn link_dir(link: &Path, target: &Path) {
        let status = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/j"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "mklink /j failed");
    }

    #[test]
    fn allows_a_target_with_no_symlink_ancestors() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root.join("generated").join("testcases").join("ground.yml");

        assert!(ensure_no_symlink_ancestor(root, &target).is_ok());
    }

    #[test]
    fn rejects_a_target_behind_a_symlinked_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let outside = tempfile::tempdir().unwrap();
        let link = root.join("generated");
        link_dir(&link, outside.path());
        let target = link.join("testcases").join("ground.yml");

        let result = ensure_no_symlink_ancestor(root, &target);

        assert!(
            result.is_err(),
            "expected an error for a target behind a symlinked ancestor, got: {result:?}"
        );
    }
}
