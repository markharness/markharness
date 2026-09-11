use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::identity::{CaseRevision, CaseUid};
use crate::knowledge::{
    Behavior, Procedure, Scenario, StepItem, is_valid_slug, parse_behavior, parse_feature,
    parse_requirement, parse_scenario,
};

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct GeneratedFrom {
    /// ADR 0017 §1: informational provenance only (a Feature associates
    /// with one or more Requirements via `requirement_uids`, not one owning
    /// parent) — never part of case identity or axis. Human-readable
    /// display IDs, resolved fresh from each Requirement's current `id` at
    /// generation time (never copied stale from `requirement_uids`).
    pub requirement_ids: Vec<String>,
    /// The same relationship as `requirement_ids`, by immutable
    /// `Requirement.uid` (ADR 0013). Informational only, mirroring
    /// `feature_uid` below: never part of case identity or axis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requirement_uids: Option<Vec<String>>,
    pub feature: String,
    /// The Feature's immutable identity (ADR 0013), when it has one.
    /// `None` for a Feature that has not been migrated. Informational only:
    /// `TestCase.case_uid` (ADR 0017 §3) is derived from `ScenarioUid`
    /// alone, not from this value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_uid: Option<String>,
    pub behavior: String,
    pub scenario: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct TestCase {
    pub case_id: String,
    /// ADR 0017 §3: derived from `ScenarioUid` alone once the Scenario has
    /// one (`identity::derived_uid::case_uid`). `None` before the Scenario
    /// has been migrated, or for a project that hasn't adopted the identity
    /// model at all — never computed from a substitute value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_uid: Option<CaseUid>,
    /// ADR 0017 §3: deterministically derived from the canonical encoding
    /// of `phases` alone (`identity::derived_uid::case_revision`) — never
    /// from `case_uid` or any display-only field. Unlike `case_uid`, always
    /// present: it needs no Scenario identity, only the already-expanded
    /// Phase content every generated TestCase already has.
    pub case_revision: CaseRevision,
    /// The repo-relative paths of this case's contributing files —
    /// `feature.yml`, `behavior.yml`, `scenario.yml` — used by
    /// `identity::migration_manifest` to build this case's
    /// `LegacyElementLocator`s. Not serialized into
    /// `generated/testcases/*.yml`: it is migration-manifest plumbing, not
    /// part of the TestCase contract consumers read.
    #[serde(skip)]
    pub case_files: CaseFilePaths,
    pub generated_from: GeneratedFrom,
    /// ADR 0017 §2: `Scenario.phases`のPhase配列。`use:`参照は所属Behaviorの
    /// `procedures`へ展開済み(操作列はすべて具体的な文字列)。実行順の正本は
    /// 配列順。
    pub phases: Vec<Phase>,
    /// Feature/Behavior の axis を合成(union)したもの。決定性のため重複除去の
    /// うえソートする。ADR 0017 §1: Requirement の axis は自動継承しない
    /// (Requirement を軸にした検索は`requirement_ids`関連をたどって行う)。
    pub axis: Vec<String>,
}

/// ADR 0017 §2: Scenarioの1つのPhaseに対応する、展開済みの操作・確認の単位。
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Phase {
    pub steps: Vec<String>,
    pub results: Vec<String>,
}

/// Repo-relative (forward-slash, `.markharness/knowledge/...`-prefixed)
/// paths of a case's contributing files, in the same nesting order
/// `knowledge/` itself uses. See `TestCase::case_files`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaseFilePaths {
    pub feature: String,
    pub behavior: String,
    pub scenario: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeCaseSnapshot {
    /// ADR 0017 §1: informational only, carried through to
    /// `GeneratedFrom::requirement_ids`. Never part of case identity or
    /// `axis` (no automatic Requirement-axis inheritance).
    pub requirement_ids: Vec<String>,
    /// ADR 0017 §1・§3: the Feature's `requirement_uids` as-is, carried
    /// through to `GeneratedFrom::requirement_uids`. Informational only.
    pub requirement_uids: Vec<String>,
    pub feature_id: String,
    pub feature_uid: Option<String>,
    pub feature_axis: Vec<String>,
    pub behavior_id: String,
    pub behavior_axis: Vec<String>,
    pub scenario_id: String,
    pub scenario_uid: Option<String>,
    /// Already expanded: every `use:` step replaced by its Procedure's
    /// steps (`expand_phases`).
    pub phases: Vec<Phase>,
    /// See `TestCase::case_files`.
    pub case_files: CaseFilePaths,
}

/// Derives `case_uid` from `ScenarioUid` alone (ADR 0017 §3), or `None` if
/// the Scenario hasn't been migrated yet — never computed from a
/// substitute value.
fn compute_case_uid(case: &KnowledgeCaseSnapshot) -> Option<CaseUid> {
    let scenario_uid = case.scenario_uid.as_deref()?;
    Some(
        CaseUid::new(crate::identity::derived_uid::case_uid(scenario_uid))
            .expect("derived_uid::case_uid always formats a 36-character, non-blank UUID string"),
    )
}

/// Derives `case_revision` (ADR 0017 §3) from `phases` alone: the effective
/// input a TestCase's revision tracks (operations, expected results, and
/// their order — already procedure-expanded by `expand_phases`). `Phase`'s
/// only fields are `steps`/`results`, so this JSON encoding excludes every
/// display-only field (label, description, source) by construction — there
/// is nothing else on `Phase` a canonicalization step could leak in.
/// `serde_json` orders struct fields by declaration, not alphabetically, so
/// this is deterministic across runs without a separate canonicalization
/// pass.
///
/// ADR §3 also lists setup operations/preconditions and test data as
/// effective inputs a revision must track. Neither has a separate field in
/// this schema to have been left out: a precondition is an ordinary
/// `Phase.steps` entry (directly, or expanded from a `use:`-referenced
/// `Behavior.procedures` entry), and test data is literal text embedded in
/// a step's `action` or in `Phase.results` — both already inside `phases`
/// by the time this function runs. See `identity::derived_uid::case_revision`'s
/// doc comment for the full accounting, and this module's `case_revision_*`
/// tests for concrete coverage.
pub(crate) fn compute_case_revision(phases: &[Phase]) -> CaseRevision {
    let canonical =
        serde_json::to_string(phases).expect("Phase is plain data; serialization is infallible");
    CaseRevision::new(crate::identity::derived_uid::case_revision(&canonical))
        .expect("derived_uid::case_revision always formats a non-blank hash string")
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
    /// written to: `{feature}/{behavior}/{scenario}.yml`, mirroring
    /// `knowledge/features/`'s own hierarchy (ADR 0017 §1: Feature is a
    /// top-level, globally-unique-id directory, no longer nested under a
    /// single owning Requirement). Because this mirrors a tree that is
    /// itself collision-free (two Scenarios cannot occupy the same
    /// `knowledge/features/<feature>/<behavior>/<scenario>.yml` path), no
    /// two TestCases can ever be written to the same path.
    pub fn relative_path(&self) -> PathBuf {
        Path::new(&self.generated_from.feature)
            .join(&self.generated_from.behavior)
            .join(format!("{}.yml", self.generated_from.scenario))
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

/// Rejects an id (feature/behavior/scenario) that isn't a plain slug before
/// it can become a path component of `case_id` or of
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

/// `path`, made repo-relative and forward-slash-normalized (`Path::join`'s
/// platform separator is unsuitable for a git pathspec — see
/// `project_root::KNOWLEDGE_PATH_IN_REPO`'s own doc comment), under the
/// convention every caller of `load_knowledge_snapshot` already relies on:
/// `knowledge_root` is always `<project_root>/.markharness/knowledge`.
fn repo_relative_path(knowledge_root: &Path, path: &Path) -> String {
    let relative = path
        .strip_prefix(knowledge_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    format!("{}/{relative}", crate::project_root::KNOWLEDGE_PATH_IN_REPO)
}

/// Expands one Phase step (ADR 0017 §2): `action` passes through unchanged,
/// `use` resolves to its named Procedure's steps within the owning
/// Behavior. A `use` naming a Procedure the Behavior does not declare is an
/// explicit, rejected error — never silently dropped or left unexpanded.
fn expand_step_item(
    item: &StepItem,
    procedures: &BTreeMap<String, Procedure>,
    source_path: &Path,
) -> io::Result<Vec<String>> {
    match item {
        StepItem::Action { action } => Ok(vec![action.clone()]),
        StepItem::Use { procedure } => procedures
            .get(procedure)
            .map(|p| p.steps.clone())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "{}: phase step references unknown procedure \"{procedure}\"",
                        source_path.display()
                    ),
                )
            }),
    }
}

