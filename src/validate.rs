use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;

use crate::generate::{find_dirs_with_marker, sorted_subdirs};
use crate::knowledge;
use crate::schema;

/// One problem found under `knowledge/` or `axes/`: the offending file's
/// path (relative to the project root) and a human-readable message. Covers
/// both JSON Schema structural violations (§3.5 `schema/`) and the
/// cross-reference checks JSON Schema alone can't express — axis tags must
/// exist in the `axes/` registry, and `forked_from` must name an existing
/// Feature (§3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub path: String,
    pub message: String,
}

impl std::fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn validate_file(
    root: &Path,
    schema_file: &str,
    file_path: &Path,
    issues: &mut Vec<ValidationIssue>,
) -> io::Result<Option<String>> {
    let content = fs::read_to_string(file_path)?;
    let schema_doc = schema::load_schema(root, schema_file)?;
    match schema::validate_yaml(&schema_doc, &content) {
        Ok(()) => Ok(Some(content)),
        Err(errors) => {
            for message in errors {
                issues.push(ValidationIssue {
                    path: rel(root, file_path),
                    message,
                });
            }
            Ok(None)
        }
    }
}

/// Collects every Feature id under `knowledge/` (best-effort: unparsable
/// `feature.yml`s are silently skipped here, since they are reported as
/// schema issues by the main `validate_all` pass instead), so `forked_from`
/// references can be checked against a complete set regardless of walk order.
fn collect_feature_ids(knowledge_root: &Path) -> io::Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    for requirement_dir in sorted_subdirs(knowledge_root)? {
        for feature_dir in sorted_subdirs(&requirement_dir)? {
            let feature_path = feature_dir.join("feature.yml");
            if !feature_path.is_file() {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&feature_path)
                && let Ok(feature) = knowledge::parse_feature(&content)
            {
                ids.insert(feature.id);
            }
        }
    }
    Ok(ids)
}

fn check_axis_tags(
    root: &Path,
    file_path: &Path,
    axis: &[String],
    known_axes: &BTreeSet<String>,
) -> Vec<ValidationIssue> {
    axis.iter()
        .filter(|tag| !known_axes.contains(*tag))
        .map(|tag| ValidationIssue {
            path: rel(root, file_path),
            message: format!("axis '{tag}' is not registered under .markharness/axes/"),
        })
        .collect()
}

/// Validates every `executions/<milestone>/results.yml` against
/// `execution_result.schema.json` (§3.1 TESTEXECUTION, records appended by
/// `markharness execution record`). Records written before
/// `verified_feature_tree_shas` existed remain valid since the field is
/// optional in the schema (change-event-verification-tracking-spec.md §6:
/// no retroactive backfill, treated as "unknown" by `verify trace`/`verify
/// pending` rather than rejected here).
fn validate_executions(root: &Path, issues: &mut Vec<ValidationIssue>) -> io::Result<()> {
    let executions_dir = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("executions");
    if !executions_dir.is_dir() {
        return Ok(());
    }
    for milestone_dir in sorted_subdirs(&executions_dir)? {
        let results_path = milestone_dir.join("results.yml");
        if results_path.is_file() {
            validate_file(root, "execution_result.schema.json", &results_path, issues)?;
        }
    }
    Ok(())
}

