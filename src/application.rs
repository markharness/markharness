use std::io;
use std::path::{Component, Path, PathBuf};

use crate::canonical;
use crate::case_definition;
use crate::changes::{self, ChangeOptions};
use crate::fs_safety::{copy_unmanaged_siblings_no_follow, replace_dir_from_staging, replace_file};
use crate::generate;
use crate::presentation::CommandOutcome;
use crate::traceability;

pub fn import_native(root: &Path, git_ref: &str) -> io::Result<CommandOutcome> {
    Ok(CommandOutcome::CanonicalImported(canonical::import_native(
        root, git_ref,
    )?))
}

pub fn import_junit(
    xml: &str,
    source_locator: &str,
    bound_versions: std::collections::BTreeMap<String, String>,
) -> io::Result<CommandOutcome> {
    Ok(CommandOutcome::CanonicalImported(canonical::import_junit(
        xml,
        source_locator,
        bound_versions,
    )?))
}

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
    let snapshot = generate::load_knowledge_snapshot(
        &root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge"),
    )?;
    let testcases = generate::compile_testcases(&snapshot);
    // ADR 0017 §5: freeze each migrated TestCase's effective content under
    // its `(case_uid, case_revision)` key. Independent of the
    // staging/atomic-swap dance below — `generated/` is fully reproducible
    // from `knowledge/` on every run, but `case-definitions/` is meant to
    // accumulate durably across runs, so it's written directly rather than
    // through the disposable staging directory.
    for testcase in &testcases {
        case_definition::store_case_definition(root, testcase)?;
    }
    let generated_dir = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("generated");
    let existing_index = generated_dir.join("traceability-index.json");
    if existing_index.exists() && !existing_index.is_file() {
        return Err(io::Error::other(format!(
            "expected {} to be a file",
            existing_index.display()
        )));
    }

    let staging_parent = tempfile::Builder::new()
        .prefix(".markharness-generate-")
        .tempdir_in(root)?;
    let staging_generated = staging_parent
        .path()
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("generated");
    let testcases_dir = staging_generated.join("testcases");
    std::fs::create_dir_all(&testcases_dir)?;
    for testcase in &testcases {
        let testcase_path = safe_testcase_path(&testcases_dir, &testcase.relative_path())?;
        replace_file(
            staging_parent.path(),
            &testcase_path,
            generate::serialize_testcase(testcase).as_bytes(),
        )?;
    }
    let index = traceability::build_index(&testcases);
    let staged_index = staging_generated.join("traceability-index.json");
    replace_file(
        staging_parent.path(),
        &staged_index,
        traceability::serialize_index(&index).as_bytes(),
    )?;
    // `testcases/` and `traceability-index.json` are generator-owned and
    // fully replaced by the staged content above; everything else already
    // present under `generated/` (e.g. the `.gitkeep` placeholder `init`
    // leaves behind) is not owned by the generator, so it is carried
    // forward into staging before the atomic whole-directory swap below,
    // rather than being discarded by it.
    copy_unmanaged_siblings_no_follow(
        root,
        &generated_dir,
        &staging_generated,
        &["testcases", "traceability-index.json"],
    )?;
    replace_dir_from_staging(root, &staging_generated, &generated_dir)?;

    let mut written: Vec<PathBuf> = testcases
        .iter()
        .map(|testcase| {
            generated_dir
                .join("testcases")
                .join(testcase.relative_path())
        })
        .collect();
    written.push(generated_dir.join("traceability-index.json"));
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
    let outcome = changes::compute_changes_with_warnings(root, from, to, options)?;
    replace_file(
        root,
        &root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("changes")
            .join(format!("{to}.yaml")),
        changes::serialize_changes(&outcome.events).as_bytes(),
    )?;
    Ok(CommandOutcome::ChangesComputed {
        count: outcome.events.len(),
        to: to.to_string(),
        warnings: outcome.warnings,
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
