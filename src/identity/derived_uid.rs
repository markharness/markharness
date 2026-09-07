//! Deterministic UUIDv5 derivation for `case_uid` and `change_event_uid`
//! (design doc §8): pure functions of already-available UIDs, requiring
//! no persistent store and no randomness — the same inputs always produce
//! the same UUID.
//!
//! Fields are joined with `\0` (NUL), which never legitimately appears in
//! a `uid`/`id`/version string, so distinct field boundaries can never
//! collide via naive concatenation (`"ab" + "c"` vs. `"a" + "bc"`).

use sha1::{Digest, Sha1};

// RFC 4122 URL namespace, encoded in network byte order.
const NAMESPACE_URL: [u8; 16] = [
    0x6b, 0xa7, 0xb8, 0x11, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
];

fn canonical_encode(fields: &[&str]) -> Vec<u8> {
    fields.join("\0").into_bytes()
}

fn derive(domain_separator: &str, fields: &[&str]) -> String {
    let mut all_fields = vec![domain_separator];
    all_fields.extend_from_slice(fields);
    let name = canonical_encode(&all_fields);
    let mut hasher = Sha1::new();
    hasher.update(NAMESPACE_URL);
    hasher.update(name);
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        u32::from_be_bytes(bytes[0..4].try_into().expect("four bytes")),
        u16::from_be_bytes(bytes[4..6].try_into().expect("two bytes")),
        u16::from_be_bytes(bytes[6..8].try_into().expect("two bytes")),
        u16::from_be_bytes(bytes[8..10].try_into().expect("two bytes")),
        u64::from_be_bytes([
            0, 0, bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ])
    )
}

/// `case_uid`: derived from `scenario_uid` alone (ADR 0017 §3: **1 Scenario
/// = 1 TestCase**, so CaseUid is a type-tagged deterministic derivation of
/// ScenarioUid, requiring no random issuance or separate case registry).
/// Rename-and-reimport keeps the same ScenarioUid, so the same CaseUid
/// survives too; a rewrite of the Scenario's operations/results/phases
/// changes Case revision (a separate, content-derived value — Step 3), not
/// CaseUid.
pub fn case_uid(scenario_uid: &str) -> String {
    derive("markharness:case_uid:v3", &[scenario_uid])
}

/// `case_revision` (ADR 0017 §3): derived purely from a TestCase's effective
/// content — the canonical encoding of its (already procedure-expanded)
/// Phases, in order — never from `case_uid` or any display-only field
/// (label, description, implementation notes, source). Two Scenarios whose
/// effective Phases canonicalize identically get the same `case_revision`;
/// this is deliberate (ADR §5: "同一定義は共有する" — the immutable frozen
/// case-definition store keys on `(case_uid, case_revision)`, and identical
/// content is meant to share a stored definition).
///
/// ADR 0017 §3 lists "準備操作・事前条件、展開後の共通手順、操作、期待結果、
/// 順序、テストデータ" as the effective inputs a revision must track. The
/// current `knowledge/` schema (`scenario.schema.json`, `behavior.schema.json`)
/// has no separate `precondition`/`test_data` field to omit: setup
/// operations and preconditions are ordinary `Phase.steps` (directly, or via
/// a `use:` reference into `Behavior.procedures` — Step 2's replacement for
/// the old, removed `Behavior.preconditions`), and test data is whatever
/// literal text an author writes into a step's `action` or a Phase's
/// `results` (e.g. "Enter card number 4111-1111-1111-1111"). Both are
/// already inside `Phase.steps`/`Phase.results`, so canonicalizing `Phase`
/// alone captures every one of ADR §3's listed inputs without omission —
/// this is `case_revision`'s explicit, narrowed contract: it hashes
/// `Phase`, and `Phase` is defined to carry all of ADR §3's effective
/// inputs, and nothing else. See the `case_revision_*` tests in
/// `generate.rs` for the concrete cases this covers.
pub fn case_revision(canonical_phases: &str) -> String {
    derive("markharness:case_revision:v1", &[canonical_phases])
}

