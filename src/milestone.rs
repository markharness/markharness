use std::io;
use std::path::Path;

use serde::Deserialize;

use crate::fs_safety::replace_file;
use crate::git;

/// The audit fields `milestone_init` writes into `milestone.yml` (issue
/// #29 §3), if present. Older files (pre-dating this feature) parse with
/// both fields `None`, distinguishing "not recorded" from "recorded and
/// wrong" for `verify_audit_matches_tag`.
#[derive(Debug, Deserialize)]
struct MilestoneAudit {
    #[serde(default)]
    commit_oid: Option<String>,
    #[serde(default)]
    knowledge_schema_version: Option<u32>,
}

fn read_milestone_audit(root: &Path, name: &str) -> io::Result<Option<MilestoneAudit>> {
    let path = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("executions")
        .join(name)
        .join("milestone.yml");
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let audit: MilestoneAudit = serde_yaml_ng::from_str(&content)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{path:?}: {e}")))?;
    Ok(Some(audit))
}

/// Verifies `name`'s `milestone.yml` audit copy (issue #29 §3) agrees with
/// what `name`'s tag actually resolves to right now — issue #29's version
/// resolution policy table: "milestone.yml とtag内の正本が不一致 |
/// エラーとして報告する". A no-op when there's no `milestone.yml` for `name`
/// (an arbitrary commit ref, not a named milestone) or when the recorded
/// audit predates these fields (both `None` — "milestone.yml にバージョン
/// 情報がない | tag内の正本を使用する", so the tag alone is trusted).
///
/// Takes `name`'s already-resolved `resolved_schema` rather than resolving
/// it again internally — the caller (`changes::compute_changes_with_warnings`)
/// needs that same resolution for the fail-closed gate and the legacy
/// warning anyway, and re-resolving here would reintroduce the duplicate
/// Git read/drift risk `compute_changes_with_warnings` exists to avoid.
pub fn verify_audit_matches_tag(
    root: &Path,
    name: &str,
    resolved_schema: &crate::knowledge_schema::ResolvedSchemaVersion,
) -> io::Result<()> {
    let Some(audit) = read_milestone_audit(root, name)? else {
        return Ok(());
    };
    if let Some(recorded_oid) = &audit.commit_oid {
        let actual_oid = git::resolve_commit_oid(root, name)?;
        if recorded_oid != &actual_oid {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "milestone.yml for '{name}' recorded commit_oid {recorded_oid}, but the tag now resolves to {actual_oid}. The tag may have moved, or milestone.yml was hand-edited."
                ),
            ));
        }
    }
    if let Some(recorded_version) = audit.knowledge_schema_version
        && recorded_version != resolved_schema.version
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "milestone.yml for '{name}' recorded knowledge_schema_version {recorded_version}, but the tag now resolves to {}. The tag may have moved, or milestone.yml was hand-edited.",
                resolved_schema.version
            ),
        ));
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub enum MilestoneInitOutcome {
    Created,
    AlreadyInitialized,
}

#[derive(Debug)]
pub enum MilestoneInitError {
    TagNotFound,
    Io(io::Error),
}

impl From<io::Error> for MilestoneInitError {
    fn from(e: io::Error) -> Self {
        MilestoneInitError::Io(e)
    }
}

