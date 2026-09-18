# CLIリードモデル設計

ステータス：Draft

## 1. 目的

本書は、`markharness` CLIが外部へ公開する読み取り用データモデル（リードモデル）を定義する。主な利用者は、将来別リポジトリで実装する`markharness-view`などの表示ツールである。

本設計では、表示画面の構造やUIの都合を`markharness`へ持ち込まない。`markharness`は、Git・Knowledge・関連付け・判定結果から、再現可能な読み取りモデルを生成する。`impact`・`coverage`(および新設する`traceability`)は、各コマンド専用モジュールが構造体を組み立て、`cli.rs`から`serde_json`で直接JSONへシリアライズする(§8)。現状これらのコマンドに人間可読出力(Human Presenter相当)は実装されていない。追加する場合も同じ構造体から生成する。

```text
Knowledge / Git / Binding
          ↓
Domainの判定
          ↓
CLIリードモデル(各コマンド専用モジュールの構造体)
          ↓
JSON(serde_json直接シリアライズ)
          ↓
CLI / markharness-view
```

## 2. 背景と方針

`markharness` v2は、Change ImpactとRelease CoverageをCLI/JSONで提供し、ビューやダッシュボードは別ツールに委ねる方針である。したがって、CLIのJSON出力は単なるログではなく、外部ツールが読むための明示的な出力契約として設計する。

一方、リードモデルはDomainモデルやUIモデルと同一ではない。

- Domainモデル：知識や判定の意味を表す
- CLIリードモデル：外部の読み取り者が必要とする結果を表す
- UIモデル：画面の表示状態や操作状態を表す

本書で扱うのは、中央のCLIリードモデルである。

## 3. 設計原則

### 3.1 リードモデルをJSON出力のseamにする

Domain型を直接シリアライズしない。各コマンド専用モジュール(`impact`・`coverage`・`traceability`)がリードモデルの構造体を生成し、それを`serde_json`でシリアライズする。人間可読出力を追加する場合も、Domain型ではなく同じリードモデルの構造体から生成する。

これにより、Domainの内部構造を変更しても、外部出力契約への影響を局所化できる。

### 3.2 UIモデルにしない

リードモデルに画面タブ、選択状態、展開状態、ソート状態、ページング状態などを含めない。これらは`markharness-view`の責務である。

### 3.3 問いごとに分ける

すべてのKnowledgeを一つの巨大な`KnowledgeReadModel`へ詰め込まない。利用者の問いに対応する、複数の小さなリードモデルを定義する。

初期対象は次の3つとする。

1. `TraceabilityReadModel`
2. `ChangeImpactReadModel`
3. `ReleaseCoverageReadModel`

### 3.4 実行時に生成する

初期実装では、リードモデルを`.markharness/`へ新しい正本ファイルとして保存しない。コマンド実行時にKnowledge・Git・Binding等から生成し、標準出力へJSONとして出力する。

キャッシュや永続的な派生物が必要になった場合は、別の設計判断として扱う。

### 3.5 弱い事実を強い事実へ読み替えない

`ExecutionBinding`があることを、テストが実行済み・合格済みであることとして出力しない。`ReleaseScope`も、実行計画や実行結果として表現しない。

## 4. 共通エンベロープ

`record_kind`/`schema_version`は新設するエンベロープ形式ではない。`impact`(`src/impact.rs`の`ChangeImpact`)・`coverage`(`src/coverage.rs`の`ReleaseCoverage`)が、`CommandOutcome`/`Presenter`(`src/presentation.rs`)を一切経由せず、`cli.rs`から`serde_json`で直接出力している既存の形式(`{"schema_version": 1, "record_kind": "<tag>", ...フィールド}`)をそのまま踏襲する。`record_kind`は`markharness-v2-design.md`§9.1・ADR 0025が既に確立している永続レコード(`execution_binding`・`release_scope`・`requirement`等)の命名規約と同じ名前であり、`impact`/`coverage`のCLI出力も既にこの規約に従っている(ADR 0032参照)。

本書が新設するのは、この既存パターンに従う`traceability`だけである。`canonical_imported`/`generated`/`changes_computed`(`markharness changes compute`)は`CommandOutcome`/`Presenter`系統の別の出力形式(`outcome`フィールド)を使う、無関係な書込み系コマンドであり、本書のリードモデルの対象外とする。

```json
{
  "record_kind": "change_impact",
  "schema_version": 1
}
```

### 4.1 `record_kind`