/// `change_event_uid` (design doc §8): derived from the identity
/// canonicalization/algorithm version, the from/to snapshot identities,
/// the target `feature_uid`, the canonical change payload, and any
/// explicit, result-affecting options (already canonicalized — e.g.
/// sorted and joined — by the caller, since their shape is
/// `changes compute`'s concern, not this module's).
pub fn change_event_uid(
    algorithm_version: &str,
    from_snapshot_identity: &str,
    to_snapshot_identity: &str,
    feature_uid: &str,
    canonical_change_payload: &str,
    canonical_options: &str,
) -> String {
    derive(
        "markharness:change_event_uid:v1",
        &[
            algorithm_version,
            from_snapshot_identity,
            to_snapshot_identity,
            feature_uid,
            canonical_change_payload,
            canonical_options,
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_uid_is_deterministic() {
        let a = case_uid("scenario-uid");
        let b = case_uid("scenario-uid");
        assert_eq!(a, b);
    }

    #[test]
    fn case_uid_changes_when_scenario_uid_changes() {
        let base = case_uid("scenario-uid");
        assert_ne!(base, case_uid("other-scenario-uid"));
    }

    #[test]
    fn case_uid_is_a_valid_version_five_uuid_string() {
        let uid = case_uid("scenario-uid");
        let parts: Vec<&str> = uid.split('-').collect();
        assert_eq!(
            parts.iter().map(|part| part.len()).collect::<Vec<_>>(),
            [8, 4, 4, 4, 12]
        );
        assert!(uid.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
        assert!(parts[2].starts_with('5'));
        assert!(matches!(
            parts[3].chars().next(),
            Some('8' | '9' | 'a' | 'b')
        ));
    }

    /// Design doc `verification-plan-canonical-model-design.md` §7.4: the
    /// `.expect()` calls that convert `case_uid`/`case_revision` output into
    /// `CaseUid`/`CaseRevision` (`generate::compute_case_uid`,
    /// `generate::compute_case_revision`, `application::
    /// build_verification_plan_value`'s reconstruction from
    /// `CanonicalArtifact.uid`) rely on `derive()` always formatting a
    /// 36-character, non-blank string regardless of its input — including a
    /// blank or empty input, which this pins down explicitly so a future
    /// change to `derive()`'s format cannot silently turn those `.expect()`s
    /// into live panics.
    #[test]
    fn case_uid_is_well_formed_even_for_a_blank_input() {
        for input in ["", "   "] {
            let uid = case_uid(input);
            assert!(
                !uid.trim().is_empty(),
                "blank input {input:?} produced a blank case_uid"
            );
            assert_eq!(
                uid.len(),
                36,
                "blank input {input:?} produced a malformed case_uid"
            );
        }
    }

    /// See `case_uid_is_well_formed_even_for_a_blank_input`: `case_revision`
    /// shares the same `derive()` formatting, so the same guarantee applies.
    #[test]
    fn case_revision_is_well_formed_even_for_a_blank_input() {
        for input in ["", "   "] {
            let revision = case_revision(input);
            assert!(
                !revision.trim().is_empty(),
                "blank input {input:?} produced a blank case_revision"
            );
            assert_eq!(
                revision.len(),
                36,
                "blank input {input:?} produced a malformed case_revision"
            );
        }
    }

    #[test]
    fn change_event_uid_is_deterministic() {
        let a = change_event_uid("1", "m1", "m2", "feat-uid", "payload", "");
        let b = change_event_uid("1", "m1", "m2", "feat-uid", "payload", "");
        assert_eq!(a, b);
    }

    #[test]
    fn change_event_uid_changes_when_algorithm_version_changes() {
        let a = change_event_uid("1", "m1", "m2", "feat-uid", "payload", "");
        let b = change_event_uid("2", "m1", "m2", "feat-uid", "payload", "");
        assert_ne!(a, b);
    }

    #[test]
    fn change_event_uid_and_case_uid_never_collide_with_each_other() {
        // Different domain separators guarantee this even for otherwise
        // identical field values.
        let case = case_uid("x");
        let change = change_event_uid("x", "x", "x", "x", "x", "");
        assert_ne!(case, change);
    }

    #[test]
    fn case_revision_is_deterministic() {
        let a = case_revision("phases-json");
        let b = case_revision("phases-json");
        assert_eq!(a, b);
    }

    #[test]
    fn case_revision_changes_when_the_canonical_phases_change() {
        let base = case_revision("phases-json-a");
        assert_ne!(base, case_revision("phases-json-b"));
    }

    #[test]
    fn case_revision_never_collides_with_case_uid_or_change_event_uid() {
        let revision = case_revision("x");
        let case = case_uid("x");
        let change = change_event_uid("x", "x", "x", "x", "x", "");
        assert_ne!(revision, case);
        assert_ne!(revision, change);
    }
}