/// Expands every Phase of `scenario` against `behavior`'s procedures
/// (ADR 0017 §2). An empty `phases` array is an explicit, rejected error
/// (a Scenario with no phases cannot be a TestCase), matching the missing
/// `procedures` reference check above.
fn expand_phases(
    scenario: &Scenario,
    behavior: &Behavior,
    source_path: &Path,
) -> io::Result<Vec<Phase>> {
    if scenario.phases.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: scenario has no phases", source_path.display()),
        ));
    }
    scenario
        .phases
        .iter()
        .map(|phase| {
            let mut steps = Vec::new();
            for item in &phase.steps {
                steps.extend(expand_step_item(item, &behavior.procedures, source_path)?);
            }
            Ok(Phase {
                steps,
                results: phase.results.clone(),
            })
        })
        .collect()
}

/// Maps every migrated Requirement's `uid` to its current display `id`
/// (ADR 0017 §1・§3: a Feature's `GeneratedFrom.requirement_ids` must show
/// the Requirement's *current* display id, never a stale copy).
fn load_requirement_uid_index(knowledge_root: &Path) -> io::Result<BTreeMap<String, String>> {
    let mut index = BTreeMap::new();
    for requirement_dir in sorted_subdirs(&knowledge_root.join("requirements"))? {
        let requirement_path = requirement_dir.join("requirement.yml");
        if !requirement_path.is_file() {
            continue;
        }
        let yaml = fs::read_to_string(&requirement_path)?;
        let requirement =
            parse_requirement(&yaml).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if let Some(uid) = requirement.uid {
            index.insert(uid, requirement.id);
        }
    }
    Ok(index)
}

