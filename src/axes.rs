use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::fs_safety::{remove_file_no_follow, replace_file};
use crate::knowledge::is_valid_slug;

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
pub struct AxisEntry {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
}

/// Reads `root/axes/*.yml` and returns entries sorted by id. Returns an
/// empty list when `axes/` is missing (mirrors `knowledge_draft::load_axis_registry`).
pub fn list_axes(root: &Path) -> Vec<AxisEntry> {
    let axes_dir = root.join(crate::project_root::MARKHARNESS_DIR).join("axes");
    let Ok(entries) = fs::read_dir(&axes_dir) else {
        return Vec::new();
    };

    let mut axes: Vec<AxisEntry> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("yml"))
        .filter_map(|path| fs::read_to_string(&path).ok())
        .filter_map(|yaml| serde_yaml_ng::from_str::<AxisEntry>(&yaml).ok())
        .collect();
    axes.sort_by(|a, b| a.id.cmp(&b.id));
    axes
}

/// Creates `root/axes/<id>.yml` with `label` defaulted to `id`, mirroring
/// the default-label-equals-id convention used elsewhere (e.g.
/// `knowledge_apply::apply_draft`). Creates `axes/` if it does not exist yet.
/// Used by `knowledge add --edit`'s axis auto-registration.
pub fn create_axis(root: &Path, id: &str) -> io::Result<PathBuf> {
    let path = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("axes")
        .join(format!("{id}.yml"));
    replace_file(root, &path, format!("id: {id}\nlabel: {id}\n").as_bytes())?;
    Ok(path)
}

/// Why `axes add` can fail (`markharness axes add`, the non-interactive
/// counterpart to `knowledge add --edit`'s auto-registration).
#[derive(Debug)]
pub enum AddAxisError {
    /// `id` is not a plain slug. Rejected before it can become a path
    /// component of `axes/<id>.yml` (path-traversal defense, mirroring
    /// `generate::generate_testcases`'s slug checks).
    InvalidId,
    /// `axes/<id>.yml` already exists. Unlike `create_axis` (used by the
    /// interactive `knowledge add --edit` flow, which only ever calls it for
    /// ids already filtered out of the registry), `axes add` is a standalone
    /// creation command and refuses to silently overwrite an existing axis's
    /// label.
    AlreadyExists,
    Io(io::Error),
}

impl From<io::Error> for AddAxisError {
    fn from(e: io::Error) -> Self {
        AddAxisError::Io(e)
    }
}

/// Creates `root/axes/<id>.yml` with `label` defaulted to `id` when omitted,
/// refusing to overwrite an axis that already exists. The non-interactive
/// counterpart to `knowledge add --edit`'s axis auto-registration, for
/// scripted/agent-driven callers that can't drive an interactive editor.
pub fn add_axis(root: &Path, id: &str, label: Option<&str>) -> Result<PathBuf, AddAxisError> {
    if !is_valid_slug(id) {
        return Err(AddAxisError::InvalidId);
    }
    let path = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("axes")
        .join(format!("{id}.yml"));
    if path.is_file() {
        return Err(AddAxisError::AlreadyExists);
    }
    let label = label.unwrap_or(id);
    replace_file(
        root,
        &path,
        format!("id: {id}\nlabel: {label}\n").as_bytes(),
    )?;
    Ok(path)
}

#[derive(Debug, Deserialize)]
struct AxisBearingEntry {
    #[serde(default)]
    axis: Vec<String>,
}

/// Recursively collects every axis id referenced by a `requirement.yml`,
/// `feature.yml`, or `behavior.yml` under `knowledge_root` (the only three
/// entity kinds with an `axis` field; `condition.yml`/`expected/*.yml` have
/// none).
fn collect_referenced_axes(knowledge_root: &Path) -> HashSet<String> {
    let mut referenced = HashSet::new();
    collect_referenced_axes_into(knowledge_root, &mut referenced);
    referenced
}

