use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::fs_safety::replace_file;
use crate::identity::{CaseRevision, CaseUid, Environment, ExecutionUid, TargetRevision};

/// Only the fields `record_execution` needs from a generated TestCase.
/// Includes `phases` (unlike a purely-identity view) because ADR 0017 §5
/// requires comparing them against the frozen `CaseDefinition` in full, not
/// just trusting that a matching `(case_uid, case_revision)` key means the
/// content still agrees. Deliberately has no `#[serde(default)]`: a
/// generated file missing `phases` (truncated, hand-edited) must fail to
/// parse as a `MinimalTestCase` rather than silently being treated as an
/// empty-phases TestCase, which could spuriously "match" a genuinely
/// empty-phases stored `CaseDefinition` and record false evidence.
#[derive(Deserialize)]
struct MinimalTestCase {
    case_id: String,
    #[serde(default)]
    case_uid: Option<CaseUid>,
    case_revision: CaseRevision,
    phases: Vec<crate::generate::Phase>,
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
    /// No immutable case definition (`case_definition::load_case_definition`)
    /// is stored for this TestCase's `(case_uid, case_revision)`. ADR 0017
    /// §5 requires evidence to reference a frozen definition of what was
    /// actually executed; recording against a key nothing was ever stored
    /// under (the case-definitions store was never populated, or was
    /// deleted) would produce an audit trail with nothing to audit.
    CaseDefinitionMissing,
    /// The frozen definition stored at this TestCase's `(case_uid,
    /// case_revision)` exists and is internally self-consistent, but its
    /// `phases` don't match the `phases` the generated TestCase has *right
    /// now* — e.g. `generated/testcases/*.yml` was hand-edited after
    /// generation without its `case_revision` label being recomputed, or a
    /// merge conflict was resolved by taking the wrong side. ADR 0017 §5
    /// requires evidence to reference a frozen definition of what was
    /// actually executed; recording against a definition that no longer
    /// matches what's about to run would produce evidence that audits the
    /// wrong thing.
    CaseDefinitionMismatch,
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
    pub execution_uid: ExecutionUid,
    /// Informational: the display `case_id` at record time. Never used as a
    /// matching key by itself — `case_uid`/`case_revision` are.
    pub case_id: String,
    pub case_uid: CaseUid,
    pub case_revision: CaseRevision,
    pub target_revision: TargetRevision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<Environment>,
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
    let target_revision = TargetRevision::new(args.target_revision.trim())
        .map_err(|_| RecordError::EmptyTargetRevision)?;
    let environment = args
        .environment
        .map(|environment| {
            Environment::new(environment.trim()).map_err(|_| RecordError::EmptyEnvironment)
        })
        .transpose()?;

    let Some(testcase) = find_testcase_by_case_id(root, args.case_id)? else {
        return Err(RecordError::CaseNotFound);
    };
    let Some(case_uid) = testcase.case_uid else {
        return Err(RecordError::CaseNotMigrated);
    };
    let Some(stored_definition) =
        crate::case_definition::load_case_definition(root, &case_uid, &testcase.case_revision)?
    else {
        return Err(RecordError::CaseDefinitionMissing);
    };
    let current_definition = crate::case_definition::CaseDefinition {
        case_uid: case_uid.clone(),
        case_revision: testcase.case_revision.clone(),
        phases: testcase.phases.clone(),
    };
    if stored_definition != current_definition {
        return Err(RecordError::CaseDefinitionMismatch);
    }

    let execution_uid = ExecutionUid::new(ulid::Ulid::new().to_string())
        .expect("ulid::Ulid::new().to_string() is always a non-blank string");
    let entry = ExecutionEntry {
        execution_uid: execution_uid.clone(),
        case_id: testcase.case_id,
        case_uid,
        case_revision: testcase.case_revision,
        target_revision,
        environment,
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
    use crate::generate::Phase;
    use std::fs;

    /// `write_testcase`'s callers always write `phases: []`, so this is the
    /// only `case_revision` value that will pass `load_case_definition`'s
    /// self-consistency check (`case_revision` is defined as a hash of
    /// `phases`) once a definition is stored under it.
    fn real_case_revision_for_empty_phases() -> CaseRevision {
        crate::generate::compute_case_revision(&[])
    }

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
        // `record_execution` requires the immutable case definition
        // (ADR 0017 §5) to already be stored under the same key that
        // `generate` would have populated it at.
        if let Some(case_uid) = case_uid {
            let definition_path = root
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("case-definitions")
                .join(case_uid)
                .join(format!("{case_revision}.yml"));
            fs::create_dir_all(definition_path.parent().unwrap()).unwrap();
            fs::write(
                &definition_path,
                format!("case_uid: {case_uid}\ncase_revision: {case_revision}\nphases: []\n"),
            )
            .unwrap();
        }
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

    /// ADR 0017 §5: recording evidence for a `(case_uid, case_revision)` with
    /// no stored immutable case definition (the case-definitions store was
    /// never populated, or the file was deleted) must be rejected rather
    /// than producing evidence nothing can audit.
    #[test]
    fn record_execution_errors_when_the_case_definition_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        let path = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("generated/testcases/feature/behavior/scenario.yml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "case_id: tc-ground-001\ncase_uid: case-uid-1\ncase_revision: rev-1\nphases: []\n",
        )
        .unwrap();

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

        assert!(matches!(result, Err(RecordError::CaseDefinitionMissing)));
    }