pub fn load_knowledge_snapshot(knowledge_root: &Path) -> io::Result<KnowledgeSnapshot> {
    let mut cases = Vec::new();
    let requirement_uid_index = load_requirement_uid_index(knowledge_root)?;

    for feature_dir in sorted_subdirs(&knowledge_root.join("features"))? {
        let feature_path = feature_dir.join("feature.yml");
        if !feature_path.is_file() {
            continue;
        }
        let feature_yaml = fs::read_to_string(&feature_path)?;
        let feature = parse_feature(&feature_yaml)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        require_valid_slug(&feature_path, "feature", &feature.id)?;

        for behavior_dir in find_dirs_with_marker(&feature_dir, "behavior.yml")? {
            let behavior_path = behavior_dir.join("behavior.yml");
            let behavior_yaml = fs::read_to_string(&behavior_path)?;
            let behavior = parse_behavior(&behavior_yaml)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            require_valid_slug(&behavior_path, "behavior", &behavior.id)?;

            for scenario_dir in find_dirs_with_marker(&behavior_dir, "scenario.yml")? {
                let scenario_path = scenario_dir.join("scenario.yml");
                let scenario_yaml = fs::read_to_string(&scenario_path)?;
                let scenario = parse_scenario(&scenario_yaml)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                require_valid_slug(&scenario_path, "scenario", &scenario.id)?;

                let phases = expand_phases(&scenario, &behavior, &scenario_path)?;
                let case_files = CaseFilePaths {
                    feature: repo_relative_path(knowledge_root, &feature_path),
                    behavior: repo_relative_path(knowledge_root, &behavior_path),
                    scenario: repo_relative_path(knowledge_root, &scenario_path),
                };

                // Informational only (ADR 0017 §1・§3): never part of case
                // identity or axis, so an entry that doesn't resolve to a
                // known Requirement — pre-migration, `requirement_uids` may
                // still hold a Requirement *display id* rather than its
                // uid (Issue #44) — falls back to the raw stored value
                // rather than failing generation entirely.
                let requirement_ids = feature
                    .requirement_uids
                    .iter()
                    .map(|uid| {
                        requirement_uid_index
                            .get(uid)
                            .cloned()
                            .unwrap_or_else(|| uid.clone())
                    })
                    .collect::<Vec<String>>();

                cases.push(KnowledgeCaseSnapshot {
                    requirement_ids,
                    requirement_uids: feature.requirement_uids.clone(),
                    feature_id: feature.id.clone(),
                    feature_uid: feature.uid.clone(),
                    feature_axis: feature.axis.clone(),
                    behavior_id: behavior.id.clone(),
                    behavior_axis: behavior.axis.clone(),
                    scenario_id: scenario.id,
                    scenario_uid: scenario.uid,
                    phases,
                    case_files,
                });
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
                "tc-{}-{}-{}",
                case.feature_id, case.behavior_id, case.scenario_id
            ),
            case_uid: compute_case_uid(case),
            case_revision: compute_case_revision(&case.phases),
            case_files: case.case_files.clone(),
            generated_from: GeneratedFrom {
                requirement_ids: case.requirement_ids.clone(),
                requirement_uids: Some(case.requirement_uids.clone()),
                feature: case.feature_id.clone(),
                feature_uid: case.feature_uid.clone(),
                behavior: case.behavior_id.clone(),
                scenario: case.scenario_id.clone(),
            },
            phases: case.phases.clone(),
            axis: union_axis(&[&case.feature_axis, &case.behavior_axis]),
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
            .join("knowledge/requirements")
            .join(requirement);
        fs::create_dir_all(&dir).unwrap();
        let axis_line = axis.join(", ");
        fs::write(
            dir.join("requirement.yml"),
            format!(
                "id: {requirement}\nsource: native\nlabel: {requirement}\naxis: [{axis_line}]\n"
            ),
        )
        .unwrap();
    }

    /// ADR 0017 §1: Feature is stored under `knowledge/features/<feature>`,
    /// independent of any Requirement directory; `requirement` only
    /// contributes to `requirement_ids:`.
    fn write_feature(root: &std::path::Path, requirement: &str, feature: &str, axis: &[&str]) {
        let dir = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge/features")
            .join(feature);
        fs::create_dir_all(&dir).unwrap();
        let axis_line = axis.join(", ");
        fs::write(
            dir.join("feature.yml"),
            format!(
                "id: {feature}\nrequirement_uids: [{requirement}]\nlabel: {feature}\naxis: [{axis_line}]\n"
            ),
        )
        .unwrap();
    }

    /// Writes a Behavior with no common procedures.
    fn write_behavior(root: &std::path::Path, feature: &str, behavior: &str, description: &str) {
        let dir = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge/features")
            .join(feature)
            .join(behavior);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("behavior.yml"),
            format!(
                "id: {behavior}\nfeature: {feature}\nlabel: {behavior}\naxis: [ui]\ndescription: |\n  {description}\nprocedures: {{}}\n"
            ),
        )
        .unwrap();
    }

    /// Writes a Behavior declaring one common procedure named `login`.
    fn write_behavior_with_login_procedure(
        root: &std::path::Path,
        feature: &str,
        behavior: &str,
        description: &str,
        procedure_steps: &[&str],
    ) {
        let dir = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge/features")
            .join(feature)
            .join(behavior);
        fs::create_dir_all(&dir).unwrap();
        let steps_block: String = procedure_steps
            .iter()
            .map(|step| format!("      - {}\n", serde_json::to_string(step).unwrap()))
            .collect();
        fs::write(
            dir.join("behavior.yml"),
            format!(
                "id: {behavior}\nfeature: {feature}\nlabel: {behavior}\naxis: [ui]\ndescription: |\n  {description}\nprocedures:\n  login:\n    steps:\n{steps_block}"
            ),
        )
        .unwrap();
    }

    /// Writes a Scenario with one Phase per `(steps, results)` pair, each
    /// step a plain `action:` (no `use:` reference). Lives in its own
    /// subdirectory under the Behavior (`behavior_dir/<scenario>/scenario.yml`),
    /// mirroring the pre-ADR-0017 Condition layout — `id_cache.rs`'s
    /// fixed-depth `feature_dir_of_scenario_dir` relies on this.
    fn write_scenario(
        root: &std::path::Path,
        feature: &str,
        behavior: &str,
        scenario: &str,
        description: &str,
        phases: &[(&[&str], &[&str])],
    ) {
        let dir = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge/features")
            .join(feature)
            .join(behavior)
            .join(scenario);
        fs::create_dir_all(&dir).unwrap();
        let mut phases_block = String::new();
        for (steps, results) in phases {
            phases_block.push_str("  - steps:\n");
            for step in *steps {
                phases_block.push_str(&format!(
                    "      - action: {}\n",
                    serde_json::to_string(step).unwrap()
                ));
            }
            phases_block.push_str("    results:\n");
            for result in *results {
                phases_block.push_str(&format!(
                    "      - {}\n",
                    serde_json::to_string(result).unwrap()
                ));
            }
        }
        fs::write(
            dir.join("scenario.yml"),
            format!(
                "id: {scenario}\nbehavior: {behavior}\nlabel: {scenario}\ndescription: |\n  {description}\nphases:\n{phases_block}"
            ),
        )
        .unwrap();
    }

    /// Writes a Scenario with one Phase whose steps are exactly `steps_yaml`
    /// (already-formatted `steps:` list items), for tests that need a
    /// `use:` reference.
    fn write_scenario_with_raw_steps(
        root: &std::path::Path,
        feature: &str,
        behavior: &str,
        scenario: &str,
        description: &str,
        steps_yaml: &str,
        results: &[&str],
    ) {
        let dir = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge/features")
            .join(feature)
            .join(behavior)
            .join(scenario);
        fs::create_dir_all(&dir).unwrap();
        let results_block: String = results
            .iter()
            .map(|result| format!("      - {}\n", serde_json::to_string(result).unwrap()))
            .collect();
        fs::write(
            dir.join("scenario.yml"),
            format!(
                "id: {scenario}\nbehavior: {behavior}\nlabel: {scenario}\ndescription: |\n  {description}\nphases:\n  - steps:\n{steps_yaml}    results:\n{results_block}"
            ),
        )
        .unwrap();
    }

    #[test]
    fn rejects_scenario_with_path_traversal_id() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);
        write_behavior(dir.path(), "todo", "todo-add-task", "User adds a task.");
        // The directory name is a safe slug, but a malicious repository can
        // still craft the `id:` field inside scenario.yml independently of
        // the directory it lives in.
        let scenario_dir = dir
            .path()
            .join(".markharness/knowledge/features/todo/todo-add-task/todo-add-task-evil");
        fs::create_dir_all(&scenario_dir).unwrap();
        fs::write(
            scenario_dir.join("scenario.yml"),
            "id: ../../../../evil\nbehavior: todo-add-task\nlabel: evil\ndescription: |\n  Evil.\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"Shows a validation error.\"\n",
        )
        .unwrap();

        let result = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        );

        assert!(
            result.is_err(),
            "expected an error for a scenario.id containing path traversal, got: {result:?}"
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
    fn generates_single_testcase_for_one_scenario() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui", "data"]);
        write_behavior(dir.path(), "todo", "todo-add-task", "User adds a task.");
        write_scenario(
            dir.path(),
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "Title is empty.",
            &[(
                &["Click the title field.", "Press the add button."],
                &["Shows a validation error."],
            )],
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
            "tc-todo-todo-add-task-todo-add-task-empty-input"
        );
        assert_eq!(
            tc.generated_from.requirement_ids,
            vec!["req-todo".to_string()]
        );
        assert_eq!(tc.generated_from.feature, "todo");
        assert_eq!(tc.generated_from.behavior, "todo-add-task");
        assert_eq!(tc.generated_from.scenario, "todo-add-task-empty-input");
        assert_eq!(
            tc.phases,
            vec![Phase {
                steps: vec![
                    "Click the title field.".to_string(),
                    "Press the add button.".to_string()
                ],
                results: vec!["Shows a validation error.".to_string()],
            }]
        );
    }

    #[test]
    fn generates_a_testcase_with_multiple_phases_in_order() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);
        write_behavior(
            dir.path(),
            "todo",
            "todo-complete-task",
            "User checks a task.",
        );
        write_scenario(
            dir.path(),
            "todo",
            "todo-complete-task",
            "todo-complete-task-toggle-done",
            "Task is unchecked.",
            &[
                (&["Press the checkbox."], &["Task becomes done."]),
                (&["Reload the page."], &["completedAt is recorded."]),
            ],
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
            tc.phases,
            vec![
                Phase {
                    steps: vec!["Press the checkbox.".to_string()],
                    results: vec!["Task becomes done.".to_string()],
                },
                Phase {
                    steps: vec!["Reload the page.".to_string()],
                    results: vec!["completedAt is recorded.".to_string()],
                },
            ]
        );
    }

    /// ADR 0017 §2: a Phase step may `use:` a common procedure declared by
    /// the owning Behavior; generation expands it inline.
    #[test]
    fn expands_a_use_step_into_its_procedures_steps() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-shop", &["security"]);
        write_feature(dir.path(), "req-shop", "checkout", &["ui"]);
        write_behavior_with_login_procedure(
            dir.path(),
            "checkout",
            "checkout-pay",
            "Pay.",
            &["Enter credentials.", "Press the login button."],
        );
        write_scenario_with_raw_steps(
            dir.path(),
            "checkout",
            "checkout-pay",
            "checkout-pay-valid-card",
            "Pay then log out and back in.",
            "      - use: login\n      - action: \"Log out.\"\n      - use: login\n",
            &["My page is shown again."],
        );

        let testcases = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();

        assert_eq!(
            testcases[0].phases,
            vec![Phase {
                steps: vec![
                    "Enter credentials.".to_string(),
                    "Press the login button.".to_string(),
                    "Log out.".to_string(),
                    "Enter credentials.".to_string(),
                    "Press the login button.".to_string(),
                ],
                results: vec!["My page is shown again.".to_string()],
            }]
        );
    }

    /// ADR 0017 §2: a `use:` naming a Procedure the Behavior does not
    /// declare is an explicit, rejected error, not a silently-dropped step.
    #[test]
    fn rejects_a_use_step_referencing_an_unknown_procedure() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-shop", &["security"]);
        write_feature(dir.path(), "req-shop", "checkout", &["ui"]);
        write_behavior(dir.path(), "checkout", "checkout-pay", "Pay.");
        write_scenario_with_raw_steps(
            dir.path(),
            "checkout",
            "checkout-pay",
            "checkout-pay-valid-card",
            "Pay.",
            "      - use: login\n",
            &["Confirmed."],
        );

        let result = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        );

        assert!(
            result.is_err(),
            "expected an error for a use: step referencing an unknown procedure, got: {result:?}"
        );
    }

    /// ADR 0017 §2: an empty `phases` array is an explicit, rejected error
    /// — a Scenario with no phases cannot be a TestCase.
    #[test]
    fn rejects_a_scenario_with_an_empty_phases_array() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);
        write_behavior(dir.path(), "todo", "todo-add-task", "User adds a task.");
        let scenario_dir = dir
            .path()
            .join(".markharness/knowledge/features/todo/todo-add-task/todo-add-task-empty-input");
        fs::create_dir_all(&scenario_dir).unwrap();
        fs::write(
            scenario_dir.join("scenario.yml"),
            "id: todo-add-task-empty-input\nbehavior: todo-add-task\nlabel: todo-add-task-empty-input\ndescription: |\n  Title is empty.\nphases: []\n",
        )
        .unwrap();

        let result = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        );

        assert!(
            result.is_err(),
            "expected an error for a scenario with an empty phases array, got: {result:?}"
        );
    }

    #[test]
    fn sorts_testcases_by_case_id_across_multiple_features() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);
        write_behavior(dir.path(), "todo", "todo-add-task", "User adds a task.");
        write_scenario(
            dir.path(),
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "Title is empty.",
            &[(&["Do it."], &["Shows a validation error."])],
        );

        write_requirement(dir.path(), "req-enemy", &["combat"]);
        write_feature(dir.path(), "req-enemy", "enemy", &["combat"]);
        write_behavior(dir.path(), "enemy", "enemy-attack", "Enemy attacks.");
        write_scenario(
            dir.path(),
            "enemy",
            "enemy-attack",
            "enemy-attack-melee-range",
            "Enemy is in melee range.",
            &[(&["Do it."], &["Deals damage."])],
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
            "tc-enemy-enemy-attack-enemy-attack-melee-range"
        );
        assert_eq!(
            testcases[1].case_id,
            "tc-todo-todo-add-task-todo-add-task-empty-input"
        );
    }

    #[test]
    fn produces_no_testcase_for_behavior_without_scenario() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);
        write_behavior(dir.path(), "todo", "todo-add-task", "User adds a task.");

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
        write_behavior(dir.path(), "todo", "todo-add-task", "User adds a task.");
        write_scenario(
            dir.path(),
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "Title is empty.",
            &[(&["Do it."], &["Shows a validation error."])],
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
            case_id: "tc-todo-todo-add-task-todo-add-task-empty-input".to_string(),
            case_uid: None,
            case_revision: CaseRevision::new("test-revision").unwrap(),
            case_files: CaseFilePaths::default(),
            generated_from: GeneratedFrom {
                requirement_ids: vec!["req-todo".to_string()],
                requirement_uids: None,
                feature: "todo".to_string(),
                feature_uid: None,
                behavior: "todo-add-task".to_string(),
                scenario: "todo-add-task-empty-input".to_string(),
            },
            phases: vec![Phase {
                steps: vec!["Do it.".to_string()],
                results: vec!["Shows a validation error.".to_string()],
            }],
            axis: vec!["ui".to_string()],
        };

        let yaml = serialize_testcase(&testcase);

        assert!(!yaml.starts_with('#'));
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(
            parsed["case_id"].as_str(),
            Some("tc-todo-todo-add-task-todo-add-task-empty-input")
        );
        assert_eq!(parsed["generated_from"]["feature"].as_str(), Some("todo"));
    }

    /// ADR 0013: a Feature's `uid` must propagate into
    /// `generated_from.feature_uid`, so a consumer of the generated
    /// TestCase can correlate it with the Feature's identity, not just its
    /// current `id`.
    #[test]
    fn generated_from_carries_the_feature_uid_when_the_feature_has_one() {
        const UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        let feature_dir = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge/features/todo");
        fs::create_dir_all(&feature_dir).unwrap();
        fs::write(
            feature_dir.join("feature.yml"),
            format!("id: todo\nrequirement_uids: [req-todo]\nlabel: todo\naxis: []\nuid: {UID}\n"),
        )
        .unwrap();
        write_behavior(dir.path(), "todo", "todo-add-task", "User adds a task.");
        write_scenario(
            dir.path(),
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "Title is empty.",
            &[(&["Do it."], &["Shows a validation error."])],
        );

        let testcases = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();

        assert_eq!(
            testcases[0].generated_from.feature_uid.as_deref(),
            Some(UID)
        );
    }

    const SCENARIO_UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FS0";

    /// ADR 0017 §3: `case_uid` is computed deterministically from
    /// `ScenarioUid` alone (the same value `identity::derived_uid::case_uid`
    /// alone would produce, not some ad-hoc recombination).
    #[test]
    fn case_uid_is_computed_once_the_scenario_has_a_uid() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);
        write_behavior(dir.path(), "todo", "todo-add-task", "User adds a task.");
        let scenario_dir = dir
            .path()
            .join(".markharness/knowledge/features/todo/todo-add-task/todo-add-task-empty-input");
        fs::create_dir_all(&scenario_dir).unwrap();
        fs::write(
            scenario_dir.join("scenario.yml"),
            format!(
                "id: todo-add-task-empty-input\nbehavior: todo-add-task\nlabel: todo-add-task-empty-input\ndescription: |\n  Title is empty.\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"Confirmed.\"\nuid: {SCENARIO_UID}\n"
            ),
        )
        .unwrap();

        let testcases = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();

        let expected = CaseUid::new(crate::identity::derived_uid::case_uid(SCENARIO_UID)).unwrap();
        assert_eq!(testcases[0].case_uid, Some(expected));
    }

    /// A `case_uid` is never computed from a substitute value: an
    /// unmigrated Scenario (no `uid:`) leaves `case_uid` as `None`.
    #[test]
    fn case_uid_stays_none_when_the_scenario_lacks_a_uid() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);
        write_behavior(dir.path(), "todo", "todo-add-task", "User adds a task.");
        write_scenario(
            dir.path(),
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "Title is empty.",
            &[(&["Do it."], &["Confirmed."])],
        );

        let testcases = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();

        assert_eq!(testcases[0].case_uid, None);
    }

    /// ADR 0017 §3: a description-only edit (display text, not effective
    /// content) must leave `case_revision` unchanged.
    #[test]
    fn case_revision_is_stable_across_a_description_only_edit() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);
        write_behavior(dir.path(), "todo", "todo-add-task", "User adds a task.");
        write_scenario(
            dir.path(),
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "Title is empty.",
            &[(&["Do it."], &["Confirmed."])],
        );
        let knowledge_root = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge");
        let before = generate_testcases(&knowledge_root).unwrap()[0]
            .case_revision
            .clone();

        write_scenario(
            dir.path(),
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "Title is empty (reworded description).",
            &[(&["Do it."], &["Confirmed."])],
        );
        let after = generate_testcases(&knowledge_root).unwrap()[0]
            .case_revision
            .clone();

        assert_eq!(before, after);
    }

    /// ADR 0017 §3: an operation/step content edit must change
    /// `case_revision`.
    #[test]
    fn case_revision_changes_when_a_step_changes() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);
        write_behavior(dir.path(), "todo", "todo-add-task", "User adds a task.");
        write_scenario(
            dir.path(),
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "Title is empty.",
            &[(&["Do it."], &["Confirmed."])],
        );
        let knowledge_root = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge");
        let before = generate_testcases(&knowledge_root).unwrap()[0]
            .case_revision
            .clone();

        write_scenario(
            dir.path(),
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "Title is empty.",
            &[(&["Do it differently."], &["Confirmed."])],
        );
        let after = generate_testcases(&knowledge_root).unwrap()[0]
            .case_revision
            .clone();

        assert_ne!(before, after);
    }

    /// ADR 0017 §3: "テストデータ" has no field of its own in this schema —
    /// it is literal text embedded in a step's `action` (e.g. a specific
    /// card number). Editing just that embedded value, with the rest of the
    /// step's wording unchanged, must still change `case_revision`, since it
    /// is part of `Phase.steps`.
    #[test]
    fn case_revision_changes_when_test_data_embedded_in_a_step_changes() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-shop", &["security"]);
        write_feature(dir.path(), "req-shop", "checkout", &["ui"]);
        write_behavior(dir.path(), "checkout", "checkout-pay", "Pay by card.");
        write_scenario(
            dir.path(),
            "checkout",
            "checkout-pay",
            "checkout-pay-valid-card",
            "Pay with a valid card.",
            &[(
                &["Enter card number 4111-1111-1111-1111."],
                &["Payment succeeds."],
            )],
        );
        let knowledge_root = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge");
        let before = generate_testcases(&knowledge_root).unwrap()[0]
            .case_revision
            .clone();

        write_scenario(
            dir.path(),
            "checkout",
            "checkout-pay",
            "checkout-pay-valid-card",
            "Pay with a valid card.",
            &[(
                &["Enter card number 5555-5555-5555-4444."],
                &["Payment succeeds."],
            )],
        );
        let after = generate_testcases(&knowledge_root).unwrap()[0]
            .case_revision
            .clone();

        assert_ne!(
            before, after,
            "a change to test data embedded in a step must change case_revision"
        );
    }

    /// ADR 0017 §3: reordering Phases changes `case_revision` even when the
    /// same steps/results are present, since execution order is part of the
    /// effective content.
    #[test]
    fn case_revision_changes_when_phase_order_changes() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);
        write_behavior(
            dir.path(),
            "todo",
            "todo-complete-task",
            "User checks a task.",
        );
        write_scenario(
            dir.path(),
            "todo",
            "todo-complete-task",
            "todo-complete-task-toggle-done",
            "Task is unchecked.",
            &[
                (&["Press the checkbox."], &["Task becomes done."]),
                (&["Reload the page."], &["completedAt is recorded."]),
            ],
        );
        let knowledge_root = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge");
        let before = generate_testcases(&knowledge_root).unwrap()[0]
            .case_revision
            .clone();

        write_scenario(
            dir.path(),
            "todo",
            "todo-complete-task",
            "todo-complete-task-toggle-done",
            "Task is unchecked.",
            &[
                (&["Reload the page."], &["completedAt is recorded."]),
                (&["Press the checkbox."], &["Task becomes done."]),
            ],
        );
        let after = generate_testcases(&knowledge_root).unwrap()[0]
            .case_revision
            .clone();

        assert_ne!(before, after);
    }

    /// ADR 0017 §3: an edit to a common procedure's steps must change
    /// `case_revision` for every Scenario that references it via `use:`,
    /// even though the Scenario's own `scenario.yml` bytes are untouched.
    #[test]
    fn case_revision_changes_when_a_referenced_procedures_steps_change() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-shop", &["security"]);
        write_feature(dir.path(), "req-shop", "checkout", &["ui"]);
        write_behavior_with_login_procedure(
            dir.path(),
            "checkout",
            "checkout-pay",
            "Pay.",
            &["Enter credentials.", "Press the login button."],
        );
        write_scenario_with_raw_steps(
            dir.path(),
            "checkout",
            "checkout-pay",
            "checkout-pay-valid-card",
            "Pay.",
            "      - use: login\n",
            &["Confirmed."],
        );
        let knowledge_root = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge");
        let before = generate_testcases(&knowledge_root).unwrap()[0]
            .case_revision
            .clone();

        write_behavior_with_login_procedure(
            dir.path(),
            "checkout",
            "checkout-pay",
            "Pay.",
            &[
                "Enter credentials.",
                "Press the login button.",
                "Press remember me.",
            ],
        );
        let after = generate_testcases(&knowledge_root).unwrap()[0]
            .case_revision
            .clone();

        assert_ne!(before, after);
    }

    /// ADR 0017 §1: Requirement.axis is never automatically inherited into
    /// a case's `axis` — only Feature.axis and Behavior.axis contribute.
    #[test]
    fn testcase_axis_is_union_of_feature_and_behavior_axis_and_excludes_requirement_axis() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security", "ui"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui", "data"]);
        write_behavior(dir.path(), "todo", "todo-add-task", "User adds a task.");
        write_scenario(
            dir.path(),
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "Title is empty.",
            &[(&["Do it."], &["Shows a validation error."])],
        );

        let testcases = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();

        // write_behavior always uses axis [ui]; combined with feature's
        // [ui, data], duplicates collapse. Requirement's [security, ui] is
        // deliberately absent — "security" must not appear.
        assert_eq!(
            testcases[0].axis,
            vec!["data".to_string(), "ui".to_string()]
        );
    }

    #[test]
    fn relative_path_mirrors_the_feature_behavior_scenario_hierarchy() {
        let testcase = TestCase {
            case_id: "tc-todo-todo-add-task-todo-add-task-empty-input".to_string(),
            case_uid: None,
            case_revision: CaseRevision::new("test-revision").unwrap(),
            case_files: CaseFilePaths::default(),
            generated_from: GeneratedFrom {
                requirement_ids: vec!["req-todo".to_string()],
                requirement_uids: None,
                feature: "todo".to_string(),
                feature_uid: None,
                behavior: "todo-add-task".to_string(),
                scenario: "todo-add-task-empty-input".to_string(),
            },
            phases: vec![Phase {
                steps: vec!["Do it.".to_string()],
                results: vec!["Shows a validation error.".to_string()],
            }],
            axis: vec!["ui".to_string()],
        };

        assert_eq!(
            testcase.relative_path(),
            Path::new("todo")
                .join("todo-add-task")
                .join("todo-add-task-empty-input.yml")
        );
    }

    // A Requirement-id path-traversal test previously lived here.
    // `load_knowledge_snapshot` no longer reads `requirement.yml` at all
    // (ADR 0017 §1: Requirement is decoupled from case generation), so the
    // vulnerability this guarded against cannot occur through this code
    // path any more; `requirement.id`'s slug format is still enforced by
    // JSON Schema validation (`validate.rs`).

    #[test]
    fn generate_testcases_rejects_a_feature_with_path_traversal_id() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        let feature_dir = dir.path().join(".markharness/knowledge/features/todo");
        fs::create_dir_all(&feature_dir).unwrap();
        fs::write(
            feature_dir.join("feature.yml"),
            "id: ../../../../evil\nrequirement_uids: [req-todo]\nlabel: evil\naxis: []\n",
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
            .join(".markharness/knowledge/features/todo/todo-add-task");
        fs::create_dir_all(&behavior_dir).unwrap();
        fs::write(
            behavior_dir.join("behavior.yml"),
            "id: ../../../../evil\nfeature: todo\nlabel: evil\naxis: []\ndescription: |\n  Evil.\nprocedures: {}\n",
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
