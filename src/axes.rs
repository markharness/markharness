use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
pub struct AxisEntry {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
}

/// Reads `root/axes/*.yml` and returns entries sorted by id. Returns an
/// empty list when `axes/` is missing (mirrors `knowledge_draft::load_axis_registry`).
pub fn list_axes(root: &Path) -> Vec<AxisEntry> {
    let axes_dir = root.join("axes");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_empty_list_when_axes_dir_missing() {
        let dir = tempfile::tempdir().unwrap();

        let axes = list_axes(dir.path());

        assert!(axes.is_empty());
    }

    #[test]
    fn lists_axes_sorted_by_id() {
        let dir = tempfile::tempdir().unwrap();
        let axes_dir = dir.path().join("axes");
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
        let axes_dir = dir.path().join("axes");
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
}
