/// The five persistent Knowledge element kinds that carry an immutable
/// `uid` under the identity model (design doc
/// `docs/ja/design/immutable-identity-model-design.md` §3.1). Closed by
/// design: end users never add a sixth kind, so callers match on this enum
/// directly instead of going through a trait-object `Seam`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Requirement,
    Feature,
    Behavior,
    Condition,
    ExpectedResult,
}

impl EntityKind {
    /// Every `EntityKind`, in a stable order. Used by exhaustiveness tests
    /// (design doc §3.3) to detect a kind missing from `DESCRIPTORS`, a
    /// schema file, or a fixture table.
    pub const ALL: [EntityKind; 5] = [
        EntityKind::Requirement,
        EntityKind::Feature,
        EntityKind::Behavior,
        EntityKind::Condition,
        EntityKind::ExpectedResult,
    ];

    /// The `kind:` value stored in identity events and Registry entries
    /// (design doc §4, §5).
    pub fn as_str(self) -> &'static str {
        match self {
            EntityKind::Requirement => "requirement",
            EntityKind::Feature => "feature",
            EntityKind::Behavior => "behavior",
            EntityKind::Condition => "condition",
            EntityKind::ExpectedResult => "expected_result",
        }
    }

    /// The plural directory segment under `.markharness/identity-events/`
    /// and `.markharness-cache/identities/` (design doc §4.1, §5 —
    /// `identities/features/<uid>.yml`).
    pub fn directory_segment(self) -> &'static str {
        match self {
            EntityKind::Requirement => "requirements",
            EntityKind::Feature => "features",
            EntityKind::Behavior => "behaviors",
            EntityKind::Condition => "conditions",
            EntityKind::ExpectedResult => "expected_results",
        }
    }
}

/// The declarative, per-kind differences the identity model needs
/// (design doc §3.2): parent kind, the Knowledge file name, and the JSON
/// Schema file name. Lifecycle rules (issuance, rename, retirement, ...)
/// never live here or vary per kind — only these static facts do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityDescriptor {
    pub kind: EntityKind,
    pub parent_kind: Option<EntityKind>,
    pub file_name: &'static str,
    pub schema_name: &'static str,
}

/// One `EntityDescriptor` per `EntityKind::ALL` entry, in the same order.
/// The exhaustiveness test below verifies this invariant so that adding a
/// kind without adding its descriptor fails loudly.
pub const DESCRIPTORS: [EntityDescriptor; 5] = [
    EntityDescriptor {
        kind: EntityKind::Requirement,
        parent_kind: None,
        file_name: "requirement.yml",
        schema_name: "requirement.schema.json",
    },
    EntityDescriptor {
        kind: EntityKind::Feature,
        parent_kind: Some(EntityKind::Requirement),
        file_name: "feature.yml",
        schema_name: "feature.schema.json",
    },
    EntityDescriptor {
        kind: EntityKind::Behavior,
        parent_kind: Some(EntityKind::Feature),
        file_name: "behavior.yml",
        schema_name: "behavior.schema.json",
    },
    EntityDescriptor {
        kind: EntityKind::Condition,
        parent_kind: Some(EntityKind::Behavior),
        file_name: "condition.yml",
        schema_name: "condition.schema.json",
    },
    EntityDescriptor {
        kind: EntityKind::ExpectedResult,
        parent_kind: Some(EntityKind::Condition),
        file_name: "expected/*.yml",
        schema_name: "expected_result.schema.json",
    },
];

/// Looks up the `EntityDescriptor` for `kind`. Panics only if `DESCRIPTORS`
/// itself is missing an entry, which the exhaustiveness test below
/// prevents from ever reaching a release build.
pub fn descriptor(kind: EntityKind) -> &'static EntityDescriptor {
    DESCRIPTORS
        .iter()
        .find(|d| d.kind == kind)
        .unwrap_or_else(|| panic!("no EntityDescriptor registered for {kind:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// Design doc §3.3: adding an `EntityKind` without adding its
    /// `EntityDescriptor` must fail a test, not panic at runtime on first
    /// use.
    #[test]
    fn every_entity_kind_has_exactly_one_descriptor() {
        let described: BTreeSet<EntityKind> = DESCRIPTORS.iter().map(|d| d.kind).collect();
        let all: BTreeSet<EntityKind> = EntityKind::ALL.into_iter().collect();
        assert_eq!(
            described, all,
            "DESCRIPTORS must cover exactly EntityKind::ALL"
        );
        assert_eq!(
            DESCRIPTORS.len(),
            EntityKind::ALL.len(),
            "DESCRIPTORS must not contain duplicate kinds"
        );
    }

    #[test]
    fn descriptor_looks_up_the_matching_kind() {
        assert_eq!(descriptor(EntityKind::Feature).kind, EntityKind::Feature);
        assert_eq!(descriptor(EntityKind::Feature).file_name, "feature.yml");
    }

    #[test]
    fn feature_requirement_behavior_condition_form_the_expected_parent_chain() {
        assert_eq!(descriptor(EntityKind::Requirement).parent_kind, None);
        assert_eq!(
            descriptor(EntityKind::Feature).parent_kind,
            Some(EntityKind::Requirement)
        );
        assert_eq!(
            descriptor(EntityKind::Behavior).parent_kind,
            Some(EntityKind::Feature)
        );
        assert_eq!(
            descriptor(EntityKind::Condition).parent_kind,
            Some(EntityKind::Behavior)
        );
        assert_eq!(
            descriptor(EntityKind::ExpectedResult).parent_kind,
            Some(EntityKind::Condition)
        );
    }

    #[test]
    fn as_str_is_lowercase_snake_case_and_unique() {
        let strs: BTreeSet<&str> = EntityKind::ALL.iter().map(|k| k.as_str()).collect();
        assert_eq!(strs.len(), EntityKind::ALL.len());
        assert!(
            strs.iter()
                .all(|s| s.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
        );
    }

    /// `as_str()` is used for directory segments; serde's `EntityKind`
    /// representation (embedded in `IdentityEvent`/Registry YAML) must
    /// agree with it so the two never drift apart.
    #[test]
    fn serde_representation_matches_as_str() {
        for kind in EntityKind::ALL {
            let yaml = serde_yaml_ng::to_string(&kind).unwrap();
            assert_eq!(yaml.trim(), kind.as_str());
        }
    }

    #[test]
    fn directory_segments_are_unique_plural_forms() {
        let segments: BTreeSet<&str> = EntityKind::ALL
            .iter()
            .map(|k| k.directory_segment())
            .collect();
        assert_eq!(segments.len(), EntityKind::ALL.len());
        assert_eq!(EntityKind::Feature.directory_segment(), "features");
        assert_eq!(
            EntityKind::ExpectedResult.directory_segment(),
            "expected_results"
        );
    }
}
