use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::fs_safety::replace_file;
use crate::generate;
use crate::git;
use crate::id_cache::{self, by_identity_key};
use crate::knowledge_source::{
    GitTreeKnowledgeSource, KnowledgeSource, WorkingTreeKnowledgeSource,
};
use crate::lineage::{self, LineageKind};

/// The kind of change a human attaches to a `ChangeEvent` after the fact
/// (§3.5): a specification change, a bug fix, a refactor (behavior
/// unchanged), or anything not covered by those three.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeType {
    SpecChange,
    BugFix,
    Refactor,
    Other,
}

/// One detected Feature change between two milestones (§3.5 ChangeEvent).
/// `change_type` is computed as `None` here and filled in afterwards by a
/// human via `markharness changes annotate` (per docs/en/cli-manual.md UC5, it
/// is not computed from the diff itself).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangeEvent {
    pub event_id: String,
    pub feature_id: String,
    /// The Feature's immutable identity (ADR 0013,
    /// design/immutable-identity-model-design.md), when it has one on
    /// either side of the interval. `None` for a Feature that has not
    /// been migrated — such a Feature is still tracked, by `feature_id`
    /// alone, exactly as before ADR 0013.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_uid: Option<String>,
    /// The Feature's `id:` at `from_milestone`, when it existed there.
    /// Differs from `feature_id` (which always reflects the *current*,
    /// `to_milestone`-side id when available) only across a rename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_id_at_from: Option<String>,
    /// The Feature's `id:` at `to_milestone`, when it existed there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_id_at_to: Option<String>,
    pub from_milestone: String,
    pub to_milestone: String,
    pub from_tree_sha: Option<String>,
    pub to_tree_sha: Option<String>,
    pub impacted_testcases: Vec<String>,
    /// The granularity `impacted_testcases` was narrowed down to, and the
    /// specific evidence for it when narrower than Feature (issue #15). See
    /// `ImpactReason`'s doc comment.
    #[serde(default)]
    pub impact_reason: ImpactReason,
    #[serde(default)]
    pub change_type: Option<ChangeType>,
    /// One entry per two-parent merge commit found in the
    /// `from_milestone..to_milestone` interval (§3.2) at which this Feature
    /// is a true divergence (both parents changed it differently from
    /// their `git merge-base`), oldest merge first. Empty otherwise,
    /// including the ordinary linear case covered by
    /// `from_tree_sha`/`to_tree_sha`.
    #[serde(default)]
    pub true_divergences: Vec<TrueDivergence>,
    /// `event_id`s of other `ChangeEvent`s that a human has recorded as
    /// part of the same logical change (§3.5). Purely additive and
    /// human-populated via `markharness changes annotate --related`;
    /// doesn't affect the per-Feature automatic computation in
    /// `compute_changes`.
    #[serde(default)]
    pub related_events: Vec<String>,
}

impl ChangeEvent {
    /// Stable comparison/index key during the mixed legacy/UID migration.
    /// UID-backed events use their immutable identity; legacy events retain
    /// their historical id-based behavior.
    pub fn identity_key(&self) -> &str {
        self.feature_uid.as_deref().unwrap_or(&self.feature_id)
    }
}

/// A single true-divergence merge recorded against a `ChangeEvent`: the
/// merge commit itself (auditable via `markharness changes lineage
/// --commit <merge_commit>` or `git show <merge_commit>`) and the two
/// parent tree SHAs `[tree(P1), tree(P2)]` that diverged.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrueDivergence {
    pub merge_commit: String,
    pub parent_tree_shas: [String; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachePolicy {
    Use,
    Bypass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpactSource {
    HistoricalTree,
    CurrentWorkingTree,
}

/// The unit at which `impacted_testcases` is narrowed down (issue #15).
/// `Feature` (the default, and the only behavior before this option
/// existed) keeps every TestCase generated from a changed Feature as a
/// candidate — safe-side, but over-inclusive when only some of a Feature's
/// Behaviors/Conditions actually changed. `Behavior`/`Condition` narrow the
/// candidate set to only the Behaviors/Conditions whose own subtree
/// actually changed, trading recall for precision: this tool has no way to
/// detect coupling between sibling Behaviors/Conditions that isn't
/// expressed in the schema, so choosing a finer granularity is an
/// explicit, opt-in risk the user takes on (see the design discussion on
/// issue #15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Granularity {
    #[default]
    Feature,
    Behavior,
    Condition,
}

/// Why each of a `ChangeEvent`'s `impacted_testcases` was selected (issue
/// #15): the `Granularity` `changes compute` was run with, plus the
/// repo-relative marker-file paths (`behavior.yml`/`condition.yml`) whose
/// content actually differed between `from_milestone` and `to_milestone`
/// and drove the narrowing decision. `changed_paths` is empty for
/// `Granularity::Feature` — narrowing isn't attempted at that granularity
/// (every TestCase generated from the changed Feature is included, exactly
/// as before this option existed), so there's no per-subunit evidence to
/// record; the Feature-level `from_tree_sha`/`to_tree_sha` already serve as
/// that granularity's own reason. `#[serde(default)]` on `ChangeEvent`'s
/// `impact_reason` field means a `ChangeEvent` written before this field
/// existed round-trips as `Feature` with no `changed_paths` — exactly the
/// behavior it had.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ImpactReason {
    pub granularity: Granularity,
    #[serde(default)]
    pub changed_paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangeOptions {
    pub cache: CachePolicy,
    pub impact_source: ImpactSource,
    pub granularity: Granularity,
}

impl Default for ChangeOptions {
    fn default() -> Self {
        Self {
            cache: CachePolicy::Use,
            impact_source: ImpactSource::HistoricalTree,
            granularity: Granularity::Feature,
        }
    }
}

/// A named release milestone or an arbitrary Git commit-ish accepted by
/// the change analyzer. Keeping the kind explicit lets application policy
/// require milestones while lower-level analysis can operate on commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitRef {
    Milestone(String),
    Commit(String),
}

impl CommitRef {
    pub fn milestone(value: impl Into<String>) -> Self {
        Self::Milestone(value.into())
    }

    pub fn commit(value: impl Into<String>) -> Self {
        Self::Commit(value.into())
    }

    pub fn as_git_ref(&self) -> &str {
        match self {
            Self::Milestone(value) | Self::Commit(value) => value,
        }
    }

    pub fn is_milestone(&self) -> bool {
        matches!(self, Self::Milestone(_))
    }
}

pub struct ChangeAnalyzer<'a> {
    root: &'a Path,
}

impl<'a> ChangeAnalyzer<'a> {
    pub fn new(root: &'a Path) -> Self {
        Self { root }
    }

    pub fn compute(
        &self,
        from: &CommitRef,
        to: &CommitRef,
        options: ChangeOptions,
    ) -> io::Result<Vec<ChangeEvent>> {
        compute_changes_between_refs(self.root, from.as_git_ref(), to.as_git_ref(), options)
    }
}

/// For each Feature identity key (ADR 0013: its `uid` when it has one,
/// else its `id`), `[tree(P1), tree(P2)]` when `merge_commit` is a
/// two-parent merge commit and the Feature is a true divergence per
/// `lineage::classify` (§3.2). Returns an empty map when `merge_commit`
/// isn't itself a two-parent commit (defensive; callers only pass merge
/// commits found by `find_merge_commits_in_interval`).
fn true_divergence_parent_tree_shas(
    root: &Path,
    merge_commit: &str,
    use_cache: bool,
) -> io::Result<BTreeMap<String, [String; 2]>> {
    let parents = git::parents(root, merge_commit)?;
    let [p1, p2] = parents.as_slice() else {
        return Ok(BTreeMap::new());
    };
    let base = git::merge_base(root, p1, p2)?;

    let base_versions =
        by_identity_key(id_cache::resolve_feature_versions(root, &base, use_cache)?);
    let p1_versions = by_identity_key(id_cache::resolve_feature_versions(root, p1, use_cache)?);
    let p2_versions = by_identity_key(id_cache::resolve_feature_versions(root, p2, use_cache)?);

    let all_keys: BTreeSet<&String> = p1_versions.keys().chain(p2_versions.keys()).collect();

    let mut result = BTreeMap::new();
    for key in all_keys {
        let base_sha = base_versions.get(key).map(|v| &v.tree_sha);
        let p1_sha = p1_versions.get(key).map(|v| &v.tree_sha);
        let p2_sha = p2_versions.get(key).map(|v| &v.tree_sha);
        // `TrueDivergence` can also occur when one branch deleted the
        // Feature and the other changed it (`p1_sha`/`p2_sha` not both
        // `Some`): there are no two tree SHAs to record in that case, so
        // fall back to the ordinary `from_tree_sha`/`to_tree_sha`
        // representation instead of populating `true_divergences`.
        if let (Some(p1_sha), Some(p2_sha)) = (p1_sha, p2_sha)
            && lineage::classify(base_sha, Some(p1_sha), Some(p2_sha))
                == LineageKind::TrueDivergence
        {
            result.insert(key.clone(), [p1_sha.clone(), p2_sha.clone()]);
        }
    }
    Ok(result)
}

/// All two-parent merge commits in the `from_milestone..to_milestone`
/// interval, oldest first (`--reverse`, matching `generate.rs`'s
/// deterministic-ordering convention: `git rev-list` without it yields
/// newest-first).
fn find_merge_commits_in_interval(
    root: &Path,
    from_milestone: &str,
    to_milestone: &str,
) -> io::Result<Vec<String>> {
    git::merge_commits_between(root, from_milestone, to_milestone)
}