レコードの種類を識別する。リードモデル3種の値は次のとおりとする(`change_impact`・`release_coverage`は`impact`・`coverage`が既に出力している値)。

```text
traceability
change_impact
release_coverage
```

### 4.2 `schema_version`

初期値は`1`とする。これは過去形式を読み分けるための互換機構ではなく、異なるレコード種類や将来の別モデルとの取り違えを防ぐための識別情報である。

過去形式の読み取りが必要になった場合は、既存モデルへ分岐を追加するのではなく、別のレコード種類または別のReaderとして設計する。

## 5. TraceabilityReadModel

### 5.1 目的

Requirement・Feature・Behavior・Scenario・TestCaseの関係を、外部ツールが閲覧できる形で提供する。

`--at <ref>`を省略した場合は作業ツリーを読み、指定した場合はそのGit ref時点を読む(ADR 0033)。`impact`・`coverage`と異なり、`traceability`には2点比較やリリース監査の要件がなく、`generate`・`verify`と同じく作業ツリーを直接読むことに支障がないためである。

### 5.2 構造(実装済み: `src/traceability.rs`)

```rust
struct TraceabilityReadModel {
    schema_version: u32,        // "record_kind": "traceability" と共に出力
    record_kind: &'static str,
    at: String, // "--at"省略時は固定値"working-tree"。指定時は指定文字列そのまま(ADR 0033)
    requirements: Vec<RequirementNode>,
    features: Vec<FeatureNode>,
    behaviors: Vec<BehaviorNode>,
    scenarios: Vec<ScenarioNode>,
    test_cases: Vec<TestCaseNode>,
    relations: Vec<TraceabilityRelation>,
}
```

各Nodeには、少なくとも表示IDとUIDを含める(UIDは`identity migrate`未実行の要素では`None`になりうる)。テストケースには、さらにCase revisionと生成パスを含める。

`traceability`の目的は「関係を外部ツールが**閲覧できる**形で提供する」ことであり(§5.1)、識別子(`*_id`/`*_uid`)だけでは閲覧できない。そのため、各Nodeには人間可読な`label`も含める。Feature・Behavior・Scenarioの`label`はKnowledge上で必須のためNoneにならない(`String`)。Requirementの`label`はnativeの場合のみ存在する(ADR 0023によりexternalはmarkharnessが内容を所有しないため、`source_locator`/`source_key`とは非対称に、externalでは常に`None`のまま追加しない)。

```rust
struct RequirementNode {
    requirement_id: String,
    requirement_uid: Option<String>,
    source: &'static str, // "native" | "external"
    // source: "native"の場合のみ値を持つ。externalはmarkharnessが内容を
    // 所有しないため(ADR 0023)、代表テキストを持たせない。
    label: Option<String>,
    // source: "external" の場合のみ値を持つ(ADR 0023)。StrictDoc等の実データを
    // 参照する手段。source: "native" では常にNone。
    source_locator: Option<String>, // 参照する.sdocファイルのリポジトリ内パス
    source_key: Option<String>,     // StrictDocのMID(ADR 0030)
}

struct FeatureNode {
    feature_id: String,
    feature_uid: Option<String>,
    label: String,
}

struct BehaviorNode {
    behavior_id: String,
    // 現状の実装では常にNone。生成済みTestCase(KnowledgeCaseSnapshot)は
    // Behavior UIDを保持しておらず、これを得るには別途behavior.ymlを
    // 読む経路が必要。具体的な必要性が確認されるまで追加しない(YAGNI)。
    behavior_uid: Option<String>,
    feature_id: String,
    label: String,
}

struct ScenarioNode {
    scenario_id: String,
    scenario_uid: Option<String>,
    behavior_id: String,
    label: String,
}

struct TestCaseNode {
    case_id: String,             // 表示ID。generate.rs等の既存コードと同じ用語(TestCase.case_id)に揃える
    case_uid: Option<CaseUid>,
    case_revision: CaseRevision, // ハッシュ値の文字列型。u64ではない
    relative_path: String,
    scenario_id: String,
}
```

関係は、各Nodeに相手側の配列を重複して持たせず、明示的なRelationとして表す。

```rust
enum RelationKind {
    ContributesTo, // Feature または Scenario から Requirement へ
    GeneratedFrom, // TestCase から Scenario へ
}

struct TraceabilityRelation {
    from_uid: String,
    to_uid: String,
    kind: RelationKind,
}
```

UIDを持たない要素(未migrate)は、`relations`のいずれの側にも現れない。UIDのない値同士を関連付けても、外部の読み取り側が再実行間で同一性を確認できないためである。