/// Validates every `knowledge/` YAML file against its `schema/*.schema.json`
/// (§3.5 structural validation) and, for files that pass structurally,
/// cross-reference rules that JSON Schema alone can't express: `axis` tags
/// must exist in the `axes/` registry, and `forked_from` must name an
/// existing Feature id (§3.1). Also validates `axes/*.yml` themselves, and
/// `executions/*/results.yml` against `execution_result.schema.json`.
/// Returns every issue found; an empty result means the tree is valid.
pub fn validate_all(root: &Path) -> io::Result<Vec<ValidationIssue>> {
    let mut issues = Vec::new();
    let known_axes: BTreeSet<String> = crate::axes::list_axes(root)
        .into_iter()
        .map(|a| a.id)
        .collect();
    let known_feature_ids = collect_feature_ids(
        &root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge"),
    )?;

    let axes_dir = root.join(crate::project_root::MARKHARNESS_DIR).join("axes");
    if axes_dir.is_dir() {
        let mut axis_files: Vec<_> = fs::read_dir(&axes_dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("yml"))
            .collect();
        axis_files.sort();
        for axis_path in axis_files {
            validate_file(root, "axis.schema.json", &axis_path, &mut issues)?;
        }
    }

    let knowledge_root = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("knowledge");
    for requirement_dir in sorted_subdirs(&knowledge_root)? {
        let requirement_path = requirement_dir.join("requirement.yml");
        if !requirement_path.is_file() {
            continue;
        }
        if let Some(content) = validate_file(
            root,
            "requirement.schema.json",
            &requirement_path,
            &mut issues,
        )? && let Ok(requirement) = knowledge::parse_requirement(&content)
        {
            issues.extend(check_axis_tags(
                root,
                &requirement_path,
                &requirement.axis,
                &known_axes,
            ));
        }

        for feature_dir in sorted_subdirs(&requirement_dir)? {
            let feature_path = feature_dir.join("feature.yml");
            if !feature_path.is_file() {
                continue;
            }
            if let Some(content) =
                validate_file(root, "feature.schema.json", &feature_path, &mut issues)?
                && let Ok(feature) = knowledge::parse_feature(&content)
            {
                issues.extend(check_axis_tags(
                    root,
                    &feature_path,
                    &feature.axis,
                    &known_axes,
                ));
                if let Some(forked_from) = &feature.forked_from
                    && !known_feature_ids.contains(forked_from)
                {
                    issues.push(ValidationIssue {
                        path: rel(root, &feature_path),
                        message: format!(
                            "forked_from '{forked_from}' does not match any known Feature id"
                        ),
                    });
                }
            }

            for behavior_dir in find_dirs_with_marker(&feature_dir, "behavior.yml")? {
                let behavior_path = behavior_dir.join("behavior.yml");
                if let Some(content) =
                    validate_file(root, "behavior.schema.json", &behavior_path, &mut issues)?
                    && let Ok(behavior) = knowledge::parse_behavior(&content)
                {
                    issues.extend(check_axis_tags(
                        root,
                        &behavior_path,
                        &behavior.axis,
                        &known_axes,
                    ));
                }

                for condition_dir in find_dirs_with_marker(&behavior_dir, "condition.yml")? {
                    let condition_path = condition_dir.join("condition.yml");
                    validate_file(root, "condition.schema.json", &condition_path, &mut issues)?;

                    let expected_dir = condition_dir.join("expected");
                    if !expected_dir.is_dir() {
                        continue;
                    }
                    let mut expected_paths: Vec<_> = fs::read_dir(&expected_dir)?
                        .filter_map(|e| e.ok())
                        .map(|e| e.path())
                        .filter(|p| p.is_file())
                        .collect();
                    expected_paths.sort();
                    for (i, expected_path) in expected_paths.iter().enumerate() {
                        if let Some(content) = validate_file(
                            root,
                            "expected_result.schema.json",
                            expected_path,
                            &mut issues,
                        )? && let Ok(expected) = knowledge::parse_expected_result(&content)
                            && i > 0
                            && expected
                                .additional_steps
                                .as_ref()
                                .is_none_or(|steps| steps.is_empty())
                        {
                            issues.push(ValidationIssue {
                                path: rel(root, expected_path),
                                message: format!(
                                    "additional_steps must contain at least one operation \
                                     (this file is not the first, by filename order, under \
                                     {}/expected/)",
                                    rel(root, &condition_dir)
                                ),
                            });
                        }
                    }
                }
            }
        }
    }

    validate_executions(root, &mut issues)?;
    validate_uid_mode_invariant(root, &mut issues)?;

    Ok(issues)
}