/// Separates the components of a `subunit_key` composite key. Not a
/// character any `id:` field may contain (ids are lowercase alphanumeric
/// and hyphens only — `docs/ja/cli-manual.md` 1.12節), so it can't collide
/// with real id text.
const SUBUNIT_KEY_SEP: char = '\u{1}';

/// The key `impacted` (the per-`Granularity` testcase grouping below) and
/// the "which subunits changed" computation in `diff_events` both use to
/// refer to the same Behavior/Condition, so the two sides always agree.
/// `Granularity::Feature` never calls this — `impacted` stays keyed by
/// plain `feature_id`, as it always has been.
fn subunit_key(feature_id: &str, behavior_id: &str, condition_id: Option<&str>) -> String {
    match condition_id {
        Some(condition_id) => {
            format!("{feature_id}{SUBUNIT_KEY_SEP}{behavior_id}{SUBUNIT_KEY_SEP}{condition_id}")
        }
        None => format!("{feature_id}{SUBUNIT_KEY_SEP}{behavior_id}"),
    }
}

/// `id_cache::resolve_behavior_versions`/`resolve_condition_versions`
/// results (issue #15), grouped by the Feature directory each subunit
/// belongs to — `diff_events` resolves this once per side of the interval,
/// then looks up only the slice relevant to the Feature it's currently
/// processing.
struct SubunitsByFeatureDir(BTreeMap<String, Vec<id_cache::SubunitVersion>>);

impl SubunitsByFeatureDir {
    fn new(versions: Vec<id_cache::SubunitVersion>) -> Self {
        let mut by_dir: BTreeMap<String, Vec<id_cache::SubunitVersion>> = BTreeMap::new();
        for version in versions {
            by_dir
                .entry(version.parent_feature_dir.clone())
                .or_default()
                .push(version);
        }
        Self(by_dir)
    }

