use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::changes;
use crate::fs_safety::replace_file;
use crate::git;

/// git notes namespace used to record backfill progress per to-milestone tag
/// (§4.3). Kept out of the default `refs/notes/commits` namespace so it
/// never collides with notes a human or another tool might attach.
const NOTES_REF: &str = "markharness-backfill";

/// A pair skipped because its Knowledge schema versions couldn't be
/// compared safely (issue #29 §5). `reason` is the fail-closed gate's own
/// error message verbatim — issue #29 §5 requires it to name both sides'
/// versions and state that a CLI update or migration is needed, and
/// discarding it in favor of a generic message would lose that (Spec
/// review).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncompatiblePair {
    pub to_milestone: String,
    pub reason: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct BackfillReport {
    /// to-milestone names for pairs newly computed by this run, most recent first.
    pub processed: Vec<String>,
    /// to-milestone names for pairs already backfilled in a previous run.
    pub skipped: Vec<String>,
    /// Pairs whose Knowledge schema versions couldn't be compared safely —
    /// not recorded in git notes, so a later run retries them (e.g. once a
    /// converter exists).
    pub incompatible: Vec<IncompatiblePair>,
    /// Legacy-schema-version-fallback warnings collected across every
    /// processed pair (issue #29 §6) — the same warnings `changes compute`
    /// surfaces, so `backfill run` uses an identical policy.
    pub warnings: Vec<String>,
    /// True when a pair or time limit left at least one unprocessed pair.
    pub stopped_by_limit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackfillPolicy {
    pub use_cache: bool,
    pub max_pairs: Option<usize>,
    pub time_budget: Option<Duration>,
}

/// Milestone names are `executions/<name>/milestone.yml` directory names,
/// which UC4 assumes match a `git tag <name>` (docs/en/cli-manual.md §1.1/UC4).
pub fn list_milestone_names(root: &Path) -> io::Result<Vec<String>> {
    let executions_dir = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("executions");
    let Ok(entries) = fs::read_dir(&executions_dir) else {
        return Ok(Vec::new());
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|path| path.is_dir() && path.join("milestone.yml").is_file())
        .filter_map(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    names.sort();
    Ok(names)
}

/// Orders milestone names newest-first by their tag's committer date.
/// Milestones whose tag cannot be resolved (name/tag mismatch) are dropped
/// rather than failing the whole run.
///
/// Two milestones committed within the same wall-clock second get the same
/// committer-date string, which the initial date sort cannot break a tie
/// on. A follow-up ancestry-aware pass (`git merge-base --is-ancestor`)
/// corrects any adjacent pair the date sort left in the wrong order: if the
/// name currently placed "newer" is actually an ancestor of the one behind
/// it, the two are swapped. Pairs with no provable ancestor relationship
/// (e.g. diverged branches) are left as the date sort placed them.
pub fn order_by_recency(root: &Path, names: Vec<String>) -> Vec<String> {
    let mut dated: Vec<(String, String)> = names
        .into_iter()
        .filter_map(|name| git::commit_date(root, &name).ok().map(|date| (name, date)))
        .collect();
    dated.sort_by(|a, b| b.1.cmp(&a.1));
    let mut ordered: Vec<String> = dated.into_iter().map(|(name, _)| name).collect();

    let len = ordered.len();
    for _ in 0..len {
        let mut swapped = false;
        for i in 0..len.saturating_sub(1) {
            let newer_is_actually_older =
                git::is_ancestor(root, &ordered[i], &ordered[i + 1]).unwrap_or(false);
            if newer_is_actually_older {
                ordered.swap(i, i + 1);
                swapped = true;
            }
        }
        if !swapped {
            break;
        }
    }
    ordered
}

fn already_processed(root: &Path, to_milestone: &str) -> bool {
    matches!(git::notes_show(root, NOTES_REF, to_milestone), Ok(Some(_)))
}

/// Runs one batch of UC6 backfill: pairs each milestone with the one
/// immediately before it in time and computes `changes/<to>.yaml` for any
/// pair not yet recorded in git notes, processing the most recent pairs
/// first (§4.2). Returns after a single pass over all pairs — safe to
/// re-invoke (e.g. from CI on a schedule) since already-processed pairs are
/// skipped via `git notes` (§4.3).
pub fn backfill_run(root: &Path, use_cache: bool) -> io::Result<BackfillReport> {
    backfill_run_with_policy(
        root,
        BackfillPolicy {
            use_cache,
            max_pairs: None,
            time_budget: None,
        },
    )
}

pub fn backfill_run_with_policy(root: &Path, policy: BackfillPolicy) -> io::Result<BackfillReport> {
    let names = list_milestone_names(root)?;
    let ordered = order_by_recency(root, names);
    let started_at = Instant::now();

    let mut report = BackfillReport::default();
    for pair in ordered.windows(2) {
        let to_milestone = &pair[0];
        let from_milestone = &pair[1];

        if already_processed(root, to_milestone) {
            report.skipped.push(to_milestone.clone());
            continue;
        }

        let pair_limit_reached = policy
            .max_pairs
            .is_some_and(|limit| report.processed.len() >= limit);
        let time_limit_reached = policy
            .time_budget
            .is_some_and(|budget| started_at.elapsed() >= budget);
        if pair_limit_reached || time_limit_reached {
            report.stopped_by_limit = true;
            break;
        }

        let outcome = match changes::compute_changes_with_warnings(
            root,
            from_milestone,
            to_milestone,
            changes::ChangeOptions {
                cache: if policy.use_cache {
                    changes::CachePolicy::Use
                } else {
                    changes::CachePolicy::Bypass
                },
                impact_source: changes::ImpactSource::HistoricalTree,
            },
        ) {
            Ok(outcome) => outcome,
            Err(e) if e.kind() == io::ErrorKind::Unsupported => {
                report.incompatible.push(IncompatiblePair {
                    to_milestone: to_milestone.clone(),
                    reason: e.to_string(),
                });
                continue;
            }
            Err(e) => return Err(e),
        };
        report.warnings.extend(outcome.warnings);
        let changes_dir = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("changes");
        replace_file(
            root,
            &changes_dir.join(format!("{to_milestone}.yaml")),
            changes::serialize_changes(&outcome.events).as_bytes(),
        )?;
        git::notes_add(
            root,
            NOTES_REF,
            to_milestone,
            &format!("backfilled from {from_milestone}"),
        )?;
        report.processed.push(to_milestone.clone());
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn run_git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        dir
    }

    fn write_feature(root: &Path, label: &str) {
        let dir = root.join(".markharness/knowledge/controls/player-jump");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            root.join(".markharness/knowledge/controls/requirement.yml"),
            "id: controls\nlabel: controls\naxis: [gameplay]\n",
        )
        .unwrap();
        fs::write(
            dir.join("feature.yml"),
            format!("id: player-jump\nrequirement: controls\nlabel: {label}\naxis: [gameplay]\n"),
        )
        .unwrap();
    }

    /// Commits and tags a milestone with an explicit committer date (hours
    /// after a fixed epoch, keyed by `hour_offset`) so ordering by recency
    /// is deterministic regardless of how fast the test actually runs —
    /// real commits made back-to-back can otherwise land in the same
    /// wall-clock second.
    fn commit_and_tag_milestone(root: &Path, message: &str, milestone: &str, hour_offset: u32) {
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
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["commit", "-q", "-m", message])
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .status()
            .unwrap();
        assert!(status.success(), "git commit failed");
        run_git(root, &["tag", milestone]);
    }

    fn write_config_toml(root: &Path, knowledge_schema_version: u32) {
        fs::create_dir_all(root.join(".markharness")).unwrap();
        fs::write(
            root.join(".markharness/config.toml"),
            format!(
                "schema_version = 1\n\n[knowledge]\nschema_version = {knowledge_schema_version}\n"
            ),
        )
        .unwrap();
    }

    /// Issue #29 §backfill policy: a pair whose Knowledge schema versions
    /// differ (no converter yet) must not abort the whole run — it is
    /// skipped so unrelated pairs still get their `changes/<to>.yaml`.
    #[test]
    fn backfill_run_skips_an_incompatible_pair_and_continues_with_the_rest() {
        let dir = init_repo();
        write_feature(dir.path(), "v1");
        write_config_toml(dir.path(), 2); // unknown to this CLI build (CURRENT_KNOWLEDGE_SCHEMA_VERSION == 1)
        commit_and_tag_milestone(dir.path(), "v1", "m1", 1);
        write_feature(dir.path(), "v2");
        write_config_toml(dir.path(), 1);
        commit_and_tag_milestone(dir.path(), "v2", "m2", 2);
        write_feature(dir.path(), "v3");
        write_config_toml(dir.path(), 1);
        commit_and_tag_milestone(dir.path(), "v3", "m3", 3);

        let report = backfill_run(dir.path(), false).unwrap();

        assert_eq!(report.processed, vec!["m3".to_string()]);
        assert_eq!(report.incompatible.len(), 1);
        assert_eq!(report.incompatible[0].to_milestone, "m2");
        assert!(
            report.incompatible[0].reason.contains('1')
                && report.incompatible[0].reason.contains('2'),
            "expected the reason to name both schema versions, got: {}",
            report.incompatible[0].reason
        );
        assert!(dir.path().join(".markharness/changes/m3.yaml").is_file());
        assert!(!dir.path().join(".markharness/changes/m2.yaml").is_file());
    }

    /// Spec review of issue #29 §6: `backfill run` must use the same
    /// legacy-schema-version-warning policy as `changes compute`, not just
    /// the same fail-closed gate.
    #[test]
    fn backfill_run_collects_legacy_schema_version_warnings_across_pairs() {
        let dir = init_repo();
        write_feature(dir.path(), "v1"); // no config.toml at all: legacy v1
        commit_and_tag_milestone(dir.path(), "v1", "m1", 1);
        write_feature(dir.path(), "v2");
        write_config_toml(dir.path(), 1);
        commit_and_tag_milestone(dir.path(), "v2", "m2", 2);

        let report = backfill_run(dir.path(), false).unwrap();

        assert_eq!(report.processed, vec!["m2".to_string()]);
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("m1"));
    }

    #[test]
    fn list_milestone_names_only_includes_dirs_with_milestone_yml() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".markharness/executions/m1")).unwrap();
        fs::write(
            dir.path().join(".markharness/executions/m1/milestone.yml"),
            "id: m1\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join(".markharness/executions/not-a-milestone")).unwrap();

        let names = list_milestone_names(dir.path()).unwrap();

        assert_eq!(names, vec!["m1".to_string()]);
    }

    #[test]
    fn backfill_run_skips_oldest_milestone_and_computes_remaining_pairs_newest_first() {
        let dir = init_repo();
        write_feature(dir.path(), "v1");
        commit_and_tag_milestone(dir.path(), "v1", "m1", 1);
        write_feature(dir.path(), "v2");
        commit_and_tag_milestone(dir.path(), "v2", "m2", 2);
        write_feature(dir.path(), "v3");
        commit_and_tag_milestone(dir.path(), "v3", "m3", 3);

        let report = backfill_run(dir.path(), false).unwrap();

        assert_eq!(report.processed, vec!["m3".to_string(), "m2".to_string()]);
        assert!(report.skipped.is_empty());
        assert!(dir.path().join(".markharness/changes/m2.yaml").is_file());
        assert!(dir.path().join(".markharness/changes/m3.yaml").is_file());
        assert!(!dir.path().join(".markharness/changes/m1.yaml").exists());
    }

    #[test]
    fn backfill_run_is_idempotent_via_git_notes() {
        let dir = init_repo();
        write_feature(dir.path(), "v1");
        commit_and_tag_milestone(dir.path(), "v1", "m1", 1);
        write_feature(dir.path(), "v2");
        commit_and_tag_milestone(dir.path(), "v2", "m2", 2);

        let first = backfill_run(dir.path(), false).unwrap();
        assert_eq!(first.processed, vec!["m2".to_string()]);

        let second = backfill_run(dir.path(), false).unwrap();

        assert!(second.processed.is_empty());
        assert_eq!(second.skipped, vec!["m2".to_string()]);
    }

    #[test]
    fn backfill_run_returns_empty_report_when_no_milestones_exist() {
        let dir = init_repo();

        let report = backfill_run(dir.path(), false).unwrap();

        assert!(report.processed.is_empty());
        assert!(report.skipped.is_empty());
    }

    #[test]
    fn backfill_policy_limits_newly_processed_pairs_without_counting_skips() {
        let dir = init_repo();
        write_feature(dir.path(), "v1");
        commit_and_tag_milestone(dir.path(), "v1", "m1", 1);
        write_feature(dir.path(), "v2");
        commit_and_tag_milestone(dir.path(), "v2", "m2", 2);
        write_feature(dir.path(), "v3");
        commit_and_tag_milestone(dir.path(), "v3", "m3", 3);

        let policy = BackfillPolicy {
            use_cache: false,
            max_pairs: Some(1),
            time_budget: None,
        };
        let first = backfill_run_with_policy(dir.path(), policy).unwrap();
        let second = backfill_run_with_policy(dir.path(), policy).unwrap();

        assert_eq!(first.processed, vec!["m3".to_string()]);
        assert!(first.stopped_by_limit);
        assert_eq!(second.skipped, vec!["m3".to_string()]);
        assert_eq!(second.processed, vec!["m2".to_string()]);
    }

    #[test]
    fn zero_time_budget_stops_before_writing() {
        let dir = init_repo();
        write_feature(dir.path(), "v1");
        commit_and_tag_milestone(dir.path(), "v1", "m1", 1);
        write_feature(dir.path(), "v2");
        commit_and_tag_milestone(dir.path(), "v2", "m2", 2);

        let report = backfill_run_with_policy(
            dir.path(),
            BackfillPolicy {
                use_cache: false,
                max_pairs: None,
                time_budget: Some(std::time::Duration::ZERO),
            },
        )
        .unwrap();

        assert!(report.processed.is_empty());
        assert!(report.stopped_by_limit);
        assert!(!dir.path().join(".markharness/changes/m2.yaml").exists());
    }

    /// Regression test for the README's canonical demo, which fails with
    /// `--to must be strictly newer than --from` when two milestones are
    /// committed within the same wall-clock second: `order_by_recency` used
    /// to sort purely by committer-date string, so a tie left the tags in
    /// their original (alphabetical) order regardless of which one actually
    /// came later in history. Ancestry (`git merge-base --is-ancestor`) must
    /// win over a date tie.
    #[test]
    fn order_by_recency_breaks_same_second_committer_date_ties_by_ancestry() {
        let dir = init_repo();
        write_feature(dir.path(), "v1");
        // Same hour_offset for both milestones: identical committer date.
        commit_and_tag_milestone(dir.path(), "v1", "m1", 1);
        write_feature(dir.path(), "v2");
        commit_and_tag_milestone(dir.path(), "v2", "m2", 1);

        let ordered = order_by_recency(dir.path(), vec!["m1".to_string(), "m2".to_string()]);

        assert_eq!(
            ordered,
            vec!["m2".to_string(), "m1".to_string()],
            "m2 is a descendant of m1 in history and must sort first despite the tied committer date"
        );
    }
}
