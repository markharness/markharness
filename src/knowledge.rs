use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Feature {
    pub id: String,
    pub axis: Vec<String>,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Condition {
    pub id: String,
    pub summary: String,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct ExpectedResult {
    pub id: String,
    pub result: String,
    #[serde(default)]
    pub label: Option<String>,
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
    let mut out = format!("id: {}\n", feature.id);
    if let Some(label) = &feature.label {
        out.push_str(&format!("label: {label}\n"));
    }
    out.push_str("kind: feature\naxis:\n");
    for a in &feature.axis {
        out.push_str(&format!("  - {a}\n"));
    }
    out
}

pub fn serialize_condition(condition: &Condition) -> String {
    let mut out = format!("id: {}\n", condition.id);
    if let Some(label) = &condition.label {
        out.push_str(&format!("label: {label}\n"));
    }
    out.push_str(&format!(
        "kind: condition\nsummary: {}\n",
        condition.summary
    ));
    out
}

pub fn strip_redundant_condition_prefix(feature_id: &str, condition_id: &str) -> Option<String> {
    let prefix = format!("{feature_id}-");
    condition_id
        .strip_prefix(prefix.as_str())
        .filter(|rest| !rest.is_empty())
        .map(|rest| rest.to_string())
}

pub fn contains_non_ascii(s: &str) -> bool {
    !s.is_ascii()
}

pub fn romanize_label(japanese: &str) -> String {
    kakasi::convert(japanese).romaji
}

pub fn normalize_slug_candidate(raw: &str) -> String {
    let lowered = raw.to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut last_was_hyphen = false;

    for c in lowered.chars() {
        let mapped = if c.is_ascii_lowercase() || c.is_ascii_digit() {
            Some(c)
        } else if c.is_whitespace() || c == '-' {
            Some('-')
        } else {
            None
        };

        match mapped {
            Some('-') => {
                if !last_was_hyphen && !out.is_empty() {
                    out.push('-');
                }
                last_was_hyphen = true;
            }
            Some(c) => {
                out.push(c);
                last_was_hyphen = false;
            }
            None => {}
        }
    }

    if out.ends_with('-') {
        out.pop();
    }

    out
}

pub fn is_valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn serialize_expected_result(expected: &ExpectedResult) -> String {
    let mut out = format!("id: {}\n", expected.id);
    if let Some(label) = &expected.label {
        out.push_str(&format!("label: {label}\n"));
    }
    out.push_str(&format!(
        "kind: expected-result\nresult: {}\n",
        expected.result
    ));
    out
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
            label: None,
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
            label: None,
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
            label: None,
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

    #[test]
    fn contains_non_ascii_is_false_for_ascii_string() {
        assert!(!contains_non_ascii("player-jump-001"));
    }

    #[test]
    fn contains_non_ascii_is_true_for_string_with_japanese() {
        assert!(contains_non_ascii("プレイヤーがジャンプする"));
    }

    #[test]
    fn romanize_label_converts_japanese_to_romaji() {
        assert_eq!(
            romanize_label("プレイヤーがジャンプする"),
            "pureiyaa ga janpu suru"
        );
    }

    #[test]
    fn normalize_slug_candidate_replaces_spaces_with_hyphens() {
        assert_eq!(
            normalize_slug_candidate("pureiyaa ga janpu suru"),
            "pureiyaa-ga-janpu-suru"
        );
    }

    #[test]
    fn normalize_slug_candidate_lowercases_mixed_case_input() {
        assert_eq!(
            normalize_slug_candidate("Pureiyaa GA Janpu"),
            "pureiyaa-ga-janpu"
        );
    }

    #[test]
    fn normalize_slug_candidate_strips_unsupported_symbols() {
        assert_eq!(normalize_slug_candidate("Player!! Jump??"), "player-jump");
    }

    #[test]
    fn serializes_feature_with_label_when_present() {
        let feature = Feature {
            id: "player-jump".to_string(),
            axis: vec!["gameplay".to_string()],
            label: Some("プレイヤージャンプ".to_string()),
        };

        let yaml = serialize_feature(&feature);

        assert_eq!(
            yaml,
            "id: player-jump\nlabel: プレイヤージャンプ\nkind: feature\naxis:\n  - gameplay\n"
        );
    }

    #[test]
    fn serializes_condition_with_label_when_present() {
        let condition = Condition {
            id: "jump-ground".to_string(),
            summary: "Jump from the ground and land".to_string(),
            label: Some("地上からジャンプ".to_string()),
        };

        let yaml = serialize_condition(&condition);

        assert_eq!(
            yaml,
            "id: jump-ground\nlabel: 地上からジャンプ\nkind: condition\nsummary: Jump from the ground and land\n"
        );
    }

    #[test]
    fn serializes_expected_result_with_label_when_present() {
        let expected = ExpectedResult {
            id: "player-jump-ground-001".to_string(),
            result: "lands safely".to_string(),
            label: Some("安全に着地".to_string()),
        };

        let yaml = serialize_expected_result(&expected);

        assert_eq!(
            yaml,
            "id: player-jump-ground-001\nlabel: 安全に着地\nkind: expected-result\nresult: lands safely\n"
        );
    }
}
