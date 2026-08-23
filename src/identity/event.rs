use serde::{Deserialize, Serialize};

use crate::identity::EntityKind;

/// A single identity-lifecycle declaration (design doc §4.2), Git-tracked
/// under `.markharness/identity-events/<kind>/<entity_uid>/<event_uid>.yml`.
/// Ordinary Knowledge edits never produce one of these — only issuance,
/// rename, retirement, restoration, release, reissue, and explicit
/// branch-divergence resolution do (design doc §2, Background).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityEvent {
    pub identity_event_uid: String,
    pub entity_uid: String,
    pub entity_kind: EntityKind,
    /// The single predecessor for every mutation except `Resolved`
    /// (design doc §4.2). `None` only for `Issued`, the root of an
    /// entity's causal graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_identity_event_uid: Option<String>,
    /// Every divergent head being joined. Populated only for `Resolved`;
    /// empty otherwise. Replay order is decided entirely by these
    /// predecessor references, never by `recorded_at`, filename, or
    /// filesystem iteration order (design doc §4.3).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previous_identity_event_uids: Vec<String>,
    pub recorded_at: String,
    #[serde(flatten)]
    pub mutation: IdentityMutation,
}

/// The seven identity-lifecycle mutation kinds (design doc §4.2 table).
/// Closed by design, matching `EntityKind`'s closed-enum-dispatch style
/// (design doc §3.1) — no trait objects, no per-kind subtyping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IdentityMutation {
    /// New UID issuance, carrying the entity's initial `id`. Always a root
    /// of the entity's event graph (no predecessor) — replay's
    /// `id_history` starts here, per the Registry example in design doc
    /// §5.
    Issued { id: String },
    /// An `id:` change (`markharness feature rename-id` and equivalents).
    Renamed { from_id: String, to_id: String },
    /// UID retirement triggered by deleting the Knowledge element.
    Retired,
    /// Restoration of a previously retired UID.
    Restored,
    /// Explicit lift of the reuse reservation on a retired id
    /// (`markharness identity release`, design doc §9).
    Released { released_id: String },
    /// New UID issuance during copy/import as a distinct element, carrying
    /// the entity's initial `id` in the importing project. Like `Issued`,
    /// this is always a root (no predecessor); `source_uid` optionally
    /// records provenance in the source project, for audit purposes only
    /// (it is never resolved as an entity in this project).
    Reissued {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_uid: Option<String>,
    },
    /// Explicit resolution of a branch divergence
    /// (`markharness identity resolve`, design doc §7).
    /// `previous_identity_event_uids` on the containing `IdentityEvent`
    /// lists every divergent head being joined; this field records which
    /// one's outcome (`id`/status) the resolution keeps.
    Resolved { winning_event_uid: String },
}

