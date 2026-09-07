use std::io;
use std::path::{Component, Path, PathBuf};

use crate::canonical;
use crate::case_definition;
use crate::changes::{self, ChangeOptions};
use crate::fs_safety::{copy_unmanaged_siblings_no_follow, replace_dir_from_staging, replace_file};
use crate::generate;
use crate::identity::{CaseRevision, CaseUid, Environment, TargetRevision};
use crate::plan::{self, BoundVersions, CaseVersion, PlanEvidence, PlanInput};
use crate::presentation::CommandOutcome;
use crate::traceability;

pub fn import_native(root: &Path, git_ref: &str) -> io::Result<CommandOutcome> {
    Ok(CommandOutcome::CanonicalImported(canonical::import_native(
        root, git_ref,
    )?))
}

pub fn import_junit(
    xml: &str,
    source_locator: &str,
    bound_versions: std::collections::BTreeMap<String, String>,
) -> io::Result<CommandOutcome> {
    Ok(CommandOutcome::CanonicalImported(canonical::import_junit(
        xml,
        source_locator,
        bound_versions,
    )?))
}

pub fn build_verification_plan(
    root: &Path,
    base: &str,
    head: &str,
    environment: Option<&str>,
    canonical_inputs: &[canonical::CanonicalSnapshot],
) -> io::Result<CommandOutcome> {
    Ok(CommandOutcome::PlanBuilt(build_verification_plan_value(
        root,
        base,
        head,
        environment,
        canonical_inputs,
    )?))
}

