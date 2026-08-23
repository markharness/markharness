use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::changes::ChangeEvent;
use crate::execution::read_all_results;
use crate::fs_safety::replace_file;
use crate::id_cache;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexPaths {
    pub features: PathBuf,
    pub change_events: PathBuf,
    pub executions: PathBuf,
}

#[derive(Serialize)]
struct FeatureIndexEntry {
    tree_sha: String,
    /// The Feature's immutable identity (ADR 0013), when it has one at
    /// `git_ref` — lets an external consumer correlate this entry with
    /// ones from an index rebuilt at a later `git_ref`, even across a
    /// rename that changed the `by_id` key itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    uid: Option<String>,
}

#[derive(Serialize)]
struct FeatureIndex<'a> {
    schema_version: u32,
    git_ref: &'a str,
    by_id: BTreeMap<String, FeatureIndexEntry>,
}

#[derive(Serialize)]
struct ChangeEventIndex {
    schema_version: u32,
    /// Keyed by Feature identity (ADR 0013: `uid` when the ChangeEvent has
    /// one, else `feature_id`) rather than `feature_id` alone, so a
    /// Feature's ChangeEvents recorded before and after a rename land under
    /// the same key instead of being split across two.
    by_feature: BTreeMap<String, Vec<String>>,
}

#[derive(Serialize)]
struct ExecutionIndex {
    schema_version: u32,
    by_case: BTreeMap<String, Vec<ExecutionIndexEntry>>,
}

#[derive(Serialize)]
struct ExecutionIndexEntry {
    result: String,
    executed_at: String,
    bound_versions: BTreeMap<String, String>,
}

fn serialize<T: Serialize>(value: &T) -> io::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn rebuild_indexes(root: &Path, git_ref: &str) -> io::Result<IndexPaths> {
    let index_dir = root.join(".markharness-cache/index");
    let paths = IndexPaths {
        features: index_dir.join("features.json"),
        change_events: index_dir.join("change-events.json"),
        executions: index_dir.join("executions.json"),
    };

    let feature_index = FeatureIndex {
        schema_version: 1,
        git_ref,
        by_id: id_cache::resolve_feature_versions(root, git_ref, false)?
            .into_iter()
            .map(|feature| {
                (
                    feature.id,
                    FeatureIndexEntry {
                        tree_sha: feature.tree_sha,
                        uid: feature.uid,
                    },
                )
            })
            .collect(),
    };

    let mut by_feature: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let changes_dir = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("changes");
    let mut change_paths: Vec<PathBuf> = fs::read_dir(&changes_dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("yaml"))
        .collect();
    change_paths.sort();
    for path in change_paths {
        let events: Vec<ChangeEvent> = serde_yaml_ng::from_str(&fs::read_to_string(path)?)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        for event in events {
            by_feature
                .entry(event.identity_key().to_string())
                .or_default()
                .push(event.event_id);
        }
    }
    for event_ids in by_feature.values_mut() {
        event_ids.sort();
        event_ids.dedup();
    }
    let change_index = ChangeEventIndex {
        schema_version: 1,
        by_feature,
    };

    let mut by_case: BTreeMap<String, Vec<ExecutionIndexEntry>> = BTreeMap::new();
    for execution in read_all_results(root)? {
        by_case
            .entry(execution.case_id)
            .or_default()
            .push(ExecutionIndexEntry {
                result: execution.result,
                executed_at: execution.executed_at,
                bound_versions: execution.verified_feature_tree_shas,
            });
    }
    let execution_index = ExecutionIndex {
        schema_version: 1,
        by_case,
    };

    replace_file(root, &paths.features, &serialize(&feature_index)?)?;
    replace_file(root, &paths.change_events, &serialize(&change_index)?)?;
    replace_file(root, &paths.executions, &serialize(&execution_index)?)?;
    Ok(paths)
}
