//! Requirement-side operations: relating a Feature to a Requirement, and
//! re-pinning an external Requirement's `.sdoc` revision (ADR 0023).
//!
//! The Feature owns the relation (ADR 0017 §1/§3), so `link`/`unlink` edit
//! `feature.yml`'s `requirement_uids` rather than introducing a second place
//! where the relation could be stated.

use std::io;
use std::path::{Path, PathBuf};

use crate::fs_safety::replace_file;
use crate::knowledge::{self, RequirementSource};

#[derive(Debug)]
pub enum RequirementOpError {
    FeatureNotFound(String),
    RequirementNotFound(String),
    /// The Requirement has no `uid` yet, so nothing stable can be linked to.
    /// Display ids are never used as matching keys (ADR 0013).
    RequirementNotMigrated(String),
    /// `repin` only applies to `source: external` — a native Requirement has
    /// no pinned reference to move.
    NotExternal(String),
    /// The `.sdoc` the locator names is absent from the working tree.
    LocatorMissing {
        id: String,
        locator: String,
    },
    Malformed {
        path: PathBuf,
        message: String,
    },
    Io(io::Error),
}

impl From<io::Error> for RequirementOpError {
    fn from(e: io::Error) -> Self {
        RequirementOpError::Io(e)
    }
}

impl std::fmt::Display for RequirementOpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequirementOpError::FeatureNotFound(id) => {
                write!(
                    f,
                    "no Feature '{id}' under .markharness/knowledge/features/"
                )
            }
            RequirementOpError::RequirementNotFound(id) => write!(
                f,
                "no Requirement '{id}' under .markharness/knowledge/requirements/"
            ),
            RequirementOpError::RequirementNotMigrated(id) => write!(
                f,
                "Requirement '{id}' has no uid yet; run `markharness identity migrate` first"
            ),
            RequirementOpError::NotExternal(id) => write!(
                f,
                "Requirement '{id}' is source: native; repin applies only to source: external"
            ),
            RequirementOpError::LocatorMissing { id, locator } => write!(
                f,
                "Requirement '{id}' points at '{locator}', which does not exist in the working tree"
            ),
            RequirementOpError::Malformed { path, message } => {
                write!(f, "{}: {message}", path.display())
            }
            RequirementOpError::Io(e) => write!(f, "filesystem error: {e}"),
        }
    }
}

fn knowledge_root(root: &Path) -> PathBuf {
    root.join(crate::project_root::MARKHARNESS_DIR)
        .join("knowledge")
}

fn feature_path(root: &Path, feature_id: &str) -> PathBuf {
    knowledge_root(root)
        .join("features")
        .join(feature_id)
        .join("feature.yml")
}

fn requirement_path(root: &Path, requirement_id: &str) -> PathBuf {
    knowledge_root(root)
        .join("requirements")
        .join(requirement_id)
        .join("requirement.yml")
}

fn read_requirement(
    root: &Path,
    requirement_id: &str,
) -> Result<knowledge::Requirement, RequirementOpError> {
    let path = requirement_path(root, requirement_id);
    if !path.is_file() {
        return Err(RequirementOpError::RequirementNotFound(
            requirement_id.to_string(),
        ));
    }
    let content = std::fs::read_to_string(&path)?;
    knowledge::parse_requirement(&content).map_err(|e| RequirementOpError::Malformed {
        path,
        message: e.to_string(),
    })
}

fn read_feature(root: &Path, feature_id: &str) -> Result<knowledge::Feature, RequirementOpError> {
    let path = feature_path(root, feature_id);
    if !path.is_file() {
        return Err(RequirementOpError::FeatureNotFound(feature_id.to_string()));
    }
    let content = std::fs::read_to_string(&path)?;
    knowledge::parse_feature(&content).map_err(|e| RequirementOpError::Malformed {
        path,
        message: e.to_string(),
    })
}