pub fn build_verification_plan_value(
    root: &Path,
    base: &str,
    head: &str,
    environment: Option<&str>,
    canonical_inputs: &[canonical::CanonicalSnapshot],
) -> io::Result<plan::VerificationPlan> {
    let environment = environment
        .map(Environment::new)
        .transpose()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let analyzer = changes::ChangeAnalyzer::new(root);
    let changes = analyzer.compute(
        &changes::CommitRef::commit(base),
        &changes::CommitRef::commit(head),
        ChangeOptions::default(),
    )?;
    // ADR 0017 §5: the plan's `target_revision` requirement is `head`
    // resolved to a concrete commit OID, so a symbolic `head` (a branch or
    // tag name that could move) still matches evidence recorded against the
    // exact commit that was actually tested.
    let target_revision = TargetRevision::new(crate::git::resolve_commit_oid(root, head)?)
        .expect("git::resolve_commit_oid returns a non-blank commit OID on success");

    // ADR 0017 §5: the evidence candidates a plan judges are limited to
    // native execution records (`execution::ExecutionEntry`). Canonical
    // evidence (`CanonicalSnapshot::evidence`, e.g. from `import_junit`) is
    // deliberately never merged in here — it has no `execution_uid` to
    // explicitly associate with the plan's judgement (ADR 0017 §5's "計画
    // には採用する実行結果を明示的に関連付ける"), and mixing it in would let
    // a hand-crafted `bound_versions` blob satisfy the Case UID/revision
    // model without ever having gone through `record_execution`'s
    // guarantees. `canonical_inputs` is still used below for test discovery
    // (`stored_traces`), which is a separate concern from pass/fail
    // evidence.
    let evidence: Vec<PlanEvidence> = crate::execution::read_all_results(root)?
        .into_iter()
        // ADR 0017 §5: a record whose immutable case definition
        // (`case_definition::load_case_definition`) is missing — the
        // case-definitions store was never populated, or the file was
        // deleted after recording — has nothing to audit against and must
        // never count toward a passing plan. A definition that exists but
        // fails to parse (on-disk corruption) is a stronger integrity
        // failure than "inapplicable evidence", so it's surfaced as a hard
        // error via `?` rather than silently dropped.
        .filter_map(|entry| {
            match crate::case_definition::load_case_definition(
                root,
                &entry.case_uid,
                &entry.case_revision,
            ) {
                Ok(Some(_)) => Some(Ok(entry)),
                Ok(None) => None,
                Err(e) => Some(Err(e)),
            }
        })
        .collect::<io::Result<Vec<_>>>()?
        .into_iter()
        .map(|entry| {
            let bound_versions = BoundVersions {
                case_uid: entry.case_uid,
                case_revision: entry.case_revision,
                target_revision: entry.target_revision,
                environment: entry.environment,
            };
            PlanEvidence {
                test_id: entry.case_id,
                result: match entry.result.as_str() {
                    "pass" => canonical::EvidenceResult::Pass,
                    "fail" => canonical::EvidenceResult::Fail,
                    _ => canonical::EvidenceResult::Skip,
                },
                executed_at: Some(entry.executed_at),
                execution_uid: Some(entry.execution_uid),
                bound_versions,
            }
        })
        .collect();

    let native = canonical::import_native(root, head)?;
    let case_versions: std::collections::BTreeMap<String, CaseVersion> = native
        .artifacts
        .iter()
        .filter(|artifact| artifact.kind == canonical::ArtifactKind::TestCase)
        .filter_map(|artifact| {
            // `CanonicalArtifact.uid`/`canonical_hash` are untyped `String`
            // (design doc `verification-plan-canonical-model-design.md`
            // §7.3: out of scope for type-splitting here), but for a
            // `TestCase`-kind artifact from `import_native` they are always
            // `CaseUid`/`CaseRevision` values stringified a moment earlier
            // in this same call (`canonical::import_native` reads them from
            // `generate::TestCase.case_uid`/`case_revision`, both already
            // typed). `CaseUid`/`CaseRevision::new` re-parsing that string
            // cannot fail (design doc §7.4): `derived_uid::case_uid`/
            // `case_revision` always format a 36-character, non-blank hash
            // string regardless of input.
            let case_uid = CaseUid::new(artifact.uid.clone()?)
                .expect("a TestCase artifact's uid, when present, is always a CaseUid's own string form");
            let case_revision = CaseRevision::new(artifact.version.canonical_hash.clone()?)
                .expect("a TestCase artifact's canonical_hash, when present, is always a CaseRevision's own string form");
            Some((
                artifact.external_id.clone(),
                CaseVersion {
                    case_uid,
                    case_revision,
                },
            ))
        })
        .collect();
    let mut condition_features = std::collections::BTreeMap::new();
    for change in &changes {
        for case_id in &change.impacted_testcases {
            let native_test_id = format!("markharness-native:test_case:{case_id}");
            for relation in native.relations.iter().filter(|relation| {
                relation.from == native_test_id
                    && relation.origin.kind == canonical::RelationOriginKind::Derived
            }) {
                condition_features.insert(relation.to.clone(), change.feature_id.clone());
            }
        }
    }
    let stored_traces = canonical_inputs
        .iter()
        .flat_map(|snapshot| &snapshot.relations)
        .filter(|relation| relation.origin.kind == canonical::RelationOriginKind::Stored)
        .filter_map(|relation| {
            condition_features.get(&relation.to).map(|feature_id| {
                let test_id = relation
                    .from
                    .strip_prefix("junit:test_case:")
                    .map_or_else(|| relation.from.clone(), |id| format!("junit:{id}"));
                plan::StoredTrace {
                    test_id,
                    feature_id: feature_id.clone(),
                }
            })
        })
        .collect();
    Ok(plan::build_plan(PlanInput {
        base: base.to_string(),
        head: head.to_string(),
        changes,
        evidence,
        stored_traces,
        target_revision,
        environment,
        case_versions,
    }))
}

fn safe_testcase_path(testcases_dir: &Path, relative_path: &Path) -> io::Result<PathBuf> {
    if relative_path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "refusing to write testcase outside {}: {}",
                testcases_dir.display(),
                relative_path.display()
            ),
        ));
    }
    Ok(testcases_dir.join(relative_path))
}

