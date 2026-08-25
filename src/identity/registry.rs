use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fs_safety::{remove_dir_all_no_follow, replace_file};
use crate::git;
use crate::identity::engine::{self, ReplayResult};
use crate::identity::{EntityKind, IdentityEvent};

/// Bumped when the rules for which fields feed the Registry cache key
/// (not their values) change (design doc §5, mirroring `id_cache.rs`'s
/// `CANONICALIZATION_RULE_VERSION`, §3.3 of the paper).
const CANONICALIZATION_RULE_VERSION: &str = "1";
/// Bumped when the on-disk cache entry's own shape changes.
const ID_INDEX_SCHEMA_VERSION: &str = "1";

fn cache_dir(root: &Path) -> PathBuf {
    root.join(".markharness-cache").join("identities")
}

/// `.markharness-cache/identities/<kind>/<uid>.yml` (design doc §5).
fn cache_path(root: &Path, kind: EntityKind, entity_uid: &str) -> PathBuf {
    cache_dir(root)
        .join(kind.directory_segment())
        .join(format!("{entity_uid}.yml"))
}

/// `.markharness/identity-events/<kind>/<uid>` (design doc §4.1).
fn events_dir_in_repo(kind: EntityKind, entity_uid: &str) -> String {
    format!(
        "{}/identity-events/{}/{}",
        crate::project_root::MARKHARNESS_DIR,
        kind.directory_segment(),
        entity_uid
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CacheKey {
    events_tree_sha: Option<String>,
    canonicalization_rule_version: String,
    id_index_schema_version: String,
    tool_version: String,
}

#[derive(Serialize, Deserialize)]
struct CacheFile {
    key: CacheKey,
    entry: ReplayResult,
}

fn compute_cache_key(
    root: &Path,
    git_ref: &str,
    kind: EntityKind,
    entity_uid: &str,
) -> io::Result<CacheKey> {
    Ok(CacheKey {
        events_tree_sha: git::tree_sha(root, git_ref, &events_dir_in_repo(kind, entity_uid))?,
        canonicalization_rule_version: CANONICALIZATION_RULE_VERSION.to_string(),
        id_index_schema_version: ID_INDEX_SCHEMA_VERSION.to_string(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// Reads every identity event under `.markharness/identity-events/<kind>/<uid>/`
/// as it existed at `git_ref`. Returns an empty `Vec` (not an error) when
/// the entity has no events at that ref — for example, before it was ever
/// issued a UID.
///
/// A malformed event file (invalid YAML, or valid YAML that doesn't match
/// `IdentityEvent`'s shape) fails with `io::ErrorKind::InvalidData`
/// specifically — never `io::Error::other`'s `Other`, which every
/// underlying git-command failure in this same call chain also uses.
/// Callers like `identity::audit::check_causal_chain` rely on that
/// distinction to tell a malformed event file (itself an audit finding to
/// report) apart from a genuine Git/I/O failure (an infrastructure
/// problem that must keep propagating instead).
fn load_events(
    root: &Path,
    git_ref: &str,
    kind: EntityKind,
    entity_uid: &str,
) -> io::Result<Vec<IdentityEvent>> {
    let path_in_repo = events_dir_in_repo(kind, entity_uid);
    let tree_entries = git::ls_tree_recursive(root, git_ref, &path_in_repo)?;
    let mut events = Vec::new();
    for entry in tree_entries {
        if entry.kind != git::ObjectKind::Blob {
            continue;
        }
        let content = git::show_blob_by_sha(root, &entry.sha)?;
        let event: IdentityEvent = serde_yaml_ng::from_str(&content)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        events.push(event);
    }
    Ok(events)
}

/// Reads every identity event for one entity directly from the working
/// tree (not a committed `git_ref`) — the input `rename-id` and similar
/// mutating commands need, since they act before anything is committed.
/// An entity with no events yet (e.g. never migrated) yields an empty
/// `Vec`, not an error.
pub fn load_events_from_working_tree(
    root: &Path,
    kind: EntityKind,
    entity_uid: &str,
) -> io::Result<Vec<IdentityEvent>> {
    let dir = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("identity-events")
        .join(kind.directory_segment())
        .join(entity_uid);
    let mut events = Vec::new();
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(events),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        if entry.path().extension().and_then(|e| e.to_str()) != Some("yml") {
            continue;
        }
        let content = fs::read_to_string(entry.path())?;
        events.push(serde_yaml_ng::from_str(&content).map_err(io::Error::other)?);
    }
    Ok(events)
}

/// Working-tree counterpart of [`resolve`]: replays an entity's current
/// identity events as they exist on disk right now, without going through
/// `git`. Never consults or writes the Registry cache (that cache is keyed
/// by git tree SHA, which an uncommitted working tree does not have).
pub fn resolve_from_working_tree(
    root: &Path,
    kind: EntityKind,
    entity_uid: &str,
) -> io::Result<Result<ReplayResult, engine::ReplayError>> {
    let events = load_events_from_working_tree(root, kind, entity_uid)?;
    Ok(engine::replay(entity_uid, &events))
}

/// Invalidates one entity's Registry cache entry (design doc §6.1 step 4:
/// delete rather than update in place, so the next read rebuilds it from
/// events).
pub fn invalidate(root: &Path, kind: EntityKind, entity_uid: &str) -> io::Result<()> {
    crate::fs_safety::remove_file_no_follow(root, &cache_path(root, kind, entity_uid))
}

/// Resolves one entity's current identity state at `git_ref`: replays its
/// identity events (consulting `.markharness-cache/identities/` first when
/// `use_cache` is true) and returns the materialized `ReplayResult`.
/// Mirrors `id_cache::resolve_feature_versions`'s cache discipline (design
/// doc §5): a stored cache whose key no longer matches the current one is
/// silently discarded and recomputed, never trusted.
pub fn resolve(
    root: &Path,
    git_ref: &str,
    kind: EntityKind,
    entity_uid: &str,
    use_cache: bool,
) -> io::Result<Result<ReplayResult, engine::ReplayError>> {
    let current_key = if use_cache {
        Some(compute_cache_key(root, git_ref, kind, entity_uid)?)
    } else {
        None
    };

    if let Some(current_key) = &current_key
        && let Ok(cached) = fs::read_to_string(cache_path(root, kind, entity_uid))
        && let Ok(cache_file) = serde_yaml_ng::from_str::<CacheFile>(&cached)
        && &cache_file.key == current_key
    {
        return Ok(Ok(cache_file.entry));
    }

    let events = load_events(root, git_ref, kind, entity_uid)?;
    let result = engine::replay(entity_uid, &events);

    if let (Some(current_key), Ok(entry)) = (current_key, &result) {
        let cache_file = CacheFile {
            key: current_key,
            entry: entry.clone(),
        };
        let yaml = serde_yaml_ng::to_string(&cache_file).map_err(io::Error::other)?;
        replace_file(root, &cache_path(root, kind, entity_uid), yaml.as_bytes())?;
    }

    Ok(result)
}

/// `markharness identity` cache-maintenance support: discards the entire
/// Registry cache. The next `resolve` call recomputes lazily, matching
/// `id_cache::rebuild_cache`'s "delete only, no eager recompute" design.
pub fn rebuild_cache(root: &Path) -> io::Result<()> {
    remove_dir_all_no_follow(root, &cache_dir(root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::IdentityMutation;
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

    fn init_repo_with_issuance(entity_uid: &str, id: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        run_git(dir.path(), &["config", "core.autocrlf", "false"]);
        let events_dir = dir
            .path()
            .join(".markharness/identity-events/features")
            .join(entity_uid);
        fs::create_dir_all(&events_dir).unwrap();
        let event = IdentityEvent {
            identity_event_uid: "01ARZ3NDEKTSV4RRFFQ69G5FE0".to_string(),
            entity_uid: entity_uid.to_string(),
            entity_kind: EntityKind::Feature,
            previous_identity_event_uid: None,
            previous_identity_event_uids: Vec::new(),
            recorded_at: "2026-08-20T00:00:00Z".to_string(),
            mutation: IdentityMutation::Issued { id: id.to_string() },
        };
        fs::write(
            events_dir.join("01ARZ3NDEKTSV4RRFFQ69G5FE0.yml"),
            serde_yaml_ng::to_string(&event).unwrap(),
        )
        .unwrap();
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "issue"]);
        run_git(dir.path(), &["tag", "m1"]);
        dir
    }

    #[test]
    fn resolves_an_issued_entity_without_cache() {
        let dir = init_repo_with_issuance("01ARZ3NDEKTSV4RRFFQ69G5FAV", "todo-management");
        let result = resolve(
            dir.path(),
            "m1",
            EntityKind::Feature,
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            false,
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.current_id, "todo-management");
    }

    #[test]
    fn missing_entity_replays_as_no_root_event() {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        run_git(dir.path(), &["config", "core.autocrlf", "false"]);
        fs::write(dir.path().join("README.md"), "x").unwrap();
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "init"]);
        run_git(dir.path(), &["tag", "m1"]);

        let result = resolve(
            dir.path(),
            "m1",
            EntityKind::Feature,
            "does-not-exist",
            false,
        )
        .unwrap();
        assert_eq!(result, Err(engine::ReplayError::NoRootEvent));
    }

    #[test]
    fn with_cache_writes_and_reuses_cache_file() {
        let dir = init_repo_with_issuance("01ARZ3NDEKTSV4RRFFQ69G5FAV", "todo-management");
        let first = resolve(
            dir.path(),
            "m1",
            EntityKind::Feature,
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            true,
        )
        .unwrap()
        .unwrap();
        assert!(
            cache_path(
                dir.path(),
                EntityKind::Feature,
                "01ARZ3NDEKTSV4RRFFQ69G5FAV"
            )
            .is_file()
        );

        // Tamper with the working tree without committing: a cached call
        // must not see this, proving it read the cache rather than
        // recomputing via git.
        let events_dir = dir
            .path()
            .join(".markharness/identity-events/features/01ARZ3NDEKTSV4RRFFQ69G5FAV");
        fs::write(events_dir.join("bogus.yml"), "not a valid event").unwrap();

        let second = resolve(
            dir.path(),
            "m1",
            EntityKind::Feature,
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            true,
        )
        .unwrap()
        .unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn stale_cache_key_is_silently_recomputed() {
        let dir = init_repo_with_issuance("01ARZ3NDEKTSV4RRFFQ69G5FAV", "todo-management");
        resolve(
            dir.path(),
            "m1",
            EntityKind::Feature,
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            true,
        )
        .unwrap()
        .unwrap();

        let path = cache_path(
            dir.path(),
            EntityKind::Feature,
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        );
        let mut value: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        value["key"]["tool_version"] = serde_yaml_ng::Value::String("stale".to_string());
        value["entry"]["current_id"] = serde_yaml_ng::Value::String("bogus".to_string());
        fs::write(&path, serde_yaml_ng::to_string(&value).unwrap()).unwrap();

        let result = resolve(
            dir.path(),
            "m1",
            EntityKind::Feature,
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            true,
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.current_id, "todo-management");
    }

    #[test]
    fn load_events_from_working_tree_reads_uncommitted_events() {
        let dir = tempfile::tempdir().unwrap();
        let events_dir = dir
            .path()
            .join(".markharness/identity-events/features/01ARZ3NDEKTSV4RRFFQ69G5FAV");
        fs::create_dir_all(&events_dir).unwrap();
        let event = IdentityEvent {
            identity_event_uid: "01ARZ3NDEKTSV4RRFFQ69G5FE0".to_string(),
            entity_uid: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string(),
            entity_kind: EntityKind::Feature,
            previous_identity_event_uid: None,
            previous_identity_event_uids: Vec::new(),
            recorded_at: "2026-08-20T00:00:00Z".to_string(),
            mutation: IdentityMutation::Issued {
                id: "todo-management".to_string(),
            },
        };
        fs::write(
            events_dir.join("01ARZ3NDEKTSV4RRFFQ69G5FE0.yml"),
            serde_yaml_ng::to_string(&event).unwrap(),
        )
        .unwrap();
        // Deliberately not committed to git.

        let events = load_events_from_working_tree(
            dir.path(),
            EntityKind::Feature,
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        )
        .unwrap();
        assert_eq!(events, vec![event]);
    }

    #[test]
    fn load_events_from_working_tree_is_empty_when_entity_has_no_events() {
        let dir = tempfile::tempdir().unwrap();
        let events =
            load_events_from_working_tree(dir.path(), EntityKind::Feature, "does-not-exist")
                .unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn resolve_from_working_tree_replays_uncommitted_events() {
        let dir = tempfile::tempdir().unwrap();
        let events_dir = dir
            .path()
            .join(".markharness/identity-events/features/01ARZ3NDEKTSV4RRFFQ69G5FAV");
        fs::create_dir_all(&events_dir).unwrap();
        let event = IdentityEvent {
            identity_event_uid: "01ARZ3NDEKTSV4RRFFQ69G5FE0".to_string(),
            entity_uid: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string(),
            entity_kind: EntityKind::Feature,
            previous_identity_event_uid: None,
            previous_identity_event_uids: Vec::new(),
            recorded_at: "2026-08-20T00:00:00Z".to_string(),
            mutation: IdentityMutation::Issued {
                id: "todo-management".to_string(),
            },
        };
        fs::write(
            events_dir.join("01ARZ3NDEKTSV4RRFFQ69G5FE0.yml"),
            serde_yaml_ng::to_string(&event).unwrap(),
        )
        .unwrap();

        let result = resolve_from_working_tree(
            dir.path(),
            EntityKind::Feature,
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.current_id, "todo-management");
    }

    #[test]
    fn invalidate_removes_only_the_named_entitys_cache_entry() {
        let dir = init_repo_with_issuance("01ARZ3NDEKTSV4RRFFQ69G5FAV", "todo-management");
        resolve(
            dir.path(),
            "m1",
            EntityKind::Feature,
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            true,
        )
        .unwrap()
        .unwrap();
        assert!(
            cache_path(
                dir.path(),
                EntityKind::Feature,
                "01ARZ3NDEKTSV4RRFFQ69G5FAV"
            )
            .is_file()
        );

        invalidate(
            dir.path(),
            EntityKind::Feature,
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        )
        .unwrap();

        assert!(
            !cache_path(
                dir.path(),
                EntityKind::Feature,
                "01ARZ3NDEKTSV4RRFFQ69G5FAV"
            )
            .is_file()
        );
    }

    #[test]
    fn rebuild_cache_removes_the_identities_cache_directory() {
        let dir = init_repo_with_issuance("01ARZ3NDEKTSV4RRFFQ69G5FAV", "todo-management");
        resolve(
            dir.path(),
            "m1",
            EntityKind::Feature,
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            true,
        )
        .unwrap()
        .unwrap();
        assert!(cache_dir(dir.path()).is_dir());

        rebuild_cache(dir.path()).unwrap();

        assert!(!cache_dir(dir.path()).exists());
    }
}
