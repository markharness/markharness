use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::fs_safety::replace_file;
use crate::id_cache;

/// Only the fields record_execution needs from a generated TestCase. The
/// filename under `generated/testcases/` is the condition id, not the
/// case_id (see `generate::TestCase::file_stem`), so matching case_id
/// requires reading each file's content rather than a filename lookup.
#[derive(Deserialize)]
struct MinimalTestCase {
    case_id: String,
    generated_from: MinimalGeneratedFrom,
}

#[derive(Deserialize)]
struct MinimalGeneratedFrom {
    feature: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionResult {
    Pass,
    Fail,
    Skip,
}

impl ExecutionResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExecutionResult::Pass => "pass",
            ExecutionResult::Fail => "fail",
            ExecutionResult::Skip => "skip",
        }
    }
}

pub struct RecordArgs<'a> {
    pub milestone: &'a str,
    pub case_id: &'a str,
    pub result: ExecutionResult,
    pub executor: &'a str,
    pub note: Option<&'a str>,
}

#[derive(Debug)]
pub enum RecordError {
    MilestoneNotFound,
    CaseNotFound,
    Io(io::Error),
}

impl From<io::Error> for RecordError {
    fn from(e: io::Error) -> Self {
        RecordError::Io(e)
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionEntry {
    pub case_id: String,
    pub result: String,
    pub executor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub executed_at: String,
    /// Feature identity -> directory tree SHA at `milestone`, for each
    /// Feature the TestCase's `generated_from.feature` names (§2.1 of the
    /// ChangeEvent連動仕様). The key is the Feature's `uid` (ADR 0013) when
    /// it has one at `milestone`, else its `feature_id` — matching
    /// `id_cache::identity_key`'s convention, so a later rename doesn't
    /// strand entries recorded before it. Filled in automatically by
    /// `record_execution`; absent on records made before this field
    /// existed (no retroactive backfill, per §6).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub verified_feature_tree_shas: BTreeMap<String, String>,
}

pub fn read_all_results(root: &Path) -> io::Result<Vec<ExecutionEntry>> {
    let executions = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("executions");
    let Ok(entries) = fs::read_dir(executions) else {
        return Ok(Vec::new());
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("results.yml"))
        .filter(|path| path.is_file())
        .collect();
    paths.sort();
    let mut results = Vec::new();
    for path in paths {
        let content = fs::read_to_string(path)?;
        let mut entries: Vec<ExecutionEntry> = serde_yaml_ng::from_str(&content)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        results.append(&mut entries);
    }
    results.sort_by(|a, b| {
        a.executed_at
            .cmp(&b.executed_at)
            .then(a.case_id.cmp(&b.case_id))
    });
    Ok(results)
}

/// Days since the Unix epoch (1970-01-01) to a (year, month, day) civil
/// date, per Howard Hinnant's `civil_from_days` algorithm (public domain,
/// http://howardhinnant.github.io/date_algorithms.html). Avoids pulling in
/// a date/time crate for a single UTC timestamp field.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Crate-visible so `identity::feature_ops` (and future identity-event
/// producers) can stamp `recorded_at` without duplicating this date math
/// or pulling in a date/time crate.
pub(crate) fn iso8601_utc_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before the Unix epoch")
        .as_secs() as i64;
    let days = secs.div_euclid(86400);
    let time_of_day = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    let (h, mi, s) = (
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60,
    );
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Finds the `generated/testcases/**/*.yml` file with this `case_id` (files
/// are nested `{requirement}/{feature}/{behavior}/{condition}.yml`, so the
/// filename alone can't be used — and even the full relative path is only a
/// mirror of `knowledge/`, not itself the identity being searched for).
fn find_testcase_by_case_id(root: &Path, case_id: &str) -> io::Result<Option<MinimalTestCase>> {
    let testcases_dir = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("generated")
        .join("testcases");
    for relative_path in crate::generate::list_files_recursive(&testcases_dir)? {
        if relative_path.extension().and_then(|e| e.to_str()) != Some("yml") {
            continue;
        }
        let content = fs::read_to_string(testcases_dir.join(&relative_path))?;
        if let Ok(testcase) = serde_yaml_ng::from_str::<MinimalTestCase>(&content)
            && testcase.case_id == case_id
        {
            return Ok(Some(testcase));
        }
    }
    Ok(None)
}

/// Resolves `feature_id`'s identity key (ADR 0013: `uid` when present, else
/// `feature_id` itself — `id_cache::identity_key`) and directory tree SHA at
/// `milestone`, or `None` if the Feature isn't found at that milestone tag
/// (kept out of the recorded map rather than failing the whole
/// `execution record`).
fn verified_feature_tree_sha(
    root: &Path,
    milestone: &str,
    feature_id: &str,
) -> io::Result<Option<(String, String)>> {
    let versions = id_cache::resolve_feature_versions(root, milestone, true)?;
    Ok(versions.into_iter().find(|v| v.id == feature_id).map(|v| {
        let key = id_cache::identity_key(&v);
        (key, v.tree_sha)
    }))
}

fn read_existing_entries(results_path: &Path) -> io::Result<Vec<ExecutionEntry>> {
    if !results_path.is_file() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(results_path)?;
    Ok(serde_yaml_ng::from_str(&content).unwrap_or_default())
}

pub fn record_execution(root: &Path, args: &RecordArgs) -> Result<(), RecordError> {
    let milestone_path = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("executions")
        .join(args.milestone)
        .join("milestone.yml");
    if !milestone_path.is_file() {
        return Err(RecordError::MilestoneNotFound);
    }

    let Some(testcase) = find_testcase_by_case_id(root, args.case_id)? else {
        return Err(RecordError::CaseNotFound);
    };

    let mut verified_feature_tree_shas = BTreeMap::new();
    if let Some((key, sha)) =
        verified_feature_tree_sha(root, args.milestone, &testcase.generated_from.feature)?
    {
        verified_feature_tree_shas.insert(key, sha);
    }

    let results_path = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("executions")
        .join(args.milestone)
        .join("results.yml");
    let mut entries = read_existing_entries(&results_path)?;
    entries.push(ExecutionEntry {
        case_id: args.case_id.to_string(),
        result: args.result.as_str().to_string(),
        executor: args.executor.to_string(),
        note: args.note.map(|n| n.to_string()),
        executed_at: iso8601_utc_now(),
        verified_feature_tree_shas,
    });

    let content = serde_yaml_ng::to_string(&entries)
        .expect("Vec<ExecutionEntry> serialization is infallible");
    replace_file(root, &results_path, content.as_bytes())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn record_execution_errors_when_milestone_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".markharness/generated/testcases")).unwrap();

        let args = RecordArgs {
            milestone: "m1",
            case_id: "tc-ground-001",
            result: ExecutionResult::Pass,
            executor: "yamada",
            note: None,
        };
        let result = record_execution(dir.path(), &args);

        assert!(matches!(result, Err(RecordError::MilestoneNotFound)));
    }