    fn under(&self, feature_dir: Option<&str>) -> &[id_cache::SubunitVersion] {
        feature_dir
            .and_then(|dir| self.0.get(dir))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

/// The result of narrowing a Feature's `impacted_testcases` at Behavior/
/// Condition granularity: the narrowed candidate set, plus the evidence for
/// it (`ImpactReason::changed_paths`, issue #15).
struct SubunitNarrowing {
    testcases: Vec<String>,
    changed_paths: Vec<String>,
}

/// Narrows a Feature's `impacted_testcases` down to only the Behaviors/
/// Conditions that actually changed within it (issue #15), instead of
/// every TestCase generated from the Feature. `from_here`/`to_here` are
/// already scoped to this one Feature (`SubunitsByFeatureDir::under`).
/// Keyed by `(parent_behavior_id, id)` rather than bare `id` because a
/// Condition's `id` is only unique among its own Behavior's siblings (see
/// `id_cache::resolve_marker_versions`'s doc comment) — without the
/// Behavior discriminant, two Conditions with the same id under different
/// Behaviors of the same Feature would collide.
fn changed_subunit_impacted_testcases(
    feature_id: &str,
    granularity: Granularity,
    from_here: &[id_cache::SubunitVersion],
    to_here: &[id_cache::SubunitVersion],
    impacted: &BTreeMap<String, Vec<String>>,
) -> io::Result<SubunitNarrowing> {
    let marker_file = match granularity {
        Granularity::Behavior => "behavior.yml",
        Granularity::Condition => "condition.yml",
        Granularity::Feature => {
            unreachable!("diff_events only calls this for Behavior/Condition granularity")
        }
    };
    let from_by_key: BTreeMap<(Option<&str>, &str), &id_cache::SubunitVersion> = from_here
        .iter()
        .map(|v| ((v.parent_behavior_id.as_deref(), v.id.as_str()), v))
        .collect();
    let to_by_key: BTreeMap<(Option<&str>, &str), &id_cache::SubunitVersion> = to_here
        .iter()
        .map(|v| ((v.parent_behavior_id.as_deref(), v.id.as_str()), v))
        .collect();
    let all_keys: BTreeSet<(Option<&str>, &str)> = from_by_key
        .keys()
        .chain(to_by_key.keys())
        .cloned()
        .collect();

    let mut testcases = Vec::new();
    let mut changed_paths = Vec::new();
    for key @ (parent_behavior_id, id) in all_keys {
        let from_version = from_by_key.get(&key).copied();
        let to_version = to_by_key.get(&key).copied();
        let from_sha = from_version.map(|v| &v.tree_sha);
        let to_sha = to_version.map(|v| &v.tree_sha);
        if from_sha == to_sha {
            continue;
        }
        // The changed subunit's directory itself, whichever side still has
        // it (`to` when it exists there — the current, post-change state;
        // `from` for a deletion) — evidence for `ImpactReason::changed_paths`.
        let subunit_dir = to_version.or(from_version).map(|v| v.path.as_str());
        if let Some(subunit_dir) = subunit_dir {
            changed_paths.push(format!("{subunit_dir}/{marker_file}"));
        }
        let subunit_key_value = match granularity {
            Granularity::Behavior => subunit_key(feature_id, id, None),
            Granularity::Condition => {
                // A `condition.yml` whose directory isn't beneath a
                // resolvable `behavior.yml` (malformed `knowledge/`) — not
                // this project's structural invariant to enforce silently,
                // so fail with a clear error rather than panic on
                // untrusted/external Knowledge content.
                let Some(behavior_id) = parent_behavior_id else {
                    let offending_path = to_version
                        .or(from_version)
                        .map(|v| v.path.as_str())
                        .unwrap_or(id);
                    return Err(io::Error::other(format!(
                        "condition '{id}' at '{offending_path}' has no resolvable parent Behavior id; \
                         --granularity condition requires every Condition directory to sit under a valid behavior.yml"
                    )));
                };
                subunit_key(feature_id, behavior_id, Some(id))
            }
            Granularity::Feature => {
                unreachable!("diff_events only calls this for Behavior/Condition granularity")
            }
        };
        if let Some(case_ids) = impacted.get(&subunit_key_value) {
            testcases.extend(case_ids.iter().cloned());
        }
    }
    Ok(SubunitNarrowing {
        testcases,
        changed_paths,
    })
}

/// Maps each Feature id to the `case_id`s of testcases generated from it,
/// using the *current* `knowledge/` working tree as the structural
/// generation graph (§3.2(A): `CONDITION`→`TESTCASE`, does not need version
/// history — only the version-history side, `derived_from`, does). Legacy
/// behavior, opted into via `ImpactSource::CurrentWorkingTree`: recomputing
/// the same past `from_milestone..to_milestone` interval later can yield a
/// different `impacted_testcases` set as the working tree keeps changing.
fn impacted_testcases_by_feature(
    root: &Path,
    granularity: Granularity,
) -> io::Result<BTreeMap<String, Vec<String>>> {
    let source = WorkingTreeKnowledgeSource::new(
        root.join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge"),
    );
    Ok(testcases_by_key(
        generate::compile_testcases(&source.load_snapshot()?),
        granularity,
    ))
}

/// Maps each Feature id to the `case_id`s of testcases generated from it, as
/// `knowledge/` existed at `milestone` (a git tag), independent of the
/// current working tree. This is `compute_changes`'s default: recomputing a
/// past `from_milestone..to_milestone` interval later always yields the same
/// `impacted_testcases`, because it's derived from `to_milestone`'s
/// committed tree rather than whatever `knowledge/` looks like right now.
///
/// `knowledge/` at `milestone` is read through `GitTreeKnowledgeSource`,
/// avoiding a temporary Git worktree and its repository metadata updates.
fn historical_testcases_by_feature(
    root: &Path,
    milestone: &str,
    granularity: Granularity,
) -> io::Result<BTreeMap<String, Vec<String>>> {
    let source = GitTreeKnowledgeSource::new(root, milestone);
    Ok(testcases_by_key(
        generate::compile_testcases(&source.load_snapshot()?),
        granularity,
    ))
}

/// Groups `case_id`s by the id text at `granularity` (issue #15): plain
/// `feature_id` for `Granularity::Feature` (unchanged from before this
/// option existed), or a `subunit_key` composite for `Behavior`/
/// `Condition` so `diff_events` can narrow the candidate set to only the
/// Behaviors/Conditions it independently determined actually changed.
fn testcases_by_key(
    testcases: Vec<generate::TestCase>,
    granularity: Granularity,
) -> BTreeMap<String, Vec<String>> {
    let mut by_key: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for testcase in testcases {
        let generated_from = &testcase.generated_from;
        let key = match granularity {
            Granularity::Feature => generated_from.feature.clone(),
            Granularity::Behavior => {
                subunit_key(&generated_from.feature, &generated_from.behavior, None)
            }
            Granularity::Condition => subunit_key(
                &generated_from.feature,
                &generated_from.behavior,
                Some(&generated_from.condition),
            ),
        };
        by_key.entry(key).or_default().push(testcase.case_id);
    }
    by_key
}

/// Computes `derived_from`-style change events between `from_milestone` and
/// `to_milestone` (two git tags) by comparing each Feature's directory tree
/// SHA at each tag (§3.2〜3.4 の簡易版; マイルストーン=引数のtag名をそのまま
/// 使用)。Using the whole directory's tree SHA rather than just
/// `feature.yml`'s blob SHA means Condition/Behavior/ExpectedResult changes
/// are detected even when `feature.yml` itself is untouched.
///
/// `impacted_testcases` is derived from `to_milestone`'s tree by default
/// (`ImpactSource::HistoricalTree`), so recomputing the same past interval later
/// is deterministic. Pass `ImpactSource::CurrentWorkingTree` to opt into the legacy
/// behavior of reading the current `knowledge/` working tree instead (see
/// `impacted_testcases_by_feature` / `historical_testcases_by_feature`).
pub fn compute_changes(
    root: &Path,
    from_milestone: &str,
    to_milestone: &str,
    options: ChangeOptions,
) -> io::Result<Vec<ChangeEvent>> {
    ChangeAnalyzer::new(root).compute(
        &CommitRef::milestone(from_milestone),
        &CommitRef::milestone(to_milestone),
        options,
    )
}

/// The result of `compute_changes_with_warnings`: the computed
/// `ChangeEvent`s alongside any non-fatal issues found while resolving the
/// two refs' Knowledge schema versions (issue #29 §6 — a legacy fallback).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputeChangesOutcome {
    pub events: Vec<ChangeEvent>,
    pub warnings: Vec<String>,
}

/// Like `compute_changes`, but also returns the legacy-schema-version
/// warnings collected while resolving `from_milestone`/`to_milestone`
/// (issue #29 §6). Resolves each ref's Knowledge schema version exactly
/// once and reuses that same result for both the fail-closed gate and the
/// warning text — `application::compute_changes` and
/// `backfill::backfill_run_with_policy` call this instead of re-resolving
/// independently (Standards review: avoids duplicate Git reads and the
/// risk of the gate decision and the displayed warning disagreeing).
pub fn compute_changes_with_warnings(
    root: &Path,
    from_milestone: &str,
    to_milestone: &str,
    options: ChangeOptions,
) -> io::Result<ComputeChangesOutcome> {
    let from_schema = crate::knowledge_schema::resolve(root, from_milestone)?;
    let to_schema = crate::knowledge_schema::resolve(root, to_milestone)?;

    // issue #29's version-resolution policy table: "milestone.yml とtag内の
    // 正本が不一致 | エラーとして報告する". A ref with no milestone.yml (an
    // arbitrary commit, or a milestone predating the audit fields) is not
    // checked. Reuses the resolution just above rather than re-resolving.
    crate::milestone::verify_audit_matches_tag(root, from_milestone, &from_schema)?;
    crate::milestone::verify_audit_matches_tag(root, to_milestone, &to_schema)?;

    let legacy_warnings: Vec<String> = [
        crate::knowledge_schema::legacy_warning(from_milestone, &from_schema),
        crate::knowledge_schema::legacy_warning(to_milestone, &to_schema),
    ]
    .into_iter()
    .flatten()
    .collect();

    // Spec review of issue #29 §6: a legacy-fallback warning must not be
    // lost just because the pair also fails the §5 fail-closed gate —
    // `ComputeChangesOutcome.warnings` only exists on the `Ok` path, so the
    // only way to carry it on `Err` is folding it into the error message.
    if let Err(e) = crate::knowledge_schema::ensure_compatible(&from_schema, &to_schema) {
        let mut message = e.to_string();
        for warning in &legacy_warnings {
            message.push(' ');
            message.push_str(warning);
        }
        return Err(io::Error::new(e.kind(), message));
    }
    let warnings = legacy_warnings;

    let events = diff_events(root, from_milestone, to_milestone, options)?;
    Ok(ComputeChangesOutcome { events, warnings })
}

fn compute_changes_between_refs(
    root: &Path,
    from_milestone: &str,
    to_milestone: &str,
    options: ChangeOptions,
) -> io::Result<Vec<ChangeEvent>> {
    Ok(compute_changes_with_warnings(root, from_milestone, to_milestone, options)?.events)
}

/// The tree-SHA diff itself, once `from_milestone`/`to_milestone` are
/// already known to be schema-compatible (`compute_changes_with_warnings`
/// is the only caller — it runs the fail-closed gate first).
fn diff_events(
    root: &Path,
    from_milestone: &str,
    to_milestone: &str,
    options: ChangeOptions,
) -> io::Result<Vec<ChangeEvent>> {
    let use_cache = options.cache == CachePolicy::Use;
    let from_versions = by_identity_key(id_cache::resolve_feature_versions(
        root,
        from_milestone,
        use_cache,
    )?);
    let to_versions = by_identity_key(id_cache::resolve_feature_versions(
        root,
        to_milestone,
        use_cache,
    )?);
    let impacted = match options.impact_source {
        ImpactSource::HistoricalTree => {
            historical_testcases_by_feature(root, to_milestone, options.granularity)?
        }
        ImpactSource::CurrentWorkingTree => {
            impacted_testcases_by_feature(root, options.granularity)?
        }
    };
    // Only resolved when narrower than Feature granularity is requested
    // (issue #15): an extra `git ls-tree` per side that a `Feature`-only
    // run (the default, and every run before this option existed) must not
    // pay for.
    let subunits = match options.granularity {
        Granularity::Feature => None,
        Granularity::Behavior => Some((
            SubunitsByFeatureDir::new(id_cache::resolve_behavior_versions(root, from_milestone)?),
            SubunitsByFeatureDir::new(id_cache::resolve_behavior_versions(root, to_milestone)?),
        )),
        Granularity::Condition => Some((
            SubunitsByFeatureDir::new(id_cache::resolve_condition_versions(root, from_milestone)?),
            SubunitsByFeatureDir::new(id_cache::resolve_condition_versions(root, to_milestone)?),
        )),
    };
    let merge_commits = find_merge_commits_in_interval(root, from_milestone, to_milestone)?;
    let mut true_divergences_by_key: BTreeMap<String, Vec<TrueDivergence>> = BTreeMap::new();
    for merge_commit in &merge_commits {
        let divergences = true_divergence_parent_tree_shas(root, merge_commit, use_cache)?;
        for (key, parent_tree_shas) in divergences {
            true_divergences_by_key
                .entry(key)
                .or_default()
                .push(TrueDivergence {
                    merge_commit: merge_commit.clone(),
                    parent_tree_shas,
                });
        }
    }

    let all_keys: BTreeSet<&String> = from_versions.keys().chain(to_versions.keys()).collect();

    let mut events = Vec::new();
    for key in all_keys {
        let from = from_versions.get(key);
        let to = to_versions.get(key);
        let from_tree_sha = from.map(|v| v.tree_sha.clone());
        let to_tree_sha = to.map(|v| v.tree_sha.clone());
        if from_tree_sha == to_tree_sha {
            continue;
        }
        let true_divergences = true_divergences_by_key
            .get(key)
            .cloned()
            .unwrap_or_default();
        let raw_id_at_from = from.map(|v| v.id.clone());
        let raw_id_at_to = to.map(|v| v.id.clone());
        // Always the *current* display id when available (design doc
        // §2): the `to`-side id if the Feature still exists there,
        // otherwise the last known `from`-side id (a deletion).
        let feature_id = raw_id_at_to
            .clone()
            .or_else(|| raw_id_at_from.clone())
            .unwrap_or_else(|| key.clone());
        let feature_uid = to
            .and_then(|v| v.uid.clone())
            .or_else(|| from.and_then(|v| v.uid.clone()));
        // Only surface the per-side ids for a Feature actually
        // participating in the identity model (has a `uid` somewhere in
        // this interval, design doc §2) — an un-migrated Feature keeps the
        // exact pre-ADR-0013 `ChangeEvent` shape, since `feature_id` alone
        // already carries this information there (no rename can be
        // detected without a uid, so `_at_from`/`_at_to` would be
        // redundant noise for every ordinary content-only change).
        let (feature_id_at_from, feature_id_at_to) = if feature_uid.is_some() {
            (raw_id_at_from.clone(), raw_id_at_to.clone())
        } else {
            (None, None)
        };
        // `impacted` is keyed by the literal id text `generate.rs` wrote
        // into `generated_from.feature` at the relevant tree state
        // (§2.1), never by uid — look it up by display id, not `key`.
        let (impacted_testcases, changed_paths) = match &subunits {
            None => (
                raw_id_at_to
                    .as_ref()
                    .or(raw_id_at_from.as_ref())
                    .and_then(|id| impacted.get(id))
                    .cloned()
                    .unwrap_or_default(),
                Vec::new(),
            ),
            Some((from_subunits, to_subunits)) => {
                let narrowing = changed_subunit_impacted_testcases(
                    &feature_id,
                    options.granularity,
                    from_subunits.under(from.map(|v| v.path.as_str())),
                    to_subunits.under(to.map(|v| v.path.as_str())),
                    &impacted,
                )?;
                (narrowing.testcases, narrowing.changed_paths)
            }
        };
        events.push(ChangeEvent {
            event_id: format!("{feature_id}--{from_milestone}--{to_milestone}"),
            feature_id,
            feature_uid,
            feature_id_at_from,
            feature_id_at_to,
            from_milestone: from_milestone.to_string(),
            to_milestone: to_milestone.to_string(),
            from_tree_sha,
            to_tree_sha,
            impacted_testcases,
            impact_reason: ImpactReason {
                granularity: options.granularity,
                changed_paths,
            },
            change_type: None,
            true_divergences,
            related_events: Vec::new(),
        });
    }

    Ok(events)
}

pub fn serialize_changes(events: &[ChangeEvent]) -> String {
    serde_yaml_ng::to_string(events).expect("ChangeEvent serialization is infallible")
}

/// Reads `changes/<milestone>.yaml` (the ChangeEvents whose `to_milestone`
/// is `milestone`, written by `compute_changes`/`serialize_changes`).
/// Returns an empty list if the file doesn't exist, rather than an error.
pub fn read_changes(root: &Path, milestone: &str) -> io::Result<Vec<ChangeEvent>> {
    let path = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("changes")
        .join(format!("{milestone}.yaml"));
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path)?;
    let mut value: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&content).map_err(io::Error::other)?;
    migrate_legacy_granularity_field(&mut value);
    serde_yaml_ng::from_value(value).map_err(io::Error::other)
}

