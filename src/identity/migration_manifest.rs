//! The migration manifest (ADR 0013, design doc §12 "残っている論点";
//! scoped per grilling session to the concrete gap identified while
//! implementing Phase 4): a `legacy_case_id` -> `case_uid` Rosetta stone.
//!
//! Every one of the five Knowledge element kinds carries its own identity
//! event log, so its pre-migration id is already recoverable from its root
//! `Issued` event — no separate manifest is needed at that level. A
//! TestCase is different: it is a derived artifact, not one of the five
//! `EntityKind`s, so it has no identity event log of its own. Once all five
//! of its contributing elements are migrated, `generate::compute_case_uid`
//! can finally compute its `case_uid` (design doc §8) — but the
//! `case_id` string a project's existing `changes/*.yaml`,
//! `executions/*/results.yml`, and external tooling already reference
//! predates that and needs a durable place to resolve into it. This module
//! is that place.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::execution::iso8601_utc_now;
use crate::fs_safety::replace_file;
use crate::generate::{TestCase, generate_testcases};
use crate::identity::EntityKind;

fn manifest_path(root: &Path) -> PathBuf {
    root.join(crate::project_root::MARKHARNESS_DIR)
        .join("identity-migration-manifest.yml")
}

/// Repo-relative path to the manifest, for `git` pathspecs (`Path::join`
/// is deliberately not used here — see `project_root::KNOWLEDGE_PATH_IN_REPO`'s
/// own doc comment on why a raw string is required for those).
const MANIFEST_PATH_IN_REPO: &str = ".markharness/identity-migration-manifest.yml";

/// Where one of a case's five contributing elements lived within a
/// `LegacySnapshot`'s shared `tree_sha` (ADR 0013 design doc §12: "entity
/// kind、旧ID、旧path/content locator") — its `EntityKind`, the id it
/// carried at capture time, and its repo-relative path. This is the
/// qualifier that turns the shared, project-wide `tree_sha` into a locator
/// for this specific element: `tree_sha` alone says "the whole knowledge/
/// tree looked like this," and a locator says "and this element, of this
/// kind, under this id, lived at this path within it."
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LegacyElementLocator {
    pub entity_kind: EntityKind,
    pub legacy_id: String,
    pub path: String,
}

/// A case's legacy (pre-migration) snapshot identity (ADR 0013 design doc
/// §12: "legacy snapshot identity (tree SHA)、entity kind、旧ID、旧path/
/// content locator").
///
/// `tree_sha` is the real git tree SHA of the whole `.markharness/knowledge`
/// directory (`git::write_tree_prefix` for a live, possibly-uncommitted
/// working tree, or `git::tree_sha` for a committed ref) as it stood at the
/// moment this case's legacy identity was captured. It is *not* a per-case
/// value: every case captured in the same operation shares the exact same
/// `tree_sha`, because it names one point in the project's history, not a
/// fingerprint scoped to one case. (A TestCase itself spans four *nested*
/// Knowledge levels — requirement -> feature -> behavior -> condition —
/// plus every ExpectedResult under its Condition, so no single subtree of
/// `knowledge/` names exactly "this one case's five files and nothing
/// else": the tree SHA of, say, the Requirement's own directory would
/// necessarily also cover every sibling Feature under it. Using the whole
/// `knowledge/` tree's SHA sidesteps that entirely — it needs no case-
/// shaped subtree to exist.)
///
/// The five `LegacyElementLocator`s are the qualifier ADR 0013 also names
/// ("entity kind、旧ID、旧path/content locator"): they say which entity_kind,
/// which id, and which path each of this case's five contributing elements
/// had within that shared `tree_sha`, so the same tree_sha can be reused
/// (and correctly compared) across every case captured alongside this one.
///
/// `case_id` alone cannot serve as a lookup key here — it is built only
/// from the four ancestor ids (requirement/feature/behavior/condition), so
/// it stays identical across an ExpectedResult being added, removed, or
/// edited even though `case_uid` (which *does* depend on the ExpectedResult
/// set) changes. Content-hash-only designs (an ExpectedResult id set, then
/// raw file bytes, then git blob SHAs per file) were each tried and
/// rejected in earlier review rounds: either they missed a reissue
/// scenario, or — for the raw-bytes and per-file-blob-SHA schemes — they
/// were real, git-native identity but not literally the ADR's named "tree
/// SHA". A whole-`knowledge/`-tree SHA is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LegacySnapshot {
    pub tree_sha: String,
    pub requirement: LegacyElementLocator,
    pub feature: LegacyElementLocator,
    pub behavior: LegacyElementLocator,
    pub condition: LegacyElementLocator,
    /// Sorted by path, so file read order never matters.
    pub expected: Vec<LegacyElementLocator>,
}

/// One `case_id` -> `case_uid` resolution, and when it was recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub legacy_case_id: String,
    pub case_uid: String,
    pub legacy_snapshot: LegacySnapshot,
    pub recorded_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub entries: Vec<ManifestEntry>,
}

/// Reads the manifest, or an empty one if it doesn't exist yet (a project
/// that has never had a `case_uid` become computable has nothing to
/// record).
pub fn read(root: &Path) -> io::Result<Manifest> {
    match fs::read_to_string(manifest_path(root)) {
        Ok(content) => serde_yaml_ng::from_str(&content).map_err(io::Error::other),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Manifest::default()),
        Err(e) => Err(e),
    }
}

fn write(root: &Path, manifest: &Manifest) -> io::Result<()> {
    let yaml = serde_yaml_ng::to_string(manifest).map_err(io::Error::other)?;
    replace_file(root, &manifest_path(root), yaml.as_bytes())
}

/// Builds `testcase`'s `LegacySnapshot` from a shared `tree_sha` (the same
/// value for every case captured in one operation — see `LegacySnapshot`'s
/// own doc comment) and `testcase`'s own ids (`TestCase::generated_from`)
/// and paths (`TestCase::case_files`), which are already known to line up
/// index-for-index for `expected` (both are built from the same sorted
/// `expected/*.yml` directory listing in `generate::load_knowledge_snapshot`).
fn build_legacy_snapshot(tree_sha: String, testcase: &TestCase) -> LegacySnapshot {
    let locator = |entity_kind: EntityKind, legacy_id: &str, path: &str| LegacyElementLocator {
        entity_kind,
        legacy_id: legacy_id.to_string(),
        path: path.to_string(),
    };
    LegacySnapshot {
        tree_sha,
        requirement: locator(
            EntityKind::Requirement,
            &testcase.generated_from.requirement,
            &testcase.case_files.requirement,
        ),
        feature: locator(
            EntityKind::Feature,
            &testcase.generated_from.feature,
            &testcase.case_files.feature,
        ),
        behavior: locator(
            EntityKind::Behavior,
            &testcase.generated_from.behavior,
            &testcase.case_files.behavior,
        ),
        condition: locator(
            EntityKind::Condition,
            &testcase.generated_from.condition,
            &testcase.case_files.condition,
        ),
        expected: testcase
            .generated_from
            .expected_results
            .iter()
            .zip(&testcase.case_files.expected)
            .map(|(legacy_id, path)| locator(EntityKind::ExpectedResult, legacy_id, path))
            .collect(),
    }
}

