use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use serde::Serialize;

use crate::backfill;
use crate::execution::ExecutionEntry;
use crate::generate::{generate_testcases, list_files_recursive, serialize_testcase};
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
    /// Path relative to `generated/` (not `generated/testcases/`), so a
    /// caller can print `generated/{file_name}` uniformly for every entry.
    /// Testcase files are prefixed `testcases/...` (mirroring `knowledge/`'s
    /// nesting since Step A); the traceability index is the bare
    /// `traceability-index.json`, which actually lives directly under
    /// `generated/`, not under `generated/testcases/`. Forward-slash
    /// separated regardless of platform.
    pub file_name: String,
    pub kind: DiffKind,
}

/// Forward-slash-normalizes a path relative to `generated/testcases/`, then
/// prefixes it with `testcases/` so it is relative to `generated/` like
/// every other `DiffEntry::file_name` (see that field's doc comment).
fn to_diff_key(relative_path: &Path) -> String {
    format!(
        "testcases/{}",
        relative_path.to_string_lossy().replace('\\', "/")
    )
}

fn read_existing_testcases(generated_dir: &Path) -> io::Result<BTreeMap<String, String>> {
    let mut existing = BTreeMap::new();
    for relative_path in list_files_recursive(generated_dir)? {
        let content = fs::read_to_string(generated_dir.join(&relative_path))?;
        existing.insert(to_diff_key(&relative_path), content);
    }
    Ok(existing)
}

/// Regenerates testcases from `root/knowledge` and compares them against the
/// committed files in `root/generated/testcases/`, without writing anything.
pub fn diff_generated_testcases(root: &Path) -> io::Result<Vec<DiffEntry>> {
    let testcases = generate_testcases(
        &root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge"),
    )?;
    let mut expected: BTreeMap<String, String> = BTreeMap::new();
    for testcase in &testcases {
        let key = to_diff_key(&testcase.relative_path());
        expected.insert(key, serialize_testcase(testcase));
    }

    let existing = read_existing_testcases(
        &root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("generated")
            .join("testcases"),
    )?;

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
    let index_path = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("generated")
        .join("traceability-index.json");
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

/// Which ChangeEvent a `verified_feature_tree_shas` entry reflects (§3.1 Q1).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReflectedChange {
    pub event_id: String,
    pub from_milestone: String,
    pub to_milestone: String,
}

/// One Feature's trace result within a TestExecution (a TestCase can span
/// more than one Feature per §2.1's map-shaped `verified_feature_tree_shas`).
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct TraceEntry {
    pub feature_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature_uid: Option<String>,
    /// `None` when no ChangeEvent with a matching `to_tree_sha` exists in
    /// `changes/` (e.g. `changes compute`/`backfill` hasn't been run for the
    /// milestone pair where this blob last changed).
    pub reflects_change: Option<ReflectedChange>,
}

/// The `verify trace` answer for one TestExecution record.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct TraceResult {
    /// Always `AuditScope::TwoSnapshot` (design doc §11, ADR 0013 検証規則):
    /// `trace` only ever compares the two `.markharness` snapshots named by
    /// `verified_feature_tree_shas` and `changes/`, never full commit
    /// history — that's `identity audit`'s job. A machine-readable marker
    /// so a CI gate can tell the two apart without documentation alone.
    pub audit_scope: crate::audit_scope::AuditScope,
    pub case_id: String,
    pub executed_at: String,
    pub entries: Vec<TraceEntry>,
}

