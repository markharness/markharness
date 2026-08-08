use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use serde::Serialize;

use crate::backfill;
use crate::execution::ExecutionEntry;
use crate::generate::{generate_testcases, serialize_testcase};
use crate::id_cache;
use crate::traceability::{build_index, serialize_index};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum DiffKind {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DiffEntry {
    pub file_name: String,
    pub kind: DiffKind,
}

fn read_existing_testcases(generated_dir: &Path) -> io::Result<BTreeMap<String, String>> {
    let mut existing = BTreeMap::new();
    if !generated_dir.is_dir() {
        return Ok(existing);
    }
    for entry in fs::read_dir(generated_dir)? {
        let path = entry?.path();
        if path.is_file()
            && let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned())
        {
            existing.insert(name, fs::read_to_string(&path)?);
        }
    }
    Ok(existing)
}

/// Regenerates testcases from `root/knowledge` and compares them against the
/// committed files in `root/generated/testcases/`, without writing anything.
pub fn diff_generated_testcases(root: &Path) -> io::Result<Vec<DiffEntry>> {
    let testcases = generate_testcases(&root.join("knowledge"))?;
    let mut expected: BTreeMap<String, String> = BTreeMap::new();
    for testcase in &testcases {
        let file_name = format!("{}.yml", testcase.file_stem());
        expected.insert(file_name, serialize_testcase(testcase));
    }

    let existing = read_existing_testcases(&root.join("generated").join("testcases"))?;

    let mut diffs = Vec::new();
    for (file_name, content) in &expected {
        match existing.get(file_name) {
            None => diffs.push(DiffEntry {
                file_name: file_name.clone(),
                kind: DiffKind::Added,
            }),
            Some(existing_content) if existing_content != content => diffs.push(DiffEntry {
                file_name: file_name.clone(),
                kind: DiffKind::Changed,
            }),
            Some(_) => {}
        }
    }
    for file_name in existing.keys() {
        if !expected.contains_key(file_name) {
            diffs.push(DiffEntry {
                file_name: file_name.clone(),
                kind: DiffKind::Removed,
            });
        }
    }

    let index = build_index(&testcases);
    let expected_index_json = serialize_index(&index);
    let index_path = root.join("generated").join("traceability-index.json");
    match fs::read_to_string(&index_path) {
        Err(_) => diffs.push(DiffEntry {
            file_name: "traceability-index.json".to_string(),
            kind: DiffKind::Added,
        }),
        Ok(existing_json) if existing_json != expected_index_json => diffs.push(DiffEntry {
            file_name: "traceability-index.json".to_string(),
            kind: DiffKind::Changed,
        }),
        Ok(_) => {}
    }

    diffs.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    Ok(diffs)
}

/// Which ChangeEvent a `verified_feature_blobs` entry reflects (§3.1 Q1).
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ReflectedChange {
    pub event_id: String,
    pub from_milestone: String,
    pub to_milestone: String,
}

/// One Feature's trace result within a TestExecution (a TestCase can span
/// more than one Feature per §2.1's map-shaped `verified_feature_blobs`).
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct TraceEntry {
    pub feature_id: String,
    /// `None` when no ChangeEvent with a matching `to_blob` exists in
    /// `changes/` (e.g. `changes compute`/`backfill` hasn't been run for the
    /// milestone pair where this blob last changed).
    pub reflects_change: Option<ReflectedChange>,
}

/// The `verify trace` answer for one TestExecution record.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct TraceResult {
    pub case_id: String,
    pub executed_at: String,
    pub entries: Vec<TraceEntry>,
}

#[derive(Debug)]
pub enum TraceError {
    /// No `results.yml` record for this `case_id` at this milestone, or the
    /// record predates `verified_feature_blobs` (no retroactive backfill,
    /// per §6) and so has nothing to trace.
    NoVerifiedBlobs,
    Io(io::Error),
}

impl From<io::Error> for TraceError {
    fn from(e: io::Error) -> Self {
        TraceError::Io(e)
    }
}

fn read_results(root: &Path, milestone: &str) -> io::Result<Vec<ExecutionEntry>> {
    let path = root.join("executions").join(milestone).join("results.yml");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path)?;
    Ok(serde_yaml_ng::from_str(&content).unwrap_or_default())
}

