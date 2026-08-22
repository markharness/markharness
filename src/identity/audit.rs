//! `IdentityAuditor` (design doc §11, ADR 0013 検証規則): the one place
//! that walks Git commit history itself, rather than comparing two
//! snapshots. `changes compute`/`verify`/`identity migrate` only ever look
//! at the `.markharness` state of one or two refs (see their `audit_scope`
//! JSON field) and trust that everything outside that narrow window is
//! sound; this module is what actually checks that trust is warranted —
//! that identity events, once committed, are never silently deleted or
//! rewritten, and that every commit's event set still replays without a
//! causal contradiction.
//!
//! Scope: the first-parent history of one ref (design doc's "全履歴" is
//! read as "the linear history that actually shipped on this branch", not
//! every ref/tag in the repository — a branch that was never merged was
//! also never published, so its own history isn't this project's audit
//! trail).

use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::path::Path;

use serde::Serialize;

use crate::git;
use crate::identity::{EntityKind, registry};
use crate::project_root::MARKHARNESS_DIR;

fn identity_events_root() -> String {
    format!("{MARKHARNESS_DIR}/identity-events")
}

fn kind_from_directory_segment(segment: &str) -> Option<EntityKind> {
    EntityKind::ALL
        .into_iter()
        .find(|kind| kind.directory_segment() == segment)
}

struct ParsedEventPath {
    kind: EntityKind,
    entity_uid: String,
    event_uid: String,
}

/// Parses `.markharness/identity-events/<kind>/<uid>/<event_uid>.yml`.
/// `None` for anything else under that root (there is nothing else there
/// in a well-formed project, but a hand-edited tree should be ignored
/// rather than panicking the audit).
fn parse_event_path(path: &str) -> Option<ParsedEventPath> {
    let prefix = format!("{}/", identity_events_root());
    let rest = path.strip_prefix(&prefix)?;
    let mut parts = rest.split('/');
    let kind_segment = parts.next()?;
    let entity_uid = parts.next()?;
    let file_name = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let event_uid = file_name.strip_suffix(".yml")?;
    let kind = kind_from_directory_segment(kind_segment)?;
    Some(ParsedEventPath {
        kind,
        entity_uid: entity_uid.to_string(),
        event_uid: event_uid.to_string(),
    })
}

/// One historical finding an `IdentityAuditor` walk turned up. Deliberately
/// not a `ValidationIssue` (`validate.rs`'s type): these describe a
/// property of *history* (a commit range, sometimes a since-deleted path),
/// which `ValidationIssue`'s single-snapshot `path`/`message` shape doesn't
/// carry.
///
/// `Serialize` (tagged, `type` field) and `Display` are both implemented
/// here, next to the enum itself, so a new variant is a compile error in
/// exactly one place (this file) instead of two hand-written `match`es in
/// `cli.rs` that could silently drift out of sync with each other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuditViolation {
    /// An identity event file present at an earlier commit is gone at a
    /// later one. Identity events are append-only by design (design doc
    /// §4.2) — no legitimate operation ever removes one — so this means
    /// either history was rewritten or the file was deleted out of band.
    EventDisappeared {
        kind: EntityKind,
        entity_uid: String,
        event_uid: String,
        path: String,
        missing_at_commit: String,
    },
    /// The same identity event file's content differs between two
    /// commits. An identity event is meant to be an immutable record once
    /// written.
    EventContentChanged {
        kind: EntityKind,
        entity_uid: String,
        event_uid: String,
        path: String,
        changed_at_commit: String,
    },
    /// At `commit`, replaying `entity_uid`'s event set (as it existed at
    /// that commit) failed. `error` is `engine::ReplayError`'s `Debug`
    /// rendering (e.g. a dangling predecessor left behind by a deleted
    /// event, or an unresolved branch divergence that was never actually
    /// committed as `Resolved`).
    CausalChainContradiction {
        kind: EntityKind,
        entity_uid: String,
        commit: String,
        error: String,
    },
}