/// Whether the write changed anything, so the CLI can say "already linked"
/// instead of implying an edit that did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkOutcome {
    Changed,
    AlreadyInDesiredState,
}

/// Adds the Requirement's **uid** to the Feature's `requirement_uids`.
/// The uid, not the display id, is what gets stored (ADR 0013): a later
/// rename of the Requirement's `id:` must not break the relation.
pub fn link(
    root: &Path,
    feature_id: &str,
    requirement_id: &str,
) -> Result<LinkOutcome, RequirementOpError> {
    let requirement = read_requirement(root, requirement_id)?;
    let uid = requirement
        .uid
        .ok_or_else(|| RequirementOpError::RequirementNotMigrated(requirement_id.to_string()))?;
    let mut feature = read_feature(root, feature_id)?;
    if feature
        .requirement_uids
        .iter()
        .any(|existing| existing == &uid)
    {
        return Ok(LinkOutcome::AlreadyInDesiredState);
    }
    feature.requirement_uids.push(uid);
    // Sorted so the file's content depends on the set of relations, not on
    // the order the links happened to be created in.
    feature.requirement_uids.sort();
    write_feature(root, feature_id, &feature)?;
    Ok(LinkOutcome::Changed)
}

pub fn unlink(
    root: &Path,
    feature_id: &str,
    requirement_id: &str,
) -> Result<LinkOutcome, RequirementOpError> {
    let requirement = read_requirement(root, requirement_id)?;
    let uid = requirement
        .uid
        .ok_or_else(|| RequirementOpError::RequirementNotMigrated(requirement_id.to_string()))?;
    let mut feature = read_feature(root, feature_id)?;
    let before = feature.requirement_uids.len();
    feature.requirement_uids.retain(|existing| existing != &uid);
    if feature.requirement_uids.len() == before {
        return Ok(LinkOutcome::AlreadyInDesiredState);
    }
    write_feature(root, feature_id, &feature)?;
    Ok(LinkOutcome::Changed)
}

