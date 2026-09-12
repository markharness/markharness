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
    for feature_dir in sorted_subdirs(&knowledge_root.join("features"))? {
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
    Ok(ids)
}

/// Collects every migrated Requirement's `uid` under `knowledge/`
/// (best-effort: unparsable `requirement.yml`s are skipped here, reported
/// as schema issues by the main `validate_all` pass instead), so a
/// Feature's `requirement_uids` can be checked against a complete set
/// regardless of walk order.
fn collect_requirement_uids(knowledge_root: &Path) -> io::Result<BTreeSet<String>> {
    let mut uids = BTreeSet::new();
    for requirement_dir in sorted_subdirs(&knowledge_root.join("requirements"))? {
        let requirement_path = requirement_dir.join("requirement.yml");
        if !requirement_path.is_file() {
            continue;
        }
        if let Ok(content) = fs::read_to_string(&requirement_path)
            && let Ok(requirement) = knowledge::parse_requirement(&content)
            && let Some(uid) = requirement.uid
        {
            uids.insert(uid);
        }
    }
    Ok(uids)
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

/// Surfaces malformed `bindings/*.yml` (ADR 0020, ADR 0025). Validation
/// lives in `ExecutionBinding`'s own `deny_unknown_fields` parse rather than
/// a JSON Schema: the point is that an execution-fact field such as `result`
/// must be an error, which a schema check alone would not guarantee for a
/// type this small.
fn validate_bindings(root: &Path, issues: &mut Vec<ValidationIssue>) -> io::Result<()> {
    match crate::binding::read_all(root) {
        Ok(_) => Ok(()),
        Err(crate::binding::BindingError::Io(e)) => Err(e),
        Err(e) => {
            issues.push(ValidationIssue {
                path: crate::binding::bindings_dir(root)
                    .to_string_lossy()
                    .replace('\\', "/"),
                message: e.to_string(),
            });
            Ok(())
        }
    }
}

/// Validates every `knowledge/` YAML file against its `schema/*.schema.json`
/// (§3.5 structural validation) and, for files that pass structurally,
/// cross-reference rules that JSON Schema alone can't express: `axis` tags
/// must exist in the `axes/` registry, and `forked_from` must name an
/// existing Feature id (§3.1). Also validates `axes/*.yml` themselves, and
/// surfaces malformed `bindings/*.yml`.
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
    let known_requirement_uids = collect_requirement_uids(
        &root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge"),
    )?;
    // ADR 0013と同じ理由(cutover前は移行途中のuidなし要素を誤検出しない):
    // cutover前はどのRequirementもuidを持たないため、Feature.requirement_uids
    // が解決できないのは移行途中の通常状態であり、violationではない。
    let uid_mode = crate::identity::is_uid_mode(root)?;

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
    for requirement_dir in sorted_subdirs(&knowledge_root.join("requirements"))? {
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
    }

    for feature_dir in sorted_subdirs(&knowledge_root.join("features"))? {
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
            if uid_mode {
                for uid in &feature.requirement_uids {
                    if !known_requirement_uids.contains(uid) {
                        issues.push(ValidationIssue {
                            path: rel(root, &feature_path),
                            message: format!(
                                "requirement_uids references unknown requirement uid '{uid}'"
                            ),
                        });
                    }
                }
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

            for scenario_dir in find_dirs_with_marker(&behavior_dir, "scenario.yml")? {
                let scenario_path = scenario_dir.join("scenario.yml");
                validate_file(root, "scenario.schema.json", &scenario_path, &mut issues)?;
            }
        }
    }

    validate_bindings(root, &mut issues)?;
    validate_uid_mode_invariant(root, &mut issues)?;

    Ok(issues)
}

