//! Canonical Knowledge file paths (matches `knowledge_apply::apply_draft`'s
//! convention: `<kind-plural>/<id>/...`, and for Behavior/Scenario, nested
//! under their parent's own `<id>` directory). Shared by [`super::plan`]
//! (to detect a stale path collision before planning a "new" element) and
//! [`super::execute`] (to know where to write).

use std::path::{Path, PathBuf};

pub fn requirement_path(root: &Path, id: &str) -> PathBuf {
    root.join(crate::project_root::MARKHARNESS_DIR)
        .join("knowledge")
        .join("requirements")
        .join(id)
        .join("requirement.yml")
}

pub fn feature_path(root: &Path, id: &str) -> PathBuf {
    root.join(crate::project_root::MARKHARNESS_DIR)
        .join("knowledge")
        .join("features")
        .join(id)
        .join("feature.yml")
}

pub fn behavior_path(root: &Path, feature_id: &str, behavior_id: &str) -> PathBuf {
    root.join(crate::project_root::MARKHARNESS_DIR)
        .join("knowledge")
        .join("features")
        .join(feature_id)
        .join(behavior_id)
        .join("behavior.yml")
}

pub fn scenario_path(
    root: &Path,
    feature_id: &str,
    behavior_id: &str,
    scenario_id: &str,
) -> PathBuf {
    root.join(crate::project_root::MARKHARNESS_DIR)
        .join("knowledge")
        .join("features")
        .join(feature_id)
        .join(behavior_id)
        .join(scenario_id)
        .join("scenario.yml")
}