/// A map of every currently-generatable case's `case_id` -> `LegacySnapshot`,
/// constructible only by [`capture_case_signatures`] (a genuine, fresh
/// capture) or [`LegacyCaseSignatures::from_durable_payload`] (an exact
/// reconstruction of a snapshot `to_durable_payload` previously produced) —
/// never from an arbitrary map a caller assembled by hand. This exists so a
/// type error, not a code review, catches a caller accidentally passing
/// `record_new_case_uids` a signature map recomputed from the *current*
/// (possibly already-migrated) working tree instead of the actual
/// pre-migration legacy snapshot `capture_case_signatures` captured before
/// `feature_ops::migrate_all` wrote anything.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LegacyCaseSignatures(std::collections::BTreeMap<String, LegacySnapshot>);

impl LegacyCaseSignatures {
    fn get(&self, case_id: &str) -> Option<&LegacySnapshot> {
        self.0.get(case_id)
    }

    /// Serializes to a flat string map suitable for `recovery::Intent`'s
    /// generic `caller_payload` field, so a legacy snapshot captured before
    /// a migration's logical commit point survives a crash and is
    /// available again — via [`from_durable_payload`](Self::from_durable_payload)
    /// — to whichever process eventually runs `roll_forward`.
    pub fn to_durable_payload(&self) -> io::Result<std::collections::BTreeMap<String, String>> {
        self.0
            .iter()
            .map(|(case_id, snapshot)| {
                serde_yaml_ng::to_string(snapshot)
                    .map(|yaml| (case_id.clone(), yaml))
                    .map_err(io::Error::other)
            })
            .collect()
    }

    /// The exact inverse of [`to_durable_payload`](Self::to_durable_payload)
    /// — reconstructs precisely what was captured, never a fresh capture of
    /// whatever the working tree looks like now. This is what lets crash
    /// recovery see the same legacy identity `capture_case_signatures`
    /// originally captured, even though by the time recovery runs the
    /// working tree has already been migrated.
    pub fn from_durable_payload(
        payload: &std::collections::BTreeMap<String, String>,
    ) -> io::Result<Self> {
        payload
            .iter()
            .map(|(case_id, yaml)| {
                serde_yaml_ng::from_str(yaml)
                    .map(|snapshot| (case_id.clone(), snapshot))
                    .map_err(io::Error::other)
            })
            .collect::<io::Result<std::collections::BTreeMap<String, LegacySnapshot>>>()
            .map(LegacyCaseSignatures)
    }
}

/// Snapshots every currently-generatable case's `case_id` -> `LegacySnapshot`
/// from the working tree as it stands *right now* — the ground truth for
/// "what did this case's five contributing files actually look like" at
/// this instant, whether or not `case_uid` is computable yet.
/// `feature_ops::migrate_all` calls this *before* writing any migration
/// changes (and durably persists the result via
/// `LegacyCaseSignatures::to_durable_payload`, see `recovery::Intent::caller_payload`),
/// so the map it returns is this project's legacy (pre-migration) state,
/// suitable for `record_new_case_uids`'s `legacy_signatures` argument. Also
/// used directly by callers (tests, or any caller invoking
/// `record_new_case_uids` outside of `migrate_all`) where the working tree
/// hasn't changed since the last relevant read, and so "right now" and
/// "legacy" coincide.
pub fn capture_case_signatures(root: &Path) -> io::Result<LegacyCaseSignatures> {
    let testcases = generate_testcases(
        &root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge"),
    )?;
    if testcases.is_empty() {
        return Ok(LegacyCaseSignatures(std::collections::BTreeMap::new()));
    }
    // One tree_sha for every case in this capture (see `LegacySnapshot`'s
    // doc comment) — computed once, not per case.
    let tree_sha =
        crate::git::write_tree_prefix(root, crate::project_root::KNOWLEDGE_PATH_IN_REPO)?;
    let mut map = std::collections::BTreeMap::new();
    for testcase in testcases {
        let snapshot = build_legacy_snapshot(tree_sha.clone(), &testcase);
        map.insert(testcase.case_id, snapshot);
    }
    Ok(LegacyCaseSignatures(map))
}

/// Recomputes `generate_testcases` against the current (post-migration)
/// working tree and appends a manifest entry for every `(case_id,
/// case_uid)` pair that is now computable and not already recorded, whose
/// `legacy_snapshot` comes from `legacy_signatures` — the case's identity
/// as it stood *before* whatever migration just made its `case_uid`
/// computable, not the (already-mutated) tree this function itself reads
/// to learn `case_uid`. See `LegacySnapshot`'s own doc comment for why
/// recomputing from the post-migration tree instead would be wrong.
/// Idempotent — safe to call after every `identity migrate`, and safe to
/// call again with the same or a freshly-recaptured `legacy_signatures`
/// snapshot: a `case_uid` is a pure function of five `uid`s that never
/// change once issued, so once a `(case_id, case_uid)` pair has its legacy
/// identity recorded once, that recording is permanent — a later call
/// recomputing `legacy_signatures` against the (by then already-migrated)
/// working tree must never overwrite or duplicate it with a "legacy"
/// snapshot that isn't actually legacy. Keyed on `(case_id, case_uid)`, not
/// on `case_id` alone, precisely so an ExpectedResult being added, removed,
/// or otherwise changed under an unchanged `case_id` — which changes
/// `case_uid` — records a *new* entry instead of being silently skipped
/// because `case_id` was already "recorded". Returns the newly recorded
/// entries, if any.
pub fn record_new_case_uids(
    root: &Path,
    legacy_signatures: &LegacyCaseSignatures,
) -> io::Result<Vec<ManifestEntry>> {
    let mut manifest = read(root)?;
    let already_recorded: std::collections::BTreeSet<(&str, &str)> = manifest
        .entries
        .iter()
        .map(|entry| (entry.legacy_case_id.as_str(), entry.case_uid.as_str()))
        .collect();

    let testcases = generate_testcases(
        &root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge"),
    )?;
    let recorded_at = iso8601_utc_now();
    let mut newly_recorded = Vec::new();
    for testcase in testcases {
        let Some(case_uid) = testcase.case_uid else {
            continue;
        };
        if already_recorded.contains(&(testcase.case_id.as_str(), case_uid.as_str())) {
            continue;
        }
        // No entry for this case_id in the legacy snapshot means it did not
        // exist yet when that snapshot was captured — nothing to record a
        // legacy identity for.
        let Some(legacy_snapshot) = legacy_signatures.get(&testcase.case_id) else {
            continue;
        };
        newly_recorded.push(ManifestEntry {
            legacy_case_id: testcase.case_id,
            case_uid,
            legacy_snapshot: legacy_snapshot.clone(),
            recorded_at: recorded_at.clone(),
        });
    }

    if newly_recorded.is_empty() {
        return Ok(newly_recorded);
    }
    manifest.entries.extend(newly_recorded.clone());
    manifest.entries.sort_by(|a, b| {
        (&a.legacy_case_id, &a.legacy_snapshot).cmp(&(&b.legacy_case_id, &b.legacy_snapshot))
    });
    write(root, &manifest)?;
    Ok(newly_recorded)
}

/// `legacy_case_id` resolved to more than one distinct `case_uid` within a
/// single manifest (design doc §12: "複数候補が残る場合は決定的なエラー
/// にする" — never silently pick one). Carries every distinct candidate
/// found, sorted, for a caller to report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmbiguousCaseId {
    pub legacy_case_id: String,
    pub case_uids: Vec<String>,
}