例：

```json
{
  "from_uid": "feature-uid-1",
  "to_uid": "requirement-uid-1",
  "kind": "contributes_to"
}
```

`TraceabilityReadModel`には、Bindingの有無やCoverageの判定結果を含めない。それらは`ReleaseCoverageReadModel`の責務とする。

`requirements`・`features`はKnowledgeに存在する全件を対象とする(生成されたTestCaseの有無を問わない。`coverage`のAC21と同じ理由で、対応するTestCaseがまだ無いFeatureも可視化する)。`behaviors`・`scenarios`・`test_cases`は生成される全TestCaseから導出する。空のPhaseを持つScenarioは`generate`が生成時に拒否するため、実在するScenarioは必ず1件のTestCaseに対応し、この導出に抜け漏れは生じない。

### 5.4 編集用Intentとの関係

`TraceabilityReadModel`は読み取り用であり、これを直接Knowledgeファイルへ書き戻すための形式にはしない。既存要素を修正する場合は、読み取り結果から最小限のKnowledge Intentを生成し、`knowledge reconcile`へ渡す。

既存要素は`uid`で選択する。

```yaml
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: 01J8Z...
    id: todo-create
    label: Create a TODO
    contributes_to: [01J8A..., 01J8B...]
```

このIntentの意味は次のとおりである。

- `uid`を指定した要素は既存要素の更新対象になる。
- `uid`を持たない新規要素は、Intent内のローカルな`key`で相互参照する。
- Intentで省略した単一値フィールドは現在値を維持する。
- `axis`・`contributes_to`・`procedures`などのcollectionは、指定した場合に限り全置換する。
- collectionを空配列で指定した場合は、現在値を空にする。
- `id`を変更した場合は、UIDを維持したrenameになる。
- `mode: merge`では、Intentに含まれない既存要素を削除しない。

したがって、GUIが編集用Intentを生成する際は、少なくとも次を含める。

1. 編集対象のUID
2. 利用者が変更した単一値フィールド
3. 変更対象にしたcollectionの全内容

全KnowledgeをIntentへ無条件に展開する必要はない。未変更のフィールドを省略することで、Intentを小さく保ち、意図しない更新を避ける。

GUIからの適用は次の経路とする。

```text
TraceabilityReadModel
          ↓
編集対象UID付きの最小Intent
          ↓
knowledge reconcile --check
          ↓
利用者の確認
          ↓
knowledge reconcile
```

`--check`後にKnowledgeやidentity stateが変化していた場合、通常実行はstale planとして停止する。GUIは最新のリードモデルを再取得し、編集内容を再確認してからIntentを再生成する。GUIやviewがKnowledgeファイルを直接書き換えてはならない。

## 6. ChangeImpactReadModel(`impact`は実装済み)

### 6.1 目的

`impact`コマンドが算出した、変更されたRequirement・関連するFeature・TestCase、および対応確認状態を提供する。

### 6.2 構造(既存の`impact::ChangeImpact`)

`ChangeImpactReadModel`は新規設計ではなく、`src/impact.rs`の`ChangeImpact`を指す。既に`record_kind`/`schema_version`付きで実装・出力されている。

```rust
struct ChangeImpact {
    schema_version: u32,       // "record_kind": "change_impact" と共に既に出力
    record_kind: &'static str,
    rule_version: u32,
    base_commit: String,
    head_commit: String,
    requirements: Vec<RequirementImpact>, // 各RequirementのFeature・TestCase対応を含む
    stale_pins: Vec<StalePin>,
    rejected_trailers: Vec<RejectedTrailerReport>,
}

struct RequirementImpact {
    requirement_id: String,
    requirement_uid: Option<String>,
    source: &'static str,   // "native" | "external"
    spec_changed: bool,
    feature_ids: Vec<String>,
    cases: Vec<CaseAlignment>,
}

struct CaseAlignment {
    case_id: String,
    case_uid: Option<String>,
    case_changed: bool,
    status: AlignmentStatus, // Confirmed | FollowedUp | Unconfirmed
}
```

`Spec-Reviewed` trailerによる対応確認状態(`AlignmentStatus`)は、次の区別を維持する(ADR 0019)。

```text
confirmed      その組に対する有効なSpec-Reviewedがある
followed_up    区間内で両側が変更されたが、確認の記録は無い
unconfirmed    仕様側が変更され、追随した形跡も確認の記録も無い
```

