# 0031: ScenarioにRequirementへの`contributes_to`を追加する

## ステータス

Accepted(2026-09-15決定)。[0017](0017-scenario-case-revision-and-execution-evidence.md)がFeature単位で定めたRequirementとの関連(`feature.requirement_uids`)を、Scenario単位でも持てるように拡張する。

## 背景

StrictDocを用いる実運用の事例(利用者が実際にコマンドを実行して構築したサンプルプロジェクトの報告。本ADRはそのプロジェクトへのリンク・言及を含めない)で、次の実害が確認された。

1つのFeatureが複数のRequirementに`contributes_to`する場合(例: 「タスク管理」Featureが「タスクの管理」と「タスクの永続化」という2つのRequirementに関連する)、そのFeature配下で生成される**全てのTestCase**が、実際の検証内容に関わらず両方のRequirementの`generated_from.requirement_uids`を機械的に持ってしまう。具体的には、「空白入力の抑止」を検証するTestCaseが「永続化」のRequirementとも関連付けられ、逆に「破損データからの復旧」を検証するTestCaseが「タスク管理」のRequirementとも関連付けられる、という誤った対応関係が生じた。

これはNorth Starの問い1(「どの機能に関連するテストケースか、どの要件に関連するかを確認する」)に不正確に答える結果になる。StrictDocのLow-Level Requirement(LLR)は、High-Level Requirement(HLR)よりも細かく、1つの具体的な振る舞い(=1 Scenario、ADR 0017§3の「1 Scenario = 1 TestCase」と同じ粒度)に対応する形で書かれることが一般的であり、Feature単位の関連だけではこの粒度を表現できない。

## 決定

### 1. `Scenario`に`requirement_uids: Vec<String>`を追加する

`Feature.requirement_uids`と同じ型・同じ意味を持つフィールドを`Scenario`(`src/knowledge.rs`)に追加する。`#[serde(default)]`とし、省略時は空配列になる(本コードベースの追加的フィールドの既存パターンに従う。必須制約は型ではなく個別のバリデーションルールに委ねる)。

### 2. markharnessはRequirementの階層(HLR/LLR)を知らない

StrictDoc側でRequirement同士がParent/Child関係(HLR/LLRの階層)を持っていても、markharnessはそれを一切保持・解釈しない。StrictDocの`[REQUIREMENT]`が1個あれば、それがHLRであろうとLLRであろうと関係なく、markharness側では同じ形の1個の`Requirement`(`source: external`であれば`source_key`にそのUIDを保持)になるだけである(P5「Coreは外部形式を知らない」)。Requirement同士の関係(親子含む)は[0017](0017-scenario-case-revision-and-execution-evidence.md)が既に「別の未決定事項」としており、本ADRもその立場を維持する。将来必要になれば別ADRで検討する。

### 3. FeatureとScenario、両方に関連が書かれた場合の合成規則

生成されるTestCaseの`generated_from.requirement_uids`は次の規則で決める。

- Scenarioの`requirement_uids`が1件以上あれば、**Scenario側の値だけ**を使う(Feature側は無視する)。
- Scenarioの`requirement_uids`が空であれば、Feature側の`requirement_uids`をそのまま使う(フォールバック)。

和集合(常にFeatureとScenarioを合算する)は採用しない。目的は「TestCaseごとに正確なRequirementを答える」ことであり、精度の高い情報(Scenario)がある場面で粗い情報(Feature)を混ぜる理由がないためである。Feature側の関連は、Scenario単位に分解していないFeatureのための後方互換的なフォールバックとして機能し続ける。

### 4. FeatureとScenarioの内容一致は検証しない

Scenarioが参照するRequirement UIDがFeatureの`requirement_uids`に含まれていなくても、`validate`はこれを検出・拒否しない。FeatureとScenarioの関連は独立した情報源として扱い、整合性は著者の責任に委ねる。この制約は実際の必要性が確認されるまで追加しない(YAGNI)。

### 5. Knowledge Intentのフィールド名は`contributes_to`で統一する

`RequirementIntent`と同様、`ScenarioIntent`にも`contributes_to: [<requirement key or uid>]`を追加する(保存後のフィールド名は`requirement_uids`)。Feature側の既存フィールドと同じ命名にすることで、利用者が覚えるルールを増やさない。

