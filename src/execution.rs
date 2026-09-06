use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::fs_safety::replace_file;

/// Only the fields `record_execution` needs from a generated TestCase.
#[derive(Deserialize)]
struct MinimalTestCase {
    case_id: String,
    #[serde(default)]
    case_uid: Option<String>,
    case_revision: String,
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
    pub case_id: &'a str,
    /// ADR 0017 §3/§5: an opaque, non-empty identifier of the build/commit
    /// under test — never inferred from a Feature's Git tree SHA. Kind-
    /// agnostic (a commit OID, a CI build number, a release tag — whatever
    /// the caller's pipeline uses), per the grilling notes decided ahead of
    /// this Step.
    pub target_revision: &'a str,
    /// ADR 0017 §5: a free-text environment identifier. `None` means
    /// explicitly unknown — never inferred or defaulted to "matches
    /// anything".
    pub environment: Option<&'a str>,
    pub result: ExecutionResult,
    pub executor: &'a str,
    pub note: Option<&'a str>,
}

#[derive(Debug)]
pub enum RecordError {
    /// No generated TestCase with this `case_id` exists under
    /// `generated/testcases/`.
    CaseNotFound,
    /// The TestCase's Scenario has no `case_uid` yet (ADR 0017 §3: derived
    /// from `ScenarioUid`, which requires `identity migrate` to have run).
    /// Execution evidence is keyed by `(case_uid, case_revision)`, so an
    /// unmigrated case has nothing to record evidence against.
    CaseNotMigrated,
    /// `target_revision` was empty or whitespace-only.
    EmptyTargetRevision,
    /// `environment` was given (`Some`) but empty or whitespace-only. A
    /// blank string is not a real environment identifier and must never be
    /// stored as one — `plan::evidence_status` treats *any* recorded
    /// `environment` value as "known", so a stored blank would wrongly
    /// satisfy a plan with no specific environment requirement (ADR 0017
    /// §5: unknown environments must never satisfy passing).
    EmptyEnvironment,
    Io(io::Error),
}

impl From<io::Error> for RecordError {
    fn from(e: io::Error) -> Self {
        RecordError::Io(e)
    }
}

/// ADR 0017 §5: one recorded execution result, keyed by its own immutable
/// `execution_uid` and carrying the Case UID/Case revision/target/
/// environment `record_execution` resolved at record time — never a
/// Feature-tree-SHA proxy. Applicability against a `VerificationPlan`'s
/// requirements (`plan::evidence_status`) is a separate concern from this
/// type: this is only the durable record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionEntry {
    pub execution_uid: String,
    /// Informational: the display `case_id` at record time. Never used as a
    /// matching key by itself — `case_uid`/`case_revision` are.
    pub case_id: String,
    pub case_uid: String,
    pub case_revision: String,
    pub target_revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    pub result: String,
    pub executor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub executed_at: String,
    /// Room for a record decomposed from an external report to point back
    /// at its source (ADR 0017 §5 grilling notes). Key generation and
    /// duplicate-import detection are deferred to Step 5, once real
    /// external-tool data is available to design against — this field only
    /// exists so that decision doesn't require a schema change later.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_report: Option<String>,
}

/// The directory execution records live under: one file per execution (ADR
/// 0017 §5 grilling notes — "1実行=1ファイル"), named by its own
/// `execution_uid` so concurrent recorders never contend on the same path.
fn records_dir(root: &Path) -> PathBuf {
    root.join(crate::project_root::MARKHARNESS_DIR)
        .join("executions")
        .join("records")
}