pub fn milestone_init(root: &Path, tag: &str) -> Result<MilestoneInitOutcome, MilestoneInitError> {
    if !git::tag_exists(root, tag)? {
        return Err(MilestoneInitError::TagNotFound);
    }

    let milestone_dir = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("executions")
        .join(tag);
    let milestone_path = milestone_dir.join("milestone.yml");
    if milestone_path.is_file() {
        return Ok(MilestoneInitOutcome::AlreadyInitialized);
    }

    let commit_oid = git::resolve_commit_oid(root, tag)?;
    let knowledge_schema_version = crate::knowledge_schema::resolve(root, tag)?.version;
    replace_file(
        root,
        &milestone_path,
        format!("id: {tag}\ncommit_oid: {commit_oid}\nknowledge_schema_version: {knowledge_schema_version}\n")
            .as_bytes(),
    )?;
    Ok(MilestoneInitOutcome::Created)
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
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        dir
    }

    fn commit_all(root: &Path, message: &str) {
        run_git(root, &["add", "-A"]);
        run_git(root, &["commit", "-q", "-m", message]);
    }

    #[cfg(unix)]
    fn link_dir(link: &Path, target: &Path) {
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[cfg(windows)]
    fn link_dir(link: &Path, target: &Path) {
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let status = Command::new("cmd")
            .args(["/c", "mklink", "/j"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "mklink /j failed");
    }

    #[test]
    fn milestone_init_refuses_to_follow_a_symlinked_executions_dir() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]);
        let outside = tempfile::tempdir().unwrap();
        link_dir(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("executions"),
            outside.path(),
        );

        let result = milestone_init(dir.path(), "m1");

        assert!(matches!(result, Err(MilestoneInitError::Io(_))));
        assert!(!outside.path().join("m1").exists());
    }

    #[test]
    fn milestone_init_errors_when_tag_does_not_exist() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");

        let result = milestone_init(dir.path(), "m1");

        assert!(matches!(result, Err(MilestoneInitError::TagNotFound)));
    }

    #[test]
    fn milestone_init_writes_milestone_yml_when_tag_exists() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]);
        let commit_oid = crate::git::resolve_commit_oid(dir.path(), "m1").unwrap();

        let result = milestone_init(dir.path(), "m1");

        assert_eq!(result.unwrap(), MilestoneInitOutcome::Created);
        let written =
            fs::read_to_string(dir.path().join(".markharness/executions/m1/milestone.yml"))
                .unwrap();
        assert_eq!(
            written,
            format!("id: m1\ncommit_oid: {commit_oid}\nknowledge_schema_version: 1\n")
        );
    }

    /// Standards/Spec review of issue #29: the policy table requires
    /// `milestone.yml` を正本と食い違った場合エラーとして報告する — but
    /// nothing previously checked this. An arbitrary commit ref (no
    /// `milestone.yml` for that name) is not audited at all.
    #[test]
    fn verify_audit_matches_tag_is_a_noop_when_there_is_no_milestone_yml_for_the_name() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]);
        let resolved = crate::knowledge_schema::resolve(dir.path(), "m1").unwrap();

        assert!(verify_audit_matches_tag(dir.path(), "m1", &resolved).is_ok());
    }

    #[test]
    fn verify_audit_matches_tag_passes_when_the_recorded_audit_agrees_with_the_tag() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]);
        milestone_init(dir.path(), "m1").unwrap();
        let resolved = crate::knowledge_schema::resolve(dir.path(), "m1").unwrap();

        assert!(verify_audit_matches_tag(dir.path(), "m1", &resolved).is_ok());
    }

    #[test]
    fn verify_audit_matches_tag_is_a_noop_for_a_milestone_yml_predating_the_audit_fields() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]);
        let milestone_path = dir.path().join(".markharness/executions/m1/milestone.yml");
        fs::create_dir_all(milestone_path.parent().unwrap()).unwrap();
        fs::write(&milestone_path, "id: m1\n").unwrap();
        let resolved = crate::knowledge_schema::resolve(dir.path(), "m1").unwrap();

        assert!(verify_audit_matches_tag(dir.path(), "m1", &resolved).is_ok());
    }

    #[test]
    fn verify_audit_matches_tag_errors_when_the_recorded_commit_oid_disagrees_with_the_tag() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]);
        let milestone_path = dir.path().join(".markharness/executions/m1/milestone.yml");
        fs::create_dir_all(milestone_path.parent().unwrap()).unwrap();
        fs::write(
            &milestone_path,
            "id: m1\ncommit_oid: 0000000000000000000000000000000000000000\nknowledge_schema_version: 1\n",
        )
        .unwrap();
        let resolved = crate::knowledge_schema::resolve(dir.path(), "m1").unwrap();

        let err = verify_audit_matches_tag(dir.path(), "m1", &resolved).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn verify_audit_matches_tag_errors_when_the_recorded_knowledge_schema_version_disagrees_with_the_tag()
     {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]);
        let commit_oid = crate::git::resolve_commit_oid(dir.path(), "m1").unwrap();
        let milestone_path = dir.path().join(".markharness/executions/m1/milestone.yml");
        fs::create_dir_all(milestone_path.parent().unwrap()).unwrap();
        fs::write(
            &milestone_path,
            format!("id: m1\ncommit_oid: {commit_oid}\nknowledge_schema_version: 2\n"),
        )
        .unwrap();
        let resolved = crate::knowledge_schema::resolve(dir.path(), "m1").unwrap();

        let err = verify_audit_matches_tag(dir.path(), "m1", &resolved).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn milestone_init_records_the_knowledge_schema_version_recorded_at_the_tag() {
        let dir = init_repo();
        fs::create_dir_all(dir.path().join(".markharness")).unwrap();
        fs::write(
            dir.path().join(".markharness/config.toml"),
            "schema_version = 1\n\n[knowledge]\nschema_version = 2\n",
        )
        .unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]);

        milestone_init(dir.path(), "m1").unwrap();

        let written =
            fs::read_to_string(dir.path().join(".markharness/executions/m1/milestone.yml"))
                .unwrap();
        assert!(written.contains("knowledge_schema_version: 2\n"));
    }

    #[test]
    fn milestone_init_is_idempotent_and_leaves_existing_file_untouched() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]);
        let milestone_path = dir.path().join(".markharness/executions/m1/milestone.yml");
        fs::create_dir_all(milestone_path.parent().unwrap()).unwrap();
        fs::write(&milestone_path, "id: m1\nlabel: hand-edited\n").unwrap();

        let result = milestone_init(dir.path(), "m1");

        assert_eq!(result.unwrap(), MilestoneInitOutcome::AlreadyInitialized);
        let content = fs::read_to_string(&milestone_path).unwrap();
        assert_eq!(content, "id: m1\nlabel: hand-edited\n");
    }
}