### 6.3 markharness-viewから見た`impact`

`impact --base <ref> --head <ref> --format json`は、件数だけの簡易結果ではなく、`ChangeImpact`全体(各RequirementのFeature・TestCase対応、対応確認状態)を既に返している。`markharness-view`はこの出力をそのまま読み取り対象にできる。追加の実装は不要である。

## 7. ReleaseCoverageReadModel(`coverage`は実装済み)

### 7.1 目的

指定されたRequirement集合とGit refにおける、Feature・TestCase・検証手段・coverage gap・ReleaseScopeの選定状態を提供する。

### 7.2 構造(既存の`coverage::ReleaseCoverage`)

`ReleaseCoverageReadModel`は新規設計ではなく、`src/coverage.rs`の`ReleaseCoverage`を指す。既に`record_kind`/`schema_version`付きで実装・出力されている。

```rust
struct ReleaseCoverage {
    schema_version: u32,       // "record_kind": "release_coverage" と共に既に出力
    record_kind: &'static str,
    rule_version: u32,
    at_commit: String,
    requirements: Vec<RequirementCoverage>,
    gaps: Vec<CoverageGap>,
    release: Option<ReleaseView>, // releaseを指定しなかった場合は省略(フィールド自体が出力されない)
}

struct RequirementCoverage {
    requirement_id: String,
    requirement_uid: Option<String>,
    source: &'static str,   // "native" | "external"
    feature_ids: Vec<String>,
    cases: Vec<CaseCoverage>,
}

struct CaseCoverage {
    case_id: String,
    case_uid: Option<String>,
    feature_id: String,
    binding_mode: Option<String>,       // 未宣言なら省略
    binding_reference: Option<String>,  // 未宣言なら省略
    selected: Option<bool>,             // releaseを指定した場合のみ意味を持つ
}

struct CoverageGap {
    kind: GapKind, // "requirement_has_no_feature" | "feature_has_no_case"
    requirement_id: String,
    feature_id: Option<String>,
}

struct ReleaseView {
    release_id: String,
    selected_case_uids: Vec<String>,
    unselected_case_uids: Vec<String>,
    absent_case_uids: Vec<String>,
}
```

`binding_mode`/`binding_reference`は、`ExecutionBinding`(検証手段の宣言)をそのまま表す。これらの値を「実行済み」「合格済み」と表示してはならない。合否・実行日時・実行者・実行環境は別ツールの責務である(ADR 0025 §1)。「選定された(`selected`)」と「実行された」も混同しない(ADR 0024 §5)。

### 7.3 markharness-viewから見た`coverage`

`coverage --requirements <ids-or-all> [--release <id>] --at <ref> --format json`は、上記`ReleaseCoverage`全体を既に返している。`markharness-view`はこの出力をそのまま読み取り対象にできる。追加の実装は不要である。

## 8. CLI内部の結果との関係

既存の`CommandOutcome`(`src/presentation.rs`)は、`CanonicalImported`・`Generated`・`ChangesComputed`の3variantを持つ型で、`canonical_import`・`generate`・`changes compute`という**書込み系**コマンドの処理結果を表す。`Presenter`(`HumanPresenter`・`JsonPresenter`)がこれをシリアライズする。

リードモデルはこの`CommandOutcome`を拡張しない。`impact`(`ChangeImpact`)・`coverage`(`ReleaseCoverage`)は、そもそも`CommandOutcome`/`Presenter`を経由せず、各コマンド専用のモジュールが自分の構造体を`cli.rs`から直接`serde_json`でシリアライズしている。新設する`traceability`(`TraceabilityReadModel`)も、この`impact`/`coverage`と同じ経路に合わせる(ADR 0032)。

なお`src/traceability.rs`という名前は、`generate`が組み立てる生成アーティファクト`TraceabilityIndex`(`.markharness/generated/traceability-index.json`。`verify`が再生成の決定性確認に使う)が既に使っている。両者は生成物か問い合わせ結果かという点を含め責務が異なるため、既存ファイルを`src/traceability_index.rs`へリネームし、空いた`src/traceability.rs`を新設の`traceability`コマンド用に使う(ADR 0032 決定2)。

```text
CommandOutcome(書込み系。CanonicalImported / Generated / ChangesComputed)
  → Presenter(Human / Json)

読み取り系コマンドの専用モジュール(CommandOutcomeとは無関係)
  impact::ChangeImpact                     → 実装済み。cli.rsからserde_jsonで直接出力
  coverage::ReleaseCoverage                → 実装済み。cli.rsからserde_jsonで直接出力
  traceability::TraceabilityReadModel      → 新設。同じ経路に合わせる
  traceability_index::TraceabilityIndex    → 既存(旧traceability.rsからリネーム)。generateの生成アーティファクト。読み取り系コマンドではない
```

