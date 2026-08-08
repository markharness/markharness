use std::fs;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Only the field record_execution needs from a generated TestCase. The
/// filename under `generated/testcases/` is the condition id, not the
/// case_id (see `generate::TestCase::file_stem`), so matching case_id
/// requires reading each file's content rather than a filename lookup.
#[derive(Deserialize)]
struct MinimalTestCase {
    case_id: String,
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

fn iso8601_utc_now() -> String {
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

/// Whether any `generated/testcases/*.yml` file has this `case_id` (the
/// file's stem is the condition id, so the filename itself can't be used).
fn case_id_exists(root: &Path, case_id: &str) -> io::Result<bool> {
    let testcases_dir = root.join("generated").join("testcases");
    let Ok(entries) = fs::read_dir(&testcases_dir) else {
        return Ok(false);
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yml") {
            continue;
        }
        let content = fs::read_to_string(&path)?;
        if let Ok(testcase) = serde_yaml_ng::from_str::<MinimalTestCase>(&content)
            && testcase.case_id == case_id
        {
            return Ok(true);
        }
    }
    Ok(false)
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
        .join("executions")
        .join(args.milestone)
        .join("milestone.yml");
    if !milestone_path.is_file() {
        return Err(RecordError::MilestoneNotFound);
    }

    if !case_id_exists(root, args.case_id)? {
        return Err(RecordError::CaseNotFound);
    }

    let results_path = root
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
    });

    let content = serde_yaml_ng::to_string(&entries)
        .expect("Vec<ExecutionEntry> serialization is infallible");
    let tmp_path = results_path.with_extension("yml.tmp");
    fs::write(&tmp_path, content)?;
    fs::rename(&tmp_path, &results_path)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn record_execution_errors_when_milestone_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("generated/testcases")).unwrap();

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
        fs::create_dir_all(dir.path().join("executions/m1")).unwrap();
        fs::write(dir.path().join("executions/m1/milestone.yml"), "id: m1\n").unwrap();
        fs::create_dir_all(dir.path().join("generated/testcases")).unwrap();

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

    fn write_generated_testcase(root: &Path, condition_id: &str, case_id: &str) {
        let dir = root.join("generated/testcases");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("{condition_id}.yml")),
            format!("case_id: {case_id}\n"),
        )
        .unwrap();
    }

    #[test]
    fn record_execution_creates_results_yml_with_one_entry() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("executions/m1")).unwrap();
        fs::write(dir.path().join("executions/m1/milestone.yml"), "id: m1\n").unwrap();
        write_generated_testcase(dir.path(), "ground", "tc-ground-001");

        let args = RecordArgs {
            milestone: "m1",
            case_id: "tc-ground-001",
            result: ExecutionResult::Pass,
            executor: "yamada",
            note: None,
        };
        record_execution(dir.path(), &args).unwrap();

        let content = fs::read_to_string(dir.path().join("executions/m1/results.yml")).unwrap();
        assert!(content.contains("case_id: tc-ground-001"));
        assert!(content.contains("result: pass"));
        assert!(content.contains("executor: yamada"));
    }

    #[test]
    fn record_execution_appends_to_existing_results_yml_keeping_prior_entries() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("executions/m1")).unwrap();
        fs::write(dir.path().join("executions/m1/milestone.yml"), "id: m1\n").unwrap();
        write_generated_testcase(dir.path(), "ground", "tc-ground-001");
        write_generated_testcase(dir.path(), "air", "tc-air-001");

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

        let content = fs::read_to_string(dir.path().join("executions/m1/results.yml")).unwrap();
        let entries: Vec<ExecutionEntry> = serde_yaml_ng::from_str(&content).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].case_id, "tc-ground-001");
        assert_eq!(entries[0].result, "pass");
        assert_eq!(entries[1].case_id, "tc-air-001");
        assert_eq!(entries[1].result, "fail");
        assert_eq!(entries[1].note.as_deref(), Some("timed out"));
    }
}
