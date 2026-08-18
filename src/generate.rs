use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::knowledge::{
    is_valid_slug, parse_behavior, parse_condition, parse_expected_result, parse_feature,
    parse_requirement,
};

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct GeneratedFrom {
    pub requirement: String,
    pub feature: String,
    pub behavior: String,
    pub condition: String,
    pub expected_results: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct TestCase {
    pub case_id: String,
    pub generated_from: GeneratedFrom,
    pub title: String,
    pub steps: Vec<String>,
    pub expected: Vec<String>,
    /// Requirement/Feature/Behavior の axis を合成(union)したもの。決定性のため
    /// 重複除去のうえソートする(§3.4 axisの継承)。
    pub axis: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedSnapshot {
    pub id: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeCaseSnapshot {
    pub requirement_id: String,
    pub requirement_axis: Vec<String>,
    pub feature_id: String,
    pub feature_axis: Vec<String>,
    pub behavior_id: String,
    pub behavior_description: String,
    pub behavior_axis: Vec<String>,
    pub condition_id: String,
    pub condition_description: String,
    pub expected: Vec<ExpectedSnapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnowledgeSnapshot {
    pub cases: Vec<KnowledgeCaseSnapshot>,
}

/// Deduplicates and sorts axis values from multiple hierarchy levels into one
/// deterministic list, independent of input order or duplication.
fn union_axis(sources: &[&[String]]) -> Vec<String> {
    let mut union: Vec<String> = sources
        .iter()
        .flat_map(|axis| axis.iter().cloned())
        .collect();
    union.sort();
    union.dedup();
    union
}

impl TestCase {
    /// The path, relative to `generated/testcases/`, this TestCase is
    /// written to: `{requirement}/{feature}/{behavior}/{condition}.yml`,
    /// mirroring `knowledge/`'s own hierarchy. Because this mirrors a tree
    /// that is itself collision-free (two Conditions cannot occupy the same
    /// `knowledge/<req>/<feature>/<behavior>/<condition>/` directory), no two
    /// TestCases can ever be written to the same path, unlike the flat
    /// `<condition.id>.yml` naming this replaced (which silently overwrote
    /// when the same condition.id was reused under a different Behavior).
    pub fn relative_path(&self) -> PathBuf {
        Path::new(&self.generated_from.requirement)
            .join(&self.generated_from.feature)
            .join(&self.generated_from.behavior)
            .join(format!("{}.yml", self.generated_from.condition))
    }
}

/// Lists `dir`'s direct subdirectories, excluding symlinks (and, on
/// Windows, directory junctions — `DirEntry::file_type()` reports neither as
/// a plain directory). Unlike `Path::is_dir()`, `file_type()` does not
/// follow links, so a link pointing at an ancestor or at a directory outside
/// the knowledge tree is skipped rather than walked into.
pub(crate) fn sorted_subdirs(dir: &Path) -> io::Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut dirs: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|ft| ft.is_dir()))
        .map(|entry| entry.path())
        .collect();
    dirs.sort();
    Ok(dirs)
}

/// Defense in depth beyond the symlink exclusion in `sorted_subdirs`: caps
/// how many directories a single `find_dirs_with_marker` call will visit, so
/// an unexpectedly huge (but ordinary, link-free) tree fails fast instead of
/// consuming unbounded time and memory.
const MAX_VISITED_DIRS: usize = 100_000;

/// Recursively searches `root` for directories directly containing `marker_file`,
/// stopping the search along a branch as soon as a match is found.
pub(crate) fn find_dirs_with_marker(root: &Path, marker_file: &str) -> io::Result<Vec<PathBuf>> {
    find_dirs_with_marker_limited(root, marker_file, MAX_VISITED_DIRS)
}

fn find_dirs_with_marker_limited(
    root: &Path,
    marker_file: &str,
    max_visited: usize,
) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut visited = 0usize;
    while let Some(dir) = stack.pop() {
        visited += 1;
        if visited > max_visited {
            return Err(io::Error::other(format!(
                "knowledge tree traversal under {} visited more than {max_visited} directories; aborting",
                root.display()
            )));
        }
        if dir.join(marker_file).is_file() {
            found.push(dir);
            continue;
        }
        for child in sorted_subdirs(&dir)? {
            stack.push(child);
        }
    }
    found.sort();
    Ok(found)
}

