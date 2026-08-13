use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Requirement {
    pub id: String,
    pub label: String,
    pub axis: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub related_issues: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Feature {
    pub id: String,
    pub requirement: String,
    pub label: String,
    pub axis: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// 概念的な派生元Feature id(§3.1)。Git履歴に現れないドメイン知識のため手動記述。
    #[serde(default)]
    pub forked_from: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Behavior {
    pub id: String,
    pub feature: String,
    pub label: String,
    pub axis: Vec<String>,
    pub description: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Condition {
    pub id: String,
    pub behavior: String,
    pub label: String,
    pub description: String,
}

/// How an `ExpectedResult`'s content was produced. Omitting the field
/// (`Option::None`) means unknown, not `Manual`; a `knowledge/` file
/// written before this field existed round-trips to `None` via
/// `#[serde(default)]`, and that must not be read as "written manually".
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GeneratedBy {
    Manual,
    Llm,
    AutoCombination,
}

/// A human review gate on an `ExpectedResult`. Omitting the whole
/// `verified_by` field means not (yet) reviewed; `human_review` is
/// required whenever the object is present (no ambiguous partial state).
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct VerifiedBy {
    pub human_review: bool,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct ExpectedResult {
    pub id: String,
    pub condition: String,
    pub description: String,
    #[serde(default)]
    pub generated_by: Option<GeneratedBy>,
    #[serde(default)]
    pub verified_by: Option<VerifiedBy>,
}

pub fn parse_requirement(yaml: &str) -> Result<Requirement, serde_yaml_ng::Error> {
    serde_yaml_ng::from_str(yaml)
}

pub fn parse_feature(yaml: &str) -> Result<Feature, serde_yaml_ng::Error> {
    serde_yaml_ng::from_str(yaml)
}

pub fn parse_behavior(yaml: &str) -> Result<Behavior, serde_yaml_ng::Error> {
    serde_yaml_ng::from_str(yaml)
}

pub fn parse_condition(yaml: &str) -> Result<Condition, serde_yaml_ng::Error> {
    serde_yaml_ng::from_str(yaml)
}

pub fn parse_expected_result(yaml: &str) -> Result<ExpectedResult, serde_yaml_ng::Error> {
    serde_yaml_ng::from_str(yaml)
}

fn yaml_flow_array(items: &[String]) -> String {
    format!("[{}]", items.join(", "))
}

pub fn serialize_requirement(requirement: &Requirement) -> String {
    let mut out = format!(
        "id: {}\nlabel: {}\naxis: {}\n",
        requirement.id,
        requirement.label,
        yaml_flow_array(&requirement.axis)
    );
    if let Some(description) = &requirement.description {
        out.push_str(&format!("description: |\n  {description}\n"));
    }
    out
}

pub fn serialize_feature(feature: &Feature) -> String {
    let mut out = format!(
        "id: {}\nrequirement: {}\nlabel: {}\naxis: {}\n",
        feature.id,
        feature.requirement,
        feature.label,
        yaml_flow_array(&feature.axis)
    );
    if let Some(description) = &feature.description {
        out.push_str(&format!("description: |\n  {description}\n"));
    }
    if let Some(forked_from) = &feature.forked_from {
        out.push_str(&format!("forked_from: {forked_from}\n"));
    }
    out
}

pub fn serialize_behavior(behavior: &Behavior) -> String {
    format!(
        "id: {}\nfeature: {}\nlabel: {}\naxis: {}\ndescription: |\n  {}\n",
        behavior.id,
        behavior.feature,
        behavior.label,
        yaml_flow_array(&behavior.axis),
        behavior.description
    )
}

pub fn serialize_condition(condition: &Condition) -> String {
    format!(
        "id: {}\nbehavior: {}\nlabel: {}\ndescription: |\n  {}\n",
        condition.id, condition.behavior, condition.label, condition.description
    )
}

pub fn serialize_expected_result(expected: &ExpectedResult) -> String {
    format!(
        "id: {}\ncondition: {}\ndescription: |\n  {}\n",
        expected.id, expected.condition, expected.description
    )
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
    use wana_kana::ConvertJapanese;
    japanese.to_romaji()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_requirement_yaml() {
        let yaml = "id: account-management\nlabel: account-management\naxis: [security]\n";

        let requirement: Requirement = parse_requirement(yaml).unwrap();

        assert_eq!(requirement.id, "account-management");
        assert_eq!(requirement.label, "account-management");
        assert_eq!(requirement.axis, vec!["security"]);
        assert_eq!(requirement.description, None);
    }

    #[test]
    fn serializes_requirement_to_deterministic_yaml() {
        let requirement = Requirement {
            id: "account-management".to_string(),
            label: "account-management".to_string(),
            axis: vec!["security".to_string()],
            description: None,
            source: None,
            related_issues: Vec::new(),
        };

        let yaml = serialize_requirement(&requirement);

        assert_eq!(
            yaml,
            "id: account-management\nlabel: account-management\naxis: [security]\n"
        );
    }

    #[test]
    fn serializes_requirement_with_description_when_present() {
        let requirement = Requirement {
            id: "account-management".to_string(),
            label: "アカウント管理".to_string(),
            axis: vec!["security".to_string()],
            description: Some("Account related requirements.".to_string()),
            source: None,
            related_issues: Vec::new(),
        };

        let yaml = serialize_requirement(&requirement);

        assert_eq!(
            yaml,
            "id: account-management\nlabel: アカウント管理\naxis: [security]\ndescription: |\n  Account related requirements.\n"
        );
    }

    #[test]
    fn parses_feature_yaml() {
        let yaml = "id: player-jump\nrequirement: player-controls\nlabel: player-jump\naxis: [gameplay, animation]\n";

        let feature: Feature = parse_feature(yaml).unwrap();

        assert_eq!(feature.id, "player-jump");
        assert_eq!(feature.requirement, "player-controls");
        assert_eq!(feature.label, "player-jump");
        assert_eq!(feature.axis, vec!["gameplay", "animation"]);
        assert_eq!(feature.description, None);
    }

    #[test]
    fn parses_behavior_yaml() {
        let yaml = "id: player-jump-jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\n";

        let behavior: Behavior = parse_behavior(yaml).unwrap();

        assert_eq!(behavior.id, "player-jump-jump");
        assert_eq!(behavior.feature, "player-jump");
        assert_eq!(behavior.label, "jump");
        assert_eq!(behavior.axis, vec!["gameplay"]);
        assert_eq!(behavior.description, "Player presses jump.\n");
    }

    #[test]
    fn parses_condition_yaml() {
        let yaml = "id: player-jump-jump-ground\nbehavior: player-jump-jump\nlabel: ground\ndescription: |\n  Jump from the ground and land.\n";

        let condition: Condition = parse_condition(yaml).unwrap();

        assert_eq!(condition.id, "player-jump-jump-ground");
        assert_eq!(condition.behavior, "player-jump-jump");
        assert_eq!(condition.label, "ground");
        assert_eq!(condition.description, "Jump from the ground and land.\n");
    }

    #[test]
    fn parses_expected_result_yaml() {
        let yaml = "id: player-jump-jump-ground-001\ncondition: player-jump-jump-ground\ndescription: |\n  Lands safely.\n";

        let expected: ExpectedResult = parse_expected_result(yaml).unwrap();

        assert_eq!(expected.id, "player-jump-jump-ground-001");
        assert_eq!(expected.condition, "player-jump-jump-ground");
        assert_eq!(expected.description, "Lands safely.\n");
    }

    #[test]
    fn serializes_feature_to_deterministic_yaml() {
        let feature = Feature {
            id: "player-jump".to_string(),
            requirement: "player-controls".to_string(),
            label: "player-jump".to_string(),
            axis: vec!["gameplay".to_string(), "animation".to_string()],
            description: None,
            forked_from: None,
        };

        let yaml = serialize_feature(&feature);

        assert_eq!(
            yaml,
            "id: player-jump\nrequirement: player-controls\nlabel: player-jump\naxis: [gameplay, animation]\n"
        );
    }

    #[test]
    fn serializes_feature_with_description_when_present() {
        let feature = Feature {
            id: "player-jump".to_string(),
            requirement: "player-controls".to_string(),
            label: "プレイヤージャンプ".to_string(),
            axis: vec!["gameplay".to_string()],
            description: Some("Jump related behaviors.".to_string()),
            forked_from: None,
        };

        let yaml = serialize_feature(&feature);

        assert_eq!(
            yaml,
            "id: player-jump\nrequirement: player-controls\nlabel: プレイヤージャンプ\naxis: [gameplay]\ndescription: |\n  Jump related behaviors.\n"
        );
    }

    #[test]
    fn parses_feature_yaml_with_forked_from() {
        let yaml = "id: player-double-jump\nrequirement: player-controls\nlabel: player-double-jump\naxis: [gameplay]\nforked_from: player-jump\n";

        let feature: Feature = parse_feature(yaml).unwrap();

        assert_eq!(feature.forked_from, Some("player-jump".to_string()));
    }

    #[test]
    fn parses_feature_yaml_without_forked_from_as_none() {
        let feature: Feature = parse_feature(
            "id: player-jump\nrequirement: player-controls\nlabel: player-jump\naxis: [gameplay]\n",
        )
        .unwrap();

        assert_eq!(feature.forked_from, None);
    }

    #[test]
    fn serializes_feature_with_forked_from_when_present() {
        let feature = Feature {
            id: "player-double-jump".to_string(),
            requirement: "player-controls".to_string(),
            label: "player-double-jump".to_string(),
            axis: vec!["gameplay".to_string()],
            description: None,
            forked_from: Some("player-jump".to_string()),
        };

        let yaml = serialize_feature(&feature);

        assert_eq!(
            yaml,
            "id: player-double-jump\nrequirement: player-controls\nlabel: player-double-jump\naxis: [gameplay]\nforked_from: player-jump\n"
        );
    }

    #[test]
    fn serializes_behavior_to_deterministic_yaml() {
        let behavior = Behavior {
            id: "player-jump-jump".to_string(),
            feature: "player-jump".to_string(),
            label: "jump".to_string(),
            axis: vec!["gameplay".to_string()],
            description: "Player presses jump.".to_string(),
        };

        let yaml = serialize_behavior(&behavior);

        assert_eq!(
            yaml,
            "id: player-jump-jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\n"
        );
    }

    #[test]
    fn serializes_condition_to_deterministic_yaml() {
        let condition = Condition {
            id: "player-jump-jump-ground".to_string(),
            behavior: "player-jump-jump".to_string(),
            label: "ground".to_string(),
            description: "Jump from the ground and land.".to_string(),
        };

        let yaml = serialize_condition(&condition);

        assert_eq!(
            yaml,
            "id: player-jump-jump-ground\nbehavior: player-jump-jump\nlabel: ground\ndescription: |\n  Jump from the ground and land.\n"
        );
    }

    #[test]
    fn serializes_expected_result_to_deterministic_yaml() {
        let expected = ExpectedResult {
            id: "player-jump-jump-ground-001".to_string(),
            condition: "player-jump-jump-ground".to_string(),
            description: "Lands safely.".to_string(),
            generated_by: None,
            verified_by: None,
        };

        let yaml = serialize_expected_result(&expected);

        assert_eq!(
            yaml,
            "id: player-jump-jump-ground-001\ncondition: player-jump-jump-ground\ndescription: |\n  Lands safely.\n"
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
        // wana_kana (MIT) converts kana character-by-character without word
        // segmentation, so there is no space between words unless the input
        // already has one. Kanji are not converted (they are dropped later by
        // normalize_slug_candidate's ASCII filter).
        assert_eq!(
            romanize_label("プレイヤーがジャンプする"),
            "pureiyaagajanpusuru"
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
}