/// Looks up `legacy_case_id`'s `case_uid`, if the manifest has recorded
/// exactly one. `Err` when it has recorded more than one distinct
/// `case_uid` for the same `case_id` (design doc §12) — this is expected,
/// not corruption, whenever ExpectedResults were added or removed without
/// the case's ancestor ids changing; resolving it requires more context
/// than a bare `case_id` carries (e.g. which milestone/ref the caller's
/// own record was made against), which is exactly why this function
/// refuses to guess.
pub fn resolve_case_uid<'a>(
    manifest: &'a Manifest,
    legacy_case_id: &str,
) -> Result<Option<&'a str>, AmbiguousCaseId> {
    let mut case_uids: Vec<&'a str> = manifest
        .entries
        .iter()
        .filter(|entry| entry.legacy_case_id == legacy_case_id)
        .map(|entry| entry.case_uid.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    match case_uids.len() {
        0 => Ok(None),
        1 => Ok(Some(case_uids.remove(0))),
        _ => Err(AmbiguousCaseId {
            legacy_case_id: legacy_case_id.to_string(),
            case_uids: case_uids.into_iter().map(str::to_string).collect(),
        }),
    }
}

/// Looks up `legacy_case_id`'s `case_uid`, using `legacy_snapshot` as the
/// snapshot-qualifying key (design doc §12). When a snapshot is given, it
/// is authoritative: a precise `(legacy_case_id, legacy_snapshot)` match
/// resolves to that entry's `case_uid` (or `Err` if more than one distinct
/// `case_uid` is somehow recorded under the exact same pair — data
/// corruption, not ambiguity), and *no* match returns `Ok(None)` rather
/// than falling back to a case-id-only guess — a snapshot that fails to
/// match means the caller's snapshot genuinely is not the one any recorded
/// entry names, so nothing here can be trusted to identify it (design doc
/// §12: never resolve a migration boundary comparison without proof). The
/// case-id-only behavior of `resolve_case_uid` is only used when no
/// snapshot is given at all — the caller has no snapshot content to
/// qualify with, not merely one that failed to match.
pub fn resolve_case_uid_with_signature<'a>(
    manifest: &'a Manifest,
    legacy_case_id: &str,
    legacy_snapshot: Option<&LegacySnapshot>,
) -> Result<Option<&'a str>, AmbiguousCaseId> {
    let Some(snapshot) = legacy_snapshot else {
        return resolve_case_uid(manifest, legacy_case_id);
    };
    let mut case_uids: Vec<&'a str> = manifest
        .entries
        .iter()
        .filter(|entry| {
            entry.legacy_case_id == legacy_case_id && entry.legacy_snapshot == *snapshot
        })
        .map(|entry| entry.case_uid.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    match case_uids.len() {
        0 => Ok(None),
        1 => Ok(Some(case_uids.remove(0))),
        _ => Err(AmbiguousCaseId {
            legacy_case_id: legacy_case_id.to_string(),
            case_uids: case_uids.into_iter().map(str::to_string).collect(),
        }),
    }
}

/// Reads the manifest as committed at `git_ref` (not the working tree),
/// or an empty one if the file didn't exist yet at that ref (a snapshot
/// from before any `case_uid` had ever become computable).
pub fn read_from_ref(root: &Path, git_ref: &str) -> io::Result<Manifest> {
    let Some(blob_sha) = crate::git::tree_sha(root, git_ref, MANIFEST_PATH_IN_REPO)? else {
        return Ok(Manifest::default());
    };
    let content = crate::git::show_blob_by_sha(root, &blob_sha)?;
    serde_yaml_ng::from_str(&content).map_err(io::Error::other)
}

/// Checks `git_ref` out into a detached temporary worktree and looks for a
/// generated TestCase with this exact `case_id` there (mirroring
/// `canonical::import_native`'s worktree pattern), together with its real
/// `LegacySnapshot` — the ground truth for "what did this case's five
/// contributing elements actually look like at this ref," independent of
/// whether `case_uid` was computable there yet. `None` if `case_id`
/// doesn't exist verbatim at that ref (it may have been renamed away, or
/// not created yet). The worktree is only needed to run `generate_testcases`
/// (to learn the case's ids and paths as they were at that ref); the
/// snapshot's `tree_sha` itself is read directly from `git_ref` as
/// *committed* (`git::tree_sha`, no worktree needed for that part).
fn testcase_and_legacy_snapshot_at_ref(
    root: &Path,
    git_ref: &str,
    case_id: &str,
) -> io::Result<Option<(TestCase, LegacySnapshot)>> {
    let temporary = tempfile::tempdir()?;
    let worktree = temporary.path().join("snapshot");
    crate::git::add_detached_worktree(root, &worktree, git_ref)?;
    let result = generate_testcases(
        &worktree
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge"),
    )
    .map(|testcases| testcases.into_iter().find(|tc| tc.case_id == case_id));
    let _ = crate::git::remove_worktree(root, &worktree);
    let Some(testcase) = result? else {
        return Ok(None);
    };
    let Some(tree_sha) =
        crate::git::tree_sha(root, git_ref, crate::project_root::KNOWLEDGE_PATH_IN_REPO)?
    else {
        // `testcase` was just found by generating against this exact ref's
        // knowledge/, so knowledge/ existing there is not in question —
        // this would only happen if the ref moved out from under us mid-call.
        return Ok(None);
    };
    let snapshot = build_legacy_snapshot(tree_sha, &testcase);
    Ok(Some((testcase, snapshot)))
}

/// Like [`testcase_and_legacy_snapshot_at_ref`], for callers that only need
/// the TestCase itself (e.g. to read `case_uid`). Test-only: every
/// production caller needs the `LegacySnapshot` too, so only tests
/// (verifying `case_uid` independent of manifest resolution) use this.
#[cfg(test)]
fn testcase_at_ref(root: &Path, git_ref: &str, case_id: &str) -> io::Result<Option<TestCase>> {
    Ok(testcase_and_legacy_snapshot_at_ref(root, git_ref, case_id)?.map(|(tc, _)| tc))
}

/// Why `resolve_case_uid_across_refs` could not identify `case_id` as the
/// same logical TestCase at both `from_ref` and `to_ref` (design doc §12
/// "残っている論点": a migration-boundary comparison must never proceed
/// on a guess).
#[derive(Debug)]
pub enum CrossBoundaryError {
    /// `case_id` doesn't exist verbatim at `to_ref`, and no manifest entry
    /// at either ref resolves it to a `case_uid` — there is nothing to
    /// bridge the boundary with.
    NoResolution,
    /// The two refs' manifests disagree: `case_id` resolves to a
    /// different `case_uid` depending on which side is consulted. This
    /// should never happen from ordinary use (a `case_uid` is a pure
    /// function of five `uid`s that never change once issued — see
    /// `record_new_case_uids`), so treat it as corruption rather than
    /// picking a side.
    Ambiguous {
        from_case_uid: String,
        to_case_uid: String,
    },
    /// One ref's own manifest already has more than one candidate
    /// `case_uid` for `case_id` (e.g. an ExpectedResult was added or
    /// removed under an unchanged `case_id` sometime in that ref's
    /// history) — `resolve_case_uid_across_refs` cannot disambiguate on
    /// `case_id` alone, so it refuses rather than guessing.
    AmbiguousWithinRef {
        git_ref: String,
        candidates: AmbiguousCaseId,
    },
    Io(io::Error),
}