/// ADR 0013 検証規則: schema version 2 の公開cutover後(`[identity]
/// mode = "uid"`)は、UIDなし要素の新規追加を通常コマンドが拒否する。
/// copy/import/手編集で紛れ込んだuidなし要素をここで検出し、
/// `markharness identity migrate`(明示的なrepair操作)を促す。cutover前
/// のprojectでは`mode`マーカー自体が存在しないため、このチェックは
/// 常にno-op(移行途中のuidなし要素を誤検出しない)。
fn validate_uid_mode_invariant(root: &Path, issues: &mut Vec<ValidationIssue>) -> io::Result<()> {
    if !crate::identity::is_uid_mode(root)? {
        return Ok(());
    }
    for kind in crate::identity::EntityKind::ALL {
        for entity in crate::identity::knowledge_walk::list_entities(root, kind)? {
            if entity.uid.is_none() {
                issues.push(ValidationIssue {
                    path: rel(root, &entity.path),
                    message: format!(
                        "project is in UID mode ([identity] mode = \"uid\") but this {} '{}' has no uid; run `markharness identity migrate` to repair",
                        kind.as_str(),
                        entity.id
                    ),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_project(dir: &Path) {
        crate::init::run_init(dir).unwrap();
    }

    fn write_valid_tree(root: &Path) {
        fs::create_dir_all(root.join(crate::project_root::MARKHARNESS_DIR).join("axes")).unwrap();
        fs::write(
            root.join(".markharness/axes/gameplay.yml"),
            "id: gameplay\nlabel: Gameplay\n",
        )
        .unwrap();

        let base = root.join(".markharness/knowledge/controls/player-jump/jump/ground");
        fs::create_dir_all(&base).unwrap();
        fs::write(
            root.join(".markharness/knowledge/controls/requirement.yml"),
            "id: controls\nlabel: controls\naxis: [gameplay]\n",
        )
        .unwrap();
        fs::write(
            root.join(".markharness/knowledge/controls/player-jump/feature.yml"),
            "id: player-jump\nrequirement: controls\nlabel: player-jump\naxis: [gameplay]\n",
        )
        .unwrap();
        fs::write(
            root.join(".markharness/knowledge/controls/player-jump/jump/behavior.yml"),
            "id: jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\npreconditions:\n  - \"Press the jump button.\"\n",
        )
        .unwrap();
        fs::write(
            base.join("condition.yml"),
            "id: ground\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from the ground.\nsteps:\n  - \"Do it.\"\nadditional_preconditions: []\n",
        )
        .unwrap();
        fs::create_dir_all(base.join("expected")).unwrap();
        fs::write(
            base.join("expected/001.yml"),
            "id: ground-001\ncondition: ground\ndescription: |\n  lands safely\nresults:\n  - \"Confirmed.\"\n",
        )
        .unwrap();
    }

    #[test]
    fn a_fully_valid_tree_has_no_issues() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());

        let issues = validate_all(dir.path()).unwrap();

        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
    }

    /// ADR 0016 §1: Condition内でファイル名順が先頭の`expected_result`は
    /// `additional_steps`を省略してよい。
    #[test]
    fn accepts_a_lone_expected_result_without_additional_steps() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());

        let issues = validate_all(dir.path()).unwrap();

        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
    }

    /// ADR 0016 §1: "先頭のexpected_resultのみ省略可、または空でよい" — an
    /// explicit `additional_steps: []` on the first (by filename)
    /// expected_result is exactly as valid as omitting the field, both at
    /// the schema level (no `minItems` on `additional_steps`) and at
    /// `validate.rs`'s cross-reference check (which only requires non-empty
    /// content starting from the second file).
    #[test]
    fn accepts_a_lone_expected_result_with_an_explicitly_empty_additional_steps_array() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());
        let base = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/jump/ground");
        fs::write(
            base.join("expected/001.yml"),
            "id: ground-001\ncondition: ground\ndescription: |\n  lands safely\nadditional_steps: []\nresults:\n  - \"Confirmed.\"\n",
        )
        .unwrap();

        let issues = validate_all(dir.path()).unwrap();

        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
    }

    /// ADR 0016 §1: 2番目以降の`expected_result`で`additional_steps`が
    /// 省略(または空配列)の場合はクロスリファレンスエラーとなる。
    #[test]
    fn reports_a_second_expected_result_missing_additional_steps() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());
        let base = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/jump/ground");
        fs::write(
            base.join("expected/002.yml"),
            "id: ground-002\ncondition: ground\ndescription: |\n  falls over\nresults:\n  - \"Confirmed.\"\n",
        )
        .unwrap();

        let issues = validate_all(dir.path()).unwrap();

        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains("additional_steps")
                    && issue.path.ends_with("expected/002.yml")),
            "expected an additional_steps issue for expected/002.yml, got: {issues:?}"
        );
    }

    /// ADR 0016 §1: 2番目以降の`expected_result`に`additional_steps`が
    /// 1操作以上あれば成功する。
    #[test]
    fn accepts_a_second_expected_result_with_additional_steps() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());
        let base = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/jump/ground");
        fs::write(
            base.join("expected/002.yml"),
            "id: ground-002\ncondition: ground\ndescription: |\n  still on the ground after reload\nadditional_steps:\n  - \"Reload the page.\"\nresults:\n  - \"Confirmed.\"\n",
        )
        .unwrap();

        let issues = validate_all(dir.path()).unwrap();

        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
    }

    /// ADR 0013 検証規則: cutover前(markerなし)のprojectでは、uidなし
    /// 要素があってもUID mode違反として報告してはならない(移行途中の
    /// 通常状態のため)。
    #[test]
    fn does_not_flag_uid_less_elements_before_the_uid_mode_cutover() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());

        let issues = validate_all(dir.path()).unwrap();

        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
    }

    /// cutover後(`[identity] mode = "uid"`)は、uidを持たない要素の
    /// 存在自体がvalidation issueとして報告されなければならない。
    #[test]
    fn flags_a_uid_less_feature_once_the_project_is_in_uid_mode() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());
        crate::identity::marker::mark_uid_mode(dir.path()).unwrap();

        let issues = validate_all(dir.path()).unwrap();

        assert!(
            issues.iter().any(|i| i.path.contains("feature.yml")
                && i.message.contains("no uid")
                && i.message.contains("identity migrate")),
            "expected a UID-mode violation for the uid-less feature, got: {issues:?}"
        );
    }

    /// Once every element actually has a `uid`, being in UID mode must not
    /// itself produce spurious issues.
    #[test]
    fn does_not_flag_anything_when_every_element_has_a_uid_in_uid_mode() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());
        let git_status = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .status()
                .unwrap()
        };
        assert!(git_status(&["init", "-q"]).success());
        assert!(git_status(&["config", "user.email", "test@example.com"]).success());
        assert!(git_status(&["config", "user.name", "Test"]).success());
        assert!(git_status(&["config", "core.autocrlf", "false"]).success());
        crate::identity::migrate_entities(dir.path()).unwrap();

        let issues = validate_all(dir.path()).unwrap();

        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
    }

    #[test]
    fn reports_a_schema_violation_when_condition_id_is_not_a_valid_slug() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());
        fs::write(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/jump/ground/condition.yml"),
            "id: ../../../../evil\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from the ground.\nsteps:\n  - \"Do it.\"\nadditional_preconditions: []\n",
        )
        .unwrap();

        let issues = validate_all(dir.path()).unwrap();

        assert!(
            issues
                .iter()
                .any(|i| i.path.contains("condition.yml") && i.message.contains("does not match")),
            "expected a pattern-violation issue for condition.yml, got: {issues:?}"
        );
    }

    #[test]
    fn reports_a_schema_violation_when_a_required_field_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());
        fs::write(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
            "id: player-jump\nlabel: player-jump\naxis: [gameplay]\n",
        )
        .unwrap();

        let issues = validate_all(dir.path()).unwrap();

        assert!(
            issues
                .iter()
                .any(|i| i.path.contains("feature.yml") && i.message.contains("requirement")),
            "expected a missing-requirement issue, got: {issues:?}"
        );
    }

    #[test]
    fn reports_an_unregistered_axis_tag() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());
        fs::write(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/feature.yml"),
            "id: player-jump\nrequirement: controls\nlabel: player-jump\naxis: [not-registered]\n",
        )
        .unwrap();

        let issues = validate_all(dir.path()).unwrap();

        assert!(
            issues.iter().any(|i| i.message.contains("not-registered")),
            "expected an unregistered-axis issue, got: {issues:?}"
        );
    }

    #[test]
    fn reports_a_forked_from_pointing_at_an_unknown_feature() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());
        fs::write(
            dir.path().join(".markharness/knowledge/controls/player-jump/feature.yml"),
            "id: player-jump\nrequirement: controls\nlabel: player-jump\naxis: [gameplay]\nforked_from: no-such-feature\n",
        )
        .unwrap();

        let issues = validate_all(dir.path()).unwrap();

        assert!(
            issues.iter().any(|i| i.message.contains("forked_from")),
            "expected a forked_from issue, got: {issues:?}"
        );
    }

    #[test]
    fn accepts_a_valid_results_yml_including_verified_feature_tree_shas() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());
        fs::create_dir_all(dir.path().join(".markharness/executions/m1")).unwrap();
        fs::write(
            dir.path().join(".markharness/executions/m1/results.yml"),
            "- case_id: tc-ground-001\n  result: pass\n  executor: yamada\n  executed_at: 2026-08-08T03:15:00Z\n  verified_feature_tree_shas:\n    player-jump: 1a2b3c\n",
        )
        .unwrap();

        let issues = validate_all(dir.path()).unwrap();

        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
    }

    #[test]
    fn accepts_a_pre_existing_results_yml_without_verified_feature_tree_shas() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());
        fs::create_dir_all(dir.path().join(".markharness/executions/m1")).unwrap();
        fs::write(
            dir.path().join(".markharness/executions/m1/results.yml"),
            "- case_id: tc-ground-001\n  result: pass\n  executor: yamada\n  executed_at: 2026-08-08T03:15:00Z\n",
        )
        .unwrap();

        let issues = validate_all(dir.path()).unwrap();

        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
    }

    #[test]
    fn reports_an_invalid_result_value_in_results_yml() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());
        fs::create_dir_all(dir.path().join(".markharness/executions/m1")).unwrap();
        fs::write(
            dir.path().join(".markharness/executions/m1/results.yml"),
            "- case_id: tc-ground-001\n  result: bogus\n  executor: yamada\n  executed_at: 2026-08-08T03:15:00Z\n",
        )
        .unwrap();

        let issues = validate_all(dir.path()).unwrap();

        assert!(
            issues
                .iter()
                .any(|i| i.path.contains("results.yml") && i.message.contains("bogus")),
            "expected a results.yml schema issue, got: {issues:?}"
        );
    }

    #[test]
    fn accepts_a_forked_from_pointing_at_a_known_feature() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());
        fs::create_dir_all(
            dir.path()
                .join(".markharness/knowledge/controls/player-double-jump"),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/controls/player-double-jump/feature.yml"),
            "id: player-double-jump\nrequirement: controls\nlabel: player-double-jump\naxis: [gameplay]\nforked_from: player-jump\n",
        )
        .unwrap();

        let issues = validate_all(dir.path()).unwrap();

        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
    }
}