## 9. コマンドとの対応

初期の対応は次のとおりとする。

```text
markharness traceability [--at <ref>] --format json
  → TraceabilityReadModel(--at省略時は作業ツリー、指定時はGit ref。ADR 0033)

markharness impact --base <ref> --head <ref> --format json
  → ChangeImpactReadModel

markharness coverage --requirements <ids-or-all> [--release <id>] --at <ref> --format json
  → ReleaseCoverageReadModel
```

`traceability`は新設する。既存の`generate`は生成物を書き込むコマンドであり、生成処理の成功結果とKnowledgeの閲覧結果を同じコマンドへ混ぜない。`traceability`は読み取り専用とし、`--at`を指定すればそのGit ref時点、省略すれば作業ツリーのKnowledgeと生成済みTestCaseを読む。`impact`・`coverage`は2点比較・リリース監査という性質上`--at`(または`--base`/`--head`)を必須のままとする(ADR 0033)。

`impact`と`coverage`は、`ChangeImpactReadModel`・`ReleaseCoverageReadModel`全体を返す`--format json`実装が既に完了している。両コマンドとも現状`--format`の値は`json`のみで、人間可読出力は未実装である。人間可読出力を追加する場合も、同じ構造体から生成する。

コマンド名とJSONの`record_kind`は一致させるが、JSON契約の識別はコマンド名ではなく`record_kind`と`schema_version`で行う。

## 10. 永続化しない範囲

初期リードモデルでは、次を新しい正本ファイルとして保存しない。

- `.markharness/`配下のKnowledge全体の複製
- 画面別の表示状態
- 検索インデックス
- UIのキャッシュ
- 実行結果や証跡

既存のKnowledge、Binding、ReleaseScope、Git履歴を入力として、必要な時点で決定的に生成する。

## 11. 検証方針

リードモデルは、次の観点でテストする。

1. 同じ入力から同じJSONが生成される。
2. `record_kind`と`schema_version`が必ず出力される。
3. 人間可読出力を追加した場合、JSON出力と同じ判定結果を表示する。
4. UID、Case revision、Git refが結果から失われない。
5. `ExecutionBinding`を実行結果と誤認させる表現がない。
6. Requirementのsourceがnativeかexternalかという意味が失われない。externalの場合、`source_locator`・`source_key`により、StrictDoc等の実データへ実際に辿り着けること(区別が付くだけでは不十分)。
7. 未知の任意フィールドを追加しても、既存の読み取り側が必須フィールドを処理できる。

JSON fixtureをリードモデル単位で用意し、viewリポジトリが参照できる出力例としても利用する。

fixtureは`tests/fixtures/read-models/<record_kind>/v1/`に置く。markharness側のfixtureを各リードモデルの正規例とし、CLI統合テストで実際の出力と一致することを検証する。

## 12. 後から追加するもの

次のモデルは、viewで具体的な必要性が確認されてから追加する。

- TestCaseの詳細表示専用モデル(`phases`・`axis`等、TestCaseの本文相当のcontent。`test_cases[].relative_path`が指す`.markharness/generated/testcases/<relative_path>`を直接読むことで当面代替できる)
- Knowledge全文を対象とした検索結果モデル
- Git履歴比較モデル
- 複数refを横断する集計モデル
- 永続化された検索・表示キャッシュ

これらを将来性だけを理由に初期モデルへ含めない。二つ目の実在する読み取り形式や、具体的な利用上の不足が現れた時点で、新しいリードモデルまたはReaderを設計する。

なお各Nodeの`label`(§5.2)はこの限りではない。`traceability`自身の目的である「閲覧できる形での提供」に直接必要な、識別子の付随情報であり、TestCaseの本文のような独立したcontentではないため、初期モデルに含める。

## 13. 初期設計で確定する事項

### 13.1 `traceability`は独立コマンドにする

`generate`への統合は採用しない。`generate`はKnowledgeからTestCaseと生成物を作成する書込みコマンドであり、`traceability`はKnowledgeと生成済みTestCaseを読む読み取りコマンドである。両者を分けることで、読み取りだけを行いたいviewが生成処理やファイル書込みを発生させずに済む。

### 13.2 Nodeは型ごとの配列にする