/// All ChangeEvents recorded anywhere under `changes/`, across every
/// `to_milestone` file (a Feature's blob can have last changed several
/// milestones before the one being traced).
fn read_all_changes(root: &Path) -> io::Result<Vec<crate::changes::ChangeEvent>> {
    let changes_dir = root.join("changes");
    let Ok(entries) = fs::read_dir(&changes_dir) else {
        return Ok(Vec::new());
    };
    let mut all = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        let Some(milestone) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        all.extend(crate::changes::read_changes(root, milestone)?);
    }
    Ok(all)
}

/// Q1 (§3.1): for each Feature a TestExecution's `verified_feature_blobs`
/// names, finds the ChangeEvent whose `to_blob` matches the recorded blob
/// SHA — i.e. which change this execution result reflects.
pub fn trace(root: &Path, case_id: &str, milestone: &str) -> Result<TraceResult, TraceError> {
    let entries = read_results(root, milestone)?;
    let Some(entry) = entries.iter().rev().find(|e| e.case_id == case_id) else {
        return Err(TraceError::NoVerifiedBlobs);
    };
    if entry.verified_feature_blobs.is_empty() {
        return Err(TraceError::NoVerifiedBlobs);
    }

    let all_changes = read_all_changes(root)?;

    let mut trace_entries = Vec::new();
    for (feature_id, blob_sha) in &entry.verified_feature_blobs {
        let reflects_change = all_changes
            .iter()
            .find(|e| &e.feature_id == feature_id && e.to_blob.as_deref() == Some(blob_sha))
            .map(|e| ReflectedChange {
                event_id: e.event_id.clone(),
                from_milestone: e.from_milestone.clone(),
                to_milestone: e.to_milestone.clone(),
            });
        trace_entries.push(TraceEntry {
            feature_id: feature_id.clone(),
            reflects_change,
        });
    }
    Ok(TraceResult {
        case_id: entry.case_id.clone(),
        executed_at: entry.executed_at.clone(),
        entries: trace_entries,
    })
}

/// One impacted TestCase not yet re-executed against the ChangeEvent's
/// `to_blob` (§3.2/§3.3 pending: the target hasn't moved since the change).
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct PendingEntry {
    pub case_id: String,
    pub feature_id: String,
    pub event_id: String,
}

/// One impacted TestCase not yet re-executed *and* whose Feature has since
/// changed again (§3.3 stale: re-confirming the old target is meaningless
/// now). `current_event` is the latest ChangeEvent for the Feature — the
/// "実質的な確認対象" — or `None` if none is on record.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct StaleEntry {
    pub case_id: String,
    pub feature_id: String,
    pub original_event_id: String,
    pub current_event: Option<ReflectedChange>,
}

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct PendingReport {
    pub pending: Vec<PendingEntry>,
    pub stale: Vec<StaleEntry>,
}

#[derive(Debug)]
pub enum PendingError {
    /// `range` was `None` and fewer than two milestones exist to pair.
    NoMilestonePair,
    /// An explicitly given `--from`/`--to` milestone has no
    /// `executions/<name>/milestone.yml`.
    MilestoneNotFound,
    /// `to` is not strictly newer than `from` by committer date.
    InvalidRange,
    Io(io::Error),
}

impl From<io::Error> for PendingError {
    fn from(e: io::Error) -> Self {
        PendingError::Io(e)
    }
}