    /// A generated `testcases/*.yml` file missing its `phases` key entirely
    /// (truncated, hand-edited, or otherwise malformed) must never be
    /// silently treated as an empty-phases TestCase — doing so could let it
    /// spuriously "match" a genuinely empty-phases stored `CaseDefinition`
    /// and record evidence for content that was never actually verified.
    #[test]
    fn record_execution_does_not_treat_a_testcase_file_missing_phases_as_empty_phases() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        // A real, empty-phases definition is stored under this key — if a
        // missing `phases` key defaulted to `vec![]`, it would incorrectly
        // match this and let `record_execution` succeed.
        let case_revision = real_case_revision_for_empty_phases();
        let definition_path = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("case-definitions")
            .join("case-uid-1")
            .join(format!("{case_revision}.yml"));
        fs::create_dir_all(definition_path.parent().unwrap()).unwrap();
        fs::write(
            &definition_path,
            format!("case_uid: case-uid-1\ncase_revision: {case_revision}\nphases: []\n"),
        )
        .unwrap();
        let testcase_path = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("generated/testcases/feature/behavior/scenario.yml");
        fs::create_dir_all(testcase_path.parent().unwrap()).unwrap();
        fs::write(
            &testcase_path,
            format!(
                "case_id: tc-ground-001\ncase_uid: case-uid-1\ncase_revision: {case_revision}\n"
            ),
        )
        .unwrap();

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

        assert!(
            result.is_err(),
            "a testcase file with no phases key must never be recorded as evidence, got: {result:?}"
        );
    }

    /// ADR 0017 §5: the generated TestCase must be checked in full against
    /// the frozen definition, not just by `(case_uid, case_revision)` key
    /// presence. Here the generated `testcases/*.yml` file's `phases` no
    /// longer match what's frozen at its own `case_revision` key (as if it
    /// were hand-edited after generation without the revision being
    /// recomputed) — recording evidence against it must be rejected, since
    /// the stored definition it would be audited against isn't actually
    /// what's about to run.
    #[test]
    fn record_execution_errors_when_the_generated_testcase_disagrees_with_the_stored_definition() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        let original_phases = vec![Phase {
            steps: vec!["Do it.".to_string()],
            results: vec!["Confirmed.".to_string()],
        }];
        let case_revision = crate::generate::compute_case_revision(&original_phases);
        let definition = crate::case_definition::CaseDefinition {
            case_uid: CaseUid::new("case-uid-1").unwrap(),
            case_revision: case_revision.clone(),
            phases: original_phases,
        };
        let definition_path = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("case-definitions")
            .join("case-uid-1")
            .join(format!("{case_revision}.yml"));
        fs::create_dir_all(definition_path.parent().unwrap()).unwrap();
        fs::write(
            &definition_path,
            crate::case_definition::serialize_case_definition(&definition),
        )
        .unwrap();

        // The generated file still claims the original `case_revision`, but
        // its `phases` were edited afterward without recomputing it.
        let testcase_path = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("generated/testcases/feature/behavior/scenario.yml");
        fs::create_dir_all(testcase_path.parent().unwrap()).unwrap();
        fs::write(
            &testcase_path,
            format!(
                "case_id: tc-ground-001\ncase_uid: case-uid-1\ncase_revision: {case_revision}\n\
                 phases:\n  - steps: [\"Do it differently.\"]\n    results: [\"Confirmed.\"]\n"
            ),
        )
        .unwrap();

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

        assert!(matches!(result, Err(RecordError::CaseDefinitionMismatch)));
    }

    #[test]
    fn record_execution_writes_one_file_per_execution_and_reads_it_back() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        let case_revision = real_case_revision_for_empty_phases();
        write_testcase(
            dir.path(),
            "tc-ground-001",
            Some("case-uid-1"),
            case_revision.as_str(),
        );

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
        assert_eq!(all[0].case_uid.as_str(), "case-uid-1");
        assert_eq!(all[0].case_revision, case_revision);
        assert_eq!(all[0].target_revision.as_str(), "abc123");
        assert_eq!(
            all[0].environment.as_ref().map(Environment::as_str),
            Some("staging")
        );
        assert_eq!(all[0].result, "pass");
        assert_eq!(all[0].note.as_deref(), Some("looks good"));
    }

    #[test]
    fn record_execution_twice_produces_two_distinct_files() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_testcase(
            dir.path(),
            "tc-ground-001",
            Some("case-uid-1"),
            real_case_revision_for_empty_phases().as_str(),
        );

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