/// ADR 0013 検証規則: UID modeへの公開cutover後(`[identity]
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

        let base = root.join(".markharness/knowledge/features/player-jump/jump/ground");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(root.join(".markharness/knowledge/requirements/controls")).unwrap();
        fs::write(
            root.join(".markharness/knowledge/requirements/controls/requirement.yml"),
            "id: controls\nlabel: controls\naxis: [gameplay]\n",
        )
        .unwrap();
        fs::write(
            root.join(".markharness/knowledge/features/player-jump/feature.yml"),
            "id: player-jump\nrequirement_uids: [controls]\nlabel: player-jump\naxis: [gameplay]\n",
        )
        .unwrap();
        fs::write(
            root.join(".markharness/knowledge/features/player-jump/jump/behavior.yml"),
            "id: jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\nprocedures: {}\n",
        )
        .unwrap();
        fs::write(
            base.join("scenario.yml"),
            "id: ground\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from the ground.\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"lands safely\"\n",
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

    /// ADR 0017 §2: Scenarioは複数Phaseを配列順に持てる。
    #[test]
    fn accepts_a_scenario_with_multiple_phases() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());
        let base = dir
            .path()
            .join(".markharness/knowledge/features/player-jump/jump/ground");
        fs::write(
            base.join("scenario.yml"),
            "id: ground\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from the ground.\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"lands safely\"\n  - steps:\n      - action: \"Reload the page.\"\n    results:\n      - \"still on the ground\"\n",
        )
        .unwrap();

        let issues = validate_all(dir.path()).unwrap();

        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
    }

    /// ADR 0017 §2: 空のPhase配列はschema(`minItems: 1`)で拒否される。
    #[test]
    fn reports_a_scenario_with_an_empty_phases_array() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());
        let base = dir
            .path()
            .join(".markharness/knowledge/features/player-jump/jump/ground");
        fs::write(
            base.join("scenario.yml"),
            "id: ground\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from the ground.\nphases: []\n",
        )
        .unwrap();

        let issues = validate_all(dir.path()).unwrap();

        assert!(
            issues
                .iter()
                .any(|issue| issue.path.ends_with("scenario.yml")),
            "expected a schema issue for an empty phases array, got: {issues:?}"
        );
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
        // `identity migrate` only assigns `uid`s; it does not rewrite an
        // already-existing Feature's `requirement_uids` (ADR 0017 §1・§3 —
        // that conversion is out of scope, see checklist-issue-44). Simulate
        // the follow-up manual fix so this "everything migrated" fixture is
        // actually internally consistent.
        let requirement_path = dir
            .path()
            .join(".markharness/knowledge/requirements/controls/requirement.yml");
        let requirement =
            knowledge::parse_requirement(&fs::read_to_string(&requirement_path).unwrap()).unwrap();
        let feature_path = dir
            .path()
            .join(".markharness/knowledge/features/player-jump/feature.yml");
        let mut feature =
            knowledge::parse_feature(&fs::read_to_string(&feature_path).unwrap()).unwrap();
        feature.requirement_uids = vec![requirement.uid.expect("requirement was just migrated")];
        fs::write(&feature_path, knowledge::serialize_feature(&feature)).unwrap();

        let issues = validate_all(dir.path()).unwrap();

        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
    }

    /// ADR 0017 §1・§3: uid mode到達後、Feature.requirement_uidsがどの
    /// Requirementのuidとも一致しない(壊れた参照)場合を検知する。
    #[test]
    fn flags_a_feature_requirement_uid_that_matches_no_requirement_once_in_uid_mode() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());
        crate::identity::marker::mark_uid_mode(dir.path()).unwrap();
        // requirement_uids still holds the pre-migration placeholder
        // ("controls", a display id) — never a real uid — so once in uid
        // mode this must be reported as a dangling reference.

        let issues = validate_all(dir.path()).unwrap();

        assert!(
            issues.iter().any(|i| i.path.contains("feature.yml")
                && i.message.contains("requirement_uids")
                && i.message.contains("controls")),
            "expected a dangling requirement_uids reference issue, got: {issues:?}"
        );
    }

    #[test]
    fn reports_a_schema_violation_when_scenario_id_is_not_a_valid_slug() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/player-jump/jump/ground/scenario.yml"),
            "id: ../../../../evil\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from the ground.\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"Confirmed.\"\n",
        )
        .unwrap();

        let issues = validate_all(dir.path()).unwrap();

        assert!(
            issues
                .iter()
                .any(|i| i.path.contains("scenario.yml") && i.message.contains("does not match")),
            "expected a pattern-violation issue for scenario.yml, got: {issues:?}"
        );
    }

    #[test]
    fn reports_a_schema_violation_when_a_required_field_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/player-jump/feature.yml"),
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
                .join(".markharness/knowledge/features/player-jump/feature.yml"),
            "id: player-jump\nrequirement_uids: [controls]\nlabel: player-jump\naxis: [not-registered]\n",
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
            dir.path().join(".markharness/knowledge/features/player-jump/feature.yml"),
            "id: player-jump\nrequirement_uids: [controls]\nlabel: player-jump\naxis: [gameplay]\nforked_from: no-such-feature\n",
        )
        .unwrap();

        let issues = validate_all(dir.path()).unwrap();

        assert!(
            issues.iter().any(|i| i.message.contains("forked_from")),
            "expected a forked_from issue, got: {issues:?}"
        );
    }

    #[test]
    fn accepts_a_forked_from_pointing_at_a_known_feature() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path());
        write_valid_tree(dir.path());
        fs::create_dir_all(
            dir.path()
                .join(".markharness/knowledge/features/player-double-jump"),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/player-double-jump/feature.yml"),
            "id: player-double-jump\nrequirement_uids: [controls]\nlabel: player-double-jump\naxis: [gameplay]\nforked_from: player-jump\n",
        )
        .unwrap();

        let issues = validate_all(dir.path()).unwrap();

        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
    }
}
