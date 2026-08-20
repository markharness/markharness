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

/// `case_uid` (design doc §8): derived from the sorted set
/// `{requirement_uid, feature_uid, behavior_uid, condition_uid,
/// expected_result_uid}`. Sorting `expected_result_uids` first makes the
/// result independent of the order they happen to be listed in.
pub fn case_uid(
    requirement_uid: &str,
    feature_uid: &str,
    behavior_uid: &str,
    condition_uid: &str,
    expected_result_uids: &[String],
) -> String {
    let mut sorted: Vec<&str> = expected_result_uids.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    let joined_expected = sorted.join(",");
    derive(
        "markharness:case_uid:v1",
        &[
            requirement_uid,
            feature_uid,
            behavior_uid,
            condition_uid,
            &joined_expected,
        ],
    )
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
        let a = case_uid(
            "req",
            "feat",
            "beh",
            "cond",
            &["er1".to_string(), "er2".to_string()],
        );
        let b = case_uid(
            "req",
            "feat",
            "beh",
            "cond",
            &["er1".to_string(), "er2".to_string()],
        );
        assert_eq!(a, b);
    }

    #[test]
    fn case_uid_is_independent_of_expected_result_order() {
        let a = case_uid(
            "req",
            "feat",
            "beh",
            "cond",
            &["er1".to_string(), "er2".to_string()],
        );
        let b = case_uid(
            "req",
            "feat",
            "beh",
            "cond",
            &["er2".to_string(), "er1".to_string()],
        );
        assert_eq!(a, b);
    }

    #[test]
    fn case_uid_changes_when_any_component_changes() {
        let base = case_uid("req", "feat", "beh", "cond", &["er1".to_string()]);
        assert_ne!(
            base,
            case_uid("req2", "feat", "beh", "cond", &["er1".to_string()])
        );
        assert_ne!(
            base,
            case_uid("req", "feat2", "beh", "cond", &["er1".to_string()])
        );
        assert_ne!(
            base,
            case_uid("req", "feat", "beh", "cond", &["er2".to_string()])
        );
    }

    /// Guards against naive concatenation collisions ("ab"+"c" vs "a"+"bc").
    #[test]
    fn case_uid_does_not_collide_across_a_shifted_field_boundary() {
        let a = case_uid("ab", "c", "beh", "cond", &[]);
        let b = case_uid("a", "bc", "beh", "cond", &[]);
        assert_ne!(a, b);
    }

    #[test]
    fn case_uid_is_a_valid_version_five_uuid_string() {
        let uid = case_uid("req", "feat", "beh", "cond", &[]);
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
        let case = case_uid("x", "x", "x", "x", &[]);
        let change = change_event_uid("x", "x", "x", "x", "x", "");
        assert_ne!(case, change);
    }
}