pub fn generate_testcases(root: &Path) -> io::Result<CommandOutcome> {
    let snapshot = generate::load_knowledge_snapshot(
        &root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge"),
    )?;
    let testcases = generate::compile_testcases(&snapshot);
    // ADR 0017 §5: freeze each migrated TestCase's effective content under
    // its `(case_uid, case_revision)` key. Independent of the
    // staging/atomic-swap dance below — `generated/` is fully reproducible
    // from `knowledge/` on every run, but `case-definitions/` is meant to
    // accumulate durably across runs, so it's written directly rather than
    // through the disposable staging directory.
    for testcase in &testcases {
        case_definition::store_case_definition(root, testcase)?;
    }
    let generated_dir = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("generated");
    let existing_index = generated_dir.join("traceability-index.json");
    if existing_index.exists() && !existing_index.is_file() {
        return Err(io::Error::other(format!(
            "expected {} to be a file",
            existing_index.display()
        )));
    }

    let staging_parent = tempfile::Builder::new()
        .prefix(".markharness-generate-")
        .tempdir_in(root)?;
    let staging_generated = staging_parent
        .path()
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("generated");
    let testcases_dir = staging_generated.join("testcases");
    std::fs::create_dir_all(&testcases_dir)?;
    for testcase in &testcases {
        let testcase_path = safe_testcase_path(&testcases_dir, &testcase.relative_path())?;
        replace_file(
            staging_parent.path(),
            &testcase_path,
            generate::serialize_testcase(testcase).as_bytes(),
        )?;
    }
    let index = traceability::build_index(&testcases);
    let staged_index = staging_generated.join("traceability-index.json");
    replace_file(
        staging_parent.path(),
        &staged_index,
        traceability::serialize_index(&index).as_bytes(),
    )?;
    // `testcases/` and `traceability-index.json` are generator-owned and
    // fully replaced by the staged content above; everything else already
    // present under `generated/` (e.g. the `.gitkeep` placeholder `init`
    // leaves behind) is not owned by the generator, so it is carried
    // forward into staging before the atomic whole-directory swap below,
    // rather than being discarded by it.
    copy_unmanaged_siblings_no_follow(
        root,
        &generated_dir,
        &staging_generated,
        &["testcases", "traceability-index.json"],
    )?;
    replace_dir_from_staging(root, &staging_generated, &generated_dir)?;

    let mut written: Vec<PathBuf> = testcases
        .iter()
        .map(|testcase| {
            generated_dir
                .join("testcases")
                .join(testcase.relative_path())
        })
        .collect();
    written.push(generated_dir.join("traceability-index.json"));
    Ok(CommandOutcome::Generated {
        count: testcases.len(),
        written,
    })
}

pub fn compute_changes(
    root: &Path,
    from: &str,
    to: &str,
    options: ChangeOptions,
) -> io::Result<CommandOutcome> {
    let outcome = changes::compute_changes_with_warnings(root, from, to, options)?;
    replace_file(
        root,
        &root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("changes")
            .join(format!("{to}.yaml")),
        changes::serialize_changes(&outcome.events).as_bytes(),
    )?;
    Ok(CommandOutcome::ChangesComputed {
        count: outcome.events.len(),
        to: to.to_string(),
        warnings: outcome.warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_testcase_path_joins_a_nested_relative_path() {
        let dir = PathBuf::from("generated/testcases");
        let relative = PathBuf::from("req-todo/todo/todo-add-task/ground.yml");

        assert_eq!(
            safe_testcase_path(&dir, &relative).unwrap(),
            dir.join(relative)
        );
    }

    #[test]
    fn safe_testcase_path_rejects_a_path_that_escapes_the_output_directory() {
        let dir = PathBuf::from("generated/testcases");

        assert!(safe_testcase_path(&dir, Path::new("../../evil.yml")).is_err());
    }
}
