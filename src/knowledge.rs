use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Feature {
    pub id: String,
    pub axis: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Condition {
    pub id: String,
    pub summary: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct ExpectedResult {
    pub id: String,
    pub result: String,
}

pub fn parse_feature(yaml: &str) -> Result<Feature, serde_yaml_ng::Error> {
    serde_yaml_ng::from_str(yaml)
}

pub fn parse_condition(yaml: &str) -> Result<Condition, serde_yaml_ng::Error> {
    serde_yaml_ng::from_str(yaml)
}

pub fn parse_expected_result(yaml: &str) -> Result<ExpectedResult, serde_yaml_ng::Error> {
    serde_yaml_ng::from_str(yaml)
}

pub fn serialize_feature(feature: &Feature) -> String {
    let mut out = format!("id: {}\nkind: feature\naxis:\n", feature.id);
    for a in &feature.axis {
        out.push_str(&format!("  - {a}\n"));
    }
    out
}

pub fn serialize_condition(condition: &Condition) -> String {
    format!(
        "id: {}\nkind: condition\nsummary: {}\n",
        condition.id, condition.summary
    )
}

pub fn strip_redundant_condition_prefix(feature_id: &str, condition_id: &str) -> Option<String> {
    let prefix = format!("{feature_id}-");
    condition_id
        .strip_prefix(prefix.as_str())
        .filter(|rest| !rest.is_empty())
        .map(|rest| rest.to_string())
}

pub fn is_valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn serialize_expected_result(expected: &ExpectedResult) -> String {
    format!(
        "id: {}\nkind: expected-result\nresult: {}\n",
        expected.id, expected.result
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_feature_yaml() {
        let yaml = "id: player-jump\nkind: feature\naxis:\n  - gameplay\n  - animation\n";

        let feature: Feature = parse_feature(yaml).unwrap();

        assert_eq!(feature.id, "player-jump");
        assert_eq!(feature.axis, vec!["gameplay", "animation"]);
    }

    #[test]
    fn parses_condition_yaml() {
        let yaml = "id: jump-ground\nkind: condition\nsummary: Jump from the ground and land\n";

        let condition: Condition = parse_condition(yaml).unwrap();

        assert_eq!(condition.id, "jump-ground");
        assert_eq!(condition.summary, "Jump from the ground and land");
    }

    #[test]
    fn parses_expected_result_yaml() {
        let yaml = "id: player-jump-ground-001\nkind: expected-result\nresult: lands safely\n";

        let expected: ExpectedResult = parse_expected_result(yaml).unwrap();

        assert_eq!(expected.id, "player-jump-ground-001");
        assert_eq!(expected.result, "lands safely");
    }

    #[test]
    fn serializes_feature_to_deterministic_yaml() {
        let feature = Feature {
            id: "player-jump".to_string(),
            axis: vec!["gameplay".to_string(), "animation".to_string()],
        };

        let yaml = serialize_feature(&feature);

        assert_eq!(
            yaml,
            "id: player-jump\nkind: feature\naxis:\n  - gameplay\n  - animation\n"
        );
    }

    #[test]
    fn serializes_condition_to_deterministic_yaml() {
        let condition = Condition {
            id: "jump-ground".to_string(),
            summary: "Jump from the ground and land".to_string(),
        };

        let yaml = serialize_condition(&condition);

        assert_eq!(
            yaml,
            "id: jump-ground\nkind: condition\nsummary: Jump from the ground and land\n"
        );
    }

    #[test]
    fn serializes_expected_result_to_deterministic_yaml() {
        let expected = ExpectedResult {
            id: "player-jump-ground-001".to_string(),
            result: "lands safely".to_string(),
        };

        let yaml = serialize_expected_result(&expected);

        assert_eq!(
            yaml,
            "id: player-jump-ground-001\nkind: expected-result\nresult: lands safely\n"
        );
    }

    #[test]
    fn rejects_empty_slug() {
        assert!(!is_valid_slug(""));
    }

    #[test]
    fn rejects_slug_with_invalid_characters() {
        assert!(!is_valid_slug("Player Jump!"));
    }

    #[test]
    fn accepts_lowercase_alphanumeric_hyphen_slug() {
        assert!(is_valid_slug("player-jump-001"));
    }

    #[test]
    fn strips_redundant_prefix_when_condition_id_starts_with_feature_id() {
        assert_eq!(
            strip_redundant_condition_prefix("player-jump", "player-jump-ground"),
            Some("ground".to_string())
        );
    }

    #[test]
    fn does_not_strip_when_condition_id_has_no_matching_prefix() {
        assert_eq!(
            strip_redundant_condition_prefix("player-jump", "jump-ground"),
            None
        );
    }

    #[test]
    fn does_not_strip_when_remainder_would_be_empty() {
        assert_eq!(
            strip_redundant_condition_prefix("player-jump", "player-jump-"),
            None
        );
    }
}