fn write_feature(
    root: &Path,
    feature_id: &str,
    feature: &knowledge::Feature,
) -> Result<(), RequirementOpError> {
    replace_file(
        root,
        &feature_path(root, feature_id),
        knowledge::serialize_feature(feature).as_bytes(),
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepinOutcome {
    pub locator: String,
    pub previous_revision: String,
    pub new_revision: String,
}

impl RepinOutcome {
    pub fn changed(&self) -> bool {
        self.previous_revision != self.new_revision
    }
}

/// Moves an external Requirement's `source_revision` to the blob OID the
/// locator currently resolves to.
///
/// Repinning records that a human looked; it does not cancel a spec change.
/// Change Impact compares the `.sdoc` blob between base and head, so a repin
/// inside the same range leaves that comparison untouched (v2 design §6.1,
/// AC18/AC19).
pub fn repin(root: &Path, requirement_id: &str) -> Result<RepinOutcome, RequirementOpError> {
    let requirement = read_requirement(root, requirement_id)?;
    if requirement.source != RequirementSource::External {
        return Err(RequirementOpError::NotExternal(requirement_id.to_string()));
    }
    let locator =
        requirement
            .source_locator
            .clone()
            .ok_or_else(|| RequirementOpError::Malformed {
                path: requirement_path(root, requirement_id),
                message: "source: external without source_locator".to_string(),
            })?;
    let previous_revision = requirement.source_revision.clone().unwrap_or_default();

    if !root.join(&locator).is_file() {
        return Err(RequirementOpError::LocatorMissing {
            id: requirement_id.to_string(),
            locator,
        });
    }
    let new_revision = crate::git::hash_object(root, &locator)?;

    let updated = knowledge::Requirement {
        source_revision: Some(new_revision.clone()),
        ..requirement
    };
    replace_file(
        root,
        &requirement_path(root, requirement_id),
        knowledge::serialize_requirement(&updated).as_bytes(),
    )?;
    Ok(RepinOutcome {
        locator,
        previous_revision,
        new_revision,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        dir
    }

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        #[allow(clippy::disallowed_methods)]
        std::fs::write(path, content).unwrap();
    }

    fn native_requirement(root: &Path, id: &str, uid: Option<&str>) {
        let mut body = format!("id: {id}\nsource: native\nlabel: {id}\naxis: []\n");
        if let Some(uid) = uid {
            body.push_str(&format!("uid: {uid}\n"));
        }
        write(&requirement_path(root, id), &body);
    }

    fn feature(root: &Path, id: &str, requirement_uids: &[&str]) {
        let uids = requirement_uids.join(", ");
        write(
            &feature_path(root, id),
            &format!("id: {id}\nrequirement_uids: [{uids}]\nlabel: {id}\naxis: []\n"),
        );
    }

    const UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    #[test]
    fn link_stores_the_requirements_uid_not_its_display_id() {
        let dir = project();
        native_requirement(dir.path(), "controls", Some(UID));
        feature(dir.path(), "player-jump", &[]);

        assert_eq!(
            link(dir.path(), "player-jump", "controls").unwrap(),
            LinkOutcome::Changed
        );

        let content = std::fs::read_to_string(feature_path(dir.path(), "player-jump")).unwrap();
        assert!(content.contains(UID), "{content}");
        assert!(!content.contains("[controls]"), "{content}");
    }

    #[test]
    fn link_is_idempotent() {
        let dir = project();
        native_requirement(dir.path(), "controls", Some(UID));
        feature(dir.path(), "player-jump", &[UID]);

        assert_eq!(
            link(dir.path(), "player-jump", "controls").unwrap(),
            LinkOutcome::AlreadyInDesiredState
        );
    }

    #[test]
    fn unlink_removes_the_uid_and_is_idempotent() {
        let dir = project();
        native_requirement(dir.path(), "controls", Some(UID));
        feature(dir.path(), "player-jump", &[UID]);

        assert_eq!(
            unlink(dir.path(), "player-jump", "controls").unwrap(),
            LinkOutcome::Changed
        );
        assert_eq!(
            unlink(dir.path(), "player-jump", "controls").unwrap(),
            LinkOutcome::AlreadyInDesiredState
        );
    }

    #[test]
    fn link_refuses_a_requirement_without_a_uid() {
        let dir = project();
        native_requirement(dir.path(), "controls", None);
        feature(dir.path(), "player-jump", &[]);

        assert!(matches!(
            link(dir.path(), "player-jump", "controls"),
            Err(RequirementOpError::RequirementNotMigrated(_))
        ));
    }

    #[test]
    fn link_refuses_an_unknown_feature_or_requirement() {
        let dir = project();
        native_requirement(dir.path(), "controls", Some(UID));
        feature(dir.path(), "player-jump", &[]);

        assert!(matches!(
            link(dir.path(), "no-such-feature", "controls"),
            Err(RequirementOpError::FeatureNotFound(_))
        ));
        assert!(matches!(
            link(dir.path(), "player-jump", "no-such-requirement"),
            Err(RequirementOpError::RequirementNotFound(_))
        ));
    }

    #[test]
    fn repin_refuses_a_native_requirement() {
        let dir = project();
        native_requirement(dir.path(), "controls", Some(UID));

        assert!(matches!(
            repin(dir.path(), "controls"),
            Err(RequirementOpError::NotExternal(_))
        ));
    }

    #[test]
    fn repin_reports_a_missing_locator_instead_of_writing() {
        let dir = project();
        write(
            &requirement_path(dir.path(), "controls"),
            "id: controls\nsource: external\nsource_locator: docs/requirements.sdoc\nsource_revision: 0000000000000000000000000000000000000000\naxis: []\n",
        );

        assert!(matches!(
            repin(dir.path(), "controls"),
            Err(RequirementOpError::LocatorMissing { .. })
        ));
    }
}
