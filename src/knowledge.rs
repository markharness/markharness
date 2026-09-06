use std::collections::BTreeMap;

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
    /// 関連するRequirementのUID配列(ADR 0017 §1・§3)。Featureが関係の正本であり、
    /// Requirementはこの一覧を通じて逆参照される。複数要件への対等な関連付けを表し、
    /// Feature単独で要件全体を満たす証明ではない。表示IDとUIDを同じ照合キーとして
    /// 扱わないため、要素はRequirement.uidの値(ULID)であり、表示IDではない。
    pub requirement_uids: Vec<String>,
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

/// ADR 0017 §2: Behaviorが定義する共通手順。Scenarioの`Phase.steps`が
/// `use: <name>`で明示参照する。先頭への自動挿入はしない。共通手順から
/// 別の共通手順を呼ぶ入れ子は認めない(検証は生成側で行う)。
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Procedure {
    pub steps: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Behavior {
    pub id: String,
    pub feature: String,
    pub label: String,
    pub axis: Vec<String>,
    pub description: String,
    /// ADR 0017 §2: この Behavior に属する Scenario が明示参照できる共通手順。
    /// キーが手順名。決定的シリアライズのため`BTreeMap`(キー昇順)を用いる。
    #[serde(default)]
    pub procedures: BTreeMap<String, Procedure>,
    /// 不変identity(ADR 0013、design/immutable-identity-model-design.md)。
    /// `identity::registry`のreplay結果から書き戻される値であり、未移行の
    /// プロジェクトや`identity migrate`未実行のBehaviorでは`None`(§後方互換)。
    #[serde(default)]
    pub uid: Option<String>,
}

/// ADR 0017 §2: Phase内の1操作。`action`は自由記述の操作、`use`は所属
/// BehaviorのProcedure名への明示参照(展開は生成側が行う)。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum StepItem {
    Action {
        action: String,
    },
    Use {
        #[serde(rename = "use")]
        procedure: String,
    },
}

/// ADR 0017 §2: Scenarioが所有する順序付き操作・確認の単位。実行順の正本は
/// 配列順であり、独立UID・独立ライフサイクルを持たない。
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Phase {
    pub steps: Vec<StepItem>,
    pub results: Vec<String>,
}

/// How a `Scenario`'s content was produced. Omitting the field
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

/// A human review gate on a `Scenario`. Omitting the whole `verified_by`
/// field means not (yet) reviewed; `human_review` is required whenever the
/// object is present (no ambiguous partial state).
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct VerifiedBy {
    pub human_review: bool,
}

/// ADR 0017 §2/§3: Condition と ExpectedResult を統合した単位。
/// **1 Scenario = 1 TestCase**(§3)。CaseUidはScenarioUidから決定的に導出する。
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Scenario {
    pub id: String,
    pub behavior: String,
    pub label: String,
    pub description: String,
    /// 実行順に並んだPhase配列。空配列は生成側で明示エラーとする。
    pub phases: Vec<Phase>,
    /// 実装根拠メモ。生成には使わない。
    #[serde(default)]
    pub implementation_note: Option<String>,
    #[serde(default)]
    pub generated_by: Option<GeneratedBy>,
    #[serde(default)]
    pub verified_by: Option<VerifiedBy>,
    /// 不変identity(ADR 0013、design/immutable-identity-model-design.md)。
    /// `identity::registry`のreplay結果から書き戻される値であり、未移行の
    /// プロジェクトや`identity migrate`未実行のScenarioでは`None`(§後方互換)。
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

pub fn parse_scenario(yaml: &str) -> Result<Scenario, serde_yaml_ng::Error> {
    serde_yaml_ng::from_str(yaml)
}

fn yaml_flow_array(items: &[String]) -> String {
    format!("[{}]", items.join(", "))
}

/// Appends a trailing `uid: <value>\n` line when `uid` is present, shared
/// by every `serialize_*` function (ADR 0013: all persistent Knowledge
/// element kinds carry the same optional `uid:` field, always written last).
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
        "id: {}\nrequirement_uids: {}\nlabel: {}\naxis: {}\n",
        feature.id,
        yaml_flow_array(&feature.requirement_uids),
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
    if behavior.procedures.is_empty() {
        out.push_str("procedures: {}\n");
    } else {
        out.push_str("procedures:\n");
        for (name, procedure) in &behavior.procedures {
            out.push_str(&format!("  {name}:\n"));
            out.push_str("    steps:\n");
            for step in &procedure.steps {
                out.push_str(&format!(
                    "      - {}\n",
                    serde_json::to_string(step).unwrap()
                ));
            }
        }
    }
    append_uid_line(&mut out, &behavior.uid);
    out
}