/// Migrates a `ChangeEvent` written with issue #15's short-lived
/// intermediate shape — a top-level `granularity` scalar, from before the
/// `impact_reason` follow-up fix (PR #36) — into the current
/// `impact_reason.granularity` shape. Without this, `#[serde(default)]` on
/// `impact_reason` would silently discard the recorded granularity and
/// reset every such `ChangeEvent` to `Feature`, misrepresenting what
/// `changes compute` was actually run with (a plain schema-evolution
/// oversight caught by Codex review, not an intentional legacy-fallback
/// policy elsewhere in this codebase). `changed_paths` cannot be
/// recovered — that intermediate shape never recorded it — so it migrates
/// to empty, same as `Granularity::Feature`'s.
fn migrate_legacy_granularity_field(value: &mut serde_yaml_ng::Value) {
    let Some(events) = value.as_sequence_mut() else {
        return;
    };
    for event in events {
        let Some(mapping) = event.as_mapping_mut() else {
            continue;
        };
        if mapping.contains_key("impact_reason") {
            continue;
        }
        let Some(granularity) = mapping.remove("granularity") else {
            continue;
        };
        let mut impact_reason = serde_yaml_ng::Mapping::new();
        impact_reason.insert("granularity".into(), granularity);
        impact_reason.insert(
            "changed_paths".into(),
            serde_yaml_ng::Value::Sequence(Vec::new()),
        );
        mapping.insert("impact_reason".into(), impact_reason.into());
    }
}

/// Why `markharness changes annotate` failed to set a `change_type` or
/// `related_events`.
#[derive(Debug)]
pub enum AnnotateError {
    /// No event with this `event_id` exists under `changes/`. Carries the
    /// offending id: for `annotate_related_events` this may be either the
    /// target `event_id` or one of the `--related` ids.
    NotFound(String),
    Io(io::Error),
}

impl From<io::Error> for AnnotateError {
    fn from(e: io::Error) -> Self {
        AnnotateError::Io(e)
    }
}

/// Sets `change_type` on the `ChangeEvent` identified by `event_id`,
/// searching every `changes/*.yaml` file (event ids are unique but a
/// caller need not know which milestone interval an event belongs to), and
/// rewrites that file in place (§3.5: `change_type` is filled in by a human
/// after `compute_changes`, not computed).
pub fn annotate_change_type(
    root: &Path,
    event_id: &str,
    change_type: ChangeType,
) -> Result<(), AnnotateError> {
    for path in changes_yaml_paths(root)? {
        let content = fs::read_to_string(&path)?;
        let mut events: Vec<ChangeEvent> =
            serde_yaml_ng::from_str(&content).map_err(io::Error::other)?;
        let Some(event) = events.iter_mut().find(|e| e.event_id == event_id) else {
            continue;
        };
        event.change_type = Some(change_type);
        replace_file(root, &path, serialize_changes(&events).as_bytes())?;
        return Ok(());
    }

    Err(AnnotateError::NotFound(event_id.to_string()))
}

fn changes_yaml_paths(root: &Path) -> io::Result<Vec<PathBuf>> {
    let changes_dir = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("changes");
    let mut entries: Vec<PathBuf> = fs::read_dir(&changes_dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "yaml"))
        .collect();
    entries.sort();
    Ok(entries)
}

/// Checks that `event_id` and every id in `related_ids` exist as an
/// `event_id` somewhere under `changes/*.yaml`, without writing anything.
/// Shared by `annotate_related_events` (which re-checks right before it
/// writes) and by callers that want to validate a `--related` id *before*
/// running an unrelated write (e.g. `changes annotate --type ... --related
/// ...`, so a typo'd `--related` id can't leave `change_type` written while
/// `related_events` isn't — see `markharness changes annotate`'s CLI
/// dispatch).
pub fn validate_annotate_ids(
    root: &Path,
    event_id: &str,
    related_ids: &[String],
) -> Result<(), AnnotateError> {
    let mut known_ids: BTreeSet<String> = BTreeSet::new();
    for path in changes_yaml_paths(root)? {
        let content = fs::read_to_string(&path)?;
        let events: Vec<ChangeEvent> =
            serde_yaml_ng::from_str(&content).map_err(io::Error::other)?;
        known_ids.extend(events.into_iter().map(|e| e.event_id));
    }

    if !known_ids.contains(event_id) {
        return Err(AnnotateError::NotFound(event_id.to_string()));
    }
    for related_id in related_ids {
        if !known_ids.contains(related_id) {
            return Err(AnnotateError::NotFound(related_id.clone()));
        }
    }
    Ok(())
}