`RequirementNode`・`FeatureNode`・`BehaviorNode`・`ScenarioNode`・`TestCaseNode`を分ける。全種類を共通の`Node { kind, uid, fields }`へ正規化する方式は採用しない。

理由は、型ごとの必須項目・外部Requirementの制約・TestCaseのCase revisionなどの意味をJSON契約から失わせないためである。関係の表現だけは共通の`TraceabilityRelation`へ正規化する。

### 13.3 JSON Schemaをリポジトリで管理する

外部契約を実装とfixtureだけで管理しない。既存の`schema/`ディレクトリに、次のJSON Schemaを追加する。

```text
schema/traceability-read-model.schema.json
schema/change-impact-read-model.schema.json
schema/release-coverage-read-model.schema.json
```

JSON Schemaは構造と型を検証する。UIDの存在、Git refの意味、Bindingを実行結果と解釈してはならないことなど、意味上の不変条件はRustのDomain/Applicationテストで検証する。

### 13.4 fixtureはmarkharnessを正本にする

markharnessとviewは別リポジトリのため、初期段階で共有ディレクトリ、submodule、実行時の相互参照は設けない。

markharnessは、リードモデルのJSON Schemaと代表fixtureを公開契約として管理する。viewは対応する`record_kind`と`schema_version`のfixtureを自分のリポジトリへ取り込み、契約テストに使用する。fixture更新は、Schema変更または意図的な出力契約変更として明示的に行う。

この分離により、viewのビルドがmarkharnessの作業ツリーやローカルパスへ依存しない。一方、markharness側ではfixtureをCLI統合テストへ使うため、実際のCLI出力と外部契約の乖離を検知できる。

## 14. 最終的なCLIコマンド一覧

CLIリードモデル設計に関係して、最終的に提供するコマンドは次のとおりとする。

| コマンド | 区分 | 書込み | 主な用途 | 出力リードモデル |
|---|---|---:|---|---|
| `markharness traceability [--at <ref>] [--format json]` | 新規追加 | なし | Requirement・Feature・Behavior・Scenario・TestCaseの関係を読む(`--at`省略時は作業ツリー) | `TraceabilityReadModel` |
| `markharness impact --base <ref> --head <ref> [--format json]` | 既存(実装済み) | なし | 変更影響、影響を受けるTestCase、対応確認状態を読む | `ChangeImpactReadModel` |
| `markharness coverage --requirements <ids-or-all> [--release <id>] --at <ref> [--format json]` | 既存(実装済み) | なし | Release Coverage、検証手段、coverage gapを読む | `ReleaseCoverageReadModel` |
| `markharness knowledge reconcile <intent-file> [--check]` | 既存コマンド | あり（`--check`時はなし） | UID付きIntentを検証し、Knowledgeを作成・更新・renameする | 反映結果。リードモデルの入力を更新する |

### 14.1 view向けの読み取りコマンド

`markharness-view`が利用するのは、次の3つの読み取りコマンドである。

```text
markharness traceability --format json                # 編集中のプレビュー: 作業ツリーを読む(ADR 0033)
markharness traceability --at HEAD --format json       # コミット済み状態を確認したい場合
markharness impact --base main --head HEAD --format json
markharness coverage --requirements all --at HEAD --format json
```

これらは標準出力へ1つのJSONリードモデルを出力する。viewはKnowledgeや`.markharness/`を直接読み取らず、これらの出力を入力とする。`traceability`は、利用者が編集した直後(コミット前)の状態をそのまま反映できる点が、`impact`・`coverage`と異なる(ADR 0033)。

### 14.2 書込みコマンドとの関係

GUIやviewがテスト知識を修正する場合は、`knowledge reconcile`だけを通す。

```text
リードモデルを取得
  ↓
UID付きの最小Intentを生成・編集
  ↓
markharness knowledge reconcile <intent-file> --check
  ↓
markharness knowledge reconcile <intent-file>
```

`traceability`・`impact`・`coverage`がKnowledgeファイルを直接変更することはない。また、`view`がKnowledgeファイルを直接変更することもない。

### 14.3 初期段階で追加しないコマンド

次のコマンドは、初期リードモデル設計には追加しない。

- `markharness view`：viewは別リポジトリの別ツールとして提供する
- `markharness serve`：ローカルWebサーバーを本体へ組み込まない
- `markharness search`：具体的な検索要件が確認されるまで作らない
- `markharness edit`：編集経路を増やさず、Intentと`knowledge reconcile`へ統一する