/// Q2 (§3.2/§3.3): TestCases impacted by ChangeEvents in `(from, to]`
/// (`range`, or the most recent adjacent milestone pair if `None`) that
/// haven't been re-executed against the new blob, split into `pending`
/// (still the right target) and `stale` (the Feature moved on again before
/// anyone re-ran the case).
pub fn pending(
    root: &Path,
    range: Option<(&str, &str)>,
    use_cache: bool,
) -> Result<PendingReport, PendingError> {
    let names = backfill::list_milestone_names(root)?;
    let ordered = backfill::order_by_recency(root, names); // newest first

    let (from_milestone, to_milestone) = match range {
        Some((from, to)) => (from.to_string(), to.to_string()),
        None => {
            if ordered.len() < 2 {
                return Err(PendingError::NoMilestonePair);
            }
            (ordered[1].clone(), ordered[0].clone())
        }
    };

    let to_index = ordered
        .iter()
        .position(|m| m == &to_milestone)
        .ok_or(PendingError::MilestoneNotFound)?;
    let from_index = ordered
        .iter()
        .position(|m| m == &from_milestone)
        .ok_or(PendingError::MilestoneNotFound)?;
    if to_index >= from_index {
        return Err(PendingError::InvalidRange);
    }

    // Milestones strictly after `from_milestone` up to and including
    // `to_milestone`, oldest first: each contributes its `changes/<m>.yaml`
    // to the Impacted set.
    let aggregation_targets: Vec<&String> = ordered[to_index..from_index].iter().rev().collect();

    let mut impacted: BTreeMap<String, crate::changes::ChangeEvent> = BTreeMap::new();
    for milestone in &aggregation_targets {
        for event in crate::changes::read_changes(root, milestone)? {
            for case_id in &event.impacted_testcases {
                impacted.insert(case_id.clone(), event.clone());
            }
        }
    }

    // Milestones at or after `to_milestone` (newest first): a re-execution
    // recorded in any of these counts as "re-verified" (§3.2 step 2).
    let at_or_after_to = &ordered[..=to_index];

    let current_milestone = ordered.first();
    let all_changes = read_all_changes(root)?;

    let mut report = PendingReport::default();
    for (case_id, event) in impacted {
        if was_reexecuted(root, at_or_after_to, &case_id, &event)? {
            continue;
        }

        let current_blob = match current_milestone {
            Some(m) => id_cache::resolve_feature_blobs(root, m, use_cache)?
                .into_iter()
                .find(|b| b.id == event.feature_id)
                .map(|b| b.blob_sha),
            None => None,
        };

        if current_blob == event.to_blob {
            report.pending.push(PendingEntry {
                case_id,
                feature_id: event.feature_id,
                event_id: event.event_id,
            });
        } else {
            let current_event = current_blob.as_ref().and_then(|current_blob| {
                all_changes
                    .iter()
                    .find(|e| {
                        e.feature_id == event.feature_id && e.to_blob.as_ref() == Some(current_blob)
                    })
                    .map(|e| ReflectedChange {
                        event_id: e.event_id.clone(),
                        from_milestone: e.from_milestone.clone(),
                        to_milestone: e.to_milestone.clone(),
                    })
            });
            report.stale.push(StaleEntry {
                case_id,
                feature_id: event.feature_id,
                original_event_id: event.event_id,
                current_event,
            });
        }
    }

    report.pending.sort_by(|a, b| a.case_id.cmp(&b.case_id));
    report.stale.sort_by(|a, b| a.case_id.cmp(&b.case_id));
    Ok(report)
}