fn serialize_step_item_inline(item: &StepItem) -> String {
    match item {
        StepItem::Action { action } => {
            format!("action: {}", serde_json::to_string(action).unwrap())
        }
        StepItem::Use { procedure } => format!("use: {procedure}"),
    }
}

fn serialize_phase_block(phase: &Phase) -> String {
    let mut out = String::new();
    out.push_str("  - steps:\n");
    for step in &phase.steps {
        out.push_str(&format!("      - {}\n", serialize_step_item_inline(step)));
    }
    out.push_str("    results:\n");
    for result in &phase.results {
        out.push_str(&format!(
            "      - {}\n",
            serde_json::to_string(result).unwrap()
        ));
    }
    out
}

pub fn serialize_scenario(scenario: &Scenario) -> String {
    let mut out = format!(
        "id: {}\nbehavior: {}\nlabel: {}\ndescription: |\n",
        scenario.id, scenario.behavior, scenario.label
    );
    out.push_str(&indent_block_scalar(&scenario.description, "  "));
    out.push_str("phases:\n");
    for phase in &scenario.phases {
        out.push_str(&serialize_phase_block(phase));
    }
    if let Some(implementation_note) = &scenario.implementation_note {
        out.push_str("implementation_note: |\n");
        out.push_str(&indent_block_scalar(implementation_note, "  "));
    }
    if let Some(generated_by) = &scenario.generated_by {
        let value = match generated_by {
            GeneratedBy::Manual => "manual",
            GeneratedBy::Llm => "llm",
            GeneratedBy::AutoCombination => "auto_combination",
        };
        out.push_str(&format!("generated_by: {value}\n"));
    }
    if let Some(verified_by) = &scenario.verified_by {
        out.push_str("verified_by:\n");
        out.push_str(&format!("  human_review: {}\n", verified_by.human_review));
    }
    append_uid_line(&mut out, &scenario.uid);
    out
}

