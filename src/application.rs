use std::io;
use std::path::{Component, Path, PathBuf};

use crate::changes::{self, ChangeOptions};
use crate::fs_safety::{remove_dir_all_no_follow, replace_file};
use crate::generate;
use crate::presentation::CommandOutcome;
use crate::traceability;
use crate::verify::{self, PendingError};

fn safe_testcase_path(testcases_dir: &Path, relative_path: &Path) -> io::Result<PathBuf> {
    if relative_path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "refusing to write testcase outside {}: {}",
                testcases_dir.display(),
                relative_path.display()
            ),
        ));
    }
    Ok(testcases_dir.join(relative_path))
}

pub fn generate_testcases(root: &Path) -> io::Result<CommandOutcome> {
    let testcases = generate::generate_testcases(&root.join("knowledge"))?;
    let testcases_dir = root.join("generated/testcases");
    remove_dir_all_no_follow(root, &testcases_dir)?;
    std::fs::create_dir_all(&testcases_dir)?;
    let mut written = Vec::new();
    for testcase in &testcases {
        let testcase_path = safe_testcase_path(&testcases_dir, &testcase.relative_path())?;
        replace_file(
            root,
            &testcase_path,
            generate::serialize_testcase(testcase).as_bytes(),
        )?;
        written.push(testcase_path);
    }
    let index = traceability::build_index(&testcases);
    let index_path = root.join("generated/traceability-index.json");
    replace_file(
        root,
        &index_path,
        traceability::serialize_index(&index).as_bytes(),
    )?;
    written.push(index_path);
    Ok(CommandOutcome::Generated {
        count: testcases.len(),
        written,
    })
}

pub fn compute_changes(
    root: &Path,
    from: &str,
    to: &str,
    options: ChangeOptions,
) -> io::Result<CommandOutcome> {
    let events = changes::compute_changes(root, from, to, options)?;
    replace_file(
        root,
        &root.join("changes").join(format!("{to}.yaml")),
        changes::serialize_changes(&events).as_bytes(),
    )?;
    Ok(CommandOutcome::ChangesComputed {
        count: events.len(),
        to: to.to_string(),
    })
}

pub fn verify_pending(
    root: &Path,
    range: Option<(&str, &str)>,
    use_cache: bool,
    fail_on_pending: bool,
) -> Result<CommandOutcome, PendingError> {
    Ok(CommandOutcome::Pending {
        report: verify::pending(root, range, use_cache)?,
        fail_on_pending,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_testcase_path_joins_a_nested_relative_path() {
        let dir = PathBuf::from("generated/testcases");
        let relative = PathBuf::from("req-todo/todo/todo-add-task/ground.yml");

        assert_eq!(
            safe_testcase_path(&dir, &relative).unwrap(),
            dir.join(relative)
        );
    }

    #[test]
    fn safe_testcase_path_rejects_a_path_that_escapes_the_output_directory() {
        let dir = PathBuf::from("generated/testcases");

        assert!(safe_testcase_path(&dir, Path::new("../../evil.yml")).is_err());
    }
}