fn collect_referenced_axes_into(dir: &Path, out: &mut HashSet<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            collect_referenced_axes_into(&path, out);
            continue;
        }
        let is_axis_bearing_file = matches!(
            path.file_name().and_then(|n| n.to_str()),
            Some("requirement.yml" | "feature.yml" | "behavior.yml")
        );
        if !is_axis_bearing_file {
            continue;
        }
        if let Ok(yaml) = fs::read_to_string(&path)
            && let Ok(parsed) = serde_yaml_ng::from_str::<AxisBearingEntry>(&yaml)
        {
            out.extend(parsed.axis);
        }
    }
}

/// Returns the ids of every `axes/*.yml` entry not referenced by any
/// `requirement`/`feature`/`behavior`'s `axis:` list anywhere under
/// `knowledge/`, sorted by id.
pub fn find_unused(root: &Path) -> Vec<String> {
    let referenced = collect_referenced_axes(
        &root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge"),
    );
    let mut unused: Vec<String> = list_axes(root)
        .into_iter()
        .map(|entry| entry.id)
        .filter(|id| !referenced.contains(id))
        .collect();
    unused.sort();
    unused
}

/// Returns the ids `find_unused` reports, additionally deleting each one's
/// `axes/<id>.yml` when `delete` is true (a no-op report otherwise).
pub fn prune(root: &Path, delete: bool) -> io::Result<Vec<String>> {
    let unused = find_unused(root);
    if delete {
        for id in &unused {
            let path = root
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("axes")
                .join(format!("{id}.yml"));
            remove_file_no_follow(root, &path)?;
        }
    }
    Ok(unused)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_unused_reports_an_axis_referenced_by_nothing() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("axes"),
        )
        .unwrap();
        fs::write(
            dir.path().join(".markharness/axes/orphan.yml"),
            "id: orphan\nlabel: orphan\n",
        )
        .unwrap();

        let unused = find_unused(dir.path());

        assert_eq!(unused, vec!["orphan".to_string()]);
    }

    #[test]
    fn find_unused_excludes_an_axis_referenced_by_a_requirement() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("axes"),
        )
        .unwrap();
        fs::write(
            dir.path().join(".markharness/axes/gameplay.yml"),
            "id: gameplay\nlabel: gameplay\n",
        )
        .unwrap();
        let requirement_dir = dir.path().join(".markharness/knowledge/controls");
        fs::create_dir_all(&requirement_dir).unwrap();
        fs::write(
            requirement_dir.join("requirement.yml"),
            "id: controls\nlabel: controls\naxis: [gameplay]\n",
        )
        .unwrap();

        let unused = find_unused(dir.path());

        assert!(unused.is_empty());
    }

    #[test]
    fn prune_without_delete_reports_unused_axes_but_leaves_files_in_place() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("axes"),
        )
        .unwrap();
        fs::write(
            dir.path().join(".markharness/axes/orphan.yml"),
            "id: orphan\nlabel: orphan\n",
        )
        .unwrap();

        let pruned = prune(dir.path(), false).unwrap();

        assert_eq!(pruned, vec!["orphan".to_string()]);
        assert!(dir.path().join(".markharness/axes/orphan.yml").exists());
    }

    #[test]
    fn prune_with_delete_removes_only_the_unused_axis_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("axes"),
        )
        .unwrap();
        fs::write(
            dir.path().join(".markharness/axes/orphan.yml"),
            "id: orphan\nlabel: orphan\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".markharness/axes/gameplay.yml"),
            "id: gameplay\nlabel: gameplay\n",
        )
        .unwrap();
        let requirement_dir = dir.path().join(".markharness/knowledge/controls");
        fs::create_dir_all(&requirement_dir).unwrap();
        fs::write(
            requirement_dir.join("requirement.yml"),
            "id: controls\nlabel: controls\naxis: [gameplay]\n",
        )
        .unwrap();

        let pruned = prune(dir.path(), true).unwrap();

        assert_eq!(pruned, vec!["orphan".to_string()]);
        assert!(!dir.path().join(".markharness/axes/orphan.yml").exists());
        assert!(
            dir.path().join(".markharness/axes/gameplay.yml").exists(),
            "an axis still referenced by a requirement must not be deleted"
        );
    }

    #[test]
    fn returns_empty_list_when_axes_dir_missing() {
        let dir = tempfile::tempdir().unwrap();

        let axes = list_axes(dir.path());

        assert!(axes.is_empty());
    }

    #[test]
    fn lists_axes_sorted_by_id() {
        let dir = tempfile::tempdir().unwrap();
        let axes_dir = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("axes");
        fs::create_dir_all(&axes_dir).unwrap();
        fs::write(
            axes_dir.join("network.yml"),
            "id: network\nlabel: Network\n",
        )
        .unwrap();
        fs::write(
            axes_dir.join("gameplay.yml"),
            "id: gameplay\nlabel: Gameplay\n",
        )
        .unwrap();

        let axes = list_axes(dir.path());

        assert_eq!(
            axes,
            vec![
                AxisEntry {
                    id: "gameplay".to_string(),
                    label: Some("Gameplay".to_string()),
                },
                AxisEntry {
                    id: "network".to_string(),
                    label: Some("Network".to_string()),
                },
            ]
        );
    }

    #[test]
    fn axis_without_label_field_defaults_to_none() {
        let dir = tempfile::tempdir().unwrap();
        let axes_dir = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("axes");
        fs::create_dir_all(&axes_dir).unwrap();
        fs::write(axes_dir.join("ai.yml"), "id: ai\n").unwrap();

        let axes = list_axes(dir.path());

        assert_eq!(
            axes,
            vec![AxisEntry {
                id: "ai".to_string(),
                label: None,
            }]
        );
    }

    #[test]
    fn create_axis_writes_id_and_label_defaulted_to_id() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("axes"),
        )
        .unwrap();

        create_axis(dir.path(), "state").unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join(".markharness/axes/state.yml")).unwrap(),
            "id: state\nlabel: state\n"
        );
    }

    #[test]
    fn create_axis_creates_axes_dir_when_missing() {
        let dir = tempfile::tempdir().unwrap();

        create_axis(dir.path(), "state").unwrap();

        assert!(dir.path().join(".markharness/axes/state.yml").is_file());
    }

    #[test]
    fn create_axis_makes_it_discoverable_by_list_axes() {
        let dir = tempfile::tempdir().unwrap();

        create_axis(dir.path(), "state").unwrap();
        let axes = list_axes(dir.path());

        assert!(axes.iter().any(|a| a.id == "state"));
    }

    #[test]
    fn add_axis_writes_id_and_label_defaulted_to_id_when_label_omitted() {
        let dir = tempfile::tempdir().unwrap();

        let path = add_axis(dir.path(), "state", None).unwrap();

        assert_eq!(
            path,
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("axes")
                .join("state.yml")
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "id: state\nlabel: state\n"
        );
    }

    #[test]
    fn add_axis_writes_the_given_label_when_provided() {
        let dir = tempfile::tempdir().unwrap();

        let path = add_axis(dir.path(), "state", Some("State")).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "id: state\nlabel: State\n"
        );
    }

    #[test]
    fn add_axis_creates_axes_dir_when_missing() {
        let dir = tempfile::tempdir().unwrap();

        add_axis(dir.path(), "state", None).unwrap();

        assert!(dir.path().join(".markharness/axes/state.yml").is_file());
    }

    #[test]
    fn add_axis_rejects_an_id_that_already_exists() {
        let dir = tempfile::tempdir().unwrap();
        add_axis(dir.path(), "state", None).unwrap();

        let result = add_axis(dir.path(), "state", Some("Different label"));

        assert!(matches!(result, Err(AddAxisError::AlreadyExists)));
        // The pre-existing file must be left untouched, not overwritten.
        assert_eq!(
            fs::read_to_string(dir.path().join(".markharness/axes/state.yml")).unwrap(),
            "id: state\nlabel: state\n"
        );
    }

    #[test]
    fn add_axis_rejects_an_id_that_is_not_a_valid_slug() {
        let dir = tempfile::tempdir().unwrap();

        let result = add_axis(dir.path(), "../../evil", None);

        assert!(matches!(result, Err(AddAxisError::InvalidId)));
        assert!(
            !dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("axes")
                .exists()
        );
    }
}