impl fmt::Display for AuditViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuditViolation::EventDisappeared {
                kind,
                entity_uid,
                event_uid,
                path,
                missing_at_commit,
            } => write!(
                f,
                "event disappeared: {} '{entity_uid}' event '{event_uid}' is missing as of commit {missing_at_commit} ({path})",
                kind.as_str()
            ),
            AuditViolation::EventContentChanged {
                kind,
                entity_uid,
                event_uid,
                path,
                changed_at_commit,
            } => write!(
                f,
                "event content changed: {} '{entity_uid}' event '{event_uid}' differs as of commit {changed_at_commit} ({path})",
                kind.as_str()
            ),
            AuditViolation::CausalChainContradiction {
                kind,
                entity_uid,
                commit,
                error,
            } => write!(
                f,
                "causal chain contradiction: {} '{entity_uid}' at commit {commit}: {error}",
                kind.as_str()
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuditReport {
    /// First-parent commits actually walked.
    pub commits_scanned: usize,
    pub violations: Vec<AuditViolation>,
}

/// Replays `entity_uid`'s events as they existed at `commit` and appends a
/// `CausalChainContradiction` if that fails — whether the failure is a
/// logical one (`engine::ReplayError`, e.g. a dangling predecessor) or an
/// event file that fails to parse as YAML (`registry::resolve` surfaces
/// that specifically as `io::ErrorKind::InvalidData`, see `load_events`'s
/// doc comment). Deliberately does not abort the whole run for either: a
/// file that fails to parse is itself something the audit must report as
/// a violation, not something that discards every violation already found
/// (notably the `EventContentChanged` a caller will have already recorded
/// for that exact file, from the `git diff` that got us here).
///
/// A genuine infrastructure failure (Git command failed, filesystem
/// error) is a different matter — that's not a finding *about the
/// audited history*, it means the audit itself couldn't run, so it is
/// propagated with `?` rather than misreported as a causal-chain
/// contradiction in the entity's history.
fn check_causal_chain(
    root: &Path,
    commit: &str,
    kind: EntityKind,
    entity_uid: String,
    violations: &mut Vec<AuditViolation>,
) -> io::Result<()> {
    let error = match registry::resolve(root, commit, kind, &entity_uid, false) {
        Ok(Ok(_)) => return Ok(()),
        Ok(Err(replay_error)) => format!("{replay_error:?}"),
        Err(io_error) if io_error.kind() == io::ErrorKind::InvalidData => format!("{io_error}"),
        Err(io_error) => return Err(io_error),
    };
    violations.push(AuditViolation::CausalChainContradiction {
        kind,
        entity_uid,
        commit: commit.to_string(),
        error,
    });
    Ok(())
}

/// Walks `git_ref`'s first-parent history, diffing
/// `.markharness/identity-events/` between every consecutive commit pair
/// (cheap: one `git diff --name-status` per pair, not a full tree listing)
/// and replaying only the entities whose event set actually changed at
/// each step (design decision: full re-replay of every entity at every
/// commit does not scale with history length, and a diff-driven walk finds
/// the same contradictions since a causal break can only first appear at
/// the commit that changed the entity's events).
pub fn run_audit(root: &Path, git_ref: &str) -> io::Result<AuditReport> {
    let commits = git::first_parent_history(root, git_ref)?;
    let mut report = AuditReport {
        commits_scanned: commits.len(),
        violations: Vec::new(),
    };
    let path_in_repo = identity_events_root();

    // The oldest commit has no predecessor to diff against, so the
    // pairwise loop below never inspects it. A causal-chain contradiction
    // already present there (and never touched again) would otherwise go
    // undetected forever — replay every entity that has events at that
    // commit once, up front.
    if let Some(oldest) = commits.first() {
        let mut entities_at_oldest: BTreeSet<(EntityKind, String)> = BTreeSet::new();
        for entry in git::ls_tree_recursive(root, oldest, &path_in_repo)? {
            if entry.kind != git::ObjectKind::Blob {
                continue;
            }
            if let Some(parsed) = parse_event_path(&entry.path) {
                entities_at_oldest.insert((parsed.kind, parsed.entity_uid));
            }
        }
        for (kind, entity_uid) in entities_at_oldest {
            check_causal_chain(root, oldest, kind, entity_uid, &mut report.violations)?;
        }
    }

    for pair in commits.windows(2) {
        let (from, to) = (&pair[0], &pair[1]);
        let diffs = git::diff_name_status(root, from, to, &path_in_repo)?;
        let mut changed_entities: BTreeSet<(EntityKind, String)> = BTreeSet::new();

        for entry in &diffs {
            let Some(parsed) = parse_event_path(&entry.path) else {
                continue;
            };
            match entry.status {
                git::DiffStatus::Deleted => {
                    report.violations.push(AuditViolation::EventDisappeared {
                        kind: parsed.kind,
                        entity_uid: parsed.entity_uid.clone(),
                        event_uid: parsed.event_uid.clone(),
                        path: entry.path.clone(),
                        missing_at_commit: to.clone(),
                    });
                    changed_entities.insert((parsed.kind, parsed.entity_uid));
                }
                git::DiffStatus::Modified => {
                    report.violations.push(AuditViolation::EventContentChanged {
                        kind: parsed.kind,
                        entity_uid: parsed.entity_uid.clone(),
                        event_uid: parsed.event_uid.clone(),
                        path: entry.path.clone(),
                        changed_at_commit: to.clone(),
                    });
                    changed_entities.insert((parsed.kind, parsed.entity_uid));
                }
                git::DiffStatus::Added => {
                    changed_entities.insert((parsed.kind, parsed.entity_uid));
                }
            }
        }

        for (kind, entity_uid) in changed_entities {
            check_causal_chain(root, to, kind, entity_uid, &mut report.violations)?;
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let status = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .status()
                .unwrap()
        };
        assert!(status(&["init", "-q", "-b", "main"]).success());
        assert!(status(&["config", "user.email", "test@example.com"]).success());
        assert!(status(&["config", "user.name", "Test"]).success());
        dir
    }

    fn commit_all(dir: &Path, message: &str) -> String {
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["add", "-A"])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["commit", "-q", "-m", message])
            .status()
            .unwrap();
        String::from_utf8(
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string()
    }

    fn write_full_tree(root: &Path, feature_id: &str) {
        let knowledge = root.join(".markharness/knowledge/req-todo");
        let base = knowledge
            .join(feature_id)
            .join("todo-add-task/todo-add-task-empty-input");
        fs::create_dir_all(base.join("expected")).unwrap();
        fs::write(
            knowledge.join("requirement.yml"),
            "id: req-todo\nlabel: req-todo\naxis: []\n",
        )
        .unwrap();
        fs::write(
            knowledge.join(feature_id).join("feature.yml"),
            format!("id: {feature_id}\nrequirement: req-todo\nlabel: todo\naxis: []\n"),
        )
        .unwrap();
        fs::write(
            base.parent().unwrap().join("behavior.yml"),
            format!(
                "id: todo-add-task\nfeature: {feature_id}\nlabel: todo-add-task\naxis: []\ndescription: |\n  d\n"
            ),
        )
        .unwrap();
        fs::write(
            base.join("condition.yml"),
            "id: todo-add-task-empty-input\nbehavior: todo-add-task\nlabel: todo-add-task-empty-input\ndescription: |\n  d\n",
        )
        .unwrap();
        fs::write(
            base.join("expected/001.yml"),
            "id: todo-add-task-empty-input-001\ncondition: todo-add-task-empty-input\ndescription: |\n  d\n",
        )
        .unwrap();
    }

    #[test]
    fn event_disappeared_serializes_with_a_tagged_snake_case_type_field() {
        let violation = AuditViolation::EventDisappeared {
            kind: EntityKind::Feature,
            entity_uid: "uid-1".to_string(),
            event_uid: "event-1".to_string(),
            path: ".markharness/identity-events/features/uid-1/event-1.yml".to_string(),
            missing_at_commit: "deadbeef".to_string(),
        };

        let value = serde_json::to_value(&violation).unwrap();

        assert_eq!(value["type"], "event_disappeared");
        assert_eq!(value["kind"], "feature");
        assert_eq!(value["entity_uid"], "uid-1");
        assert_eq!(value["missing_at_commit"], "deadbeef");
    }

    #[test]
    fn causal_chain_contradiction_display_names_the_kind_entity_and_commit() {
        let violation = AuditViolation::CausalChainContradiction {
            kind: EntityKind::Behavior,
            entity_uid: "uid-2".to_string(),
            commit: "cafef00d".to_string(),
            error: "DanglingPredecessor".to_string(),
        };

        let rendered = violation.to_string();

        assert!(rendered.contains("behavior"));
        assert!(rendered.contains("uid-2"));
        assert!(rendered.contains("cafef00d"));
        assert!(rendered.contains("DanglingPredecessor"));
    }

    #[test]
    fn a_clean_migrate_history_has_no_violations() {
        let dir = init_repo();
        write_full_tree(dir.path(), "todo");
        commit_all(dir.path(), "initial");

        crate::identity::migrate_entities(dir.path()).unwrap();
        commit_all(dir.path(), "migrate");

        let report = run_audit(dir.path(), "HEAD").unwrap();

        assert!(
            report.violations.is_empty(),
            "unexpected violations: {:?}",
            report.violations
        );
        assert_eq!(report.commits_scanned, 2);
    }

    /// Deleting a *root* (no successor) identity event file out of band
    /// must be reported as `EventDisappeared` but cannot itself break
    /// replay for any other event (nothing points at it).
    #[test]
    fn detects_an_identity_event_file_deleted_out_of_band() {
        let dir = init_repo();
        write_full_tree(dir.path(), "todo");
        commit_all(dir.path(), "initial");
        crate::identity::migrate_entities(dir.path()).unwrap();
        commit_all(dir.path(), "migrate");

        let feature_uid = {
            let content = fs::read_to_string(
                dir.path()
                    .join(".markharness/knowledge/req-todo/todo/feature.yml"),
            )
            .unwrap();
            crate::knowledge::parse_feature(&content)
                .unwrap()
                .uid
                .unwrap()
        };
        let events_dir = dir
            .path()
            .join(".markharness/identity-events/features")
            .join(&feature_uid);
        let event_file = fs::read_dir(&events_dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::remove_file(&event_file).unwrap();
        let tamper_commit = commit_all(dir.path(), "tamper: delete event file");

        let report = run_audit(dir.path(), "HEAD").unwrap();

        assert!(
            report.violations.iter().any(|v| matches!(
                v,
                AuditViolation::EventDisappeared { entity_uid, missing_at_commit, .. }
                    if entity_uid == &feature_uid && missing_at_commit == &tamper_commit
            )),
            "expected an EventDisappeared violation, got {:?}",
            report.violations
        );
    }

    /// A causal-chain contradiction already present in the *oldest* scanned
    /// commit — nothing before it to diff against — must still be caught.
    /// The pairwise diff loop alone can never see it, since it only
    /// inspects paths that changed between two commits; a corruption that
    /// was there from the very first commit and is never touched again
    /// produces no diff at all.
    #[test]
    fn detects_a_causal_chain_contradiction_already_present_in_the_oldest_commit() {
        let dir = init_repo();
        let entity_uid = "01BROKEN00000000000000000";
        let events_dir = dir
            .path()
            .join(".markharness/identity-events/features")
            .join(entity_uid);
        fs::create_dir_all(&events_dir).unwrap();
        // A `Renamed` event whose predecessor was never actually written —
        // dangling from the moment this commit was made, not introduced by
        // any later tamper.
        fs::write(
            events_dir.join("01RENAMED0000000000000000.yml"),
            "identity_event_uid: 01RENAMED0000000000000000\nentity_uid: 01BROKEN00000000000000000\nentity_kind: feature\nprevious_identity_event_uid: 01MISSING0000000000000000\nrecorded_at: 2026-01-01T00:00:00Z\ntype: renamed\nfrom_id: a\nto_id: b\n",
        )
        .unwrap();
        commit_all(dir.path(), "already broken from the start");
        fs::write(dir.path().join("README.md"), "unrelated change\n").unwrap();
        commit_all(dir.path(), "unrelated follow-up commit");

        let report = run_audit(dir.path(), "HEAD").unwrap();

        assert!(
            report.violations.iter().any(|v| matches!(
                v,
                AuditViolation::CausalChainContradiction { entity_uid: uid, .. }
                    if uid == entity_uid
            )),
            "expected a CausalChainContradiction for the oldest commit's pre-existing break, got {:?}",
            report.violations
        );
    }

    /// Deleting a *root* event that a later `Renamed` event's
    /// `previous_identity_event_uid` still points at leaves that Renamed
    /// event's predecessor dangling — both `EventDisappeared` and
    /// `CausalChainContradiction` must be reported for the same tamper.
    #[test]
    fn deleting_a_root_event_with_a_dependent_also_breaks_replay() {
        let dir = init_repo();
        write_full_tree(dir.path(), "todo");
        commit_all(dir.path(), "initial");
        crate::identity::migrate_entities(dir.path()).unwrap();
        commit_all(dir.path(), "migrate");

        markharness_rename_feature(dir.path(), "todo", "todo-v2");
        commit_all(dir.path(), "rename");

        let feature_uid = {
            let content = fs::read_to_string(
                dir.path()
                    .join(".markharness/knowledge/req-todo/todo/feature.yml"),
            )
            .unwrap();
            crate::knowledge::parse_feature(&content)
                .unwrap()
                .uid
                .unwrap()
        };
        let events_dir = dir
            .path()
            .join(".markharness/identity-events/features")
            .join(&feature_uid);
        let mut root_event_path = None;
        for entry in fs::read_dir(&events_dir).unwrap() {
            let path = entry.unwrap().path();
            let content = fs::read_to_string(&path).unwrap();
            if content.contains("type: issued") {
                root_event_path = Some(path);
            }
        }
        let root_event_path = root_event_path.expect("expected an Issued root event");
        fs::remove_file(&root_event_path).unwrap();
        let tamper_commit = commit_all(dir.path(), "tamper: delete root event");

        let report = run_audit(dir.path(), "HEAD").unwrap();

        assert!(
            report.violations.iter().any(|v| matches!(
                v,
                AuditViolation::EventDisappeared { entity_uid, .. } if entity_uid == &feature_uid
            )),
            "expected EventDisappeared, got {:?}",
            report.violations
        );
        assert!(
            report.violations.iter().any(|v| matches!(
                v,
                AuditViolation::CausalChainContradiction { entity_uid, commit, .. }
                    if entity_uid == &feature_uid && commit == &tamper_commit
            )),
            "expected CausalChainContradiction, got {:?}",
            report.violations
        );
    }

    fn markharness_rename_feature(root: &Path, old_id: &str, new_id: &str) {
        crate::identity::rename_id(root, old_id, new_id).unwrap();
    }

    #[test]
    fn detects_an_identity_event_files_content_rewritten_out_of_band() {
        let dir = init_repo();
        write_full_tree(dir.path(), "todo");
        commit_all(dir.path(), "initial");
        crate::identity::migrate_entities(dir.path()).unwrap();
        commit_all(dir.path(), "migrate");

        let feature_uid = {
            let content = fs::read_to_string(
                dir.path()
                    .join(".markharness/knowledge/req-todo/todo/feature.yml"),
            )
            .unwrap();
            crate::knowledge::parse_feature(&content)
                .unwrap()
                .uid
                .unwrap()
        };
        let events_dir = dir
            .path()
            .join(".markharness/identity-events/features")
            .join(&feature_uid);
        let event_file = fs::read_dir(&events_dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let original = fs::read_to_string(&event_file).unwrap();
        fs::write(&event_file, format!("{original}# tampered\n")).unwrap();
        let tamper_commit = commit_all(dir.path(), "tamper: rewrite event content");

        let report = run_audit(dir.path(), "HEAD").unwrap();

        assert!(
            report.violations.iter().any(|v| matches!(
                v,
                AuditViolation::EventContentChanged { entity_uid, changed_at_commit, .. }
                    if entity_uid == &feature_uid && changed_at_commit == &tamper_commit
            )),
            "expected an EventContentChanged violation, got {:?}",
            report.violations
        );
    }

    /// Rewriting an event file's content into something that isn't valid
    /// `IdentityEvent` YAML must not abort the whole audit run: the
    /// `EventContentChanged` violation already found for that file has to
    /// survive in the returned report, and the replay failure itself
    /// should surface as a violation too, rather than the caller losing
    /// every violation collected so far to a bubbled-up I/O error.
    #[test]
    fn tampering_an_event_file_into_invalid_yaml_still_returns_a_report() {
        let dir = init_repo();
        write_full_tree(dir.path(), "todo");
        commit_all(dir.path(), "initial");
        crate::identity::migrate_entities(dir.path()).unwrap();
        commit_all(dir.path(), "migrate");

        let feature_uid = {
            let content = fs::read_to_string(
                dir.path()
                    .join(".markharness/knowledge/req-todo/todo/feature.yml"),
            )
            .unwrap();
            crate::knowledge::parse_feature(&content)
                .unwrap()
                .uid
                .unwrap()
        };
        let events_dir = dir
            .path()
            .join(".markharness/identity-events/features")
            .join(&feature_uid);
        let event_file = fs::read_dir(&events_dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::write(&event_file, "not: [valid, identity, event\n").unwrap();
        let tamper_commit = commit_all(dir.path(), "tamper: rewrite event into invalid yaml");

        let report = run_audit(dir.path(), "HEAD").unwrap();

        assert!(
            report.violations.iter().any(|v| matches!(
                v,
                AuditViolation::EventContentChanged { entity_uid, changed_at_commit, .. }
                    if entity_uid == &feature_uid && changed_at_commit == &tamper_commit
            )),
            "expected the EventContentChanged violation to survive, got {:?}",
            report.violations
        );
        assert!(
            report.violations.iter().any(|v| matches!(
                v,
                AuditViolation::CausalChainContradiction { entity_uid, commit, .. }
                    if entity_uid == &feature_uid && commit == &tamper_commit
            )),
            "expected a CausalChainContradiction for the unreplayable event, got {:?}",
            report.violations
        );
    }

    /// A genuine Git infrastructure failure (here: the loose object
    /// backing an event file's blob is gone, so `git cat-file -p` itself
    /// fails) must propagate as an error from `run_audit`, not be
    /// misreported as a `CausalChainContradiction` in the audited
    /// entity's history — that variant means "this history is broken",
    /// not "the audit tool couldn't read the repository".
    #[test]
    fn a_genuine_git_object_read_failure_propagates_instead_of_being_misreported() {
        let dir = init_repo();
        write_full_tree(dir.path(), "todo");
        commit_all(dir.path(), "initial");
        crate::identity::migrate_entities(dir.path()).unwrap();
        let migrate_commit = commit_all(dir.path(), "migrate");

        let feature_uid = {
            let content = fs::read_to_string(
                dir.path()
                    .join(".markharness/knowledge/req-todo/todo/feature.yml"),
            )
            .unwrap();
            crate::knowledge::parse_feature(&content)
                .unwrap()
                .uid
                .unwrap()
        };
        let event_path_in_repo = format!(
            ".markharness/identity-events/features/{feature_uid}/{}",
            fs::read_dir(
                dir.path()
                    .join(".markharness/identity-events/features")
                    .join(&feature_uid)
            )
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .file_name()
            .to_str()
            .unwrap()
        );
        let blob_sha = String::from_utf8(
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args([
                    "rev-parse",
                    &format!("{migrate_commit}:{event_path_in_repo}"),
                ])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string();
        let object_path = dir
            .path()
            .join(".git/objects")
            .join(&blob_sha[0..2])
            .join(&blob_sha[2..]);
        assert!(object_path.exists(), "expected the loose object to exist");
        fs::remove_file(&object_path).unwrap();

        let result = run_audit(dir.path(), "HEAD");

        assert!(
            result.is_err(),
            "expected a genuine object-read failure to propagate as Err, got {result:?}"
        );
    }

    /// The audit only follows first parents: commits that only ever
    /// existed on a merged-in side branch are not this branch's published
    /// history, so tampering isolated to that side branch must not be
    /// reported.
    #[test]
    fn does_not_scan_commits_only_reachable_via_a_merged_side_branch() {
        let dir = init_repo();
        write_full_tree(dir.path(), "todo");
        commit_all(dir.path(), "base");

        std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["checkout", "-q", "-b", "side"])
            .status()
            .unwrap();
        crate::identity::migrate_entities(dir.path()).unwrap();
        commit_all(dir.path(), "migrate on side");
        let feature_uid = {
            let content = fs::read_to_string(
                dir.path()
                    .join(".markharness/knowledge/req-todo/todo/feature.yml"),
            )
            .unwrap();
            crate::knowledge::parse_feature(&content)
                .unwrap()
                .uid
                .unwrap()
        };
        let events_dir = dir
            .path()
            .join(".markharness/identity-events/features")
            .join(&feature_uid);
        let event_file = fs::read_dir(&events_dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::remove_file(&event_file).unwrap();
        commit_all(dir.path(), "tamper on side, before merge");

        std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["checkout", "-q", "main"])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["merge", "-q", "--no-ff", "side", "-m", "merge"])
            .status()
            .unwrap();

        let report = run_audit(dir.path(), "HEAD").unwrap();

        assert_eq!(
            report.commits_scanned, 2,
            "expected only base + merge on first-parent history"
        );
        assert!(
            report.violations.is_empty(),
            "tampering isolated to the merged side branch must not be visible: {:?}",
            report.violations
        );
    }
}