pub fn strip_redundant_scenario_prefix(feature_id: &str, scenario_id: &str) -> Option<String> {
    let prefix = format!("{feature_id}-");
    scenario_id
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
        let yaml = "id: player-jump\nrequirement_uids: [player-controls]\nlabel: player-jump\naxis: [gameplay, animation]\n";

        let feature: Feature = parse_feature(yaml).unwrap();

        assert_eq!(feature.id, "player-jump");
        assert_eq!(
            feature.requirement_uids,
            vec!["player-controls".to_string()]
        );
        assert_eq!(feature.label, "player-jump");
        assert_eq!(feature.axis, vec!["gameplay", "animation"]);
        assert_eq!(feature.description, None);
    }

    /// ADR 0017 §1: Featureは複数Requirementへ対等に関連付けられる。
    #[test]
    fn parses_feature_yaml_with_multiple_requirement_uids() {
        let yaml = "id: player-jump\nrequirement_uids: [player-controls, player-scoring]\nlabel: player-jump\naxis: [gameplay]\n";

        let feature: Feature = parse_feature(yaml).unwrap();

        assert_eq!(
            feature.requirement_uids,
            vec!["player-controls".to_string(), "player-scoring".to_string()]
        );
    }

    #[test]
    fn serializes_feature_to_deterministic_yaml() {
        let feature = Feature {
            id: "player-jump".to_string(),
            requirement_uids: vec!["player-controls".to_string()],
            label: "player-jump".to_string(),
            axis: vec!["gameplay".to_string(), "animation".to_string()],
            description: None,
            forked_from: None,
            uid: None,
        };

        let yaml = serialize_feature(&feature);

        assert_eq!(
            yaml,
            "id: player-jump\nrequirement_uids: [player-controls]\nlabel: player-jump\naxis: [gameplay, animation]\n"
        );
    }

    #[test]
    fn serializes_feature_with_description_when_present() {
        let feature = Feature {
            id: "player-jump".to_string(),
            requirement_uids: vec!["player-controls".to_string()],
            label: "プレイヤージャンプ".to_string(),
            axis: vec!["gameplay".to_string()],
            description: Some("Jump related behaviors.".to_string()),
            forked_from: None,
            uid: None,
        };

        let yaml = serialize_feature(&feature);

        assert_eq!(
            yaml,
            "id: player-jump\nrequirement_uids: [player-controls]\nlabel: プレイヤージャンプ\naxis: [gameplay]\ndescription: |\n  Jump related behaviors.\n"
        );
    }

    #[test]
    fn serializes_feature_with_multiline_description_as_valid_yaml() {
        let feature = Feature {
            id: "player-jump".to_string(),
            requirement_uids: vec!["player-controls".to_string()],
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
        let yaml = "id: player-double-jump\nrequirement_uids: [player-controls]\nlabel: player-double-jump\naxis: [gameplay]\nforked_from: player-jump\n";

        let feature: Feature = parse_feature(yaml).unwrap();

        assert_eq!(feature.forked_from, Some("player-jump".to_string()));
    }

    #[test]
    fn parses_feature_yaml_without_forked_from_as_none() {
        let feature: Feature = parse_feature(
            "id: player-jump\nrequirement_uids: [player-controls]\nlabel: player-jump\naxis: [gameplay]\n",
        )
        .unwrap();

        assert_eq!(feature.forked_from, None);
    }

    #[test]
    fn serializes_feature_with_forked_from_when_present() {
        let feature = Feature {
            id: "player-double-jump".to_string(),
            requirement_uids: vec!["player-controls".to_string()],
            label: "player-double-jump".to_string(),
            axis: vec!["gameplay".to_string()],
            description: None,
            forked_from: Some("player-jump".to_string()),
            uid: None,
        };

        let yaml = serialize_feature(&feature);

        assert_eq!(
            yaml,
            "id: player-double-jump\nrequirement_uids: [player-controls]\nlabel: player-double-jump\naxis: [gameplay]\nforked_from: player-jump\n"
        );
    }

    /// Backward compatibility (ADR 0013 design doc §2, §11): a `feature.yml`
    /// written before `uid:` existed has no such key and must still parse,
    /// with `uid` defaulting to `None` — not an error, and not confused
    /// with an empty string.
    #[test]
    fn parses_feature_yaml_without_uid_as_none() {
        let feature: Feature = parse_feature(
            "id: player-jump\nrequirement_uids: [player-controls]\nlabel: player-jump\naxis: [gameplay]\n",
        )
        .unwrap();

        assert_eq!(feature.uid, None);
    }

    #[test]
    fn parses_feature_yaml_with_uid() {
        let yaml = "id: task-management\nrequirement_uids: [player-controls]\nlabel: task-management\naxis: [gameplay]\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n";

        let feature: Feature = parse_feature(yaml).unwrap();

        assert_eq!(feature.uid, Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string()));
    }

    #[test]
    fn serializes_feature_with_uid_when_present() {
        let feature = Feature {
            id: "task-management".to_string(),
            requirement_uids: vec!["player-controls".to_string()],
            label: "task-management".to_string(),
            axis: vec!["gameplay".to_string()],
            description: None,
            forked_from: None,
            uid: Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string()),
        };

        let yaml = serialize_feature(&feature);

        assert_eq!(
            yaml,
            "id: task-management\nrequirement_uids: [player-controls]\nlabel: task-management\naxis: [gameplay]\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n"
        );
        let reparsed: Feature = parse_feature(&yaml).unwrap();
        assert_eq!(reparsed, feature);
    }

    fn sample_behavior() -> Behavior {
        Behavior {
            id: "player-jump-jump".to_string(),
            feature: "player-jump".to_string(),
            label: "jump".to_string(),
            axis: vec!["gameplay".to_string()],
            description: "Player presses jump.".to_string(),
            procedures: BTreeMap::new(),
            uid: None,
        }
    }

    #[test]
    fn parses_behavior_yaml() {
        let yaml = "id: player-jump-jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\nprocedures: {}\n";

        let behavior: Behavior = parse_behavior(yaml).unwrap();

        assert_eq!(behavior.id, "player-jump-jump");
        assert_eq!(behavior.feature, "player-jump");
        assert_eq!(behavior.label, "jump");
        assert_eq!(behavior.axis, vec!["gameplay"]);
        assert_eq!(behavior.description, "Player presses jump.\n");
        assert!(behavior.procedures.is_empty());
    }

    #[test]
    fn parses_behavior_yaml_without_procedures_key_as_empty() {
        let yaml = "id: player-jump-jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\n";

        let behavior: Behavior = parse_behavior(yaml).unwrap();

        assert!(behavior.procedures.is_empty());
    }

    #[test]
    fn parses_behavior_yaml_with_procedures() {
        let yaml = "id: checkout-pay\nfeature: checkout\nlabel: pay\naxis: []\ndescription: |\n  Pay.\nprocedures:\n  login:\n    steps:\n      - \"Enter credentials.\"\n      - \"Press the login button.\"\n";

        let behavior: Behavior = parse_behavior(yaml).unwrap();

        assert_eq!(
            behavior.procedures.get("login").unwrap().steps,
            vec![
                "Enter credentials.".to_string(),
                "Press the login button.".to_string()
            ]
        );
    }

    #[test]
    fn serializes_behavior_to_deterministic_yaml() {
        let behavior = sample_behavior();

        let yaml = serialize_behavior(&behavior);

        assert_eq!(
            yaml,
            "id: player-jump-jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\nprocedures: {}\n"
        );
    }

    #[test]
    fn serializes_behavior_with_procedures_sorted_by_name() {
        let mut behavior = sample_behavior();
        behavior.procedures.insert(
            "logout".to_string(),
            Procedure {
                steps: vec!["Press the logout button.".to_string()],
            },
        );
        behavior.procedures.insert(
            "login".to_string(),
            Procedure {
                steps: vec![
                    "Enter credentials.".to_string(),
                    "Press the login button.".to_string(),
                ],
            },
        );

        let yaml = serialize_behavior(&behavior);

        assert_eq!(
            yaml,
            "id: player-jump-jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\nprocedures:\n  login:\n    steps:\n      - \"Enter credentials.\"\n      - \"Press the login button.\"\n  logout:\n    steps:\n      - \"Press the logout button.\"\n"
        );
        let reparsed: Behavior = parse_behavior(&yaml).unwrap();
        assert_eq!(reparsed.procedures, behavior.procedures);
    }

    #[test]
    fn serializes_behavior_with_multiline_description_as_valid_yaml() {
        let mut behavior = sample_behavior();
        behavior.description =
            "line one about foo.js: bar()\nline two about baz.js: qux()\n".to_string();

        let yaml = serialize_behavior(&behavior);
        let reparsed: Behavior = parse_behavior(&yaml).unwrap();

        assert_eq!(reparsed.description, behavior.description);
    }

    #[test]
    fn serializes_behavior_with_uid_when_present() {
        let mut behavior = sample_behavior();
        behavior.uid = Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string());

        let yaml = serialize_behavior(&behavior);

        assert_eq!(
            yaml,
            "id: player-jump-jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\nprocedures: {}\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n"
        );
        let reparsed: Behavior = parse_behavior(&yaml).unwrap();
        assert_eq!(reparsed.uid, behavior.uid);
    }

    #[test]
    fn parses_behavior_yaml_without_uid_as_none() {
        let yaml = "id: player-jump-jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\nprocedures: {}\n";

        let behavior: Behavior = parse_behavior(yaml).unwrap();

        assert_eq!(behavior.uid, None);
    }

    fn sample_scenario() -> Scenario {
        Scenario {
            id: "player-jump-jump-ground".to_string(),
            behavior: "player-jump-jump".to_string(),
            label: "ground".to_string(),
            description: "Jump from the ground and land.".to_string(),
            phases: vec![Phase {
                steps: vec![StepItem::Action {
                    action: "Land on the ground.".to_string(),
                }],
                results: vec!["Player is standing on the ground.".to_string()],
            }],
            implementation_note: None,
            generated_by: None,
            verified_by: None,
            uid: None,
        }
    }

    #[test]
    fn parses_scenario_yaml() {
        let yaml = "id: player-jump-jump-ground\nbehavior: player-jump-jump\nlabel: ground\ndescription: |\n  Jump from the ground and land.\nphases:\n  - steps:\n      - action: \"Land on the ground.\"\n    results:\n      - \"Player is standing on the ground.\"\n";

        let scenario: Scenario = parse_scenario(yaml).unwrap();

        assert_eq!(scenario.id, "player-jump-jump-ground");
        assert_eq!(scenario.behavior, "player-jump-jump");
        assert_eq!(scenario.label, "ground");
        assert_eq!(scenario.description, "Jump from the ground and land.\n");
        assert_eq!(scenario.phases.len(), 1);
        assert_eq!(
            scenario.phases[0].steps,
            vec![StepItem::Action {
                action: "Land on the ground.".to_string()
            }]
        );
        assert_eq!(
            scenario.phases[0].results,
            vec!["Player is standing on the ground.".to_string()]
        );
    }

    /// ADR 0017 §2: PhaseはBehaviorの共通手順を`use:`で明示参照できる。
    #[test]
    fn parses_scenario_yaml_with_a_use_step_and_multiple_phases() {
        let yaml = "id: checkout-pay-valid-card\nbehavior: checkout-pay\nlabel: valid-card\ndescription: |\n  Pay then log out and back in.\nphases:\n  - steps:\n      - use: login\n    results:\n      - \"My page is shown.\"\n  - steps:\n      - action: \"Log out.\"\n      - use: login\n    results:\n      - \"My page is shown again.\"\n";

        let scenario: Scenario = parse_scenario(yaml).unwrap();

        assert_eq!(scenario.phases.len(), 2);
        assert_eq!(
            scenario.phases[0].steps,
            vec![StepItem::Use {
                procedure: "login".to_string()
            }]
        );
        assert_eq!(
            scenario.phases[1].steps,
            vec![
                StepItem::Action {
                    action: "Log out.".to_string()
                },
                StepItem::Use {
                    procedure: "login".to_string()
                }
            ]
        );
    }

    #[test]
    fn serializes_scenario_to_deterministic_yaml() {
        let scenario = sample_scenario();

        let yaml = serialize_scenario(&scenario);

        assert_eq!(
            yaml,
            "id: player-jump-jump-ground\nbehavior: player-jump-jump\nlabel: ground\ndescription: |\n  Jump from the ground and land.\nphases:\n  - steps:\n      - action: \"Land on the ground.\"\n    results:\n      - \"Player is standing on the ground.\"\n"
        );
    }

    #[test]
    fn serializes_scenario_with_a_use_step() {
        let mut scenario = sample_scenario();
        scenario.phases = vec![
            Phase {
                steps: vec![StepItem::Use {
                    procedure: "login".to_string(),
                }],
                results: vec!["My page is shown.".to_string()],
            },
            Phase {
                steps: vec![
                    StepItem::Action {
                        action: "Log out.".to_string(),
                    },
                    StepItem::Use {
                        procedure: "login".to_string(),
                    },
                ],
                results: vec!["My page is shown again.".to_string()],
            },
        ];

        let yaml = serialize_scenario(&scenario);
        let reparsed: Scenario = parse_scenario(&yaml).unwrap();

        assert_eq!(reparsed.phases, scenario.phases);
        assert!(yaml.contains("      - use: login\n"));
    }

    #[test]
    fn serializes_scenario_with_multiline_description_as_valid_yaml() {
        let mut scenario = sample_scenario();
        scenario.description =
            "line one about foo.js: bar()\nline two about baz.js: qux()\n".to_string();

        let yaml = serialize_scenario(&scenario);
        let reparsed: Scenario = parse_scenario(&yaml).unwrap();

        assert_eq!(reparsed.description, scenario.description);
    }

    #[test]
    fn serializes_scenario_with_implementation_note_when_present() {
        let mut scenario = sample_scenario();
        scenario.implementation_note =
            Some("saveState() persists position to localStorage.".to_string());

        let yaml = serialize_scenario(&scenario);

        assert_eq!(
            yaml,
            "id: player-jump-jump-ground\nbehavior: player-jump-jump\nlabel: ground\ndescription: |\n  Jump from the ground and land.\nphases:\n  - steps:\n      - action: \"Land on the ground.\"\n    results:\n      - \"Player is standing on the ground.\"\nimplementation_note: |\n  saveState() persists position to localStorage.\n"
        );
        let reparsed: Scenario = parse_scenario(&yaml).unwrap();
        assert_eq!(
            reparsed.implementation_note,
            scenario.implementation_note.map(|note| format!("{note}\n"))
        );
    }

    #[test]
    fn serializes_scenario_with_uid_when_present() {
        let mut scenario = sample_scenario();
        scenario.uid = Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string());

        let yaml = serialize_scenario(&scenario);

        assert_eq!(
            yaml,
            "id: player-jump-jump-ground\nbehavior: player-jump-jump\nlabel: ground\ndescription: |\n  Jump from the ground and land.\nphases:\n  - steps:\n      - action: \"Land on the ground.\"\n    results:\n      - \"Player is standing on the ground.\"\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n"
        );
        let reparsed: Scenario = parse_scenario(&yaml).unwrap();
        assert_eq!(reparsed.uid, scenario.uid);
    }

    /// A round trip (parse -> serialize) must not silently drop
    /// `generated_by`/`verified_by` — `identity::knowledge_walk::write_id_and_uid`
    /// (used by `identity migrate`/`rename`/`resolve_divergence`) performs
    /// exactly this round trip on every Scenario it touches.
    #[test]
    fn serializes_scenario_with_generated_by_and_verified_by_when_present() {
        let mut scenario = sample_scenario();
        scenario.generated_by = Some(GeneratedBy::Llm);
        scenario.verified_by = Some(VerifiedBy { human_review: true });

        let yaml = serialize_scenario(&scenario);

        assert_eq!(
            yaml,
            "id: player-jump-jump-ground\nbehavior: player-jump-jump\nlabel: ground\ndescription: |\n  Jump from the ground and land.\nphases:\n  - steps:\n      - action: \"Land on the ground.\"\n    results:\n      - \"Player is standing on the ground.\"\ngenerated_by: llm\nverified_by:\n  human_review: true\n"
        );
        let reparsed: Scenario = parse_scenario(&yaml).unwrap();
        assert_eq!(reparsed.generated_by, scenario.generated_by);
        assert_eq!(reparsed.verified_by, scenario.verified_by);
    }

    #[test]
    fn parses_scenario_yaml_without_uid_as_none() {
        let yaml = "id: player-jump-jump-ground\nbehavior: player-jump-jump\nlabel: ground\ndescription: |\n  Jump from the ground and land.\nphases:\n  - steps:\n      - action: \"Land on the ground.\"\n    results:\n      - \"Player is standing on the ground.\"\n";

        let scenario: Scenario = parse_scenario(yaml).unwrap();

        assert_eq!(scenario.uid, None);
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
    fn strips_redundant_prefix_when_scenario_id_starts_with_feature_id() {
        assert_eq!(
            strip_redundant_scenario_prefix("player-jump", "player-jump-ground"),
            Some("ground".to_string())
        );
    }

    #[test]
    fn does_not_strip_when_scenario_id_has_no_matching_prefix() {
        assert_eq!(
            strip_redundant_scenario_prefix("player-jump", "jump-ground"),
            None
        );
    }

    #[test]
    fn does_not_strip_when_remainder_would_be_empty() {
        assert_eq!(
            strip_redundant_scenario_prefix("player-jump", "player-jump-"),
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