#[derive(Debug)]
pub enum TraceError {
    /// No `results.yml` record for this `case_id` at this milestone, or the
    /// record predates `verified_feature_tree_shas` (no retroactive backfill,
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
    let path = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("executions")
        .join(milestone)
        .join("results.yml");
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
    let changes_dir = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("changes");
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

/// Q1 (§3.1): for each Feature a TestExecution's `verified_feature_tree_shas`
/// names, finds the ChangeEvent whose `to_tree_sha` matches the recorded
/// tree SHA — i.e. which change this execution result reflects.
pub fn trace(root: &Path, case_id: &str, milestone: &str) -> Result<TraceResult, TraceError> {
    let entries = read_results(root, milestone)?;
    let Some(entry) = entries.iter().rev().find(|e| e.case_id == case_id) else {
        return Err(TraceError::NoVerifiedBlobs);
    };
    if entry.verified_feature_tree_shas.is_empty() {
        return Err(TraceError::NoVerifiedBlobs);
    }

    let all_changes = read_all_changes(root)?;

    let mut trace_entries = Vec::new();
    for (feature_identity, tree_sha) in &entry.verified_feature_tree_shas {
        // `feature_identity` is `execution::verified_feature_tree_sha`'s
        // key (ADR 0013: `uid` when the Feature had one at record time,
        // else `feature_id`) — match ChangeEvents the same way so a
        // uid-tagged Feature is found even if `changes compute` later saw
        // it under a different `feature_id` (post-rename).
        let matching_event = all_changes.iter().find(|e| {
            e.identity_key() == feature_identity && e.to_tree_sha.as_deref() == Some(tree_sha)
        });
        let reflects_change = matching_event.map(|e| ReflectedChange {
            event_id: e.event_id.clone(),
            from_milestone: e.from_milestone.clone(),
            to_milestone: e.to_milestone.clone(),
        });
        trace_entries.push(TraceEntry {
            feature_id: matching_event
                .map(|event| event.feature_id.clone())
                .unwrap_or_else(|| feature_identity.clone()),
            feature_uid: matching_event.and_then(|event| event.feature_uid.clone()),
            reflects_change,
        });
    }
    Ok(TraceResult {
        audit_scope: crate::audit_scope::AuditScope::TwoSnapshot,
        case_id: entry.case_id.clone(),
        executed_at: entry.executed_at.clone(),
        entries: trace_entries,
    })
}

/// One impacted TestCase not yet re-executed against the ChangeEvent's
/// `to_tree_sha` (§3.2/§3.3 pending: the target hasn't moved since the change).
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

/// Filesystem-independent input for deciding whether an impacted testcase
/// still needs verification. The loader resolves Git and execution data;
/// the engine below only compares values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCandidate {
    pub case_id: String,
    pub feature_id: String,
    pub original_event_id: String,
    pub target_tree_sha: Option<String>,
    pub current_tree_sha: Option<String>,
    pub current_event: Option<ReflectedChange>,
    pub reexecuted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationStatus {
    Current,
    Pending,
    Stale,
    Unknown,
}

pub fn evaluate_pending_candidate(candidate: &PendingCandidate) -> VerificationStatus {
    if candidate.reexecuted {
        VerificationStatus::Current
    } else if candidate.current_tree_sha.is_none() {
        VerificationStatus::Unknown
    } else if candidate.current_tree_sha == candidate.target_tree_sha {
        VerificationStatus::Pending
    } else {
        VerificationStatus::Stale
    }
}

#[derive(Debug)]
pub enum PendingError {
    /// `range` was `None` and fewer than two milestones exist to pair.
    NoMilestonePair,
    /// An explicitly given `--from`/`--to` milestone has no
    /// `executions/<name>/milestone.yml`.
    MilestoneNotFound,
    /// `to` is not strictly newer than `from` (by committer date, with
    /// ancestry breaking same-second ties — see `backfill::order_by_recency`).
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
        let reexecuted = was_reexecuted(root, at_or_after_to, &case_id, &event)?;

        // ADR 0013: resolve the Feature's current state by identity (`uid`
        // when the ChangeEvent has one, else `feature_id`) rather than by
        // `feature_id` alone, so a Feature renamed since this event still
        // matches its later `uid`-tagged versions and ChangeEvents.
        let event_identity = event.identity_key().to_string();
        let current_tree_sha = match current_milestone {
            Some(m) => {
                id_cache::by_identity_key(id_cache::resolve_feature_versions(root, m, use_cache)?)
                    .get(&event_identity)
                    .map(|v| v.tree_sha.clone())
            }
            None => None,
        };