### 6. `coverage`のTestCase判定ロジックをFeatureメンバーシップから直接照合へ変更する

現状の`coverage.rs`(`features_for_requirement`)は、あるRequirementに`contributes_to`するFeatureの集合を求め、その**Featureに属しているというだけ**でTestCaseを「このRequirementをカバーしている」と扱っていた。`case.generated_from.requirement_uids`は一切参照していなかった。

これを、Feature単位のgap検出(`RequirementHasNoFeature`:どのFeatureも`contributes_to`していないRequirementの検出)は維持しつつ、個々のTestCaseとRequirementの対応判定は、Featureの集合によるゲートを外し、**全TestCaseを直接`case.generated_from.requirement_uids`で照合する**独立したロジックに置き換える。これにより、あるFeatureが`contributes_to`していないRequirementであっても、そのFeature配下のいずれかのScenarioが単独で`contributes_to`していれば正しく検出できる。

Scenario単位の上書きを1件も使わない場合(本ADR以前からの使い方全て)、この判定は現状と数学的に同値になる。`generated_from.requirement_uids`はフォールバックによりFeatureの`requirement_uids`をそのまま引き継ぐため、「TestCaseがそのFeatureに属している」ことと「TestCaseの`generated_from.requirement_uids`にそのRequirementが含まれている」ことが一致するからである。したがって本変更は`coverage`の既存の振る舞いを壊すものではなく、より安全側(不正確な対応を排除する側)へ倒す実装である。`impact.rs`は既に`case.generated_from.requirement_uids`を直接見ているため、追加の修正は不要である。

### 7. `schema_version`は変更しない

プロトタイプ期であるため、[markharness-v2-design.md](../design/markharness-v2-design.md) §9.2.2の既存方針([0018](0018-identity-schema-version-freeze.md)・[0030](0030-external-requirement-source-key.md)と同じ考え方)により、フィールド追加のみを理由に`schema_version`を進めない。実際に比較・互換性ゲートを実装する必要が生じるまで値は`1`のまま固定する。

## 影響範囲

- `src/knowledge.rs`(`Scenario`構造体・YAMLシリアライズ)、`src/generate.rs`(TestCase生成時の合成規則)、`src/knowledge_reconcile/`(Intentスキーマ・plan構築)、`src/coverage.rs`(TestCase判定ロジック)、`schema/scenario.schema.json`を変更する。
- `src/impact.rs`・`src/alignment.rs`(`Spec-Reviewed`トレーラー)は変更不要(前者は既に`generated_from.requirement_uids`を直接参照、後者はRequirement/Case idを直接指定する独立した仕組みのため無関係)。
- **本ADRの根拠となった実運用サンプルの反映は本ADR・本実装のスコープに含めない。** markharness本体側の変更を先に完了させ、動作確認は完了後に別途行う。

## 検討したが採用しない選択肢

- **`Behavior`単位で関連を持たせる**: 実運用サンプルでは、1つのBehavior(例:「タスクの追加」)が複数のScenario(有効な入力/空白入力)を持ち、それぞれ別のRequirementに対応する例が複数確認された。Behavior単位ではこの粒度を表現できず、今回の実害が解消されない。
- **StrictDocのRequirement階層(HLR/LLRとParent関係)をmarkharnessのドメインモデルに取り込む**: [0017](0017-scenario-case-revision-and-execution-evidence.md)が既に対象外としている領域であり、P5(Coreは外部形式を知らない)にも反する。今回の実害はRequirement階層を知らなくてもScenario単位の関連だけで解消できるため、必要性がない。
- **FeatureとScenarioの関連を常に和集合にする**: 実装は単純だが、Feature側に広い関連を残したまま一部のScenarioだけ精密化する、という段階的な移行を行った場合に、和集合によって不正確な対応が生き残ってしまう。今回の実害はまさにこのパターンで発生した。
- **FeatureとScenarioの整合性チェックを追加する**: 実際に起きた問題は「両方書かれていて食い違う」ケースではなく「Feature側にしか書けず精密化できない」ケースであり、整合性チェックを追加する具体的な動機が無い。