impl IdentityMutation {
    /// Roots are the only mutations with no predecessor (design doc §4.2).
    pub fn can_be_root(&self) -> bool {
        matches!(
            self,
            IdentityMutation::Issued { .. } | IdentityMutation::Reissued { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_issued() -> IdentityEvent {
        IdentityEvent {
            identity_event_uid: "01ARZ3NDEKTSV4RRFFQ69G5FE0".to_string(),
            entity_uid: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string(),
            entity_kind: EntityKind::Feature,
            previous_identity_event_uid: None,
            previous_identity_event_uids: Vec::new(),
            recorded_at: "2026-08-20T12:00:00Z".to_string(),
            mutation: IdentityMutation::Issued {
                id: "todo-management".to_string(),
            },
        }
    }

    #[test]
    fn issued_event_round_trips_through_yaml() {
        let event = sample_issued();
        let yaml = serde_yaml_ng::to_string(&event).unwrap();
        let parsed: IdentityEvent = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(parsed, event);
    }

    /// Design doc §4.2's YAML example: a `renamed` event carries
    /// `previous_identity_event_uid` (singular) plus `from_id`/`to_id`,
    /// and serializes `type: renamed`.
    #[test]
    fn renamed_event_serializes_with_expected_shape() {
        let event = IdentityEvent {
            identity_event_uid: "01ARZ3NDEKTSV4RRFFQ69G5FE1".to_string(),
            entity_uid: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string(),
            entity_kind: EntityKind::Feature,
            previous_identity_event_uid: Some("01ARZ3NDEKTSV4RRFFQ69G5FE0".to_string()),
            previous_identity_event_uids: Vec::new(),
            recorded_at: "2026-08-20T12:34:56Z".to_string(),
            mutation: IdentityMutation::Renamed {
                from_id: "todo-management".to_string(),
                to_id: "task-management".to_string(),
            },
        };

        let yaml = serde_yaml_ng::to_string(&event).unwrap();
        let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(value["type"].as_str(), Some("renamed"));
        assert_eq!(value["from_id"].as_str(), Some("todo-management"));
        assert_eq!(value["to_id"].as_str(), Some("task-management"));
        assert_eq!(
            value["previous_identity_event_uid"].as_str(),
            Some("01ARZ3NDEKTSV4RRFFQ69G5FE0")
        );
        assert!(value.get("previous_identity_event_uids").is_none());

        let parsed: IdentityEvent = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(parsed, event);
    }

    /// Design doc §4.2: `Resolved` is the only mutation carrying multiple
    /// predecessors, via the outer struct's plural field.
    #[test]
    fn resolved_event_carries_multiple_predecessors() {
        let event = IdentityEvent {
            identity_event_uid: "01ARZ3NDEKTSV4RRFFQ69G5FE9".to_string(),
            entity_uid: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string(),
            entity_kind: EntityKind::Feature,
            previous_identity_event_uid: None,
            previous_identity_event_uids: vec![
                "01ARZ3NDEKTSV4RRFFQ69G5FE1".to_string(),
                "01ARZ3NDEKTSV4RRFFQ69G5FE2".to_string(),
            ],
            recorded_at: "2026-08-21T09:00:00Z".to_string(),
            mutation: IdentityMutation::Resolved {
                winning_event_uid: "01ARZ3NDEKTSV4RRFFQ69G5FE1".to_string(),
            },
        };

        let yaml = serde_yaml_ng::to_string(&event).unwrap();
        let parsed: IdentityEvent = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(parsed, event);
        assert_eq!(parsed.previous_identity_event_uids.len(), 2);
    }

    #[test]
    fn only_issued_and_reissued_can_be_roots() {
        assert!(
            IdentityMutation::Issued {
                id: "x".to_string()
            }
            .can_be_root()
        );
        assert!(
            IdentityMutation::Reissued {
                id: "x".to_string(),
                source_uid: None
            }
            .can_be_root()
        );
        assert!(!IdentityMutation::Retired.can_be_root());
        assert!(!IdentityMutation::Restored.can_be_root());
        assert!(
            !IdentityMutation::Renamed {
                from_id: "a".to_string(),
                to_id: "b".to_string()
            }
            .can_be_root()
        );
    }

    #[test]
    fn every_mutation_kind_round_trips() {
        let base = sample_issued();
        let mutations = vec![
            IdentityMutation::Issued {
                id: "todo-management".to_string(),
            },
            IdentityMutation::Renamed {
                from_id: "a".to_string(),
                to_id: "b".to_string(),
            },
            IdentityMutation::Retired,
            IdentityMutation::Restored,
            IdentityMutation::Released {
                released_id: "a".to_string(),
            },
            IdentityMutation::Reissued {
                id: "imported-feature".to_string(),
                source_uid: Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string()),
            },
            IdentityMutation::Reissued {
                id: "imported-feature".to_string(),
                source_uid: None,
            },
            IdentityMutation::Resolved {
                winning_event_uid: "01ARZ3NDEKTSV4RRFFQ69G5FE1".to_string(),
            },
        ];
        for mutation in mutations {
            let event = IdentityEvent {
                mutation: mutation.clone(),
                ..base.clone()
            };
            let yaml = serde_yaml_ng::to_string(&event).unwrap();
            let parsed: IdentityEvent = serde_yaml_ng::from_str(&yaml).unwrap();
            assert_eq!(parsed.mutation, mutation);
        }
    }
}