        let current_event = current_tree_sha.as_ref().and_then(|current_tree_sha| {
            all_changes
                .iter()
                .find(|e| {
                    e.identity_key() == event_identity
                        && e.to_tree_sha.as_ref() == Some(current_tree_sha)
                })
                .map(|e| ReflectedChange {
                    event_id: e.event_id.clone(),
                    from_milestone: e.from_milestone.clone(),
                    to_milestone: e.to_milestone.clone(),
                })
        });
        let candidate = PendingCandidate {
            case_id,
            feature_id: event.feature_id,
            original_event_id: event.event_id,
            target_tree_sha: event.to_tree_sha,
            current_tree_sha,
            current_event,
            reexecuted,
        };

        match evaluate_pending_candidate(&candidate) {
            VerificationStatus::Current => {}
            VerificationStatus::Pending => report.pending.push(PendingEntry {
                case_id: candidate.case_id,
                feature_id: candidate.feature_id,
                event_id: candidate.original_event_id,
            }),
            VerificationStatus::Unknown if candidate.target_tree_sha.is_none() => {
                report.pending.push(PendingEntry {
                    case_id: candidate.case_id,
                    feature_id: candidate.feature_id,
                    event_id: candidate.original_event_id,
                });
            }
            VerificationStatus::Stale | VerificationStatus::Unknown => {
                report.stale.push(StaleEntry {
                    case_id: candidate.case_id,
                    feature_id: candidate.feature_id,
                    original_event_id: candidate.original_event_id,
                    current_event: candidate.current_event,
                });
            }
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
    // ADR 0013: `verified_feature_tree_shas` is keyed by identity (`uid`
    // when the Feature has one, else `feature_id` —
    // `execution::verified_feature_tree_sha`'s convention), so the lookup
    // key must match the ChangeEvent's own identity the same way.
    let event_identity = event.identity_key();
    for milestone in milestones {
        let entries = read_results(root, milestone)?;
        if entries.iter().any(|e| {
            e.case_id == case_id
                && e.verified_feature_tree_shas.get(event_identity) == event.to_tree_sha.as_ref()
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
        let dir = root
            .join(".markharness/knowledge/req-todo/todo/todo-add-task/todo-add-task-empty-input");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            root.join(".markharness/knowledge/req-todo/requirement.yml"),
            "id: req-todo\nlabel: req-todo\naxis: [ui]\n",
        )
        .unwrap();
        fs::write(
            root.join(".markharness/knowledge/req-todo/todo/feature.yml"),
            "id: todo\nrequirement: req-todo\nlabel: todo\naxis: [ui]\n",
        )
        .unwrap();
        fs::write(
            root.join(".markharness/knowledge/req-todo/todo/todo-add-task/behavior.yml"),
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
        let testcases = generate_testcases(
            &root
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();
        let index = build_index(&testcases);
        fs::write(
            root.join(".markharness/generated/traceability-index.json"),
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
                file_name: "testcases/req-todo/todo/todo-add-task/todo-add-task-empty-input.yml"
                    .to_string(),
                kind: DiffKind::Added,
            }]
        );
    }

    #[test]
    fn reports_no_diff_when_committed_file_matches_regeneration() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_knowledge_todo_add_task(dir.path());

        let testcases = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();
        let testcases_dir = dir.path().join(".markharness/generated/testcases");
        for testcase in &testcases {
            let path = testcases_dir.join(testcase.relative_path());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, serialize_testcase(testcase)).unwrap();
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

        let testcases_dir = dir
            .path()
            .join(".markharness/generated/testcases/req-todo/todo/todo-add-task");
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
                file_name: "testcases/req-todo/todo/todo-add-task/todo-add-task-empty-input.yml"
                    .to_string(),
                kind: DiffKind::Changed,
            }]
        );
    }

    #[test]
    fn reports_removed_when_committed_file_no_longer_generated() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        let testcases_dir = dir
            .path()
            .join(".markharness/generated/testcases/req-todo/todo/todo-add-task");
        fs::create_dir_all(&testcases_dir).unwrap();
        fs::write(testcases_dir.join("stale-condition.yml"), "stale content\n").unwrap();
        write_matching_index(dir.path());

        let diffs = diff_generated_testcases(dir.path()).unwrap();

        assert_eq!(
            diffs,
            vec![DiffEntry {
                file_name: "testcases/req-todo/todo/todo-add-task/stale-condition.yml".to_string(),
                kind: DiffKind::Removed,
            }]
        );
    }

    #[test]
    fn reports_added_for_traceability_index_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_knowledge_todo_add_task(dir.path());
        let testcases = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();
        let testcases_dir = dir.path().join(".markharness/generated/testcases");
        for testcase in &testcases {
            let path = testcases_dir.join(testcase.relative_path());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, serialize_testcase(testcase)).unwrap();
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
        let testcases = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();
        let testcases_dir = dir.path().join(".markharness/generated/testcases");
        for testcase in &testcases {
            let path = testcases_dir.join(testcase.relative_path());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, serialize_testcase(testcase)).unwrap();
        }
        fs::write(
            dir.path()
                .join(".markharness/generated/traceability-index.json"),
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
        let dir = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("executions")
            .join(milestone);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("results.yml"), yaml).unwrap();
    }

    fn write_changes(root: &Path, to_milestone: &str, yaml: &str) {
        let dir = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("changes");
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
        fs::create_dir_all(
            root.join(crate::project_root::MARKHARNESS_DIR)
                .join("executions")
                .join(milestone),
        )
        .unwrap();
        fs::write(
            root.join(crate::project_root::MARKHARNESS_DIR)
                .join("executions")
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
        let dir = root.join(".markharness/knowledge/req-todo/todo-edit");
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
    /// as impacted. Returns the `to_tree_sha` SHA actually produced.
    fn init_repo_with_pending_change() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);

        write_feature(dir.path(), "v1");
        commit_and_tag_milestone(dir.path(), "test1", 1);

        write_feature(dir.path(), "v2");
        commit_and_tag_milestone(dir.path(), "test2", 2);

        let to_tree_sha = crate::id_cache::resolve_feature_versions(dir.path(), "test2", false)
            .unwrap()
            .into_iter()
            .find(|v| v.id == "todo-edit")
            .unwrap()
            .tree_sha;

        write_changes(
            dir.path(),
            "test2",
            &format!(
                "- event_id: todo-edit--test1--test2\n  feature_id: todo-edit\n  from_milestone: test1\n  to_milestone: test2\n  from_tree_sha: null\n  to_tree_sha: {to_tree_sha}\n  impacted_testcases:\n  - tc-edit-existing-todo-001\n"
            ),
        );

        (dir, to_tree_sha)
    }

    #[test]
    fn pending_reports_impacted_testcase_not_yet_reexecuted() {
        let (dir, to_tree_sha) = init_repo_with_pending_change();

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
        let _ = to_tree_sha;
    }

    #[test]
    fn pending_does_not_report_testcase_already_reexecuted_against_the_new_blob() {
        let (dir, to_tree_sha) = init_repo_with_pending_change();
        write_results(
            dir.path(),
            "test2",
            &format!(
                "- case_id: tc-edit-existing-todo-001\n  result: pass\n  executor: soreiyu52\n  executed_at: 2026-08-08T16:38:52Z\n  verified_feature_tree_shas:\n    todo-edit: {to_tree_sha}\n"
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
        commit_and_tag_milestone(dir.path(), "test1", 1);
        write_feature(dir.path(), "v2");
        commit_and_tag_milestone(dir.path(), "test2", 2);

        // `test1` is genuinely `test2`'s descendant here (created later, in
        // the same history), so asking for `--from test2 --to test1` is a
        // real inversion, not just a naming choice.
        let result = pending(dir.path(), Some(("test2", "test1")), false);

        assert!(matches!(result, Err(PendingError::InvalidRange)));
    }

    /// Regression test for the flaky README quick start: two milestones
    /// committed within the same wall-clock second used to leave
    /// `order_by_recency` unable to break the committer-date tie, so an
    /// explicit `--from test1 --to test2` (correct in history) was wrongly
    /// rejected as `InvalidRange`. Ancestry must resolve the tie instead.
    #[test]
    fn pending_accepts_explicit_range_when_milestones_share_the_same_committer_second() {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        write_feature(dir.path(), "v1");
        commit_and_tag_milestone(dir.path(), "test1", 1);
        write_feature(dir.path(), "v2");
        commit_and_tag_milestone(dir.path(), "test2", 1);

        let result = pending(dir.path(), Some(("test1", "test2")), false);

        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    fn write_feature_with_uid(root: &Path, id: &str, label: &str, uid: &str) {
        let dir = root.join(".markharness/knowledge/req-todo/todo-edit");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("feature.yml"),
            format!("id: {id}\nrequirement: req-todo\nlabel: {label}\naxis: []\nuid: {uid}\n"),
        )
        .unwrap();
    }

    /// ADR 0013: when a Feature is renamed (uid preserved) between the
    /// milestone a ChangeEvent recorded it at and the current milestone,
    /// `pending` must still resolve the Feature's current tree SHA and
    /// `current_event` by `uid` — not lose track of it because `feature_id`
    /// no longer matches.
    #[test]
    fn pending_resolves_current_state_by_uid_across_a_rename() {
        const UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);

        write_feature_with_uid(dir.path(), "todo-edit", "v1", UID);
        commit_and_tag_milestone(dir.path(), "test1", 1);

        write_feature_with_uid(dir.path(), "todo-edit", "v2", UID);
        commit_and_tag_milestone(dir.path(), "test2", 2);

        let tree_sha2 = crate::id_cache::resolve_feature_versions(dir.path(), "test2", false)
            .unwrap()
            .into_iter()
            .find(|v| v.uid.as_deref() == Some(UID))
            .unwrap()
            .tree_sha;
        write_changes(
            dir.path(),
            "test2",
            &format!(
                "- event_id: todo-edit--test1--test2\n  feature_id: todo-edit\n  feature_uid: {UID}\n  from_milestone: test1\n  to_milestone: test2\n  from_tree_sha: null\n  to_tree_sha: {tree_sha2}\n  impacted_testcases:\n  - tc-edit-existing-todo-001\n"
            ),
        );

        // Renamed *and* changed again on the way to test3 — same uid, new
        // id, so a naive `v.id == event.feature_id` lookup at test3 would
        // no longer find this Feature at all.
        write_feature_with_uid(dir.path(), "todo-edit-item", "v3", UID);
        commit_and_tag_milestone(dir.path(), "test3", 3);
        let tree_sha3 = crate::id_cache::resolve_feature_versions(dir.path(), "test3", false)
            .unwrap()
            .into_iter()
            .find(|v| v.uid.as_deref() == Some(UID))
            .unwrap()
            .tree_sha;
        write_changes(
            dir.path(),
            "test3",
            &format!(
                "- event_id: todo-edit-item--test2--test3\n  feature_id: todo-edit-item\n  feature_uid: {UID}\n  from_milestone: test2\n  to_milestone: test3\n  from_tree_sha: {tree_sha2}\n  to_tree_sha: {tree_sha3}\n  impacted_testcases: []\n"
            ),
        );

        let report = pending(dir.path(), Some(("test1", "test2")), false).unwrap();

        // The Feature moved on again (renamed + re-edited) before anyone
        // re-ran the case: `stale`, and — because lookup is uid-based —
        // `current_event` correctly names the rename's ChangeEvent instead
        // of coming back `None` for lack of an `id` match.
        assert!(report.pending.is_empty());
        assert_eq!(
            report.stale,
            vec![StaleEntry {
                case_id: "tc-edit-existing-todo-001".to_string(),
                feature_id: "todo-edit".to_string(),
                original_event_id: "todo-edit--test1--test2".to_string(),
                current_event: Some(ReflectedChange {
                    event_id: "todo-edit-item--test2--test3".to_string(),
                    from_milestone: "test2".to_string(),
                    to_milestone: "test3".to_string(),
                }),
            }],
            "expected uid-based lookup to find the rename's ChangeEvent, got {report:?}"
        );
    }

    #[test]
    fn trace_keeps_the_display_id_separate_from_the_feature_uid() {
        const UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".markharness/executions/m2")).unwrap();
        fs::create_dir_all(dir.path().join(".markharness/changes")).unwrap();
        fs::write(
            dir.path().join(".markharness/executions/m2/results.yml"),
            format!("- case_id: tc-1\n  result: pass\n  executor: test\n  executed_at: 2026-08-20T00:00:00Z\n  verified_feature_tree_shas:\n    {UID}: tree-2\n"),
        )
        .unwrap();
        fs::write(
            dir.path().join(".markharness/changes/m2.yaml"),
            format!("- event_id: event-1\n  feature_id: renamed-feature\n  feature_uid: {UID}\n  from_milestone: m1\n  to_milestone: m2\n  from_tree_sha: tree-1\n  to_tree_sha: tree-2\n  impacted_testcases: []\n"),
        )
        .unwrap();

        let result = trace(dir.path(), "tc-1", "m2").unwrap();

        assert_eq!(result.entries[0].feature_id, "renamed-feature");
        assert_eq!(result.entries[0].feature_uid.as_deref(), Some(UID));
    }

    #[test]
    fn pending_reports_stale_when_feature_changed_again_after_the_original_change() {
        let (dir, tree_sha2) = init_repo_with_pending_change();
        write_feature(dir.path(), "v3");
        commit_and_tag_milestone(dir.path(), "test3", 3);
        let tree_sha3 = crate::id_cache::resolve_feature_versions(dir.path(), "test3", false)
            .unwrap()
            .into_iter()
            .find(|v| v.id == "todo-edit")
            .unwrap()
            .tree_sha;
        write_changes(
            dir.path(),
            "test3",
            &format!(
                "- event_id: todo-edit--test2--test3\n  feature_id: todo-edit\n  from_milestone: test2\n  to_milestone: test3\n  from_tree_sha: {tree_sha2}\n  to_tree_sha: {tree_sha3}\n  impacted_testcases: []\n"
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
            "- case_id: tc-edit-existing-todo-001\n  result: pass\n  executor: soreiyu52\n  executed_at: 2026-08-08T16:38:52Z\n  verified_feature_tree_shas:\n    todo-edit: bbb\n",
        );
        write_changes(
            dir.path(),
            "test2",
            "- event_id: todo-edit--test1--test2\n  feature_id: todo-edit\n  from_milestone: test1\n  to_milestone: test2\n  from_tree_sha: aaa\n  to_tree_sha: bbb\n  impacted_testcases:\n  - tc-edit-existing-todo-001\n",
        );

        let result = trace(dir.path(), "tc-edit-existing-todo-001", "test2").unwrap();

        assert_eq!(
            result.audit_scope,
            crate::audit_scope::AuditScope::TwoSnapshot
        );
        assert_eq!(result.case_id, "tc-edit-existing-todo-001");
        assert_eq!(result.executed_at, "2026-08-08T16:38:52Z");
        assert_eq!(
            result.entries,
            vec![TraceEntry {
                feature_id: "todo-edit".to_string(),
                feature_uid: None,
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
            "- case_id: tc-edit-existing-todo-001\n  result: pass\n  executor: soreiyu52\n  executed_at: 2026-08-08T16:38:52Z\n  verified_feature_tree_shas:\n    todo-edit: bbb\n",
        );

        let result = trace(dir.path(), "tc-edit-existing-todo-001", "test2").unwrap();

        assert_eq!(
            result.entries,
            vec![TraceEntry {
                feature_id: "todo-edit".to_string(),
                feature_uid: None,
                reflects_change: None,
            }]
        );
    }
}