    #[test]
    fn record_execution_errors_when_case_id_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".markharness/executions/m1")).unwrap();
        fs::write(
            dir.path().join(".markharness/executions/m1/milestone.yml"),
            "id: m1\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join(".markharness/generated/testcases")).unwrap();

        let args = RecordArgs {
            milestone: "m1",
            case_id: "tc-ground-001",
            result: ExecutionResult::Pass,
            executor: "yamada",
            note: None,
        };
        let result = record_execution(dir.path(), &args);

        assert!(matches!(result, Err(RecordError::CaseNotFound)));
    }

    #[cfg(unix)]
    fn link_dir(link: &Path, target: &Path) {
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[cfg(windows)]
    fn link_dir(link: &Path, target: &Path) {
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
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
    fn record_execution_refuses_to_follow_a_symlinked_milestone_dir() {
        let dir = init_repo_with_milestone_and_feature("player-jump", "m1");
        write_generated_testcase_with_feature(dir.path(), "ground", "tc-ground-001", "player-jump");
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("milestone.yml"), "id: m1\n").unwrap();
        let milestone_dir = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("executions")
            .join("m1");
        fs::remove_dir_all(&milestone_dir).unwrap();
        link_dir(&milestone_dir, outside.path());

        let result = record_execution(
            dir.path(),
            &RecordArgs {
                milestone: "m1",
                case_id: "tc-ground-001",
                result: ExecutionResult::Pass,
                executor: "yamada",
                note: None,
            },
        );

        assert!(
            matches!(result, Err(RecordError::Io(_))),
            "expected an Io error, got: {result:?}"
        );
        assert!(!outside.path().join("results.yml").exists());
    }

    fn run_git(root: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    /// A milestone tag with a `player-jump` Feature committed and tagged,
    /// matching the `id: m1` written to `executions/m1/milestone.yml` by
    /// callers (record_execution's blob resolution needs a real git ref).
    fn init_repo_with_milestone_and_feature(
        feature_id: &str,
        milestone: &str,
    ) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        run_git(dir.path(), &["config", "core.autocrlf", "false"]);
        let feature_dir = dir
            .path()
            .join(".markharness/knowledge/controls")
            .join(feature_id);
        fs::create_dir_all(&feature_dir).unwrap();
        fs::write(
            feature_dir.join("feature.yml"),
            format!("id: {feature_id}\nrequirement: controls\nlabel: {feature_id}\naxis: []\n"),
        )
        .unwrap();
        fs::create_dir_all(
            dir.path()
                .join(format!(".markharness/executions/{milestone}")),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(format!(".markharness/executions/{milestone}/milestone.yml")),
            format!("id: {milestone}\n"),
        )
        .unwrap();
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "init"]);
        run_git(dir.path(), &["tag", milestone]);
        dir
    }

    fn write_generated_testcase_with_feature(
        root: &Path,
        condition_id: &str,
        case_id: &str,
        feature_id: &str,
    ) {
        let dir = root.join(".markharness/generated/testcases");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("{condition_id}.yml")),
            format!("case_id: {case_id}\ngenerated_from:\n  feature: {feature_id}\n"),
        )
        .unwrap();
    }

    #[test]
    fn record_execution_populates_verified_feature_tree_shas_for_the_testcases_feature() {
        let dir = init_repo_with_milestone_and_feature("player-jump", "m1");
        write_generated_testcase_with_feature(dir.path(), "ground", "tc-ground-001", "player-jump");
        let expected_versions =
            crate::id_cache::resolve_feature_versions(dir.path(), "m1", false).unwrap();
        let expected_sha = expected_versions
            .iter()
            .find(|v| v.id == "player-jump")
            .unwrap()
            .tree_sha
            .clone();

        record_execution(
            dir.path(),
            &RecordArgs {
                milestone: "m1",
                case_id: "tc-ground-001",
                result: ExecutionResult::Pass,
                executor: "yamada",
                note: None,
            },
        )
        .unwrap();

        let content =
            fs::read_to_string(dir.path().join(".markharness/executions/m1/results.yml")).unwrap();
        let entries: Vec<ExecutionEntry> = serde_yaml_ng::from_str(&content).unwrap();
        assert_eq!(
            entries[0].verified_feature_tree_shas.get("player-jump"),
            Some(&expected_sha)
        );
    }

    /// Like `init_repo_with_milestone_and_feature`, but the Feature carries
    /// a `uid` (ADR 0013).
    fn init_repo_with_milestone_and_uid_feature(
        feature_id: &str,
        uid: &str,
        milestone: &str,
    ) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        run_git(dir.path(), &["config", "core.autocrlf", "false"]);
        let feature_dir = dir
            .path()
            .join(".markharness/knowledge/controls")
            .join(feature_id);
        fs::create_dir_all(&feature_dir).unwrap();
        fs::write(
            feature_dir.join("feature.yml"),
            format!(
                "id: {feature_id}\nrequirement: controls\nlabel: {feature_id}\naxis: []\nuid: {uid}\n"
            ),
        )
        .unwrap();
        fs::create_dir_all(
            dir.path()
                .join(format!(".markharness/executions/{milestone}")),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(format!(".markharness/executions/{milestone}/milestone.yml")),
            format!("id: {milestone}\n"),
        )
        .unwrap();
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "init"]);
        run_git(dir.path(), &["tag", milestone]);
        dir
    }

    /// ADR 0013: `verified_feature_tree_shas` must key its entries by the
    /// Feature's `uid` (not `feature_id`), so an entry recorded now survives
    /// a later rename that preserves the `uid`.
    #[test]
    fn record_execution_keys_verified_feature_tree_shas_by_uid_when_the_feature_has_one() {
        const UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let dir = init_repo_with_milestone_and_uid_feature("player-jump", UID, "m1");
        write_generated_testcase_with_feature(dir.path(), "ground", "tc-ground-001", "player-jump");

        record_execution(
            dir.path(),
            &RecordArgs {
                milestone: "m1",
                case_id: "tc-ground-001",
                result: ExecutionResult::Pass,
                executor: "yamada",
                note: None,
            },
        )
        .unwrap();

        let content =
            fs::read_to_string(dir.path().join(".markharness/executions/m1/results.yml")).unwrap();
        let entries: Vec<ExecutionEntry> = serde_yaml_ng::from_str(&content).unwrap();
        assert!(
            entries[0].verified_feature_tree_shas.contains_key(UID),
            "expected verified_feature_tree_shas to be keyed by uid, got {:?}",
            entries[0].verified_feature_tree_shas
        );
        assert!(
            !entries[0]
                .verified_feature_tree_shas
                .contains_key("player-jump")
        );
    }

    #[test]
    fn record_execution_creates_results_yml_with_one_entry() {
        let dir = init_repo_with_milestone_and_feature("player-jump", "m1");
        write_generated_testcase_with_feature(dir.path(), "ground", "tc-ground-001", "player-jump");

        let args = RecordArgs {
            milestone: "m1",
            case_id: "tc-ground-001",
            result: ExecutionResult::Pass,
            executor: "yamada",
            note: None,
        };
        record_execution(dir.path(), &args).unwrap();

        let content =
            fs::read_to_string(dir.path().join(".markharness/executions/m1/results.yml")).unwrap();
        assert!(content.contains("case_id: tc-ground-001"));
        assert!(content.contains("result: pass"));
        assert!(content.contains("executor: yamada"));
    }

    #[test]
    fn record_execution_appends_to_existing_results_yml_keeping_prior_entries() {
        let dir = init_repo_with_milestone_and_feature("player-jump", "m1");
        write_generated_testcase_with_feature(dir.path(), "ground", "tc-ground-001", "player-jump");
        write_generated_testcase_with_feature(dir.path(), "air", "tc-air-001", "player-jump");

        record_execution(
            dir.path(),
            &RecordArgs {
                milestone: "m1",
                case_id: "tc-ground-001",
                result: ExecutionResult::Pass,
                executor: "yamada",
                note: None,
            },
        )
        .unwrap();
        record_execution(
            dir.path(),
            &RecordArgs {
                milestone: "m1",
                case_id: "tc-air-001",
                result: ExecutionResult::Fail,
                executor: "ci-github-actions",
                note: Some("timed out"),
            },
        )
        .unwrap();

        let content =
            fs::read_to_string(dir.path().join(".markharness/executions/m1/results.yml")).unwrap();
        let entries: Vec<ExecutionEntry> = serde_yaml_ng::from_str(&content).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].case_id, "tc-ground-001");
        assert_eq!(entries[0].result, "pass");
        assert_eq!(entries[1].case_id, "tc-air-001");
        assert_eq!(entries[1].result, "fail");
        assert_eq!(entries[1].note.as_deref(), Some("timed out"));
    }
}