pub fn read_all_results(root: &Path) -> io::Result<Vec<ExecutionEntry>> {
    let dir = records_dir(root);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("yml"))
        .collect();
    paths.sort();
    let mut results = Vec::new();
    for path in paths {
        let content = fs::read_to_string(&path)?;
        let entry: ExecutionEntry = serde_yaml_ng::from_str(&content)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        results.push(entry);
    }
    results.sort_by(|a, b| {
        a.executed_at
            .cmp(&b.executed_at)
            .then(a.execution_uid.cmp(&b.execution_uid))
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
/// are nested `{feature}/{behavior}/{scenario}.yml`, so the filename alone
/// can't be used — and even the full relative path is only a mirror of
/// `knowledge/`, not itself the identity being searched for).
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

/// Records one execution result (ADR 0017 §5). Resolves `case_uid`/
/// `case_revision` from the generated TestCase at record time — never
/// supplied by the caller, so a caller can't record evidence against a
/// mismatched or stale identity. `target_revision`/`environment` are always
/// exactly what the caller passed, never inferred.
pub fn record_execution(root: &Path, args: &RecordArgs) -> Result<ExecutionEntry, RecordError> {
    let target_revision = args.target_revision.trim();
    if target_revision.is_empty() {
        return Err(RecordError::EmptyTargetRevision);
    }
    let environment = match args.environment {
        Some(environment) => {
            let trimmed = environment.trim();
            if trimmed.is_empty() {
                return Err(RecordError::EmptyEnvironment);
            }
            Some(trimmed)
        }
        None => None,
    };

    let Some(testcase) = find_testcase_by_case_id(root, args.case_id)? else {
        return Err(RecordError::CaseNotFound);
    };
    let Some(case_uid) = testcase.case_uid else {
        return Err(RecordError::CaseNotMigrated);
    };

    let execution_uid = ulid::Ulid::new().to_string();
    let entry = ExecutionEntry {
        execution_uid: execution_uid.clone(),
        case_id: testcase.case_id,
        case_uid,
        case_revision: testcase.case_revision,
        target_revision: target_revision.to_string(),
        environment: environment.map(str::to_string),
        result: args.result.as_str().to_string(),
        executor: args.executor.to_string(),
        note: args.note.map(str::to_string),
        executed_at: iso8601_utc_now(),
        source_report: None,
    };

    let path = records_dir(root).join(format!("{execution_uid}.yml"));
    let content =
        serde_yaml_ng::to_string(&entry).expect("ExecutionEntry serialization is infallible");
    replace_file(root, &path, content.as_bytes())?;

    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_testcase(root: &Path, case_id: &str, case_uid: Option<&str>, case_revision: &str) {
        let path = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("generated/testcases/feature/behavior/scenario.yml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let uid_line = case_uid
            .map(|uid| format!("case_uid: {uid}\n"))
            .unwrap_or_default();
        fs::write(
            &path,
            format!("case_id: {case_id}\n{uid_line}case_revision: {case_revision}\nphases: []\n"),
        )
        .unwrap();
    }

    #[test]
    fn record_execution_errors_when_case_id_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        let result = record_execution(
            dir.path(),
            &RecordArgs {
                case_id: "tc-unknown",
                target_revision: "abc123",
                environment: None,
                result: ExecutionResult::Pass,
                executor: "tester",
                note: None,
            },
        );

        assert!(matches!(result, Err(RecordError::CaseNotFound)));
    }

    #[test]
    fn record_execution_errors_when_target_revision_is_blank() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_testcase(dir.path(), "tc-ground-001", Some("case-uid-1"), "rev-1");

        let result = record_execution(
            dir.path(),
            &RecordArgs {
                case_id: "tc-ground-001",
                target_revision: "   ",
                environment: None,
                result: ExecutionResult::Pass,
                executor: "tester",
                note: None,
            },
        );

        assert!(matches!(result, Err(RecordError::EmptyTargetRevision)));
    }

    /// A blank `environment` must never be stored: `plan::evidence_status`
    /// treats any recorded `environment` value as "known", so a stored
    /// blank would wrongly satisfy a plan with no specific environment
    /// requirement (ADR 0017 §5).
    #[test]
    fn record_execution_errors_when_environment_is_blank() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_testcase(dir.path(), "tc-ground-001", Some("case-uid-1"), "rev-1");

        let result = record_execution(
            dir.path(),
            &RecordArgs {
                case_id: "tc-ground-001",
                target_revision: "abc123",
                environment: Some("   "),
                result: ExecutionResult::Pass,
                executor: "tester",
                note: None,
            },
        );

        assert!(matches!(result, Err(RecordError::EmptyEnvironment)));
    }

    /// ADR 0017 §3: a Scenario with no `case_uid` (not yet migrated) has no
    /// stable key to record evidence against.
    #[test]
    fn record_execution_errors_when_the_case_has_no_case_uid() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_testcase(dir.path(), "tc-ground-001", None, "rev-1");

        let result = record_execution(
            dir.path(),
            &RecordArgs {
                case_id: "tc-ground-001",
                target_revision: "abc123",
                environment: None,
                result: ExecutionResult::Pass,
                executor: "tester",
                note: None,
            },
        );

        assert!(matches!(result, Err(RecordError::CaseNotMigrated)));
    }

    #[test]
    fn record_execution_writes_one_file_per_execution_and_reads_it_back() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_testcase(dir.path(), "tc-ground-001", Some("case-uid-1"), "rev-1");

        let entry = record_execution(
            dir.path(),
            &RecordArgs {
                case_id: "tc-ground-001",
                target_revision: "abc123",
                environment: Some("staging"),
                result: ExecutionResult::Pass,
                executor: "tester",
                note: Some("looks good"),
            },
        )
        .unwrap();

        let path = dir
            .path()
            .join(".markharness/executions/records")
            .join(format!("{}.yml", entry.execution_uid));
        assert!(path.is_file());

        let all = read_all_results(dir.path()).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].case_uid, "case-uid-1");
        assert_eq!(all[0].case_revision, "rev-1");
        assert_eq!(all[0].target_revision, "abc123");
        assert_eq!(all[0].environment.as_deref(), Some("staging"));
        assert_eq!(all[0].result, "pass");
        assert_eq!(all[0].note.as_deref(), Some("looks good"));
    }

    #[test]
    fn record_execution_twice_produces_two_distinct_files() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_testcase(dir.path(), "tc-ground-001", Some("case-uid-1"), "rev-1");

        let args = RecordArgs {
            case_id: "tc-ground-001",
            target_revision: "abc123",
            environment: None,
            result: ExecutionResult::Pass,
            executor: "tester",
            note: None,
        };
        let first = record_execution(dir.path(), &args).unwrap();
        let second = record_execution(dir.path(), &args).unwrap();

        assert_ne!(first.execution_uid, second.execution_uid);
        let all = read_all_results(dir.path()).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn read_all_results_returns_empty_when_no_records_dir_exists() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        let all = read_all_results(dir.path()).unwrap();

        assert!(all.is_empty());
    }
}