/// Appends `related_ids` to `related_events` on the `ChangeEvent`
/// identified by `event_id` (§3.5: purely additive, human-recorded
/// cross-references between ChangeEvents; doesn't affect the automatic
/// per-Feature computation). Searches every `changes/*.yaml` file like
/// `annotate_change_type`. Every id in `related_ids` must itself exist as
/// an `event_id` somewhere under `changes/` (`validate_annotate_ids`),
/// checked up front so a partial write never happens because of a typo'd
/// `--related` id.
pub fn annotate_related_events(
    root: &Path,
    event_id: &str,
    related_ids: &[String],
) -> Result<(), AnnotateError> {
    validate_annotate_ids(root, event_id, related_ids)?;

    for path in changes_yaml_paths(root)? {
        let content = fs::read_to_string(&path)?;
        let mut events: Vec<ChangeEvent> =
            serde_yaml_ng::from_str(&content).map_err(io::Error::other)?;
        let Some(event) = events.iter_mut().find(|e| e.event_id == event_id) else {
            continue;
        };
        event.related_events.extend(related_ids.iter().cloned());
        replace_file(root, &path, serialize_changes(&events).as_bytes())?;
        return Ok(());
    }

    unreachable!("event_id was validated above, so it must be in some file's events")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
        run_git(dir.path(), &["init", "-q", "-b", "main"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        run_git(dir.path(), &["config", "core.autocrlf", "false"]);
        dir
    }

    #[test]
    fn change_analyzer_accepts_arbitrary_commit_refs() {
        let dir = init_repo();
        write_full_chain(dir.path(), "Jump");
        run_git(dir.path(), &["add", "."]);
        run_git(dir.path(), &["commit", "-qm", "base"]);
        write_full_chain(dir.path(), "Higher jump");
        run_git(dir.path(), &["add", "."]);
        run_git(dir.path(), &["commit", "-qm", "head"]);

        let events = ChangeAnalyzer::new(dir.path())
            .compute(
                &CommitRef::commit("HEAD~1"),
                &CommitRef::commit("HEAD"),
                ChangeOptions::default(),
            )
            .unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].feature_id, "player-jump");
        assert_eq!(events[0].from_milestone, "HEAD~1");
        assert_eq!(events[0].to_milestone, "HEAD");
    }

    fn write_full_chain(root: &Path, label: &str) {
        let base = root.join(".markharness/knowledge/controls/player-jump/jump/ground");
        fs::create_dir_all(&base).unwrap();
        fs::write(
            root.join(".markharness/knowledge/controls/requirement.yml"),
            "id: controls\nlabel: controls\naxis: [gameplay]\n",
        )
        .unwrap();
        fs::write(
            root.join(".markharness/knowledge/controls/player-jump/feature.yml"),
            format!("id: player-jump\nrequirement: controls\nlabel: {label}\naxis: [gameplay]\n"),
        )
        .unwrap();
        fs::write(
            root.join(".markharness/knowledge/controls/player-jump/jump/behavior.yml"),
            "id: jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\n",
        )
        .unwrap();
        fs::write(
            base.join("condition.yml"),
            "id: ground\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from the ground.\n",
        )
        .unwrap();
        fs::create_dir_all(base.join("expected")).unwrap();
        fs::write(
            base.join("expected/001.yml"),
            "id: ground-001\ncondition: ground\ndescription: |\n  lands safely\n",
        )
        .unwrap();
    }

    /// Like `write_full_chain`, but the Feature has two Behaviors (`jump`,
    /// `duck`), each with one Condition/ExpectedResult — for issue #15's
    /// `--granularity` narrowing tests, where a single-Behavior Feature
    /// can't distinguish "narrowed to the changed Behavior" from "not
    /// narrowed at all".
    fn write_two_behavior_chain(root: &Path, jump_label: &str, duck_label: &str) {
        write_full_chain(root, "player-jump");
        let duck_dir = root.join(".markharness/knowledge/controls/player-jump/duck");
        let duck_base = duck_dir.join("low");
        fs::create_dir_all(&duck_base).unwrap();
        fs::write(
            root.join(".markharness/knowledge/controls/player-jump/jump/behavior.yml"),
            format!(
                "id: jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  {jump_label}\n"
            ),
        )
        .unwrap();
        fs::write(
            duck_dir.join("behavior.yml"),
            format!(
                "id: duck\nfeature: player-jump\nlabel: duck\naxis: [gameplay]\ndescription: |\n  {duck_label}\n"
            ),
        )
        .unwrap();
        fs::write(
            duck_base.join("condition.yml"),
            "id: low\nbehavior: duck\nlabel: low\ndescription: |\n  Duck low.\n",
        )
        .unwrap();
        fs::create_dir_all(duck_base.join("expected")).unwrap();
        fs::write(
            duck_base.join("expected/001.yml"),
            "id: low-001\ncondition: low\ndescription: |\n  ducks safely\n",
        )
        .unwrap();
    }

    fn commit_and_tag(root: &Path, message: &str, tag: &str) {
        run_git(root, &["add", "-A"]);
        run_git(root, &["commit", "-q", "-m", message]);
        run_git(root, &["tag", tag]);
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

    /// Issue #29 §5: a schema-only migration must not be diffed by raw tree
    /// SHA as if it were a real content change — `compute_changes` must
    /// refuse (fail closed) rather than silently generate a `ChangeEvent`
    /// for every Feature.
    #[test]
    fn compute_changes_fails_closed_when_knowledge_schema_versions_differ() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        write_config_toml(dir.path(), 1);
        commit_and_tag(dir.path(), "v1", "m1");

        write_full_chain(dir.path(), "v2");
        write_config_toml(dir.path(), 2);
        commit_and_tag(dir.path(), "v2", "m2");

        let err = compute_changes(
            dir.path(),
            "m1",
            "m2",
            ChangeOptions {
                cache: CachePolicy::Bypass,
                impact_source: ImpactSource::HistoricalTree,
                granularity: Granularity::Feature,
            },
        )
        .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::Unsupported);
    }

    /// Spec review of issue #29 §6: the legacy-schema-version-fallback
    /// warning must not be lost when the fail-closed gate rejects the
    /// comparison — `compute_changes_with_warnings`' `ComputeChangesOutcome`
    /// only carries `warnings` on the `Ok` path, so the only way to keep
    /// this information on the `Err` path is folding it into the error
    /// message itself.
    #[test]
    fn compute_changes_with_warnings_error_message_names_a_legacy_fallback_side() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1"); // no config.toml at all: legacy v1
        commit_and_tag(dir.path(), "v1", "m1");

        write_full_chain(dir.path(), "v2");
        write_config_toml(dir.path(), 2); // unknown to this CLI build
        commit_and_tag(dir.path(), "v2", "m2");

        let err = compute_changes_with_warnings(
            dir.path(),
            "m1",
            "m2",
            ChangeOptions {
                cache: CachePolicy::Bypass,
                impact_source: ImpactSource::HistoricalTree,
                granularity: Granularity::Feature,
            },
        )
        .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::Unsupported);
        let message = err.to_string();
        assert!(
            message.contains("m1") && message.contains("legacy"),
            "expected the legacy-fallback side to be named in the error, got: {message}"
        );
    }

    /// ADR 0013 / Issue #17's core motivating scenario: a Feature whose
    /// `id:` changes between milestones, but whose `uid:` stays the same,
    /// must be tracked as a single rename — one `ChangeEvent`, not a
    /// delete-then-add pair keyed by two different id strings.
    #[test]
    fn a_uid_preserving_rename_produces_a_single_change_event_not_a_delete_and_add() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        fs::write(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
            "id: player-jump\nrequirement: controls\nlabel: v1\naxis: [gameplay]\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n",
        )
        .unwrap();
        commit_and_tag(dir.path(), "v1", "m1");

        fs::write(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
            "id: player-double-jump\nrequirement: controls\nlabel: v1\naxis: [gameplay]\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n",
        )
        .unwrap();
        commit_and_tag(dir.path(), "rename", "m2");

        let events = compute_changes(
            dir.path(),
            "m1",
            "m2",
            ChangeOptions {
                cache: CachePolicy::Bypass,
                impact_source: ImpactSource::HistoricalTree,
                granularity: Granularity::Feature,
            },
        )
        .unwrap();

        assert_eq!(
            events.len(),
            1,
            "expected exactly one ChangeEvent, got {events:?}"
        );
        let event = &events[0];
        assert_eq!(event.feature_id, "player-double-jump");
        assert_eq!(
            event.feature_uid,
            Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string())
        );
        assert_eq!(event.feature_id_at_from, Some("player-jump".to_string()));
        assert_eq!(
            event.feature_id_at_to,
            Some("player-double-jump".to_string())
        );
        assert!(event.from_tree_sha.is_some());
        assert!(event.to_tree_sha.is_some());
    }

    /// The mixed-mode fallback (design doc §2): a Feature with no `uid`
    /// anywhere in the interval still gets the pre-ADR-0013 delete+add
    /// behavior when its `id:` changes, since there is no stable key to
    /// match old and new by.
    #[test]
    fn a_rename_without_uid_still_produces_a_delete_and_add_pair() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");

        fs::write(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
            "id: player-double-jump\nrequirement: controls\nlabel: v1\naxis: [gameplay]\n",
        )
        .unwrap();
        commit_and_tag(dir.path(), "rename", "m2");

        let events = compute_changes(
            dir.path(),
            "m1",
            "m2",
            ChangeOptions {
                cache: CachePolicy::Bypass,
                impact_source: ImpactSource::HistoricalTree,
                granularity: Granularity::Feature,
            },
        )
        .unwrap();

        assert_eq!(
            events.len(),
            2,
            "expected a delete+add pair, got {events:?}"
        );
        let mut ids: Vec<&str> = events.iter().map(|e| e.feature_id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["player-double-jump", "player-jump"]);
    }

    #[test]
    fn reports_no_events_when_nothing_changed_between_milestones() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");
        run_git(dir.path(), &["tag", "m2"]);

        let events = compute_changes(
            dir.path(),
            "m1",
            "m2",
            ChangeOptions {
                cache: CachePolicy::Bypass,
                impact_source: ImpactSource::HistoricalTree,
                granularity: Granularity::Feature,
            },
        )
        .unwrap();

        assert!(events.is_empty());
    }

    #[test]
    fn reports_changed_event_with_impacted_testcases_when_feature_tree_sha_differs() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");

        write_full_chain(dir.path(), "v2");
        commit_and_tag(dir.path(), "v2", "m2");

        let events = compute_changes(
            dir.path(),
            "m1",
            "m2",
            ChangeOptions {
                cache: CachePolicy::Bypass,
                impact_source: ImpactSource::HistoricalTree,
                granularity: Granularity::Feature,
            },
        )
        .unwrap();

        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.feature_id, "player-jump");
        assert_eq!(event.event_id, "player-jump--m1--m2");
        assert!(event.from_tree_sha.is_some());
        assert!(event.to_tree_sha.is_some());
        assert_ne!(event.from_tree_sha, event.to_tree_sha);
        assert_eq!(
            event.impacted_testcases,
            vec!["tc-controls-player-jump-jump-ground".to_string()]
        );
    }

    #[test]
    fn impacted_testcases_default_to_the_to_milestone_tree_ignoring_later_working_tree_changes() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");
        write_full_chain(dir.path(), "v2");
        commit_and_tag(dir.path(), "v2", "m2");

        // Simulate a later, uncommitted addition to the working tree made
        // after m2 was tagged: a second Condition/ExpectedResult under the
        // same Feature. Recomputing the m1..m2 interval later must not pick
        // this up under the default (historical) mode.
        let air = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/jump/air");
        fs::create_dir_all(&air).unwrap();
        fs::write(
            air.join("condition.yml"),
            "id: air\nbehavior: jump\nlabel: air\ndescription: |\n  Jump in the air.\n",
        )
        .unwrap();
        fs::create_dir_all(air.join("expected")).unwrap();
        fs::write(
            air.join("expected/001.yml"),
            "id: air-001\ncondition: air\ndescription: |\n  jumps safely\n",
        )
        .unwrap();

        let events = compute_changes(
            dir.path(),
            "m1",
            "m2",
            ChangeOptions {
                cache: CachePolicy::Bypass,
                impact_source: ImpactSource::HistoricalTree,
                granularity: Granularity::Feature,
            },
        )
        .unwrap();

        let event = events
            .iter()
            .find(|e| e.feature_id == "player-jump")
            .unwrap();
        assert_eq!(
            event.impacted_testcases,
            vec!["tc-controls-player-jump-jump-ground".to_string()]
        );
    }

    #[test]
    fn impacted_testcases_use_the_current_working_tree_when_use_current_tree_is_set() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");
        write_full_chain(dir.path(), "v2");
        commit_and_tag(dir.path(), "v2", "m2");

        // Same later, uncommitted addition as the historical-mode test
        // above, but this time the opt-in current-tree mode must reflect it
        // (legacy behavior, preserved for backward compatibility).
        let air = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/jump/air");
        fs::create_dir_all(&air).unwrap();
        fs::write(
            air.join("condition.yml"),
            "id: air\nbehavior: jump\nlabel: air\ndescription: |\n  Jump in the air.\n",
        )
        .unwrap();
        fs::create_dir_all(air.join("expected")).unwrap();
        fs::write(
            air.join("expected/001.yml"),
            "id: air-001\ncondition: air\ndescription: |\n  jumps safely\n",
        )
        .unwrap();

        let events = compute_changes(
            dir.path(),
            "m1",
            "m2",
            ChangeOptions {
                cache: CachePolicy::Bypass,
                impact_source: ImpactSource::CurrentWorkingTree,
                granularity: Granularity::Feature,
            },
        )
        .unwrap();

        let event = events
            .iter()
            .find(|e| e.feature_id == "player-jump")
            .unwrap();
        let mut impacted = event.impacted_testcases.clone();
        impacted.sort();
        assert_eq!(
            impacted,
            vec![
                "tc-controls-player-jump-jump-air".to_string(),
                "tc-controls-player-jump-jump-ground".to_string(),
            ]
        );
    }

    /// The core value of issue #15: with `--granularity behavior`, editing
    /// only one Behavior's Condition must not pull in TestCases generated
    /// from an untouched sibling Behavior under the same Feature — the
    /// default `Granularity::Feature` behavior this test's twin below
    /// contrasts against.
    #[test]
    fn granularity_behavior_narrows_impacted_testcases_to_the_changed_behavior_only() {
        let dir = init_repo();
        write_two_behavior_chain(dir.path(), "jump v1", "duck v1");
        commit_and_tag(dir.path(), "v1", "m1");

        // Only the `duck` Behavior's Condition changes; `jump` is untouched.
        fs::write(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/duck/low/condition.yml"),
            "id: low\nbehavior: duck\nlabel: low\ndescription: |\n  Duck lower.\n",
        )
        .unwrap();
        commit_and_tag(dir.path(), "v2", "m2");

        let events = compute_changes(
            dir.path(),
            "m1",
            "m2",
            ChangeOptions {
                cache: CachePolicy::Bypass,
                impact_source: ImpactSource::HistoricalTree,
                granularity: Granularity::Behavior,
            },
        )
        .unwrap();

        assert_eq!(events.len(), 1, "expected one ChangeEvent, got {events:?}");
        let event = &events[0];
        assert_eq!(event.feature_id, "player-jump");
        assert_eq!(event.impact_reason.granularity, Granularity::Behavior);
        assert_eq!(
            event.impacted_testcases,
            vec!["tc-controls-player-jump-duck-low".to_string()],
            "the untouched jump Behavior's TestCase must not be included"
        );
        assert_eq!(
            event.impact_reason.changed_paths,
            vec![".markharness/knowledge/controls/player-jump/duck/behavior.yml".to_string()],
            "changed_paths must name the duck Behavior as the evidence, not jump"
        );
    }

    /// Contrasts with the test above: the same edit, but with the default
    /// `Granularity::Feature`, must still include both Behaviors' TestCases
    /// — confirming `--granularity` is opt-in and doesn't change default
    /// behavior.
    #[test]
    fn granularity_feature_default_still_includes_every_behavior_under_the_changed_feature() {
        let dir = init_repo();
        write_two_behavior_chain(dir.path(), "jump v1", "duck v1");
        commit_and_tag(dir.path(), "v1", "m1");

        fs::write(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/duck/low/condition.yml"),
            "id: low\nbehavior: duck\nlabel: low\ndescription: |\n  Duck lower.\n",
        )
        .unwrap();
        commit_and_tag(dir.path(), "v2", "m2");

        let events = compute_changes(
            dir.path(),
            "m1",
            "m2",
            ChangeOptions {
                cache: CachePolicy::Bypass,
                impact_source: ImpactSource::HistoricalTree,
                granularity: Granularity::Feature,
            },
        )
        .unwrap();

        let event = &events[0];
        let mut impacted = event.impacted_testcases.clone();
        impacted.sort();
        assert_eq!(
            impacted,
            vec![
                "tc-controls-player-jump-duck-low".to_string(),
                "tc-controls-player-jump-jump-ground".to_string(),
            ]
        );
    }

    /// `--granularity condition` narrows even further than `behavior`: a
    /// second Condition added under the *same* Behavior as an untouched
    /// one must not pull the untouched Condition's TestCase in.
    #[test]
    fn granularity_condition_narrows_impacted_testcases_to_the_changed_condition_only() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        let air = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/jump/air");
        fs::create_dir_all(&air).unwrap();
        fs::write(
            air.join("condition.yml"),
            "id: air\nbehavior: jump\nlabel: air\ndescription: |\n  Jump in the air.\n",
        )
        .unwrap();
        fs::create_dir_all(air.join("expected")).unwrap();
        fs::write(
            air.join("expected/001.yml"),
            "id: air-001\ncondition: air\ndescription: |\n  jumps safely\n",
        )
        .unwrap();
        commit_and_tag(dir.path(), "v1", "m1");

        // Only the `air` Condition changes; `ground` is untouched.
        fs::write(
            air.join("condition.yml"),
            "id: air\nbehavior: jump\nlabel: air\ndescription: |\n  Jump in the air, higher.\n",
        )
        .unwrap();
        commit_and_tag(dir.path(), "v2", "m2");

        let events = compute_changes(
            dir.path(),
            "m1",
            "m2",
            ChangeOptions {
                cache: CachePolicy::Bypass,
                impact_source: ImpactSource::HistoricalTree,
                granularity: Granularity::Condition,
            },
        )
        .unwrap();

        let event = &events[0];
        assert_eq!(event.impact_reason.granularity, Granularity::Condition);
        assert_eq!(
            event.impacted_testcases,
            vec!["tc-controls-player-jump-jump-air".to_string()],
            "the untouched ground Condition's TestCase must not be included"
        );
        assert_eq!(
            event.impact_reason.changed_paths,
            vec![".markharness/knowledge/controls/player-jump/jump/air/condition.yml".to_string()],
            "changed_paths must name the air Condition as the evidence, not ground"
        );
    }

    /// Malformed `knowledge/`: a `condition.yml` directory that doesn't sit
    /// beneath a `behavior.yml` (here, directly under the Feature). Codex
    /// review flagged an earlier version of this code for panicking
    /// (`.expect`) on this input instead of failing gracefully —
    /// `CONTRIBUTING.md` requires malformed external content to error, not
    /// crash the process.
    #[test]
    fn granularity_condition_errors_instead_of_panicking_on_a_condition_with_no_parent_behavior() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");

        // Same depth as a well-formed `<feature>/<behavior>/<condition>/`
        // (so it resolves to the real `player-jump` Feature, and isn't
        // silently dropped as belonging to no Feature), but `not-a-behavior`
        // has no `behavior.yml` of its own.
        let stray = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/not-a-behavior/stray");
        fs::create_dir_all(&stray).unwrap();
        fs::write(
            stray.join("condition.yml"),
            "id: stray\nbehavior: nonexistent\nlabel: stray\ndescription: |\n  Not under a Behavior.\n",
        )
        .unwrap();
        commit_and_tag(dir.path(), "v2", "m2");

        let err = compute_changes(
            dir.path(),
            "m1",
            "m2",
            ChangeOptions {
                cache: CachePolicy::Bypass,
                impact_source: ImpactSource::HistoricalTree,
                granularity: Granularity::Condition,
            },
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("stray"),
            "expected the error to name the offending Condition, got: {err}"
        );
    }

    /// A Behavior added between milestones must have its TestCases show up
    /// as impacted at `--granularity behavior` (added, not just modified,
    /// subunits must be treated as changed).
    #[test]
    fn granularity_behavior_includes_a_newly_added_behavior() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");

        // `write_full_chain`'s fixed jump Behavior description, passed
        // through unchanged, so `jump` is byte-identical at m1 and m2 —
        // only the newly added `duck` Behavior differs.
        write_two_behavior_chain(dir.path(), "Player presses jump.", "duck v1");
        commit_and_tag(dir.path(), "v2", "m2");

        let events = compute_changes(
            dir.path(),
            "m1",
            "m2",
            ChangeOptions {
                cache: CachePolicy::Bypass,
                impact_source: ImpactSource::HistoricalTree,
                granularity: Granularity::Behavior,
            },
        )
        .unwrap();

        let event = &events[0];
        assert_eq!(
            event.impacted_testcases,
            vec!["tc-controls-player-jump-duck-low".to_string()],
            "the newly added duck Behavior's TestCase must be included, \
             and the untouched jump Behavior's must not"
        );
        assert_eq!(
            event.impact_reason.changed_paths,
            vec![".markharness/knowledge/controls/player-jump/duck/behavior.yml".to_string()],
            "changed_paths must name the newly added duck Behavior"
        );
    }

    #[test]
    fn reports_changed_event_when_only_a_condition_file_changes_and_feature_yml_is_untouched() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");

        // Only the Condition's description changes; feature.yml is
        // byte-for-byte identical between m1 and m2.
        fs::write(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/jump/ground/condition.yml"),
            "id: ground\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from a moving platform.\n",
        )
        .unwrap();
        commit_and_tag(dir.path(), "condition change", "m2");

        let events = compute_changes(
            dir.path(),
            "m1",
            "m2",
            ChangeOptions {
                cache: CachePolicy::Bypass,
                impact_source: ImpactSource::HistoricalTree,
                granularity: Granularity::Feature,
            },
        )
        .unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].feature_id, "player-jump");
    }

    #[test]
    fn reports_added_event_when_feature_did_not_exist_at_from_milestone() {
        let dir = init_repo();
        fs::create_dir_all(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();
        fs::write(dir.path().join("README.md"), "empty\n").unwrap();
        commit_and_tag(dir.path(), "empty", "m1");

        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "add feature", "m2");

        let events = compute_changes(
            dir.path(),
            "m1",
            "m2",
            ChangeOptions {
                cache: CachePolicy::Bypass,
                impact_source: ImpactSource::HistoricalTree,
                granularity: Granularity::Feature,
            },
        )
        .unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].from_tree_sha, None);
        assert!(events[0].to_tree_sha.is_some());
    }

    #[test]
    fn reports_removed_event_when_feature_no_longer_exists_at_to_milestone() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");

        fs::remove_dir_all(dir.path().join(".markharness/knowledge/controls")).unwrap();
        commit_and_tag(dir.path(), "remove feature", "m2");

        let events = compute_changes(
            dir.path(),
            "m1",
            "m2",
            ChangeOptions {
                cache: CachePolicy::Bypass,
                impact_source: ImpactSource::HistoricalTree,
                granularity: Granularity::Feature,
            },
        )
        .unwrap();

        assert_eq!(events.len(), 1);
        assert!(events[0].from_tree_sha.is_some());
        assert_eq!(events[0].to_tree_sha, None);
        assert!(events[0].impacted_testcases.is_empty());
    }

    #[test]
    fn compute_changes_leaves_change_type_as_none() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");
        write_full_chain(dir.path(), "v2");
        commit_and_tag(dir.path(), "v2", "m2");

        let events = compute_changes(
            dir.path(),
            "m1",
            "m2",
            ChangeOptions {
                cache: CachePolicy::Bypass,
                impact_source: ImpactSource::HistoricalTree,
                granularity: Granularity::Feature,
            },
        )
        .unwrap();

        assert_eq!(events[0].change_type, None);
    }

    #[test]
    fn change_type_serializes_as_snake_case() {
        let events = vec![ChangeEvent {
            event_id: "player-jump--m1--m2".to_string(),
            feature_id: "player-jump".to_string(),
            feature_uid: None,
            feature_id_at_from: None,
            feature_id_at_to: None,
            from_milestone: "m1".to_string(),
            to_milestone: "m2".to_string(),
            from_tree_sha: Some("aaa".to_string()),
            to_tree_sha: Some("bbb".to_string()),
            impacted_testcases: vec!["tc-ground-001".to_string()],
            impact_reason: ImpactReason::default(),
            change_type: Some(ChangeType::SpecChange),
            true_divergences: Vec::new(),
            related_events: Vec::new(),
        }];

        let yaml = serialize_changes(&events);

        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(parsed[0]["change_type"].as_str(), Some("spec_change"));
    }

    /// `changes/*.yaml` files written before `change_type` existed have no
    /// such key; reading them must not fail (`#[serde(default)]`).
    #[test]
    fn read_changes_defaults_change_type_to_none_for_files_written_before_the_field_existed() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("changes"),
        )
        .unwrap();
        fs::write(
            dir.path().join(".markharness/changes/m2.yaml"),
            "- event_id: player-jump--m1--m2\n  feature_id: player-jump\n  from_milestone: m1\n  to_milestone: m2\n  from_tree_sha: aaa\n  to_tree_sha: bbb\n  impacted_testcases: [tc-ground-001]\n",
        )
        .unwrap();

        let read = read_changes(dir.path(), "m2").unwrap();

        assert_eq!(read[0].change_type, None);
    }

    /// Same as above for `granularity` (issue #15): a `ChangeEvent` written
    /// before this field existed has no `granularity` key, and must be
    /// read back as `Feature` — exactly the behavior it had.
    #[test]
    fn read_changes_defaults_granularity_to_feature_for_files_written_before_the_field_existed() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("changes"),
        )
        .unwrap();
        fs::write(
            dir.path().join(".markharness/changes/m2.yaml"),
            "- event_id: player-jump--m1--m2\n  feature_id: player-jump\n  from_milestone: m1\n  to_milestone: m2\n  from_tree_sha: aaa\n  to_tree_sha: bbb\n  impacted_testcases: [tc-ground-001]\n",
        )
        .unwrap();

        let read = read_changes(dir.path(), "m2").unwrap();

        assert_eq!(read[0].impact_reason.granularity, Granularity::Feature);
    }

    /// Codex review on PR #36: a `ChangeEvent` written with issue #15's
    /// short-lived intermediate shape (a top-level `granularity: behavior`
    /// scalar, before the `impact_reason` follow-up fix) must migrate its
    /// recorded granularity into `impact_reason.granularity`, not silently
    /// discard it as `#[serde(default)]` would reset it to `Feature`.
    #[test]
    fn read_changes_migrates_the_legacy_top_level_granularity_field_into_impact_reason() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("changes"),
        )
        .unwrap();
        fs::write(
            dir.path().join(".markharness/changes/m2.yaml"),
            "- event_id: player-jump--m1--m2\n  feature_id: player-jump\n  from_milestone: m1\n  to_milestone: m2\n  from_tree_sha: aaa\n  to_tree_sha: bbb\n  impacted_testcases: [tc-ground-001]\n  granularity: behavior\n",
        )
        .unwrap();

        let read = read_changes(dir.path(), "m2").unwrap();

        assert_eq!(read[0].impact_reason.granularity, Granularity::Behavior);
        assert!(
            read[0].impact_reason.changed_paths.is_empty(),
            "changed_paths was never recorded in the intermediate shape, so it migrates to empty"
        );
    }

    /// A `ChangeEvent` already written in the current `impact_reason` shape
    /// must not be affected by the legacy-`granularity` migration (no
    /// top-level `granularity` key to accidentally reinterpret).
    #[test]
    fn read_changes_leaves_the_current_impact_reason_shape_untouched() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("changes"),
        )
        .unwrap();
        fs::write(
            dir.path().join(".markharness/changes/m2.yaml"),
            "- event_id: player-jump--m1--m2\n  feature_id: player-jump\n  from_milestone: m1\n  to_milestone: m2\n  from_tree_sha: aaa\n  to_tree_sha: bbb\n  impacted_testcases: [tc-ground-001]\n  impact_reason:\n    granularity: condition\n    changed_paths: [foo/condition.yml]\n",
        )
        .unwrap();

        let read = read_changes(dir.path(), "m2").unwrap();

        assert_eq!(read[0].impact_reason.granularity, Granularity::Condition);
        assert_eq!(
            read[0].impact_reason.changed_paths,
            vec!["foo/condition.yml".to_string()]
        );
    }

    /// `changes compute` records the granularity and the changed-path
    /// evidence it was run with (issue #15) so a reader of
    /// `changes/<to>.yaml` doesn't have to guess whether/why
    /// `impacted_testcases` was narrowed below Feature level.
    #[test]
    fn impact_reason_serializes_granularity_as_snake_case_and_carries_changed_paths() {
        let events = vec![ChangeEvent {
            impact_reason: ImpactReason {
                granularity: Granularity::Behavior,
                changed_paths: vec![
                    ".markharness/knowledge/controls/player-jump/duck/behavior.yml".to_string(),
                ],
            },
            ..sample_event("player-jump--m1--m2")
        }];

        let yaml = serialize_changes(&events);

        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(
            parsed[0]["impact_reason"]["granularity"].as_str(),
            Some("behavior")
        );
        assert_eq!(
            parsed[0]["impact_reason"]["changed_paths"][0].as_str(),
            Some(".markharness/knowledge/controls/player-jump/duck/behavior.yml")
        );
    }

    #[test]
    fn serialize_changes_produces_valid_yaml() {
        let events = vec![ChangeEvent {
            event_id: "player-jump--m1--m2".to_string(),
            feature_id: "player-jump".to_string(),
            feature_uid: None,
            feature_id_at_from: None,
            feature_id_at_to: None,
            from_milestone: "m1".to_string(),
            to_milestone: "m2".to_string(),
            from_tree_sha: Some("aaa".to_string()),
            to_tree_sha: Some("bbb".to_string()),
            impacted_testcases: vec!["tc-ground-001".to_string()],
            impact_reason: ImpactReason::default(),
            change_type: None,
            true_divergences: Vec::new(),
            related_events: Vec::new(),
        }];

        let yaml = serialize_changes(&events);

        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(parsed[0]["feature_id"].as_str(), Some("player-jump"));
    }

    #[test]
    fn read_changes_returns_events_written_by_serialize_changes() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("changes"),
        )
        .unwrap();
        let events = vec![ChangeEvent {
            event_id: "player-jump--m1--m2".to_string(),
            feature_id: "player-jump".to_string(),
            feature_uid: None,
            feature_id_at_from: None,
            feature_id_at_to: None,
            from_milestone: "m1".to_string(),
            to_milestone: "m2".to_string(),
            from_tree_sha: Some("aaa".to_string()),
            to_tree_sha: Some("bbb".to_string()),
            impacted_testcases: vec!["tc-ground-001".to_string()],
            impact_reason: ImpactReason::default(),
            change_type: None,
            true_divergences: Vec::new(),
            related_events: Vec::new(),
        }];
        fs::write(
            dir.path().join(".markharness/changes/m2.yaml"),
            serialize_changes(&events),
        )
        .unwrap();

        let read = read_changes(dir.path(), "m2").unwrap();

        assert_eq!(read, events);
    }

    #[test]
    fn read_changes_returns_empty_when_file_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();

        let read = read_changes(dir.path(), "m2").unwrap();

        assert!(read.is_empty());
    }

    fn sample_event(event_id: &str) -> ChangeEvent {
        ChangeEvent {
            event_id: event_id.to_string(),
            feature_id: "player-jump".to_string(),
            feature_uid: None,
            feature_id_at_from: None,
            feature_id_at_to: None,
            from_milestone: "m1".to_string(),
            to_milestone: "m2".to_string(),
            from_tree_sha: Some("aaa".to_string()),
            to_tree_sha: Some("bbb".to_string()),
            impacted_testcases: vec!["tc-ground-001".to_string()],
            impact_reason: ImpactReason::default(),
            change_type: None,
            true_divergences: Vec::new(),
            related_events: Vec::new(),
        }
    }

    #[test]
    fn annotate_change_type_sets_the_field_on_the_matching_event() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("changes"),
        )
        .unwrap();
        fs::write(
            dir.path().join(".markharness/changes/m2.yaml"),
            serialize_changes(&[sample_event("player-jump--m1--m2")]),
        )
        .unwrap();

        annotate_change_type(dir.path(), "player-jump--m1--m2", ChangeType::BugFix).unwrap();

        let events = read_changes(dir.path(), "m2").unwrap();
        assert_eq!(events[0].change_type, Some(ChangeType::BugFix));
    }

    #[test]
    fn annotate_change_type_preserves_other_events_in_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("changes"),
        )
        .unwrap();
        fs::write(
            dir.path().join(".markharness/changes/m2.yaml"),
            serialize_changes(&[
                sample_event("player-jump--m1--m2"),
                sample_event("other-feature--m1--m2"),
            ]),
        )
        .unwrap();

        annotate_change_type(dir.path(), "player-jump--m1--m2", ChangeType::Refactor).unwrap();

        let events = read_changes(dir.path(), "m2").unwrap();
        let untouched = events
            .iter()
            .find(|e| e.event_id == "other-feature--m1--m2")
            .unwrap();
        assert_eq!(untouched.change_type, None);
    }

    #[test]
    fn annotate_change_type_searches_across_multiple_changes_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("changes"),
        )
        .unwrap();
        fs::write(
            dir.path().join(".markharness/changes/m2.yaml"),
            serialize_changes(&[sample_event("player-jump--m1--m2")]),
        )
        .unwrap();
        fs::write(
            dir.path().join(".markharness/changes/m3.yaml"),
            serialize_changes(&[sample_event("player-jump--m2--m3")]),
        )
        .unwrap();

        annotate_change_type(dir.path(), "player-jump--m2--m3", ChangeType::Other).unwrap();

        let events = read_changes(dir.path(), "m3").unwrap();
        assert_eq!(events[0].change_type, Some(ChangeType::Other));
    }

    #[test]
    fn records_both_parent_tree_shas_when_to_milestone_is_a_true_divergence_merge_commit() {
        let dir = init_repo();
        write_full_chain(dir.path(), "base");
        commit_and_tag(dir.path(), "base", "m1");
        run_git(dir.path(), &["branch", "feature"]);

        write_full_chain(dir.path(), "changed-on-main");
        commit_and_tag(dir.path(), "on main", "main-tip");

        run_git(dir.path(), &["checkout", "-q", "feature"]);
        write_full_chain(dir.path(), "changed-on-feature");
        commit_and_tag(dir.path(), "on feature", "feature-tip");

        run_git(dir.path(), &["checkout", "-q", "main"]);
        run_git(
            dir.path(),
            &[
                "merge", "-q", "-m", "merge", "-X", "ours", "--no-ff", "feature",
            ],
        );
        run_git(dir.path(), &["tag", "m2"]);

        let events = compute_changes(
            dir.path(),
            "m1",
            "m2",
            ChangeOptions {
                cache: CachePolicy::Bypass,
                impact_source: ImpactSource::HistoricalTree,
                granularity: Granularity::Feature,
            },
        )
        .unwrap();

        let event = events
            .iter()
            .find(|e| e.feature_id == "player-jump")
            .unwrap();
        assert_eq!(event.true_divergences.len(), 1);
        let parent_tree_shas = &event.true_divergences[0].parent_tree_shas;
        assert_ne!(parent_tree_shas[0], parent_tree_shas[1]);
    }

    /// Regression: `lineage::classify` returns `TrueDivergence` not only when
    /// both parents changed a Feature differently, but also when one branch
    /// *deleted* the Feature and the other changed it (base=Some, one
    /// parent=None, other parent=Some(!=base) — neither equals the other nor
    /// the base). `true_divergence_parent_tree_shas` must not assume both
    /// parent tree SHAs are `Some` in that case.
    #[test]
    fn does_not_panic_when_true_divergence_involves_a_feature_deleted_on_one_branch() {
        let dir = init_repo();
        write_full_chain(dir.path(), "base");
        commit_and_tag(dir.path(), "base", "m1");
        run_git(dir.path(), &["branch", "feature"]);

        run_git(
            dir.path(),
            &["rm", "-rq", ".markharness/knowledge/controls/player-jump"],
        );
        commit_and_tag(dir.path(), "delete on main", "main-tip");

        run_git(dir.path(), &["checkout", "-q", "feature"]);
        write_full_chain(dir.path(), "changed-on-feature");
        commit_and_tag(dir.path(), "change on feature", "feature-tip");

        run_git(dir.path(), &["checkout", "-q", "main"]);
        // A modify/delete conflict isn't auto-resolved by `-X ours`/`-X
        // theirs`; resolve it manually by keeping the feature branch's
        // (modified, surviving) version, matching a maintainer resolving a
        // real conflict in favor of the change rather than the deletion.
        let merge_status = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["merge", "--no-ff", "-q", "-m", "merge", "feature"])
            .status()
            .unwrap();
        assert!(!merge_status.success(), "expected a merge conflict");
        run_git(
            dir.path(),
            &[
                "checkout",
                "feature",
                "--",
                ".markharness/knowledge/controls/player-jump",
            ],
        );
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "--no-edit"]);
        run_git(dir.path(), &["tag", "m2"]);

        let events = compute_changes(
            dir.path(),
            "m1",
            "m2",
            ChangeOptions {
                cache: CachePolicy::Bypass,
                impact_source: ImpactSource::HistoricalTree,
                granularity: Granularity::Feature,
            },
        )
        .unwrap();

        let event = events
            .iter()
            .find(|e| e.feature_id == "player-jump")
            .unwrap();
        assert!(event.true_divergences.is_empty());
    }

    #[test]
    fn leaves_true_divergences_empty_when_to_milestone_is_not_a_merge_commit() {
        let dir = init_repo();
        write_full_chain(dir.path(), "v1");
        commit_and_tag(dir.path(), "v1", "m1");
        write_full_chain(dir.path(), "v2");
        commit_and_tag(dir.path(), "v2", "m2");

        let events = compute_changes(
            dir.path(),
            "m1",
            "m2",
            ChangeOptions {
                cache: CachePolicy::Bypass,
                impact_source: ImpactSource::HistoricalTree,
                granularity: Granularity::Feature,
            },
        )
        .unwrap();

        assert!(events[0].true_divergences.is_empty());
    }

    #[test]
    fn annotate_change_type_errors_when_event_id_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("changes"),
        )
        .unwrap();
        fs::write(
            dir.path().join(".markharness/changes/m2.yaml"),
            serialize_changes(&[sample_event("player-jump--m1--m2")]),
        )
        .unwrap();

        let result = annotate_change_type(dir.path(), "no-such-event", ChangeType::Other);

        assert!(matches!(result, Err(AnnotateError::NotFound(id)) if id == "no-such-event"));
    }

    #[test]
    fn related_events_defaults_to_empty_and_round_trips_through_yaml() {
        let mut event = sample_event("player-jump--m1--m2");
        assert!(event.related_events.is_empty());
        event.related_events = vec!["other-feature--m1--m2".to_string()];

        let yaml = serialize_changes(&[event]);
        let parsed: Vec<ChangeEvent> = serde_yaml_ng::from_str(&yaml).unwrap();

        assert_eq!(
            parsed[0].related_events,
            vec!["other-feature--m1--m2".to_string()]
        );
    }

    /// `changes/*.yaml` files written before `related_events` existed have
    /// no such key; reading them must not fail (`#[serde(default)]`).
    #[test]
    fn read_changes_defaults_related_events_to_empty_for_files_written_before_the_field_existed() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("changes"),
        )
        .unwrap();
        fs::write(
            dir.path().join(".markharness/changes/m2.yaml"),
            "- event_id: player-jump--m1--m2\n  feature_id: player-jump\n  from_milestone: m1\n  to_milestone: m2\n  from_tree_sha: aaa\n  to_tree_sha: bbb\n  impacted_testcases: [tc-ground-001]\n",
        )
        .unwrap();

        let read = read_changes(dir.path(), "m2").unwrap();

        assert!(read[0].related_events.is_empty());
    }

    #[test]
    fn annotate_related_events_appends_ids_on_the_matching_event() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("changes"),
        )
        .unwrap();
        fs::write(
            dir.path().join(".markharness/changes/m2.yaml"),
            serialize_changes(&[
                sample_event("player-jump--m1--m2"),
                sample_event("other-feature--m1--m2"),
            ]),
        )
        .unwrap();

        annotate_related_events(
            dir.path(),
            "player-jump--m1--m2",
            &["other-feature--m1--m2".to_string()],
        )
        .unwrap();

        let events = read_changes(dir.path(), "m2").unwrap();
        let annotated = events
            .iter()
            .find(|e| e.event_id == "player-jump--m1--m2")
            .unwrap();
        assert_eq!(
            annotated.related_events,
            vec!["other-feature--m1--m2".to_string()]
        );
        let untouched = events
            .iter()
            .find(|e| e.event_id == "other-feature--m1--m2")
            .unwrap();
        assert!(untouched.related_events.is_empty());
    }

    #[test]
    fn annotate_related_events_errors_when_the_target_event_id_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("changes"),
        )
        .unwrap();
        fs::write(
            dir.path().join(".markharness/changes/m2.yaml"),
            serialize_changes(&[sample_event("player-jump--m1--m2")]),
        )
        .unwrap();

        let result = annotate_related_events(dir.path(), "no-such-event", &[]);

        assert!(matches!(result, Err(AnnotateError::NotFound(id)) if id == "no-such-event"));
    }

    #[test]
    fn annotate_related_events_errors_when_a_related_event_id_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("changes"),
        )
        .unwrap();
        fs::write(
            dir.path().join(".markharness/changes/m2.yaml"),
            serialize_changes(&[sample_event("player-jump--m1--m2")]),
        )
        .unwrap();

        let result = annotate_related_events(
            dir.path(),
            "player-jump--m1--m2",
            &["no-such-event".to_string()],
        );

        assert!(matches!(result, Err(AnnotateError::NotFound(id)) if id == "no-such-event"));
        let events = read_changes(dir.path(), "m2").unwrap();
        assert!(events[0].related_events.is_empty());
    }

    #[test]
    fn change_options_default_to_cached_historical_impact() {
        assert_eq!(
            ChangeOptions::default(),
            ChangeOptions {
                cache: CachePolicy::Use,
                impact_source: ImpactSource::HistoricalTree,
                granularity: Granularity::Feature,
            }
        );
    }
}