fn was_reexecuted(
    root: &Path,
    milestones: &[String],
    case_id: &str,
    event: &crate::changes::ChangeEvent,
) -> io::Result<bool> {
    for milestone in milestones {
        let entries = read_results(root, milestone)?;
        if entries.iter().any(|e| {
            e.case_id == case_id
                && e.verified_feature_blobs.get(&event.feature_id) == event.to_blob.as_ref()
        }) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_knowledge_todo_add_task(root: &Path) {
        let dir = root.join("knowledge/req-todo/todo/todo-add-task/todo-add-task-empty-input");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            root.join("knowledge/req-todo/requirement.yml"),
            "id: req-todo\nlabel: req-todo\naxis: [ui]\n",
        )
        .unwrap();
        fs::write(
            root.join("knowledge/req-todo/todo/feature.yml"),
            "id: todo\nrequirement: req-todo\nlabel: todo\naxis: [ui]\n",
        )
        .unwrap();
        fs::write(
            root.join("knowledge/req-todo/todo/todo-add-task/behavior.yml"),
            "id: todo-add-task\nfeature: todo\nlabel: todo-add-task\naxis: [ui]\ndescription: |\n  User adds a task.\n",
        )
        .unwrap();
        fs::write(
            dir.join("condition.yml"),
            "id: todo-add-task-empty-input\nbehavior: todo-add-task\nlabel: todo-add-task-empty-input\ndescription: |\n  Title is empty.\n",
        )
        .unwrap();
        fs::create_dir_all(dir.join("expected")).unwrap();
        fs::write(
            dir.join("expected/001.yml"),
            "id: todo-add-task-empty-input-001\ncondition: todo-add-task-empty-input\ndescription: |\n  Shows a validation error.\n",
        )
        .unwrap();
    }

    /// Writes `generated/traceability-index.json` matching a fresh
    /// regeneration from `root/knowledge`, so tests can isolate the
    /// testcases-file diff behavior from the index-file diff behavior.
    fn write_matching_index(root: &Path) {
        let testcases = generate_testcases(&root.join("knowledge")).unwrap();
        let index = build_index(&testcases);
        fs::write(
            root.join("generated/traceability-index.json"),
            serialize_index(&index),
        )
        .unwrap();
    }

    #[test]
    fn reports_no_diff_when_generated_dir_missing_and_knowledge_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_matching_index(dir.path());

        let diffs = diff_generated_testcases(dir.path()).unwrap();

        assert!(diffs.is_empty());
    }

    #[test]
    fn reports_added_when_committed_file_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_knowledge_todo_add_task(dir.path());
        write_matching_index(dir.path());

        let diffs = diff_generated_testcases(dir.path()).unwrap();

        assert_eq!(
            diffs,
            vec![DiffEntry {
                file_name: "todo-add-task-empty-input.yml".to_string(),
                kind: DiffKind::Added,
            }]
        );
    }

    #[test]
    fn reports_no_diff_when_committed_file_matches_regeneration() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_knowledge_todo_add_task(dir.path());

        let testcases = generate_testcases(&dir.path().join("knowledge")).unwrap();
        let testcases_dir = dir.path().join("generated/testcases");
        fs::create_dir_all(&testcases_dir).unwrap();
        for testcase in &testcases {
            fs::write(
                testcases_dir.join(format!("{}.yml", testcase.file_stem())),
                serialize_testcase(testcase),
            )
            .unwrap();
        }
        write_matching_index(dir.path());

        let diffs = diff_generated_testcases(dir.path()).unwrap();

        assert!(diffs.is_empty());
    }

    #[test]
    fn reports_changed_when_committed_file_content_differs() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_knowledge_todo_add_task(dir.path());

        let testcases_dir = dir.path().join("generated/testcases");
        fs::create_dir_all(&testcases_dir).unwrap();
        fs::write(
            testcases_dir.join("todo-add-task-empty-input.yml"),
            "stale content\n",
        )
        .unwrap();
        write_matching_index(dir.path());

        let diffs = diff_generated_testcases(dir.path()).unwrap();

        assert_eq!(
            diffs,
            vec![DiffEntry {
                file_name: "todo-add-task-empty-input.yml".to_string(),
                kind: DiffKind::Changed,
            }]
        );
    }

    #[test]
    fn reports_removed_when_committed_file_no_longer_generated() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        let testcases_dir = dir.path().join("generated/testcases");
        fs::create_dir_all(&testcases_dir).unwrap();
        fs::write(testcases_dir.join("stale-condition.yml"), "stale content\n").unwrap();
        write_matching_index(dir.path());

        let diffs = diff_generated_testcases(dir.path()).unwrap();

        assert_eq!(
            diffs,
            vec![DiffEntry {
                file_name: "stale-condition.yml".to_string(),
                kind: DiffKind::Removed,
            }]
        );
    }

    #[test]
    fn reports_added_for_traceability_index_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_knowledge_todo_add_task(dir.path());
        let testcases = generate_testcases(&dir.path().join("knowledge")).unwrap();
        let testcases_dir = dir.path().join("generated/testcases");
        fs::create_dir_all(&testcases_dir).unwrap();
        for testcase in &testcases {
            fs::write(
                testcases_dir.join(format!("{}.yml", testcase.file_stem())),
                serialize_testcase(testcase),
            )
            .unwrap();
        }

        let diffs = diff_generated_testcases(dir.path()).unwrap();

        assert_eq!(
            diffs,
            vec![DiffEntry {
                file_name: "traceability-index.json".to_string(),
                kind: DiffKind::Added,
            }]
        );
    }

    #[test]
    fn reports_changed_for_traceability_index_when_stale() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_knowledge_todo_add_task(dir.path());
        let testcases = generate_testcases(&dir.path().join("knowledge")).unwrap();
        let testcases_dir = dir.path().join("generated/testcases");
        fs::create_dir_all(&testcases_dir).unwrap();
        for testcase in &testcases {
            fs::write(
                testcases_dir.join(format!("{}.yml", testcase.file_stem())),
                serialize_testcase(testcase),
            )
            .unwrap();
        }
        fs::write(
            dir.path().join("generated/traceability-index.json"),
            "{\"testcases\":[]}",
        )
        .unwrap();

        let diffs = diff_generated_testcases(dir.path()).unwrap();

        assert_eq!(
            diffs,
            vec![DiffEntry {
                file_name: "traceability-index.json".to_string(),
                kind: DiffKind::Changed,
            }]
        );
    }

    fn write_results(root: &Path, milestone: &str, yaml: &str) {
        let dir = root.join("executions").join(milestone);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("results.yml"), yaml).unwrap();
    }

    fn write_changes(root: &Path, to_milestone: &str, yaml: &str) {
        let dir = root.join("changes");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{to_milestone}.yaml")), yaml).unwrap();
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

    fn commit_and_tag_milestone(root: &Path, milestone: &str, hour_offset: u32) {
        fs::create_dir_all(root.join("executions").join(milestone)).unwrap();
        fs::write(
            root.join("executions")
                .join(milestone)
                .join("milestone.yml"),
            format!("id: {milestone}\n"),
        )
        .unwrap();
        run_git(root, &["add", "-A"]);
        let date = format!("2026-01-01T{hour_offset:02}:00:00+00:00");
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["commit", "-q", "-m", milestone])
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .status()
            .unwrap();
        assert!(status.success());
        run_git(root, &["tag", milestone]);
    }

    fn write_feature(root: &Path, label: &str) {
        let dir = root.join("knowledge/req-todo/todo-edit");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("feature.yml"),
            format!("id: todo-edit\nrequirement: req-todo\nlabel: {label}\naxis: []\n"),
        )
        .unwrap();
    }

    /// Sets up a repo with two milestone tags (`test1`, `test2`) where
    /// `todo-edit`'s Feature blob changed between them, and writes
    /// `changes/test2.yaml` recording that change with `tc-edit-existing-todo-001`
    /// as impacted. Returns the `to_blob` SHA actually produced.
    fn init_repo_with_pending_change() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);

        write_feature(dir.path(), "v1");
        commit_and_tag_milestone(dir.path(), "test1", 1);

        write_feature(dir.path(), "v2");
        commit_and_tag_milestone(dir.path(), "test2", 2);

        let to_blob = crate::id_cache::resolve_feature_blobs(dir.path(), "test2", false)
            .unwrap()
            .into_iter()
            .find(|b| b.id == "todo-edit")
            .unwrap()
            .blob_sha;

        write_changes(
            dir.path(),
            "test2",
            &format!(
                "- event_id: todo-edit--test1--test2\n  feature_id: todo-edit\n  from_milestone: test1\n  to_milestone: test2\n  from_blob: null\n  to_blob: {to_blob}\n  impacted_testcases:\n  - tc-edit-existing-todo-001\n"
            ),
        );

        (dir, to_blob)
    }

    #[test]
    fn pending_reports_impacted_testcase_not_yet_reexecuted() {
        let (dir, to_blob) = init_repo_with_pending_change();

        let report = pending(dir.path(), None, false).unwrap();

        assert_eq!(
            report.pending,
            vec![PendingEntry {
                case_id: "tc-edit-existing-todo-001".to_string(),
                feature_id: "todo-edit".to_string(),
                event_id: "todo-edit--test1--test2".to_string(),
            }]
        );
        assert!(report.stale.is_empty());
        let _ = to_blob;
    }

    #[test]
    fn pending_does_not_report_testcase_already_reexecuted_against_the_new_blob() {
        let (dir, to_blob) = init_repo_with_pending_change();
        write_results(
            dir.path(),
            "test2",
            &format!(
                "- case_id: tc-edit-existing-todo-001\n  result: pass\n  executor: soreiyu52\n  executed_at: 2026-08-08T16:38:52Z\n  verified_feature_blobs:\n    todo-edit: {to_blob}\n"
            ),
        );

        let report = pending(dir.path(), None, false).unwrap();

        assert!(report.pending.is_empty());
        assert!(report.stale.is_empty());
    }

    #[test]
    fn pending_errors_instead_of_panicking_when_to_is_not_newer_than_from() {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        write_feature(dir.path(), "v1");
        // Same committer date for both: a name-order tie-break in
        // order_by_recency puts "test2" (the `to`) before "test1" (the
        // `from`) in the newest-first list, i.e. to_index > from_index.
        commit_and_tag_milestone(dir.path(), "test1", 1);
        write_feature(dir.path(), "v2");
        commit_and_tag_milestone(dir.path(), "test2", 1);

        let result = pending(dir.path(), Some(("test1", "test2")), false);

        assert!(matches!(result, Err(PendingError::InvalidRange)));
    }

    #[test]
    fn pending_reports_stale_when_feature_changed_again_after_the_original_change() {
        let (dir, blob2) = init_repo_with_pending_change();
        write_feature(dir.path(), "v3");
        commit_and_tag_milestone(dir.path(), "test3", 3);
        let blob3 = crate::id_cache::resolve_feature_blobs(dir.path(), "test3", false)
            .unwrap()
            .into_iter()
            .find(|b| b.id == "todo-edit")
            .unwrap()
            .blob_sha;
        write_changes(
            dir.path(),
            "test3",
            &format!(
                "- event_id: todo-edit--test2--test3\n  feature_id: todo-edit\n  from_milestone: test2\n  to_milestone: test3\n  from_blob: {blob2}\n  to_blob: {blob3}\n  impacted_testcases: []\n"
            ),
        );

        let report = pending(dir.path(), Some(("test1", "test2")), false).unwrap();

        assert!(report.pending.is_empty());
        assert_eq!(
            report.stale,
            vec![StaleEntry {
                case_id: "tc-edit-existing-todo-001".to_string(),
                feature_id: "todo-edit".to_string(),
                original_event_id: "todo-edit--test1--test2".to_string(),
                current_event: Some(ReflectedChange {
                    event_id: "todo-edit--test2--test3".to_string(),
                    from_milestone: "test2".to_string(),
                    to_milestone: "test3".to_string(),
                }),
            }]
        );
    }

    #[test]
    fn trace_finds_the_change_event_matching_the_verified_blob() {
        let dir = tempfile::tempdir().unwrap();
        write_results(
            dir.path(),
            "test2",
            "- case_id: tc-edit-existing-todo-001\n  result: pass\n  executor: soreiyu52\n  executed_at: 2026-08-08T16:38:52Z\n  verified_feature_blobs:\n    todo-edit: bbb\n",
        );
        write_changes(
            dir.path(),
            "test2",
            "- event_id: todo-edit--test1--test2\n  feature_id: todo-edit\n  from_milestone: test1\n  to_milestone: test2\n  from_blob: aaa\n  to_blob: bbb\n  impacted_testcases:\n  - tc-edit-existing-todo-001\n",
        );

        let result = trace(dir.path(), "tc-edit-existing-todo-001", "test2").unwrap();

        assert_eq!(result.case_id, "tc-edit-existing-todo-001");
        assert_eq!(result.executed_at, "2026-08-08T16:38:52Z");
        assert_eq!(
            result.entries,
            vec![TraceEntry {
                feature_id: "todo-edit".to_string(),
                reflects_change: Some(ReflectedChange {
                    event_id: "todo-edit--test1--test2".to_string(),
                    from_milestone: "test1".to_string(),
                    to_milestone: "test2".to_string(),
                }),
            }]
        );
    }

    #[test]
    fn trace_errors_when_no_execution_record_exists() {
        let dir = tempfile::tempdir().unwrap();

        let result = trace(dir.path(), "tc-edit-existing-todo-001", "test2");

        assert!(matches!(result, Err(TraceError::NoVerifiedBlobs)));
    }

    #[test]
    fn trace_returns_none_reflects_change_when_no_matching_change_event_exists() {
        let dir = tempfile::tempdir().unwrap();
        write_results(
            dir.path(),
            "test2",
            "- case_id: tc-edit-existing-todo-001\n  result: pass\n  executor: soreiyu52\n  executed_at: 2026-08-08T16:38:52Z\n  verified_feature_blobs:\n    todo-edit: bbb\n",
        );

        let result = trace(dir.path(), "tc-edit-existing-todo-001", "test2").unwrap();

        assert_eq!(
            result.entries,
            vec![TraceEntry {
                feature_id: "todo-edit".to_string(),
                reflects_change: None,
            }]
        );
    }
}