impl From<io::Error> for CrossBoundaryError {
    fn from(e: io::Error) -> Self {
        CrossBoundaryError::Io(e)
    }
}

/// Resolves `case_id`'s `case_uid` as of `git_ref` — **always** through
/// `git_ref`'s own committed manifest, never by trusting a directly
/// recomputed `case_uid` on its own, even when this exact ref's Knowledge
/// tree already makes one computable (`generate::compute_case_uid`). A
/// deterministic computation is not the same thing as a *corroborated*
/// one: nothing about recomputing a value this ref's own content happens
/// to yield proves that a manifest ever recorded it, and design doc §12
/// requires every migration-boundary comparison to go through the
/// manifest — trusting an uncorroborated direct computation here
/// previously let `resolve_case_uid_across_refs` succeed with a one-sided
/// `Some` even when *neither* ref's manifest had anything to say about
/// `case_id` at all (the exact defect a review round already fixed once,
/// see `resolve_case_uid_across_refs`'s own doc comment — this must not
/// regress).
///
/// If this ref's own TestCase already has a `case_uid`, a direct
/// recomputation is used only to *corroborate*: this ref's own manifest
/// must contain an entry naming this exact `(case_id, case_uid)` pair
/// (recorded by `record_new_case_uids` when this case was migrated) before
/// it is trusted. Otherwise (the case is not fully migrated at this exact
/// ref, so `case_uid` isn't computable here at all — including when
/// `case_id` doesn't even exist verbatim at this ref, e.g. it was renamed
/// away later but this ref's own manifest still carries the pairing from
/// when it was originally migrated under this id), resolution instead goes
/// through the manifest qualified by this ref's own `LegacySnapshot` when
/// one is available — which, for a ref that predates migration, is exactly
/// the legacy snapshot a manifest entry recorded elsewhere would have
/// captured (see `LegacySnapshot`'s doc comment). Either way, a value this
/// ref's own manifest does not corroborate resolves to `Ok(None)`, never a
/// case-id-only guess (`resolve_case_uid_with_signature`); the
/// case-id-only fallback only applies when this ref has no snapshot to
/// offer at all (`case_id` doesn't exist there verbatim).
fn resolve_case_uid_at_ref(
    root: &Path,
    git_ref: &str,
    case_id: &str,
) -> Result<Option<String>, CrossBoundaryError> {
    let found = testcase_and_legacy_snapshot_at_ref(root, git_ref, case_id)?;
    let manifest = read_from_ref(root, git_ref)?;
    if let Some(case_uid) = found.as_ref().and_then(|(tc, _)| tc.case_uid.clone()) {
        let corroborated = manifest
            .entries
            .iter()
            .any(|entry| entry.legacy_case_id == case_id && entry.case_uid == case_uid);
        return Ok(corroborated.then_some(case_uid));
    }
    let snapshot = found.map(|(_, snapshot)| snapshot);
    resolve_case_uid_with_signature(&manifest, case_id, snapshot.as_ref())
        .map(|hit| hit.map(str::to_string))
        .map_err(|candidates| CrossBoundaryError::AmbiguousWithinRef {
            git_ref: git_ref.to_string(),
            candidates,
        })
}