/// Recursively lists every regular file under `root`, returned as paths
/// relative to `root` (sorted for determinism). Symlinked files and
/// directories are skipped rather than followed, mirroring `sorted_subdirs`.
/// Used to read back a `generated/testcases/` tree that now mirrors
/// `knowledge/`'s own nesting instead of being flat.
pub(crate) fn list_files_recursive(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !root.is_dir() {
        return Ok(files);
    }
    let mut stack = vec![root.to_path_buf()];
    let mut visited = 0usize;
    while let Some(dir) = stack.pop() {
        visited += 1;
        if visited > MAX_VISITED_DIRS {
            return Err(io::Error::other(format!(
                "directory traversal under {} visited more than {MAX_VISITED_DIRS} directories; aborting",
                root.display()
            )));
        }
        for entry in fs::read_dir(&dir)?.filter_map(|e| e.ok()) {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                files.push(
                    path.strip_prefix(root)
                        .expect("entry path is a child of root")
                        .to_path_buf(),
                );
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Rejects an id (requirement/feature/behavior/condition) that isn't a
/// plain slug before it can become a path component of `case_id` or of
/// `generated/testcases/`'s mirrored directory tree (`TestCase::relative_path`).
/// Without this, a crafted `id:` field (independent of the trusted directory
/// name it lives in) could smuggle `../` or similar through into the write
/// path.
fn require_valid_slug(source_path: &Path, field: &str, id: &str) -> io::Result<()> {
    if is_valid_slug(id) {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "{}: {field} id \"{id}\" is not a valid slug (lowercase alphanumeric and hyphen only)",
            source_path.display()
        ),
    ))
}

pub fn load_knowledge_snapshot(knowledge_root: &Path) -> io::Result<KnowledgeSnapshot> {
    let mut cases = Vec::new();

    for requirement_dir in sorted_subdirs(knowledge_root)? {
        let requirement_path = requirement_dir.join("requirement.yml");
        if !requirement_path.is_file() {
            continue;
        }
        let requirement = parse_requirement(&fs::read_to_string(&requirement_path)?)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        require_valid_slug(&requirement_path, "requirement", &requirement.id)?;

        for feature_dir in sorted_subdirs(&requirement_dir)? {
            let feature_path = feature_dir.join("feature.yml");
            if !feature_path.is_file() {
                continue;
            }
            let feature = parse_feature(&fs::read_to_string(&feature_path)?)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            require_valid_slug(&feature_path, "feature", &feature.id)?;

            for behavior_dir in find_dirs_with_marker(&feature_dir, "behavior.yml")? {
                let behavior_path = behavior_dir.join("behavior.yml");
                let behavior = parse_behavior(&fs::read_to_string(&behavior_path)?)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                require_valid_slug(&behavior_path, "behavior", &behavior.id)?;

                for condition_dir in find_dirs_with_marker(&behavior_dir, "condition.yml")? {
                    let condition_path = condition_dir.join("condition.yml");
                    let condition = parse_condition(&fs::read_to_string(&condition_path)?)
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                    require_valid_slug(&condition_path, "condition", &condition.id)?;

                    let expected_dir = condition_dir.join("expected");
                    if !expected_dir.is_dir() {
                        continue;
                    }
                    let mut expected_paths: Vec<PathBuf> = fs::read_dir(&expected_dir)?
                        .filter_map(|entry| entry.ok())
                        .map(|entry| entry.path())
                        .filter(|path| path.is_file())
                        .collect();
                    expected_paths.sort();
                    if expected_paths.is_empty() {
                        continue;
                    }

                    let mut expected = Vec::new();
                    for expected_path in &expected_paths {
                        let parsed = parse_expected_result(&fs::read_to_string(expected_path)?)
                            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                        expected.push(ExpectedSnapshot {
                            id: parsed.id,
                            description: parsed.description,
                        });
                    }

                    cases.push(KnowledgeCaseSnapshot {
                        requirement_id: requirement.id.clone(),
                        requirement_axis: requirement.axis.clone(),
                        feature_id: feature.id.clone(),
                        feature_axis: feature.axis.clone(),
                        behavior_id: behavior.id.clone(),
                        behavior_description: behavior.description.clone(),
                        behavior_axis: behavior.axis.clone(),
                        condition_id: condition.id,
                        condition_description: condition.description,
                        expected,
                    });
                }
            }
        }
    }

    Ok(KnowledgeSnapshot { cases })
}

pub fn compile_testcases(snapshot: &KnowledgeSnapshot) -> Vec<TestCase> {
    let mut testcases: Vec<TestCase> = snapshot
        .cases
        .iter()
        .map(|case| TestCase {
            case_id: format!(
                "tc-{}-{}-{}-{}",
                case.requirement_id, case.feature_id, case.behavior_id, case.condition_id
            ),
            generated_from: GeneratedFrom {
                requirement: case.requirement_id.clone(),
                feature: case.feature_id.clone(),
                behavior: case.behavior_id.clone(),
                condition: case.condition_id.clone(),
                expected_results: case.expected.iter().map(|item| item.id.clone()).collect(),
            },
            title: case.condition_description.clone(),
            steps: vec![case.behavior_description.clone()],
            expected: case
                .expected
                .iter()
                .map(|item| item.description.clone())
                .collect(),
            axis: union_axis(&[
                &case.requirement_axis,
                &case.feature_axis,
                &case.behavior_axis,
            ]),
        })
        .collect();
    testcases.sort_by(|a, b| a.case_id.cmp(&b.case_id));
    testcases
}

pub fn generate_testcases(knowledge_root: &Path) -> io::Result<Vec<TestCase>> {
    Ok(compile_testcases(&load_knowledge_snapshot(knowledge_root)?))
}

pub fn serialize_testcase(testcase: &TestCase) -> String {
    serde_yaml_ng::to_string(testcase).expect("TestCase serialization is infallible")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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

    /// A symlink/junction pointing back at an ancestor directory currently
    /// makes `find_dirs_with_marker`/`sorted_subdirs` treat the link as an
    /// ordinary subdirectory and walk into it, re-growing the same path
    /// (`tree/loop`, `tree/loop/loop`, ...) until the OS's own path-length
    /// limit finally errors it out — a real but incidental stop, not a
    /// correct one, and exactly the resource-exhaustion behavior being
    /// fixed. Runs on a separate thread with a bounded wait so a
    /// pathological implementation fails this test instead of hanging the
    /// whole suite.
    #[test]
    fn find_dirs_with_marker_does_not_grow_the_stack_through_a_self_referential_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tree");
        fs::create_dir_all(&root).unwrap();
        link_dir(&root.join("loop"), &root);

        let root_for_thread = root.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(find_dirs_with_marker(&root_for_thread, "marker.yml"));
        });

        let result = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("find_dirs_with_marker did not terminate within 5s");
        assert!(
            result.is_ok(),
            "find_dirs_with_marker should not error out via OS path-length limits: {result:?}"
        );
    }

    /// Defense in depth on top of the symlink exclusion above: an ordinary
    /// (non-symlink) tree that is simply too large must not be walked
    /// without bound either. `find_dirs_with_marker_limited` lets tests
    /// exercise the cap without actually creating a huge tree.
    #[test]
    fn find_dirs_with_marker_limited_errors_when_visited_count_exceeds_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tree");
        for i in 0..5 {
            fs::create_dir_all(root.join(format!("dir-{i}"))).unwrap();
        }

        let result = find_dirs_with_marker_limited(&root, "marker.yml", 3);

        assert!(
            result.is_err(),
            "expected an error when the tree has more directories than the cap allows"
        );
    }

    #[test]
    fn find_dirs_with_marker_limited_succeeds_when_within_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tree");
        for i in 0..3 {
            let sub = root.join(format!("dir-{i}"));
            fs::create_dir_all(&sub).unwrap();
            fs::write(sub.join("marker.yml"), "id: x\n").unwrap();
        }

        let found = find_dirs_with_marker_limited(&root, "marker.yml", 10).unwrap();

        assert_eq!(found.len(), 3);
    }

    /// The behavior that actually matters: a symlink to a directory outside
    /// the knowledge tree must not be followed at all, even when it holds a
    /// matching marker file. Today `sorted_subdirs` uses `Path::is_dir()`,
    /// which follows the link, so this test starts Red.
    #[test]
    fn find_dirs_with_marker_does_not_follow_a_symlink_to_an_external_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tree");
        fs::create_dir_all(&root).unwrap();
        let real = root.join("real");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("marker.yml"), "id: real\n").unwrap();

        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("marker.yml"), "id: outside\n").unwrap();
        link_dir(&root.join("linked"), outside.path());

        let found = find_dirs_with_marker(&root, "marker.yml").unwrap();

        assert_eq!(found, vec![real]);
    }

    #[test]
    fn list_files_recursive_returns_empty_for_a_missing_dir() {
        let dir = tempfile::tempdir().unwrap();

        let files = list_files_recursive(&dir.path().join("does-not-exist")).unwrap();

        assert!(files.is_empty());
    }

    #[test]
    fn list_files_recursive_finds_files_nested_several_levels_deep() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tree");
        fs::create_dir_all(root.join("a/b/c")).unwrap();
        fs::write(root.join("top.yml"), "top").unwrap();
        fs::write(root.join("a/mid.yml"), "mid").unwrap();
        fs::write(root.join("a/b/c/deep.yml"), "deep").unwrap();

        let files = list_files_recursive(&root).unwrap();

        assert_eq!(
            files,
            vec![
                PathBuf::from("a/b/c/deep.yml"),
                PathBuf::from("a/mid.yml"),
                PathBuf::from("top.yml"),
            ]
        );
    }

    #[test]
    fn list_files_recursive_does_not_follow_a_symlinked_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tree");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("real.yml"), "real").unwrap();

        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.yml"), "secret").unwrap();
        link_dir(&root.join("linked"), outside.path());

        let files = list_files_recursive(&root).unwrap();

        assert_eq!(files, vec![PathBuf::from("real.yml")]);
    }

    fn write_requirement(root: &std::path::Path, requirement: &str, axis: &[&str]) {
        let dir = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge")
            .join(requirement);
        fs::create_dir_all(&dir).unwrap();
        let axis_line = axis.join(", ");
        fs::write(
            dir.join("requirement.yml"),
            format!("id: {requirement}\nlabel: {requirement}\naxis: [{axis_line}]\n"),
        )
        .unwrap();
    }

    fn write_feature(root: &std::path::Path, requirement: &str, feature: &str, axis: &[&str]) {
        let dir = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge")
            .join(requirement)
            .join(feature);
        fs::create_dir_all(&dir).unwrap();
        let axis_line = axis.join(", ");
        fs::write(
            dir.join("feature.yml"),
            format!(
                "id: {feature}\nrequirement: {requirement}\nlabel: {feature}\naxis: [{axis_line}]\n"
            ),
        )
        .unwrap();
    }

    fn write_behavior(
        root: &std::path::Path,
        requirement: &str,
        feature: &str,
        behavior: &str,
        description: &str,
    ) {
        let dir = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge")
            .join(requirement)
            .join(feature)
            .join(behavior);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("behavior.yml"),
            format!(
                "id: {behavior}\nfeature: {feature}\nlabel: {behavior}\naxis: [ui]\ndescription: |\n  {description}\n"
            ),
        )
        .unwrap();
    }

    fn write_condition(
        root: &std::path::Path,
        requirement: &str,
        feature: &str,
        behavior: &str,
        condition: &str,
        description: &str,
    ) {
        let dir = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge")
            .join(requirement)
            .join(feature)
            .join(behavior)
            .join(condition);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("condition.yml"),
            format!(
                "id: {condition}\nbehavior: {behavior}\nlabel: {condition}\ndescription: |\n  {description}\n"
            ),
        )
        .unwrap();
    }

    #[allow(clippy::too_many_arguments)]
    fn write_expected(
        root: &std::path::Path,
        requirement: &str,
        feature: &str,
        behavior: &str,
        condition: &str,
        seq: &str,
        id: &str,
        description: &str,
    ) {
        let dir = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge")
            .join(requirement)
            .join(feature)
            .join(behavior)
            .join(condition)
            .join("expected");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("{seq}.yml")),
            format!("id: {id}\ncondition: {condition}\ndescription: |\n  {description}\n"),
        )
        .unwrap();
    }

    #[test]
    fn rejects_condition_with_path_traversal_id() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);
        write_behavior(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "User adds a task.",
        );
        // The directory name is a safe slug, but a malicious repository can
        // still craft the `id:` field inside condition.yml independently of
        // the directory it lives in.
        let condition_dir = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge")
            .join("req-todo")
            .join("todo")
            .join("todo-add-task")
            .join("todo-add-task-evil");
        fs::create_dir_all(&condition_dir).unwrap();
        fs::write(
            condition_dir.join("condition.yml"),
            "id: ../../../../evil\nbehavior: todo-add-task\nlabel: evil\ndescription: |\n  Evil.\n",
        )
        .unwrap();
        write_expected(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "todo-add-task-evil",
            "001",
            "todo-add-task-evil-001",
            "Shows a validation error.",
        );

        let result = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        );

        assert!(
            result.is_err(),
            "expected an error for a condition.id containing path traversal, got: {result:?}"
        );
    }

    #[test]
    fn generates_empty_list_for_empty_knowledge_dir() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        let testcases = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();

        assert!(testcases.is_empty());
    }

    #[test]
    fn generates_single_testcase_aggregating_all_expected_files_under_one_condition() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui", "data"]);
        write_behavior(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "User adds a task.",
        );
        write_condition(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "Title is empty.",
        );
        write_expected(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "001",
            "todo-add-task-empty-input-001",
            "Shows a validation error.",
        );

        let testcases = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();

        assert_eq!(testcases.len(), 1);
        let tc = &testcases[0];
        assert_eq!(
            tc.case_id,
            "tc-req-todo-todo-todo-add-task-todo-add-task-empty-input"
        );
        assert_eq!(tc.generated_from.requirement, "req-todo");
        assert_eq!(tc.generated_from.feature, "todo");
        assert_eq!(tc.generated_from.behavior, "todo-add-task");
        assert_eq!(tc.generated_from.condition, "todo-add-task-empty-input");
        assert_eq!(
            tc.generated_from.expected_results,
            vec!["todo-add-task-empty-input-001".to_string()]
        );
        assert_eq!(tc.title, "Title is empty.\n");
        assert_eq!(tc.steps, vec!["User adds a task.\n".to_string()]);
        assert_eq!(tc.expected, vec!["Shows a validation error.\n".to_string()]);
    }

    #[test]
    fn aggregates_multiple_expected_files_into_a_single_testcase() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);
        write_behavior(
            dir.path(),
            "req-todo",
            "todo",
            "todo-complete-task",
            "User checks a task.",
        );
        write_condition(
            dir.path(),
            "req-todo",
            "todo",
            "todo-complete-task",
            "todo-complete-task-toggle-done",
            "Task is unchecked.",
        );
        write_expected(
            dir.path(),
            "req-todo",
            "todo",
            "todo-complete-task",
            "todo-complete-task-toggle-done",
            "001",
            "todo-complete-task-toggle-done-001",
            "Task becomes done.",
        );
        write_expected(
            dir.path(),
            "req-todo",
            "todo",
            "todo-complete-task",
            "todo-complete-task-toggle-done",
            "002",
            "todo-complete-task-toggle-done-002",
            "completedAt is recorded.",
        );

        let testcases = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();

        assert_eq!(testcases.len(), 1);
        let tc = &testcases[0];
        assert_eq!(
            tc.case_id,
            "tc-req-todo-todo-todo-complete-task-todo-complete-task-toggle-done"
        );
        assert_eq!(
            tc.generated_from.expected_results,
            vec![
                "todo-complete-task-toggle-done-001".to_string(),
                "todo-complete-task-toggle-done-002".to_string(),
            ]
        );
        assert_eq!(
            tc.expected,
            vec![
                "Task becomes done.\n".to_string(),
                "completedAt is recorded.\n".to_string(),
            ]
        );
    }

    #[test]
    fn sorts_testcases_by_case_id_across_multiple_features() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);
        write_behavior(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "User adds a task.",
        );
        write_condition(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "Title is empty.",
        );
        write_expected(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "001",
            "todo-add-task-empty-input-001",
            "Shows a validation error.",
        );

        write_requirement(dir.path(), "req-enemy", &["combat"]);
        write_feature(dir.path(), "req-enemy", "enemy", &["combat"]);
        write_behavior(
            dir.path(),
            "req-enemy",
            "enemy",
            "enemy-attack",
            "Enemy attacks.",
        );
        write_condition(
            dir.path(),
            "req-enemy",
            "enemy",
            "enemy-attack",
            "enemy-attack-melee-range",
            "Enemy is in melee range.",
        );
        write_expected(
            dir.path(),
            "req-enemy",
            "enemy",
            "enemy-attack",
            "enemy-attack-melee-range",
            "001",
            "enemy-attack-melee-range-001",
            "Deals damage.",
        );

        let testcases = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();

        assert_eq!(testcases.len(), 2);
        assert_eq!(
            testcases[0].case_id,
            "tc-req-enemy-enemy-enemy-attack-enemy-attack-melee-range"
        );
        assert_eq!(
            testcases[1].case_id,
            "tc-req-todo-todo-todo-add-task-todo-add-task-empty-input"
        );
    }

    #[test]
    fn produces_no_testcase_for_condition_without_expected_files() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);
        write_behavior(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "User adds a task.",
        );
        write_condition(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "Title is empty.",
        );

        let testcases = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();

        assert!(testcases.is_empty());
    }

    #[test]
    fn produces_no_testcase_for_feature_without_behavior() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);

        let testcases = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();

        assert!(testcases.is_empty());
    }

    #[test]
    fn generate_is_deterministic_across_repeated_runs() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);
        write_behavior(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "User adds a task.",
        );
        write_condition(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "Title is empty.",
        );
        write_expected(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "001",
            "todo-add-task-empty-input-001",
            "Shows a validation error.",
        );

        let first: Vec<String> = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap()
        .iter()
        .map(serialize_testcase)
        .collect();
        let second: Vec<String> = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap()
        .iter()
        .map(serialize_testcase)
        .collect();

        assert_eq!(first, second);
    }

    #[test]
    fn serialized_testcase_contains_no_leading_comment() {
        let testcase = TestCase {
            case_id: "tc-req-todo-todo-todo-add-task-todo-add-task-empty-input".to_string(),
            generated_from: GeneratedFrom {
                requirement: "req-todo".to_string(),
                feature: "todo".to_string(),
                behavior: "todo-add-task".to_string(),
                condition: "todo-add-task-empty-input".to_string(),
                expected_results: vec!["todo-add-task-empty-input-001".to_string()],
            },
            title: "Title is empty.".to_string(),
            steps: vec!["User adds a task.".to_string()],
            expected: vec!["Shows a validation error.".to_string()],
            axis: vec!["ui".to_string()],
        };

        let yaml = serialize_testcase(&testcase);

        assert!(!yaml.starts_with('#'));
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(
            parsed["case_id"].as_str(),
            Some("tc-req-todo-todo-todo-add-task-todo-add-task-empty-input")
        );
        assert_eq!(parsed["generated_from"]["feature"].as_str(), Some("todo"));
    }

    #[test]
    fn testcase_axis_is_union_of_requirement_feature_and_behavior_axis() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security", "ui"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui", "data"]);
        write_behavior(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "User adds a task.",
        );
        write_condition(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "Title is empty.",
        );
        write_expected(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "001",
            "todo-add-task-empty-input-001",
            "Shows a validation error.",
        );

        let testcases = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();

        // write_behavior always uses axis [ui]; combined with requirement's
        // [security, ui] and feature's [ui, data], duplicates must collapse.
        assert_eq!(
            testcases[0].axis,
            vec!["data".to_string(), "security".to_string(), "ui".to_string()]
        );
    }

    #[test]
    fn relative_path_mirrors_the_requirement_feature_behavior_condition_hierarchy() {
        let testcase = TestCase {
            case_id: "tc-req-todo-todo-todo-add-task-todo-add-task-empty-input".to_string(),
            generated_from: GeneratedFrom {
                requirement: "req-todo".to_string(),
                feature: "todo".to_string(),
                behavior: "todo-add-task".to_string(),
                condition: "todo-add-task-empty-input".to_string(),
                expected_results: vec!["todo-add-task-empty-input-001".to_string()],
            },
            title: "Title is empty.".to_string(),
            steps: vec!["User adds a task.".to_string()],
            expected: vec!["Shows a validation error.".to_string()],
            axis: vec!["ui".to_string()],
        };

        assert_eq!(
            testcase.relative_path(),
            Path::new("req-todo")
                .join("todo")
                .join("todo-add-task")
                .join("todo-add-task-empty-input.yml")
        );
    }

    #[test]
    fn generate_testcases_rejects_a_requirement_with_path_traversal_id() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        let requirement_dir = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge")
            .join("req-todo");
        fs::create_dir_all(&requirement_dir).unwrap();
        fs::write(
            requirement_dir.join("requirement.yml"),
            "id: ../../../../evil\nlabel: evil\naxis: []\n",
        )
        .unwrap();

        let result = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        );

        assert!(
            result.is_err(),
            "expected an error for a requirement.id containing path traversal, got: {result:?}"
        );
    }

    #[test]
    fn generate_testcases_rejects_a_feature_with_path_traversal_id() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        let feature_dir = dir.path().join(".markharness/knowledge/req-todo/todo");
        fs::create_dir_all(&feature_dir).unwrap();
        fs::write(
            feature_dir.join("feature.yml"),
            "id: ../../../../evil\nrequirement: req-todo\nlabel: evil\naxis: []\n",
        )
        .unwrap();

        let result = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        );

        assert!(
            result.is_err(),
            "expected an error for a feature.id containing path traversal, got: {result:?}"
        );
    }

    #[test]
    fn generate_testcases_rejects_a_behavior_with_path_traversal_id() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);
        let behavior_dir = dir
            .path()
            .join(".markharness/knowledge/req-todo/todo/todo-add-task");
        fs::create_dir_all(&behavior_dir).unwrap();
        fs::write(
            behavior_dir.join("behavior.yml"),
            "id: ../../../../evil\nfeature: todo\nlabel: evil\naxis: []\ndescription: |\n  Evil.\n",
        )
        .unwrap();

        let result = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        );

        assert!(
            result.is_err(),
            "expected an error for a behavior.id containing path traversal, got: {result:?}"
        );
    }
}
