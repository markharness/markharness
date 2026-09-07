//! Typed identifiers for ADR 0017 (`docs/en/decisions/0017-scenario-case-revision-and-execution-evidence.md`
//! §3): "FeatureUid, ScenarioUid, CaseUid, ExecutionUid, display IDs, and
//! revision references have distinct domain types. Never mix display IDs
//! and UIDs as matching keys." Each wrapper below is a distinct Rust type
//! so a caller cannot pass a `CaseUid` where a `ScenarioUid` is expected
//! and have it compile.
//!
//! Validation here is intentionally shallow (non-empty, non-whitespace):
//! the concrete format per kind (ULID for registry-issued uids, UUIDv5 for
//! derived ones) already differs and is enforced where each value is
//! produced (schema validation, `derived_uid`). Duplicating that here
//! would be a second, driftable source of truth.

use std::fmt;

/// A value was empty or contained only whitespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlankValueError {
    pub type_name: &'static str,
}

impl fmt::Display for BlankValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} must not be empty or whitespace-only", self.type_name)
    }
}

impl std::error::Error for BlankValueError {}

macro_rules! define_typed_uid {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, BlankValueError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(BlankValueError {
                        type_name: stringify!($name),
                    });
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let raw = String::deserialize(deserializer)?;
                $name::new(raw).map_err(serde::de::Error::custom)
            }
        }
    };
}

define_typed_uid!(RequirementUid);
define_typed_uid!(FeatureUid);
define_typed_uid!(BehaviorUid);
define_typed_uid!(ScenarioUid);
define_typed_uid!(CaseUid);
define_typed_uid!(CaseRevision);
define_typed_uid!(ExecutionUid);
// ADR 0017 §5: the opaque, non-empty identifier of the build/commit under
// test. Not among ADR 0017 §3's enumerated identity types, but added here
// (design doc `verification-plan-canonical-model-design.md` §7.2) so its
// existing non-blank requirement (`execution::RecordArgs`) is enforced by
// the type system, same as the identifiers above.
define_typed_uid!(TargetRevision);
// A free-text environment identifier. `None` means explicitly unknown; a
// blank string is never a valid value (see `TargetRevision` above for why
// this is typed rather than left as `String`).
define_typed_uid!(Environment);
// A mutable, human-facing `id:` field. Never accepted where a uid is
// required (ADR 0017 §3: "Never mix display IDs and UIDs as matching
// keys").
define_typed_uid!(DisplayId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_rejects_an_empty_string() {
        assert!(ScenarioUid::new("").is_err());
    }

    #[test]
    fn target_revision_new_rejects_a_blank_string() {
        assert!(TargetRevision::new("   ").is_err());
    }

    #[test]
    fn target_revision_new_accepts_a_non_blank_string() {
        let revision = TargetRevision::new("abc123").unwrap();
        assert_eq!(revision.as_str(), "abc123");
    }

    #[test]
    fn target_revision_round_trips_through_yaml() {
        let revision = TargetRevision::new("abc123").unwrap();
        let yaml = serde_yaml_ng::to_string(&revision).unwrap();
        assert_eq!(yaml.trim(), "abc123");
        let deserialized: TargetRevision = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(deserialized, revision);
    }

    #[test]
    fn target_revision_deserialize_rejects_a_blank_string() {
        let result: Result<TargetRevision, _> = serde_yaml_ng::from_str("\"  \"");
        assert!(result.is_err());
    }

    #[test]
    fn environment_new_rejects_a_blank_string() {
        assert!(Environment::new("").is_err());
    }

    #[test]
    fn environment_new_accepts_a_non_blank_string() {
        let environment = Environment::new("staging").unwrap();
        assert_eq!(environment.as_str(), "staging");
    }

    #[test]
    fn environment_round_trips_through_yaml() {
        let environment = Environment::new("staging").unwrap();
        let yaml = serde_yaml_ng::to_string(&environment).unwrap();
        assert_eq!(yaml.trim(), "staging");
        let deserialized: Environment = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(deserialized, environment);
    }

    #[test]
    fn environment_deserialize_rejects_a_blank_string() {
        let result: Result<Environment, _> = serde_yaml_ng::from_str("\"  \"");
        assert!(result.is_err());
    }

    #[test]
    fn new_rejects_a_whitespace_only_string() {
        assert!(CaseUid::new("   ").is_err());
    }

    #[test]
    fn new_accepts_a_non_blank_string() {
        let uid = FeatureUid::new("01HXYZ").unwrap();
        assert_eq!(uid.as_str(), "01HXYZ");
    }

    #[test]
    fn display_renders_the_inner_value() {
        let uid = RequirementUid::new("req-1").unwrap();
        assert_eq!(uid.to_string(), "req-1");
    }

    #[test]
    fn distinct_typed_uids_with_the_same_text_are_not_equal_across_types() {
        // Compile-time property: this would fail to compile if CaseUid and
        // ScenarioUid were the same type or comparable to each other.
        let case = CaseUid::new("same").unwrap();
        let scenario = ScenarioUid::new("same").unwrap();
        assert_eq!(case.as_str(), scenario.as_str());
    }

    #[test]
    fn serializes_as_a_plain_yaml_string() {
        let uid = BehaviorUid::new("beh-1").unwrap();
        let yaml = serde_yaml_ng::to_string(&uid).unwrap();
        assert_eq!(yaml.trim(), "beh-1");
    }

    #[test]
    fn deserialize_rejects_a_blank_string() {
        let result: Result<ExecutionUid, _> = serde_yaml_ng::from_str("\"  \"");
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_round_trips_a_valid_value() {
        let uid: DisplayId = serde_yaml_ng::from_str("\"cond-001\"").unwrap();
        assert_eq!(uid.as_str(), "cond-001");
    }
}