/// Resolves `case_id` (as it read at `from_ref`) to the `case_uid` that
/// identifies the same logical TestCase at `to_ref`, for comparisons that
/// cross a migration boundary — one ref predating `case_uid` being
/// computable for this case, the other postdating it, or `case_id` itself
/// having changed (e.g. via a rename) between the two refs.
///
/// Resolves each ref independently (`resolve_case_uid_at_ref`) — always
/// through that ref's own committed manifest, whether to corroborate a
/// directly-computable `case_uid` or, when one isn't computable there yet,
/// to resolve via the ref's own `LegacySnapshot` — then requires the two
/// refs to agree when both resolve. A pre-migration `from_ref`
/// legitimately resolves to `None` on its own (its own manifest, if any
/// exists yet, has no entry whose legacy snapshot matches its still-
/// pre-migration content — that gap is exactly what the manifest recorded
/// once `to_ref` migrated it exists to bridge), so a single-sided `Some` is
/// accepted; what this function refuses is ever accepting a `case_uid`
/// that no ref's own manifest actually corroborates — including when
/// *neither* ref has a manifest at all, which must resolve to
/// `NoResolution` rather than quietly succeeding off of a directly
/// recomputed value.
pub fn resolve_case_uid_across_refs(
    root: &Path,
    from_ref: &str,
    to_ref: &str,
    case_id: &str,
) -> Result<String, CrossBoundaryError> {
    let from_hit = resolve_case_uid_at_ref(root, from_ref, case_id)?;
    let to_hit = resolve_case_uid_at_ref(root, to_ref, case_id)?;

    match (from_hit, to_hit) {
        (Some(a), Some(b)) if a == b => Ok(a),
        (Some(from_case_uid), Some(to_case_uid)) => Err(CrossBoundaryError::Ambiguous {
            from_case_uid,
            to_case_uid,
        }),
        (Some(uid), None) | (None, Some(uid)) => Ok(uid),
        (None, None) => Err(CrossBoundaryError::NoResolution),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a full req -> feature -> behavior -> condition ->
    /// expected/001.yml tree under `root`, with every element already
    /// carrying a `uid` when `with_uids` is true, or none at all (a
    /// genuinely pre-migration snapshot) when false. The single fixture
    /// builder every test in this module that needs a real Knowledge tree
    /// shares, so a schema change only needs updating here.
    fn write_full_tree(root: &Path, with_uids: bool) {
        let knowledge = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge/req-todo/todo/todo-add-task/todo-add-task-empty-input");
        fs::create_dir_all(knowledge.join("expected")).unwrap();
        let uid_line = |uid: &str| {
            if with_uids {
                format!("uid: {uid}\n")
            } else {
                String::new()
            }
        };
        fs::write(
            root.join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge/req-todo/requirement.yml"),
            format!(
                "id: req-todo\nlabel: req-todo\naxis: []\n{}",
                uid_line("01ARZ3NDEKTSV4RRFFQ69G5FR0")
            ),
        )
        .unwrap();
        fs::write(
            root.join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge/req-todo/todo/feature.yml"),
            format!(
                "id: todo\nrequirement: req-todo\nlabel: todo\naxis: []\n{}",
                uid_line("01ARZ3NDEKTSV4RRFFQ69G5FE0")
            ),
        )
        .unwrap();
        fs::write(
            root.join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge/req-todo/todo/todo-add-task/behavior.yml"),
            format!(
                "id: todo-add-task\nfeature: todo\nlabel: todo-add-task\naxis: []\ndescription: |\n  User adds a task.\npreconditions:\n  - \"Press the add button.\"\n{}",
                uid_line("01ARZ3NDEKTSV4RRFFQ69G5FB0")
            ),
        )
        .unwrap();
        fs::write(
            knowledge.join("condition.yml"),
            format!(
                "id: todo-add-task-empty-input\nbehavior: todo-add-task\nlabel: todo-add-task-empty-input\ndescription: |\n  Title is empty.\nsteps:\n  - \"Do it.\"\nadditional_preconditions: []\n{}",
                uid_line("01ARZ3NDEKTSV4RRFFQ69G5FC0")
            ),
        )
        .unwrap();
        fs::write(
            knowledge.join("expected/001.yml"),
            format!(
                "id: todo-add-task-empty-input-001\ncondition: todo-add-task-empty-input\ndescription: |\n  Shows a validation error.\nresults:\n  - \"Confirmed.\"\n{}",
                uid_line("01ARZ3NDEKTSV4RRFFQ69G5FX0")
            ),
        )
        .unwrap();
    }

    /// A placeholder `LegacySnapshot` distinguished only by `tag`, for tests
    /// that exercise manifest bookkeeping (dedup keys, ambiguity, fallback
    /// behavior) without needing a real Knowledge tree to hash. Two calls
    /// with different tags always compare unequal; the same tag always
    /// compares equal.
    fn fake_snapshot(tag: &str) -> LegacySnapshot {
        let locator = |kind: EntityKind, name: &str| LegacyElementLocator {
            entity_kind: kind,
            legacy_id: format!("id-{tag}-{name}"),
            path: format!("path-{name}"),
        };
        LegacySnapshot {
            tree_sha: format!("tree-sha-{tag}"),
            requirement: locator(EntityKind::Requirement, "requirement"),
            feature: locator(EntityKind::Feature, "feature"),
            behavior: locator(EntityKind::Behavior, "behavior"),
            condition: locator(EntityKind::Condition, "condition"),
            expected: vec![locator(EntityKind::ExpectedResult, "expected-1")],
        }
    }

    #[test]
    fn read_returns_an_empty_manifest_when_no_file_exists() {
        let dir = tempfile::tempdir().unwrap();

        let manifest = read(dir.path()).unwrap();

        assert!(manifest.entries.is_empty());
    }

    #[test]
    fn record_new_case_uids_records_a_case_whose_uid_just_became_computable() {
        let dir = init_repo();
        crate::init::run_init(dir.path()).unwrap();
        write_full_tree(dir.path(), true);

        let recorded =
            record_new_case_uids(dir.path(), &capture_case_signatures(dir.path()).unwrap())
                .unwrap();

        assert_eq!(recorded.len(), 1);
        assert_eq!(
            recorded[0].legacy_case_id,
            "tc-req-todo-todo-todo-add-task-todo-add-task-empty-input"
        );
        let manifest = read(dir.path()).unwrap();
        assert_eq!(manifest.entries, recorded);
    }

    #[test]
    fn record_new_case_uids_is_idempotent() {
        let dir = init_repo();
        crate::init::run_init(dir.path()).unwrap();
        write_full_tree(dir.path(), true);
        record_new_case_uids(dir.path(), &capture_case_signatures(dir.path()).unwrap()).unwrap();

        let second_run =
            record_new_case_uids(dir.path(), &capture_case_signatures(dir.path()).unwrap())
                .unwrap();

        assert!(second_run.is_empty());
        assert_eq!(read(dir.path()).unwrap().entries.len(), 1);
    }

    #[test]
    fn record_new_case_uids_skips_a_case_still_missing_a_uid() {
        let dir = init_repo();
        crate::init::run_init(dir.path()).unwrap();
        let knowledge = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge/req-todo/todo/todo-add-task/todo-add-task-empty-input");
        fs::create_dir_all(knowledge.join("expected")).unwrap();
        fs::write(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge/req-todo/requirement.yml"),
            "id: req-todo\nlabel: req-todo\naxis: []\n",
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge/req-todo/todo/feature.yml"),
            "id: todo\nrequirement: req-todo\nlabel: todo\naxis: []\n",
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge/req-todo/todo/todo-add-task/behavior.yml"),
            "id: todo-add-task\nfeature: todo\nlabel: todo-add-task\naxis: []\ndescription: |\n  User adds a task.\npreconditions:\n  - \"Press the add button.\"\n",
        )
        .unwrap();
        fs::write(
            knowledge.join("condition.yml"),
            "id: todo-add-task-empty-input\nbehavior: todo-add-task\nlabel: todo-add-task-empty-input\ndescription: |\n  Title is empty.\nsteps:\n  - \"Do it.\"\nadditional_preconditions: []\n",
        )
        .unwrap();
        fs::write(
            knowledge.join("expected/001.yml"),
            "id: todo-add-task-empty-input-001\ncondition: todo-add-task-empty-input\ndescription: |\n  Shows a validation error.\nresults:\n  - \"Confirmed.\"\n",
        )
        .unwrap();

        let recorded =
            record_new_case_uids(dir.path(), &capture_case_signatures(dir.path()).unwrap())
                .unwrap();

        assert!(recorded.is_empty());
        assert!(
            !dir.path()
                .join(".markharness/identity-migration-manifest.yml")
                .exists()
        );
    }

    #[test]
    fn resolve_case_uid_finds_a_recorded_entry() {
        let manifest = Manifest {
            entries: vec![ManifestEntry {
                legacy_case_id: "tc-old".to_string(),
                case_uid: "case-uid-1".to_string(),
                legacy_snapshot: fake_snapshot("sig-1"),
                recorded_at: "2026-08-21T00:00:00Z".to_string(),
            }],
        };

        assert_eq!(
            resolve_case_uid(&manifest, "tc-old"),
            Ok(Some("case-uid-1"))
        );
        assert_eq!(resolve_case_uid(&manifest, "tc-unknown"), Ok(None));
    }

    /// The reviewer's Round 4 finding: even when a `case_id` has only a
    /// *single* recorded entry (so a case-id-only lookup would be
    /// unambiguous on its own), a signature that was supplied but doesn't
    /// match that entry must not fall back to it. A single entry is not
    /// proof that the caller's snapshot is the one it was recorded from —
    /// it could be an entirely different, unrelated snapshot that merely
    /// happens to reuse the same `case_id`.
    #[test]
    fn resolve_case_uid_with_signature_refuses_the_sole_entry_when_its_signature_disagrees() {
        let manifest = Manifest {
            entries: vec![ManifestEntry {
                legacy_case_id: "tc-old".to_string(),
                case_uid: "case-uid-1".to_string(),
                legacy_snapshot: fake_snapshot("sig-real"),
                recorded_at: "2026-08-21T00:00:00Z".to_string(),
            }],
        };

        assert_eq!(
            resolve_case_uid_with_signature(&manifest, "tc-old", Some(&fake_snapshot("sig-real"))),
            Ok(Some("case-uid-1")),
            "the matching signature must still resolve"
        );
        assert_eq!(
            resolve_case_uid_with_signature(
                &manifest,
                "tc-old",
                Some(&fake_snapshot("sig-from-elsewhere"))
            ),
            Ok(None),
            "a mismatched signature must never fall back to the sole entry, even though a \
             case-id-only lookup would have been unambiguous"
        );
    }

    /// The reviewer-requested guarantee: two entries recorded for the same
    /// `case_id` at two different points in its history (an ExpectedResult
    /// added between them, so each has its own `legacy_snapshot`)
    /// must each resolve to their own correct `case_uid` when the caller
    /// supplies the matching signature — not just report ambiguity.
    #[test]
    fn resolve_case_uid_with_signature_picks_the_matching_entry_when_the_same_case_id_has_two() {
        let manifest = Manifest {
            entries: vec![
                ManifestEntry {
                    legacy_case_id: "tc-shared".to_string(),
                    case_uid: "uid-before-new-expected-result".to_string(),
                    legacy_snapshot: fake_snapshot("sig-before"),
                    recorded_at: "2026-08-21T00:00:00Z".to_string(),
                },
                ManifestEntry {
                    legacy_case_id: "tc-shared".to_string(),
                    case_uid: "uid-after-new-expected-result".to_string(),
                    legacy_snapshot: fake_snapshot("sig-after"),
                    recorded_at: "2026-08-21T01:00:00Z".to_string(),
                },
            ],
        };

        assert_eq!(
            resolve_case_uid_with_signature(
                &manifest,
                "tc-shared",
                Some(&fake_snapshot("sig-before"))
            ),
            Ok(Some("uid-before-new-expected-result"))
        );
        assert_eq!(
            resolve_case_uid_with_signature(
                &manifest,
                "tc-shared",
                Some(&fake_snapshot("sig-after"))
            ),
            Ok(Some("uid-after-new-expected-result"))
        );
        // No signature at all falls back to the case-id-only check, which
        // is genuinely ambiguous here.
        assert!(resolve_case_uid_with_signature(&manifest, "tc-shared", None).is_err());
        // A signature that was supplied but matches neither entry must
        // *not* fall back to the case-id-only guess: it means the
        // caller's snapshot is demonstrably not either recorded state.
        assert_eq!(
            resolve_case_uid_with_signature(
                &manifest,
                "tc-shared",
                Some(&fake_snapshot("sig-unrelated"))
            ),
            Ok(None)
        );
    }

    /// The bug this test guards against: before the fix, dedup and
    /// resolution were keyed on `legacy_case_id` alone. An ExpectedResult
    /// added under an unchanged `case_id` changes `case_uid` (it depends
    /// on the *set* of ExpectedResult uids) without changing `case_id` —
    /// so a naive "already recorded" check would silently freeze the
    /// manifest on the stale pairing forever, and a naive lookup would
    /// silently return whichever entry happened to be recorded first.
    #[test]
    fn resolve_case_uid_reports_ambiguity_when_the_same_case_id_has_two_case_uids() {
        let manifest = Manifest {
            entries: vec![
                ManifestEntry {
                    legacy_case_id: "tc-shared".to_string(),
                    case_uid: "case-uid-before-new-expected-result".to_string(),
                    legacy_snapshot: fake_snapshot("sig-before"),
                    recorded_at: "2026-08-21T00:00:00Z".to_string(),
                },
                ManifestEntry {
                    legacy_case_id: "tc-shared".to_string(),
                    case_uid: "case-uid-after-new-expected-result".to_string(),
                    legacy_snapshot: fake_snapshot("sig-after"),
                    recorded_at: "2026-08-21T01:00:00Z".to_string(),
                },
            ],
        };

        let result = resolve_case_uid(&manifest, "tc-shared");

        assert_eq!(
            result,
            Err(AmbiguousCaseId {
                legacy_case_id: "tc-shared".to_string(),
                case_uids: vec![
                    "case-uid-after-new-expected-result".to_string(),
                    "case-uid-before-new-expected-result".to_string(),
                ],
            })
        );
    }

    /// The actual scenario that produces the ambiguity above: a second
    /// `identity migrate` run after an ExpectedResult was added under an
    /// unchanged `case_id` must add a *second* manifest entry for that
    /// `case_id`, not silently skip it because the `case_id` alone was
    /// already "recorded".
    #[test]
    fn record_new_case_uids_records_a_new_pairing_when_case_uid_changes_under_the_same_case_id() {
        let dir = init_repo();
        crate::init::run_init(dir.path()).unwrap();
        write_full_tree(dir.path(), true);
        let first_run =
            record_new_case_uids(dir.path(), &capture_case_signatures(dir.path()).unwrap())
                .unwrap();
        assert_eq!(first_run.len(), 1);
        let first_case_uid = first_run[0].case_uid.clone();

        // Add a second ExpectedResult under the same Condition: case_id
        // stays the same, but case_uid must change (it depends on the set
        // of ExpectedResult uids).
        let expected_dir = dir.path().join(
            ".markharness/knowledge/req-todo/todo/todo-add-task/todo-add-task-empty-input/expected",
        );
        fs::write(
            expected_dir.join("002.yml"),
            "id: todo-add-task-empty-input-002\ncondition: todo-add-task-empty-input\ndescription: |\n  Also shows a hint.\nresults:\n  - \"Confirmed.\"\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FX1\n",
        )
        .unwrap();

        let second_run =
            record_new_case_uids(dir.path(), &capture_case_signatures(dir.path()).unwrap())
                .unwrap();

        assert_eq!(
            second_run.len(),
            1,
            "expected a new pairing to be recorded, not skipped"
        );
        assert_ne!(second_run[0].case_uid, first_case_uid);
        assert_eq!(second_run[0].legacy_case_id, first_run[0].legacy_case_id);

        let manifest = read(dir.path()).unwrap();
        assert_eq!(manifest.entries.len(), 2);
        assert_ne!(
            first_run[0].legacy_snapshot, second_run[0].legacy_snapshot,
            "the two pairings must carry distinct snapshot-qualifying signatures"
        );
        assert_eq!(
            resolve_case_uid(&manifest, &first_run[0].legacy_case_id),
            Err(AmbiguousCaseId {
                legacy_case_id: first_run[0].legacy_case_id.clone(),
                case_uids: {
                    let mut uids = vec![first_case_uid.clone(), second_run[0].case_uid.clone()];
                    uids.sort();
                    uids
                },
            })
        );
        // With the right signature, each of the two points in this case's
        // history resolves unambiguously to its own correct case_uid.
        assert_eq!(
            resolve_case_uid_with_signature(
                &manifest,
                &first_run[0].legacy_case_id,
                Some(&first_run[0].legacy_snapshot)
            ),
            Ok(Some(first_case_uid.as_str()))
        );
        assert_eq!(
            resolve_case_uid_with_signature(
                &manifest,
                &second_run[0].legacy_case_id,
                Some(&second_run[0].legacy_snapshot)
            ),
            Ok(Some(second_run[0].case_uid.as_str()))
        );
    }

    /// The scenario the ID-only signature (rejected during review) got
    /// wrong: an ExpectedResult is deleted and a *different* one is
    /// created reusing the same `id`. `case_id` is unaffected (it doesn't
    /// depend on ExpectedResult ids at all) and the ExpectedResult *id
    /// set* is unchanged too (still just `{"...-001"}`) — an id-only
    /// signature would have judged these two states identical and
    /// resolved them to whichever `case_uid` happened to be recorded
    /// first, even though the real ExpectedResult (different description,
    /// different `uid`, since it went through its own separate `identity
    /// migrate`) is not the same. The content signature must tell them
    /// apart.
    #[test]
    fn legacy_snapshot_detects_an_expected_result_reusing_the_same_id_with_different_content() {
        let dir = init_repo();
        crate::init::run_init(dir.path()).unwrap();
        write_full_tree(dir.path(), true);
        let first_run =
            record_new_case_uids(dir.path(), &capture_case_signatures(dir.path()).unwrap())
                .unwrap();
        assert_eq!(first_run.len(), 1);

        // "Delete" the original ExpectedResult and "recreate" one reusing
        // the exact same id, but with different content (and, since it is
        // a distinct real-world entity, its own fresh uid once migrated).
        let expected_path = dir.path().join(
            ".markharness/knowledge/req-todo/todo/todo-add-task/todo-add-task-empty-input/expected/001.yml",
        );
        fs::write(
            &expected_path,
            "id: todo-add-task-empty-input-001\ncondition: todo-add-task-empty-input\ndescription: |\n  A completely different scenario now.\nresults:\n  - \"Confirmed.\"\n",
        )
        .unwrap();
        // `identity migrate` (`migrate_entities`) assigns the fresh uid
        // *and* calls `record_new_case_uids` itself as its last step (see
        // `feature_ops::migrate_all`) — so the new pairing is recorded
        // here, not by a separate explicit call.
        crate::identity::migrate_entities(dir.path()).unwrap();

        let manifest = read(dir.path()).unwrap();

        assert_eq!(
            manifest.entries.len(),
            2,
            "the id-reusing replacement must be recorded as a new pairing, not treated as \
             identical to the original just because the id set is unchanged: {manifest:?}"
        );
        let second_entry = manifest
            .entries
            .iter()
            .find(|entry| entry.legacy_snapshot != first_run[0].legacy_snapshot)
            .expect("a second, distinct signature must have been recorded");
        assert_ne!(
            first_run[0].legacy_snapshot, second_entry.legacy_snapshot,
            "different content behind the same ExpectedResult id must produce a different signature"
        );
        assert_ne!(first_run[0].case_uid, second_entry.case_uid);
    }

    /// The reviewer's Round 3 finding: reissuing an *ancestor* element
    /// (Requirement, Feature, or Behavior) — not Condition or
    /// ExpectedResult — under the same `id` and the same content, but a
    /// fresh `uid`, must also be recorded as a new pairing. Before this
    /// fix, the signature only covered `condition.yml` and
    /// `expected/*.yml`, so this exact scenario left the signature
    /// unchanged while `case_uid` changed, and `record_new_case_uids`
    /// silently skipped the new mapping as "already recorded."
    #[test]
    fn legacy_snapshot_detects_a_requirement_reissued_with_the_same_id_and_content() {
        let dir = init_repo();
        crate::init::run_init(dir.path()).unwrap();
        write_full_tree(dir.path(), true);
        let first_run =
            record_new_case_uids(dir.path(), &capture_case_signatures(dir.path()).unwrap())
                .unwrap();
        assert_eq!(first_run.len(), 1);

        // "Retire" the original Requirement and "reissue" one reusing the
        // exact same id and content, but without a uid — as if it were
        // released and recreated as a distinct real-world entity, which
        // will get its own fresh uid on the next migrate.
        let requirement_path = dir
            .path()
            .join(".markharness/knowledge/req-todo/requirement.yml");
        fs::write(
            &requirement_path,
            "id: req-todo\nlabel: req-todo\naxis: []\n",
        )
        .unwrap();
        crate::identity::migrate_entities(dir.path()).unwrap();

        let manifest = read(dir.path()).unwrap();

        assert_eq!(
            manifest.entries.len(),
            2,
            "reissuing an ancestor element (same id, same content, fresh uid) must be recorded \
             as a new pairing, not treated as identical to the original just because Condition \
             and ExpectedResult content didn't change: {manifest:?}"
        );
        let second_entry = manifest
            .entries
            .iter()
            .find(|entry| entry.legacy_snapshot != first_run[0].legacy_snapshot)
            .expect("a second, distinct signature must have been recorded");
        assert_ne!(
            first_run[0].legacy_snapshot, second_entry.legacy_snapshot,
            "a reissued Requirement's fresh uid must change the signature even though Condition \
             and ExpectedResult content is untouched"
        );
        assert_ne!(first_run[0].case_uid, second_entry.case_uid);
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

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        run_git(dir.path(), &["config", "core.autocrlf", "false"]);
        dir
    }

    fn commit_all(root: &Path, message: &str) {
        run_git(root, &["add", "-A"]);
        run_git(root, &["commit", "-q", "-m", message]);
    }

    #[test]
    fn read_from_ref_returns_an_empty_manifest_when_the_file_did_not_exist_at_that_ref() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "no manifest yet");

        let manifest = read_from_ref(dir.path(), "HEAD").unwrap();

        assert!(manifest.entries.is_empty());
    }

    #[test]
    fn read_from_ref_reads_the_committed_manifest_content() {
        let dir = init_repo();
        fs::create_dir_all(dir.path().join(".markharness")).unwrap();
        let manifest = Manifest {
            entries: vec![ManifestEntry {
                legacy_case_id: "tc-old".to_string(),
                case_uid: "case-uid-1".to_string(),
                legacy_snapshot: fake_snapshot("sig-1"),
                recorded_at: "2026-08-21T00:00:00Z".to_string(),
            }],
        };
        write(dir.path(), &manifest).unwrap();
        commit_all(dir.path(), "record manifest");

        let read_back = read_from_ref(dir.path(), "HEAD").unwrap();

        assert_eq!(read_back, manifest);
    }

    /// The core migration-boundary scenario: `from_ref` predates the
    /// manifest entirely (this case's `case_uid` wasn't computable yet),
    /// `to_ref` has it. Resolution must still succeed via `to_ref`'s side.
    #[test]
    fn resolve_case_uid_across_refs_resolves_via_the_ref_that_has_the_entry() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "before migration");
        run_git(dir.path(), &["tag", "before"]);

        fs::create_dir_all(dir.path().join(".markharness")).unwrap();
        write(
            dir.path(),
            &Manifest {
                entries: vec![ManifestEntry {
                    legacy_case_id: "tc-old".to_string(),
                    case_uid: "case-uid-1".to_string(),
                    legacy_snapshot: fake_snapshot("sig-1"),
                    recorded_at: "2026-08-21T00:00:00Z".to_string(),
                }],
            },
        )
        .unwrap();
        commit_all(dir.path(), "after migration");
        run_git(dir.path(), &["tag", "after"]);

        let resolved =
            resolve_case_uid_across_refs(dir.path(), "before", "after", "tc-old").unwrap();

        assert_eq!(resolved, "case-uid-1");
    }

    #[test]
    fn resolve_case_uid_across_refs_errors_when_neither_ref_has_a_mapping() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "no manifest anywhere");
        run_git(dir.path(), &["tag", "a"]);
        run_git(dir.path(), &["tag", "b"]);

        let result = resolve_case_uid_across_refs(dir.path(), "a", "b", "tc-unknown");

        assert!(matches!(result, Err(CrossBoundaryError::NoResolution)));
    }

    /// A `case_uid` must never change once recorded — if two refs somehow
    /// disagree about `tc-old`'s `case_uid`, that is corruption, and
    /// resolution must refuse to pick a side rather than silently trusting
    /// either ref.
    #[test]
    fn resolve_case_uid_across_refs_errors_when_the_two_refs_disagree() {
        let dir = init_repo();
        fs::create_dir_all(dir.path().join(".markharness")).unwrap();
        write(
            dir.path(),
            &Manifest {
                entries: vec![ManifestEntry {
                    legacy_case_id: "tc-old".to_string(),
                    case_uid: "case-uid-1".to_string(),
                    legacy_snapshot: fake_snapshot("sig-1"),
                    recorded_at: "2026-08-21T00:00:00Z".to_string(),
                }],
            },
        )
        .unwrap();
        commit_all(dir.path(), "first mapping");
        run_git(dir.path(), &["tag", "a"]);

        write(
            dir.path(),
            &Manifest {
                entries: vec![ManifestEntry {
                    legacy_case_id: "tc-old".to_string(),
                    case_uid: "case-uid-CORRUPTED".to_string(),
                    legacy_snapshot: fake_snapshot("sig-1"),
                    recorded_at: "2026-08-21T01:00:00Z".to_string(),
                }],
            },
        )
        .unwrap();
        commit_all(dir.path(), "corrupted mapping");
        run_git(dir.path(), &["tag", "b"]);

        let result = resolve_case_uid_across_refs(dir.path(), "a", "b", "tc-old");

        assert!(matches!(
            result,
            Err(CrossBoundaryError::Ambiguous { from_case_uid, to_case_uid })
                if from_case_uid == "case-uid-1" && to_case_uid == "case-uid-CORRUPTED"
        ));
    }

    /// The reviewer-requested end-to-end guarantee, against real git
    /// history rather than a hand-built manifest: the same `case_id`,
    /// queried from the same pre-migration ancestor, resolves to a
    /// *different* — and in each case correct — `case_uid` depending on
    /// which of two later points (before vs. after a second ExpectedResult
    /// was added) it is compared against.
    #[test]
    fn resolve_case_uid_across_refs_resolves_each_of_two_case_uids_for_the_same_case_id_correctly()
    {
        let dir = init_repo();
        let knowledge = dir
            .path()
            .join(".markharness/knowledge/req-todo/todo/todo-add-task/todo-add-task-empty-input");
        write_full_tree(dir.path(), false);
        let case_id = "tc-req-todo-todo-todo-add-task-todo-add-task-empty-input";
        commit_all(dir.path(), "pre-migration");
        run_git(dir.path(), &["tag", "v0"]);

        // Migrate: every element gets a uid, case_uid becomes computable.
        crate::identity::migrate_entities(dir.path()).unwrap();
        commit_all(dir.path(), "migrated");
        run_git(dir.path(), &["tag", "v1"]);
        let case_uid_v1 = resolve_case_uid_across_refs(dir.path(), "v0", "v1", case_id).unwrap();

        // A second ExpectedResult, same case_id, different case_uid.
        fs::write(
            knowledge.join("expected/002.yml"),
            "id: todo-add-task-empty-input-002\ncondition: todo-add-task-empty-input\ndescription: |\n  Also shows a hint.\nresults:\n  - \"Confirmed.\"\n",
        )
        .unwrap();
        crate::identity::migrate_entities(dir.path()).unwrap();
        commit_all(dir.path(), "added a second ExpectedResult");
        run_git(dir.path(), &["tag", "v2"]);
        let case_uid_v2 = resolve_case_uid_across_refs(dir.path(), "v0", "v2", case_id).unwrap();

        assert_ne!(
            case_uid_v1, case_uid_v2,
            "adding an ExpectedResult must change case_uid"
        );
        // Each resolution from the same v0 ancestor must independently
        // match what direct computation at v1/v2 says is correct.
        assert_eq!(
            Some(case_uid_v1.as_str()),
            testcase_at_ref(dir.path(), "v1", case_id)
                .unwrap()
                .and_then(|tc| tc.case_uid)
                .as_deref()
        );
        assert_eq!(
            Some(case_uid_v2.as_str()),
            testcase_at_ref(dir.path(), "v2", case_id)
                .unwrap()
                .and_then(|tc| tc.case_uid)
                .as_deref()
        );
    }

    /// The reviewer's Round 4 finding, end-to-end against real git history:
    /// a pre-migration ref whose manifest already carries a single entry
    /// for this exact `case_id` — but from an unrelated snapshot, so its
    /// `legacy_snapshot` does not match this ref's actual
    /// Condition/ExpectedResult content — must not resolve to that stale
    /// entry's `case_uid` just because it is the only candidate. It must
    /// resolve `None` on its own, leaving `to_ref`'s own directly-computed
    /// `case_uid` (once genuinely migrated) as the only trustworthy answer.
    #[test]
    fn resolve_case_uid_across_refs_refuses_a_stale_manifest_entry_whose_content_disagrees() {
        let dir = init_repo();
        write_full_tree(dir.path(), false);
        let case_id = "tc-req-todo-todo-todo-add-task-todo-add-task-empty-input";
        fs::create_dir_all(dir.path().join(".markharness")).unwrap();
        write(
            dir.path(),
            &Manifest {
                entries: vec![ManifestEntry {
                    legacy_case_id: case_id.to_string(),
                    case_uid: "case-uid-from-an-unrelated-snapshot".to_string(),
                    legacy_snapshot: fake_snapshot("not-this-refs-real-content"),
                    recorded_at: "2026-08-21T00:00:00Z".to_string(),
                }],
            },
        )
        .unwrap();
        commit_all(
            dir.path(),
            "pre-migration, with an unrelated stale manifest entry",
        );
        run_git(dir.path(), &["tag", "v0"]);

        crate::identity::migrate_entities(dir.path()).unwrap();
        commit_all(dir.path(), "migrated");
        run_git(dir.path(), &["tag", "v1"]);

        let resolved = resolve_case_uid_across_refs(dir.path(), "v0", "v1", case_id)
            .expect("v1's own directly-computed case_uid must still resolve");

        assert_ne!(
            resolved, "case-uid-from-an-unrelated-snapshot",
            "the stale, content-mismatched manifest entry must never be trusted just because \
             it was the only candidate recorded under this case_id"
        );
        assert_eq!(
            Some(resolved.as_str()),
            testcase_at_ref(dir.path(), "v1", case_id)
                .unwrap()
                .and_then(|tc| tc.case_uid)
                .as_deref()
        );
    }

    /// The reviewer's Round 7 "reappeared" finding: `to_ref`'s own content
    /// already yields a directly-computable `case_uid` (every element
    /// carries a `uid`), but *no manifest was ever written at that ref at
    /// all* (unlike ordinary use, where `identity migrate` always records
    /// one alongside — this simulates content that reached a migrated
    /// state some other way, e.g. copied in from elsewhere). Before this
    /// fix, `resolve_case_uid_at_ref` trusted the direct computation
    /// unconditionally, so this would have resolved successfully even
    /// though *neither* ref's manifest ever corroborated `case_id` at all
    /// — exactly the defect a review round already fixed once (see
    /// `resolve_case_uid_at_ref`'s own doc comment). It must now refuse.
    #[test]
    fn resolve_case_uid_across_refs_refuses_a_post_migration_ref_with_no_manifest_at_all() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "before: case does not exist yet");
        run_git(dir.path(), &["tag", "before"]);

        // Content that already carries every uid appears — but no
        // `identity migrate` ever ran, so no manifest entry was ever
        // written to corroborate it.
        write_full_tree(dir.path(), true);
        let case_id = "tc-req-todo-todo-todo-add-task-todo-add-task-empty-input";
        commit_all(
            dir.path(),
            "after: fully migrated content, but no manifest at all",
        );
        run_git(dir.path(), &["tag", "after"]);

        let result = resolve_case_uid_across_refs(dir.path(), "before", "after", case_id);

        assert!(
            matches!(result, Err(CrossBoundaryError::NoResolution)),
            "a directly-computable case_uid with zero manifest corroboration on either ref must \
             refuse to resolve, got: {result:?}"
        );
    }
}
