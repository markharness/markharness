use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
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
    /// 不変identity(ADR 0013、design/immutable-identity-model-design.md)。
    /// `identity::registry`のreplay結果から書き戻される値であり、未移行の
    /// プロジェクトや`identity migrate`未実行のRequirementでは`None`(§後方互換)。
    #[serde(default)]
    pub uid: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
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
    /// 不変identity(ADR 0013、design/immutable-identity-model-design.md)。
    /// `identity::registry`のreplay結果から書き戻される値であり、未移行の
    /// プロジェクトや`identity migrate`未実行のFeatureでは`None`(§後方互換)。
    #[serde(default)]
    pub uid: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Behavior {
    pub id: String,
    pub feature: String,
    pub label: String,
    pub axis: Vec<String>,
    pub description: String,
    /// ADR 0016: 全Conditionに共通する前提。1要素=1操作。実際の操作手順は
    /// `Condition.steps`へ移った(`behavior.description`は人間向け要約に留まる)。
    pub preconditions: Vec<String>,
    /// 不変identity(ADR 0013、design/immutable-identity-model-design.md)。
    /// `identity::registry`のreplay結果から書き戻される値であり、未移行の
    /// プロジェクトや`identity migrate`未実行のBehaviorでは`None`(§後方互換)。
    #[serde(default)]
    pub uid: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Condition {
    pub id: String,
    pub behavior: String,
    pub label: String,
    pub description: String,
    /// ADR 0016: この条件固有の操作手順。1要素=1操作。behavior.preconditionsの
    /// 後に実行される。
    pub steps: Vec<String>,
    /// ADR 0016: 手順だけでは到達できない、この条件固有の追加前提。1要素=1操作。
    #[serde(default)]
    pub additional_preconditions: Vec<String>,
    /// 不変identity(ADR 0013、design/immutable-identity-model-design.md)。
    /// `identity::registry`のreplay結果から書き戻される値であり、未移行の
    /// プロジェクトや`identity migrate`未実行のConditionでは`None`(§後方互換)。
    #[serde(default)]
    pub uid: Option<String>,
}

/// How an `ExpectedResult`'s content was produced. Omitting the field
/// (`Option::None`) means unknown, not `Manual`; a `knowledge/` file
/// written before this field existed round-trips to `None` via
/// `#[serde(default)]`, and that must not be read as "written manually".
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GeneratedBy {
    Manual,
    Llm,
    AutoCombination,
}

/// A human review gate on an `ExpectedResult`. Omitting the whole
/// `verified_by` field means not (yet) reviewed; `human_review` is
/// required whenever the object is present (no ambiguous partial state).
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct VerifiedBy {
    pub human_review: bool,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ExpectedResult {
    pub id: String,
    pub condition: String,
    pub description: String,
    /// ADR 0016: 観測可能な複数の結果。1要素=1つの観測可能な結果。
    /// テストケース生成にはこの`results`を使い、`description`は人間向け
    /// 1文要約に留まる。
    pub results: Vec<String>,
    /// ADR 0016: この結果を確認する前に必要な追加操作。Condition内で
    /// ファイル名順が先頭の`ExpectedResult`のみ省略可(`None`)。2番目以降は
    /// `validate.rs`のクロスリファレンスチェックにより非空が必須。
    #[serde(default)]
    pub additional_steps: Option<Vec<String>>,
    /// ADR 0016: 実装根拠メモ。生成には使わない。
    #[serde(default)]
    pub implementation_note: Option<String>,
    #[serde(default)]
    pub generated_by: Option<GeneratedBy>,
    #[serde(default)]
    pub verified_by: Option<VerifiedBy>,
    /// 不変identity(ADR 0013、design/immutable-identity-model-design.md)。
    /// `identity::registry`のreplay結果から書き戻される値であり、未移行の
    /// プロジェクトや`identity migrate`未実行のExpectedResultでは`None`(§後方互換)。
    #[serde(default)]
    pub uid: Option<String>,
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

/// Appends a trailing `uid: <value>\n` line when `uid` is present, shared
/// by every `serialize_*` function (ADR 0013: all five Knowledge element
/// kinds carry the same optional `uid:` field, always written last).
fn append_uid_line(out: &mut String, uid: &Option<String>) {
    if let Some(uid) = uid {
        out.push_str(&format!("uid: {uid}\n"));
    }
}

/// `text` の全行に `indent` を付与し、`key: |\n` の後に続けられる形にする。
/// 空行は `indent` を付けず素の改行にする。CRLF は LF に正規化してから分割する。
fn indent_block_scalar(text: &str, indent: &str) -> String {
    text.replace("\r\n", "\n")
        .lines()
        .map(|line| {
            if line.is_empty() {
                "\n".to_string()
            } else {
                format!("{indent}{line}\n")
            }
        })
        .collect()
}

pub fn serialize_requirement(requirement: &Requirement) -> String {
    let mut out = format!(
        // label はプレーンスカラーで出力するため単一行が前提。呼び出し側
        // (knowledge_draft::validate_draft の MultilineLabel チェック)が保証する。
        "id: {}\nlabel: {}\naxis: {}\n",
        requirement.id,
        requirement.label,
        yaml_flow_array(&requirement.axis)
    );
    if let Some(description) = &requirement.description {
        out.push_str("description: |\n");
        out.push_str(&indent_block_scalar(description, "  "));
    }
    append_uid_line(&mut out, &requirement.uid);
    out
}

pub fn serialize_feature(feature: &Feature) -> String {
    let mut out = format!(
        // label はプレーンスカラーで出力するため単一行が前提。呼び出し側
        // (knowledge_draft::validate_draft の MultilineLabel チェック)が保証する。
        "id: {}\nrequirement: {}\nlabel: {}\naxis: {}\n",
        feature.id,
        feature.requirement,
        feature.label,
        yaml_flow_array(&feature.axis)
    );
    if let Some(description) = &feature.description {
        out.push_str("description: |\n");
        out.push_str(&indent_block_scalar(description, "  "));
    }
    if let Some(forked_from) = &feature.forked_from {
        out.push_str(&format!("forked_from: {forked_from}\n"));
    }
    append_uid_line(&mut out, &feature.uid);
    out
}

pub fn serialize_behavior(behavior: &Behavior) -> String {
    let mut out = format!(
        // label はプレーンスカラーで出力するため単一行が前提。呼び出し側
        // (knowledge_draft::validate_draft の MultilineLabel チェック)が保証する。
        "id: {}\nfeature: {}\nlabel: {}\naxis: {}\ndescription: |\n",
        behavior.id,
        behavior.feature,
        behavior.label,
        yaml_flow_array(&behavior.axis)
    );
    out.push_str(&indent_block_scalar(&behavior.description, "  "));
    if behavior.preconditions.is_empty() {
        out.push_str("preconditions: []\n");
    } else {
        out.push_str("preconditions:\n");
        for precondition in &behavior.preconditions {
            out.push_str(&format!(
                "  - {}\n",
                serde_json::to_string(precondition).unwrap()
            ));
        }
    }
    append_uid_line(&mut out, &behavior.uid);
    out
}

pub fn serialize_condition(condition: &Condition) -> String {
    let mut out = format!(
        // label はプレーンスカラーで出力するため単一行が前提。呼び出し側
        // (knowledge_draft::validate_draft の MultilineLabel チェック)が保証する。
        "id: {}\nbehavior: {}\nlabel: {}\ndescription: |\n",
        condition.id, condition.behavior, condition.label
    );
    out.push_str(&indent_block_scalar(&condition.description, "  "));
    out.push_str("steps:\n");
    for step in &condition.steps {
        out.push_str(&format!("  - {}\n", serde_json::to_string(step).unwrap()));
    }
    if condition.additional_preconditions.is_empty() {
        out.push_str("additional_preconditions: []\n");
    } else {
        out.push_str("additional_preconditions:\n");
        for precondition in &condition.additional_preconditions {
            out.push_str(&format!(
                "  - {}\n",
                serde_json::to_string(precondition).unwrap()
            ));
        }
    }
    append_uid_line(&mut out, &condition.uid);
    out
}

pub fn serialize_expected_result(expected: &ExpectedResult) -> String {
    let mut out = format!(
        "id: {}\ncondition: {}\ndescription: |\n",
        expected.id, expected.condition
    );
    out.push_str(&indent_block_scalar(&expected.description, "  "));
    if let Some(additional_steps) = &expected.additional_steps {
        out.push_str("additional_steps:\n");
        for step in additional_steps {
            out.push_str(&format!("  - {}\n", serde_json::to_string(step).unwrap()));
        }
    }
    out.push_str("results:\n");
    for result in &expected.results {
        out.push_str(&format!("  - {}\n", serde_json::to_string(result).unwrap()));
    }
    if let Some(implementation_note) = &expected.implementation_note {
        out.push_str("implementation_note: |\n");
        out.push_str(&indent_block_scalar(implementation_note, "  "));
    }
    append_uid_line(&mut out, &expected.uid);
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
            uid: None,
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
            uid: None,
        };

        let yaml = serialize_requirement(&requirement);

        assert_eq!(
            yaml,
            "id: account-management\nlabel: アカウント管理\naxis: [security]\ndescription: |\n  Account related requirements.\n"
        );
    }

    #[test]
    fn serializes_requirement_with_multiline_description_as_valid_yaml() {
        let requirement = Requirement {
            id: "account-management".to_string(),
            label: "account-management".to_string(),
            axis: vec!["security".to_string()],
            description: Some(
                "line one about foo.js: bar()\nline two about baz.js: qux()\n".to_string(),
            ),
            source: None,
            related_issues: Vec::new(),
            uid: None,
        };

        let yaml = serialize_requirement(&requirement);
        let reparsed: Requirement = parse_requirement(&yaml).unwrap();

        assert_eq!(reparsed.description, requirement.description);
    }

    /// ADR 0013: a `requirement.yml` written before `uid:` existed has no
    /// such key and must still parse, with `uid` defaulting to `None` —
    /// not an error, and not confused with an empty string.
    #[test]
    fn parses_requirement_yaml_without_uid_as_none() {
        let yaml = "id: account-management\nlabel: account-management\naxis: [security]\n";

        let requirement: Requirement = parse_requirement(yaml).unwrap();

        assert_eq!(requirement.uid, None);
    }

    #[test]
    fn parses_requirement_yaml_with_uid() {
        let yaml = "id: account-management\nlabel: account-management\naxis: [security]\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n";

        let requirement: Requirement = parse_requirement(yaml).unwrap();

        assert_eq!(
            requirement.uid,
            Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string())
        );
    }

    #[test]
    fn serializes_requirement_with_uid_when_present() {
        let requirement = Requirement {
            id: "account-management".to_string(),
            label: "account-management".to_string(),
            axis: vec!["security".to_string()],
            description: None,
            source: None,
            related_issues: Vec::new(),
            uid: Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string()),
        };

        let yaml = serialize_requirement(&requirement);

        assert_eq!(
            yaml,
            "id: account-management\nlabel: account-management\naxis: [security]\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n"
        );
        let reparsed: Requirement = parse_requirement(&yaml).unwrap();
        assert_eq!(reparsed, requirement);
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
        let yaml = "id: player-jump-jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\npreconditions:\n  - \"Press the jump button.\"\n";

        let behavior: Behavior = parse_behavior(yaml).unwrap();

        assert_eq!(behavior.id, "player-jump-jump");
        assert_eq!(behavior.feature, "player-jump");
        assert_eq!(behavior.label, "jump");
        assert_eq!(behavior.axis, vec!["gameplay"]);
        assert_eq!(behavior.description, "Player presses jump.\n");
        assert_eq!(
            behavior.preconditions,
            vec!["Press the jump button.".to_string()]
        );
    }

    #[test]
    fn parses_behavior_yaml_with_multiple_preconditions() {
        let yaml = "id: player-jump-jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\npreconditions:\n  - \"Focus the player character.\"\n  - \"Press the jump button.\"\n";

        let behavior: Behavior = parse_behavior(yaml).unwrap();

        assert_eq!(
            behavior.preconditions,
            vec![
                "Focus the player character.".to_string(),
                "Press the jump button.".to_string()
            ]
        );
    }

    #[test]
    fn parses_behavior_yaml_with_no_preconditions() {
        let yaml = "id: player-jump-jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\npreconditions: []\n";

        let behavior: Behavior = parse_behavior(yaml).unwrap();

        assert_eq!(behavior.preconditions, Vec::<String>::new());
    }

    #[test]
    fn parses_condition_yaml() {
        let yaml = "id: player-jump-jump-ground\nbehavior: player-jump-jump\nlabel: ground\ndescription: |\n  Jump from the ground and land.\nsteps:\n  - \"Land on the ground.\"\nadditional_preconditions: []\n";

        let condition: Condition = parse_condition(yaml).unwrap();

        assert_eq!(condition.id, "player-jump-jump-ground");
        assert_eq!(condition.behavior, "player-jump-jump");
        assert_eq!(condition.label, "ground");
        assert_eq!(condition.description, "Jump from the ground and land.\n");
        assert_eq!(condition.steps, vec!["Land on the ground.".to_string()]);
        assert_eq!(condition.additional_preconditions, Vec::<String>::new());
    }

    #[test]
    fn parses_condition_yaml_with_additional_preconditions() {
        let yaml = "id: player-jump-jump-ground\nbehavior: player-jump-jump\nlabel: ground\ndescription: |\n  Jump from the ground and land.\nsteps:\n  - \"Land on the ground.\"\nadditional_preconditions:\n  - \"The character has already been deleted.\"\n";

        let condition: Condition = parse_condition(yaml).unwrap();

        assert_eq!(
            condition.additional_preconditions,
            vec!["The character has already been deleted.".to_string()]
        );
    }

    #[test]
    fn parses_expected_result_yaml() {
        let yaml = "id: player-jump-jump-ground-001\ncondition: player-jump-jump-ground\ndescription: |\n  Lands safely.\nresults:\n  - \"Player is standing on the ground.\"\n";

        let expected: ExpectedResult = parse_expected_result(yaml).unwrap();

        assert_eq!(expected.id, "player-jump-jump-ground-001");
        assert_eq!(expected.condition, "player-jump-jump-ground");
        assert_eq!(expected.description, "Lands safely.\n");
        assert_eq!(
            expected.results,
            vec!["Player is standing on the ground.".to_string()]
        );
        assert_eq!(expected.additional_steps, None);
        assert_eq!(expected.implementation_note, None);
    }

    #[test]
    fn parses_expected_result_yaml_with_additional_steps_and_implementation_note() {
        let yaml = "id: player-jump-jump-ground-002\ncondition: player-jump-jump-ground\ndescription: |\n  Still on the ground after reload.\nadditional_steps:\n  - \"Reload the page.\"\nresults:\n  - \"Player is still on the ground.\"\nimplementation_note: |\n  saveState() persists position to localStorage.\n";

        let expected: ExpectedResult = parse_expected_result(yaml).unwrap();

        assert_eq!(
            expected.additional_steps,
            Some(vec!["Reload the page.".to_string()])
        );
        assert_eq!(
            expected.results,
            vec!["Player is still on the ground.".to_string()]
        );
        assert_eq!(
            expected.implementation_note,
            Some("saveState() persists position to localStorage.\n".to_string())
        );
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
            uid: None,
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
            uid: None,
        };

        let yaml = serialize_feature(&feature);

        assert_eq!(
            yaml,
            "id: player-jump\nrequirement: player-controls\nlabel: プレイヤージャンプ\naxis: [gameplay]\ndescription: |\n  Jump related behaviors.\n"
        );
    }

    #[test]
    fn serializes_feature_with_multiline_description_as_valid_yaml() {
        let feature = Feature {
            id: "player-jump".to_string(),
            requirement: "player-controls".to_string(),
            label: "player-jump".to_string(),
            axis: vec!["gameplay".to_string()],
            description: Some(
                "line one about foo.js: bar()\nline two about baz.js: qux()\n".to_string(),
            ),
            forked_from: None,
            uid: None,
        };

        let yaml = serialize_feature(&feature);
        let reparsed: Feature = parse_feature(&yaml).unwrap();

        assert_eq!(reparsed.description, feature.description);
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
            uid: None,
        };

        let yaml = serialize_feature(&feature);

        assert_eq!(
            yaml,
            "id: player-double-jump\nrequirement: player-controls\nlabel: player-double-jump\naxis: [gameplay]\nforked_from: player-jump\n"
        );
    }

    /// Backward compatibility (ADR 0013 design doc §2, §11): a `feature.yml`
    /// written before `uid:` existed has no such key and must still parse,
    /// with `uid` defaulting to `None` — not an error, and not confused
    /// with an empty string.
    #[test]
    fn parses_feature_yaml_without_uid_as_none() {
        let feature: Feature = parse_feature(
            "id: player-jump\nrequirement: player-controls\nlabel: player-jump\naxis: [gameplay]\n",
        )
        .unwrap();

        assert_eq!(feature.uid, None);
    }

    #[test]
    fn parses_feature_yaml_with_uid() {
        let yaml = "id: task-management\nrequirement: player-controls\nlabel: task-management\naxis: [gameplay]\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n";

        let feature: Feature = parse_feature(yaml).unwrap();

        assert_eq!(feature.uid, Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string()));
    }

    #[test]
    fn serializes_feature_with_uid_when_present() {
        let feature = Feature {
            id: "task-management".to_string(),
            requirement: "player-controls".to_string(),
            label: "task-management".to_string(),
            axis: vec!["gameplay".to_string()],
            description: None,
            forked_from: None,
            uid: Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string()),
        };

        let yaml = serialize_feature(&feature);

        assert_eq!(
            yaml,
            "id: task-management\nrequirement: player-controls\nlabel: task-management\naxis: [gameplay]\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n"
        );
        let reparsed: Feature = parse_feature(&yaml).unwrap();
        assert_eq!(reparsed, feature);
    }

    #[test]
    fn serializes_behavior_to_deterministic_yaml() {
        let behavior = Behavior {
            id: "player-jump-jump".to_string(),
            feature: "player-jump".to_string(),
            label: "jump".to_string(),
            axis: vec!["gameplay".to_string()],
            description: "Player presses jump.".to_string(),
            preconditions: vec!["Press the jump button.".to_string()],
            uid: None,
        };

        let yaml = serialize_behavior(&behavior);

        assert_eq!(
            yaml,
            "id: player-jump-jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\npreconditions:\n  - \"Press the jump button.\"\n"
        );
    }

    #[test]
    fn serializes_behavior_with_multiple_preconditions_as_a_block_sequence() {
        let behavior = Behavior {
            id: "player-jump-jump".to_string(),
            feature: "player-jump".to_string(),
            label: "jump".to_string(),
            axis: vec!["gameplay".to_string()],
            description: "Player presses jump.".to_string(),
            preconditions: vec![
                "Focus the player character.".to_string(),
                "Press the jump button.".to_string(),
            ],
            uid: None,
        };

        let yaml = serialize_behavior(&behavior);

        assert_eq!(
            yaml,
            "id: player-jump-jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\npreconditions:\n  - \"Focus the player character.\"\n  - \"Press the jump button.\"\n"
        );
        let reparsed: Behavior = parse_behavior(&yaml).unwrap();
        assert_eq!(reparsed.preconditions, behavior.preconditions);
    }

    #[test]
    fn serializes_behavior_with_no_preconditions_as_empty_flow_sequence() {
        let behavior = Behavior {
            id: "player-jump-jump".to_string(),
            feature: "player-jump".to_string(),
            label: "jump".to_string(),
            axis: vec!["gameplay".to_string()],
            description: "Player presses jump.".to_string(),
            preconditions: vec![],
            uid: None,
        };

        let yaml = serialize_behavior(&behavior);

        assert_eq!(
            yaml,
            "id: player-jump-jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\npreconditions: []\n"
        );
    }

    #[test]
    fn serializes_behavior_with_multiline_description_as_valid_yaml() {
        let behavior = Behavior {
            id: "player-jump-jump".to_string(),
            feature: "player-jump".to_string(),
            label: "jump".to_string(),
            axis: vec!["gameplay".to_string()],
            description: "line one about foo.js: bar()\nline two about baz.js: qux()\n".to_string(),
            preconditions: vec!["Press the jump button.".to_string()],
            uid: None,
        };

        let yaml = serialize_behavior(&behavior);
        let reparsed: Behavior = parse_behavior(&yaml).unwrap();

        assert_eq!(reparsed.description, behavior.description);
    }

    #[test]
    fn serializes_behavior_with_uid_when_present() {
        let behavior = Behavior {
            id: "player-jump-jump".to_string(),
            feature: "player-jump".to_string(),
            label: "jump".to_string(),
            axis: vec!["gameplay".to_string()],
            description: "Player presses jump.".to_string(),
            preconditions: vec!["Press the jump button.".to_string()],
            uid: Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string()),
        };

        let yaml = serialize_behavior(&behavior);

        assert_eq!(
            yaml,
            "id: player-jump-jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\npreconditions:\n  - \"Press the jump button.\"\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n"
        );
        let reparsed: Behavior = parse_behavior(&yaml).unwrap();
        assert_eq!(reparsed.uid, behavior.uid);
    }

    #[test]
    fn parses_behavior_yaml_without_uid_as_none() {
        let yaml = "id: player-jump-jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\npreconditions:\n  - \"Press the jump button.\"\n";

        let behavior: Behavior = parse_behavior(yaml).unwrap();

        assert_eq!(behavior.uid, None);
    }

    #[test]
    fn serializes_condition_to_deterministic_yaml() {
        let condition = Condition {
            id: "player-jump-jump-ground".to_string(),
            behavior: "player-jump-jump".to_string(),
            label: "ground".to_string(),
            description: "Jump from the ground and land.".to_string(),
            steps: vec!["Land on the ground.".to_string()],
            additional_preconditions: vec![],
            uid: None,
        };

        let yaml = serialize_condition(&condition);

        assert_eq!(
            yaml,
            "id: player-jump-jump-ground\nbehavior: player-jump-jump\nlabel: ground\ndescription: |\n  Jump from the ground and land.\nsteps:\n  - \"Land on the ground.\"\nadditional_preconditions: []\n"
        );
    }

    #[test]
    fn serializes_condition_with_multiline_description_as_valid_yaml() {
        let condition = Condition {
            id: "player-jump-jump-ground".to_string(),
            behavior: "player-jump-jump".to_string(),
            label: "ground".to_string(),
            description: "line one about foo.js: bar()\nline two about baz.js: qux()\n".to_string(),
            steps: vec!["Land on the ground.".to_string()],
            additional_preconditions: vec![],
            uid: None,
        };

        let yaml = serialize_condition(&condition);
        let reparsed: Condition = parse_condition(&yaml).unwrap();

        assert_eq!(reparsed.description, condition.description);
    }

    #[test]
    fn serializes_condition_with_additional_preconditions_as_a_block_sequence() {
        let condition = Condition {
            id: "player-jump-jump-ground".to_string(),
            behavior: "player-jump-jump".to_string(),
            label: "ground".to_string(),
            description: "Jump from the ground and land.".to_string(),
            steps: vec!["Land on the ground.".to_string()],
            additional_preconditions: vec!["The character has already been deleted.".to_string()],
            uid: None,
        };

        let yaml = serialize_condition(&condition);

        assert_eq!(
            yaml,
            "id: player-jump-jump-ground\nbehavior: player-jump-jump\nlabel: ground\ndescription: |\n  Jump from the ground and land.\nsteps:\n  - \"Land on the ground.\"\nadditional_preconditions:\n  - \"The character has already been deleted.\"\n"
        );
        let reparsed: Condition = parse_condition(&yaml).unwrap();
        assert_eq!(
            reparsed.additional_preconditions,
            condition.additional_preconditions
        );
    }

    #[test]
    fn serializes_condition_with_uid_when_present() {
        let condition = Condition {
            id: "player-jump-jump-ground".to_string(),
            behavior: "player-jump-jump".to_string(),
            label: "ground".to_string(),
            description: "Jump from the ground and land.".to_string(),
            steps: vec!["Land on the ground.".to_string()],
            additional_preconditions: vec![],
            uid: Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string()),
        };

        let yaml = serialize_condition(&condition);

        assert_eq!(
            yaml,
            "id: player-jump-jump-ground\nbehavior: player-jump-jump\nlabel: ground\ndescription: |\n  Jump from the ground and land.\nsteps:\n  - \"Land on the ground.\"\nadditional_preconditions: []\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n"
        );
        let reparsed: Condition = parse_condition(&yaml).unwrap();
        assert_eq!(reparsed.uid, condition.uid);
    }

    #[test]
    fn parses_condition_yaml_without_uid_as_none() {
        let yaml = "id: player-jump-jump-ground\nbehavior: player-jump-jump\nlabel: ground\ndescription: |\n  Jump from the ground and land.\nsteps:\n  - \"Land on the ground.\"\nadditional_preconditions: []\n";

        let condition: Condition = parse_condition(yaml).unwrap();

        assert_eq!(condition.uid, None);
    }

    #[test]
    fn serializes_expected_result_to_deterministic_yaml() {
        let expected = ExpectedResult {
            id: "player-jump-jump-ground-001".to_string(),
            condition: "player-jump-jump-ground".to_string(),
            description: "Lands safely.".to_string(),
            results: vec!["Player is standing on the ground.".to_string()],
            additional_steps: None,
            implementation_note: None,
            generated_by: None,
            verified_by: None,
            uid: None,
        };

        let yaml = serialize_expected_result(&expected);

        assert_eq!(
            yaml,
            "id: player-jump-jump-ground-001\ncondition: player-jump-jump-ground\ndescription: |\n  Lands safely.\nresults:\n  - \"Player is standing on the ground.\"\n"
        );
    }

    #[test]
    fn serializes_expected_result_with_additional_steps_and_implementation_note() {
        let expected = ExpectedResult {
            id: "player-jump-jump-ground-002".to_string(),
            condition: "player-jump-jump-ground".to_string(),
            description: "Still on the ground after reload.".to_string(),
            results: vec!["Player is still on the ground.".to_string()],
            additional_steps: Some(vec!["Reload the page.".to_string()]),
            implementation_note: Some("saveState() persists position to localStorage.".to_string()),
            generated_by: None,
            verified_by: None,
            uid: None,
        };

        let yaml = serialize_expected_result(&expected);

        assert_eq!(
            yaml,
            "id: player-jump-jump-ground-002\ncondition: player-jump-jump-ground\ndescription: |\n  Still on the ground after reload.\nadditional_steps:\n  - \"Reload the page.\"\nresults:\n  - \"Player is still on the ground.\"\nimplementation_note: |\n  saveState() persists position to localStorage.\n"
        );
        let reparsed: ExpectedResult = parse_expected_result(&yaml).unwrap();
        assert_eq!(reparsed.additional_steps, expected.additional_steps);
        assert_eq!(
            reparsed.implementation_note,
            expected.implementation_note.map(|note| format!("{note}\n"))
        );
    }

    #[test]
    fn serializes_expected_result_with_multiline_description_as_valid_yaml() {
        let expected = ExpectedResult {
            id: "player-jump-jump-ground-001".to_string(),
            condition: "player-jump-jump-ground".to_string(),
            description: "line one about foo.js: bar()\nline two about baz.js: qux()\n".to_string(),
            results: vec!["Player is standing on the ground.".to_string()],
            additional_steps: None,
            implementation_note: None,
            generated_by: None,
            verified_by: None,
            uid: None,
        };

        let yaml = serialize_expected_result(&expected);
        let reparsed: ExpectedResult = parse_expected_result(&yaml).unwrap();

        assert_eq!(reparsed.description, expected.description);
    }

    #[test]
    fn serializes_expected_result_with_uid_when_present() {
        let expected = ExpectedResult {
            id: "player-jump-jump-ground-001".to_string(),
            condition: "player-jump-jump-ground".to_string(),
            description: "Lands safely.".to_string(),
            results: vec!["Player is standing on the ground.".to_string()],
            additional_steps: None,
            implementation_note: None,
            generated_by: None,
            verified_by: None,
            uid: Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string()),
        };

        let yaml = serialize_expected_result(&expected);

        assert_eq!(
            yaml,
            "id: player-jump-jump-ground-001\ncondition: player-jump-jump-ground\ndescription: |\n  Lands safely.\nresults:\n  - \"Player is standing on the ground.\"\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n"
        );
        let reparsed: ExpectedResult = parse_expected_result(&yaml).unwrap();
        assert_eq!(reparsed.uid, expected.uid);
    }

    #[test]
    fn parses_expected_result_yaml_without_uid_as_none() {
        let yaml = "id: player-jump-jump-ground-001\ncondition: player-jump-jump-ground\ndescription: |\n  Lands safely.\nresults:\n  - \"Player is standing on the ground.\"\n";

        let expected: ExpectedResult = parse_expected_result(yaml).unwrap();

        assert_eq!(expected.uid, None);
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
