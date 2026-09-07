# PR Verification Plan：Canonical Model と生成パイプライン設計書

**Status**: Implemented(Stage 1〜2実装済み。[decisions/0008](../decisions/0008-verification-plan-product-roadmap.md)で決定したcanonical importとVerification Planの詳細設計)
**関連ドキュメント**: [テスト知識管理のGit-nativeモデル_統合版.md](../テスト知識管理のGit-nativeモデル_統合版.md)(以下「統合版」)、[decisions/0008](../decisions/0008-verification-plan-product-roadmap.md)、`docs/Markharness_改善・実装検討_統合設計文書.md`(以下「設計検討文書」)
**対象読者**：`markharness`の実装者(Stage 1: canonical import model、Stage 2: PR Verification Plan着手時に参照する)

**位置づけ**：統合版第3章のモデル(Feature集約のtree SHAをversion identityとする`ChangeEvent`、マイルストーン境界での自動計算)は、単一Gitリポジトリの`knowledge/`とマイルストーンタグを前提に実装済みである。本資料は、これをPRの任意のbase/headへ一般化し、外部ツール由来のartifactを取り込み、Verification Planとして出力するための設計を、設計検討文書の提案からmarkharnessの既存語彙(`FEATURE`・`ChangeEvent`・`verified_feature_tree_shas`等)に合わせて具体化したものである。[decisions/0008](../decisions/0008-verification-plan-product-roadmap.md)がStage 0〜3の着手順序を決定しており、本資料はStage 1(canonical model)・Stage 2(Verification Plan)の詳細設計を扱う。Stage 0(fixture・golden dataset化)・Stage 3(GUI)は本資料のスコープ外。

---

## 1. Canonical Model

### 1.1 概念モデル

外部ツール由来のartifactと、markharness既存の`FEATURE`/`CONDITION`/`TESTCASE`/`ChangeEvent`/`TESTEXECUTION`(統合版第3.1節ER図)を接続するため、以下の中間層を追加する。

```text
ExternalArtifact(外部ツールで観測された生データ)
      │ import/normalize
      ▼
CanonicalArtifact(markharness内の論理artifact。既存FEATURE等はこのkindの一種とみなす)
      │
      ├── ArtifactVersion(統合版のtree SHA/blob SHAに相当するimmutable snapshot)
      │          │
      │          └── Change(2 ArtifactVersion間の差分。統合版のChangeEventを一般化)
      │                 │
      │                 ▼
      │          AffectedArtifact(Changeの影響候補。根拠・confidence・導出経路を持つ)
      │
      └── Relation(stored | derived。統合版の`derived_from`/`forked_from`を一般化)

Execution ── Evidence ── binds to ArtifactVersion(統合版のverified_feature_tree_shasを一般化)
                          │
                          └── valid | stale | unknown(統合版のverify trace/pendingの判定を一般化)
```

既存のmarkharness実装における`FEATURE`は`CanonicalArtifact`のkind=`feature`、Featureディレクトリのtree SHAは`ArtifactVersion`のうちsource=`markharness-native`かつ`git_oid`が入っているケースに相当する。すなわちこの一般化は既存モデルを置き換えるものではなく、既存モデルをsourceの1つとして包含する。

### 1.2 logical identityとversion identityの分離

```text
logical_identity(A)  = (source, external_id)
version_identity(A,v) = git_oid                      # source が Git 管理下の場合
                       | canonical_hash(content(A,v)) # SaaS/API 由来などGit管理外の場合
```

- `source`：`markharness-native`・`doorstop:<repo>`・`testrail:<instance/project>`等のnamespace。
- `external_id`：入力元で継続的に同一artifactを示すID。markharness-nativeの場合は`feature.yml`の`id:`フィールド(統合版第3.3節の既存仕様と同一)。
- `git_oid`：入力がGit管理下にある場合のtree/blob SHA。markharness-nativeのFeatureは常にこれを使う(統合版第3.1節の`resolve_feature_versions`をそのまま流用)。
- `canonical_hash`：順序・空白・source固有の非意味的フィールドを正規化したcontent hash。SaaS API等、Git管理下にない入力に対して`git_oid`の代替として使う(設計検討文書第3.7節)。

**Stage 1のスコープ限定**：Markharness native artifactはGit tree SHAを`git_oid`として使用する。JUnit TestCaseは正規化したlogical identityから`sha256:`形式の`canonical_hash`を決定的に算出し、実行結果は別のEvidenceとして保持する。SaaS APIレスポンス全体の正規化やsource固有fieldの除外規則は、TestRail importerに着手するStage 4まで実装しない。

### 1.3 stored traceとderived trace

統合版第3.1節の`forked_from`(stored、手動記述)と`derived_from`(derived、`ChangeEvent`のtree SHA比較から都度導出)の区別を、外部import由来のtraceにも拡張する。

```yaml
relation:
  from: test:checkout-empty-postcode
  type: verifies
  to: condition:postcode-required
  origin:
    kind: derived                # stored | derived
    rule: markharness-generate   # 生成規則の識別子(既存実装ではCONDITION→TESTCASEのgenerates関係)
    rule_version: "1"
```

markharness-native importerにおいては、`generates`関係(統合版第3.2節(A)の構造的生成グラフ)がderived traceの唯一の生成源であり、これは既存実装の`markharness generate`をそのまま再利用する。stored traceは、外部importerが持ち込むrequirement-test対応(例：Doorstop/StrictDocのlink)、またはmarkharness側で人手記述する`forked_from`に相当する。

### 1.4 canonical schemaの必須フィールド

| 領域 | フィールド例 | 既存実装との対応 |
|---|---|---|
| Identity | `source`、`external_id`、`canonical_id` | `feature.yml`の`id:`(統合版第3.3節) |
| Version | `git_oid`、`canonical_hash`、`observed_at` | Featureディレクトリのtree SHA(統合版第3.1節) |
| Type | `feature`/`requirement`/`condition`/`expected_result`/`test_case`/`external_requirement`等 | 統合版ER図の各エンティティ |
| Relations | `type`、`origin.kind`、`origin.rule`、`confidence` | `derived_from`/`forked_from`(統合版第3.1節) |
| Provenance | `importer`、`importer_version`、`source_locator` | (新規。既存実装はmarkharness-native固定のため不要だった) |
| Evidence | `result`、`executed_at`、`bound_versions` | `verified_feature_tree_shas`(統合版第3.7節) |

正規化は決定論的でなければならない(同一の意味的入力から常に同一のcanonical hash、統合版第3.3節の`canonicalization_rule_version`と同じ設計思想)。時刻・取得順・API応答順等の非決定要素はhash対象に含めない。

---

## 2. Verification Plan生成パイプライン

### 2.1 処理ステージ

```text
PR (base..head)
      │
      ├── code diff
      ├── spec diff
      └── knowledge diff ── 既存のFeature tree SHA比較(統合版第3.2〜3.4節)をbase/head任意の2点へ一般化
              │
              ▼
     Structured Changes(既存ChangeEventのschemaを流用、milestone_idの代わりにbase/head refを持つ)
              │
      ┌───────┴────────┐
      ▼                ▼
trace/derivation    gap analysis
(既存generates関係    (新規。knowledgeの新規condition/boundary/
 + stored trace)       error behaviorに対応するテストの欠落検査)
      │                │
      ▼                ▼
Affected Existing   New Required Tests(proposal。人間のacceptで初めてrequired)
Tests
      └───────┬────────┘
              ▼
       Verification Plan
              │
      Evidence resolution(既存verify trace/pendingのvalid/stale/unknown判定を再利用)
              ▼
 Passed / Pending / Failed / Stale
```

Stage 2で実装するのは「Structured Changes」「Affected Existing Tests(stored+derived trace)」「Evidence resolution」「Plan emission」の4段。「gap analysis(missing test発見)」「AI proposal adapter」はStage 2内のoptional variantとして追加し、rule-based baselineとの比較評価を行う([decisions/0008](../decisions/0008-verification-plan-product-roadmap.md)第5節参照)。

### 2.2 既存`changes compute`からの拡張点

現行の`markharness changes compute`は`from_milestone`/`to_milestone`という2つのGitタグを引数に取る(統合版第3.4節)。Verification Planはこれを一般化し、任意の2つのGit refを受け付ける。

```rust
// 概念的なシグネチャ(既存 changes::compute の一般化案)
fn compute_changes(from_ref: &str, to_ref: &str, mode: ComputeMode) -> Vec<ChangeEvent>
```

`from_milestone`/`to_milestone`という命名・milestoneタグへの限定を解く以外、tree SHA比較のロジック自体(統合版第3.2〜3.4節、`historical`/`--current-tree`の2モードを含む)は変更しない。既存の`markharness changes compute --from <tag> --to <tag>`はこの一般化されたAPIの特殊ケース(milestoneタグを渡す場合)として後方互換を維持する。

### 2.3 Verification Plan出力スキーマ(案)

```yaml
schema_version: 1
base: v2.3
head: 4c2e81a
summary:
  changed_features: 3
  affected_tests: 17
  new_tests: 4
  obsolete_tests: 2
  passed: 9
  pending: 6
  failed: 2
  stale_evidence: 5

changed_features:
  - id: feature:checkout
    from_tree_sha: 3a1b...
    to_tree_sha: 4c2e...
    confidence: 1.0          # 既存のtree SHA比較(決定論的)はconfidence常に1.0
                              # AI proposal由来のchanged_featuresのみ1.0未満を取りうる

affected_existing_tests:
  - id: test:checkout-valid-address
    reason: "derived from modified condition: postcode-required"
    origin: derived           # stored | derived(第1.3節)
    status: pending           # 既存verify pendingの判定をそのまま流用

new_required_tests:
  - proposal_id: new-test:checkout-empty-postcode
    behavior: "empty postal code is rejected"
    reason: "new mandatory constraint has no negative test"
    confidence: 0.88           # rule-based/AI由来の候補のみ。1.0固定ではない
    decision: proposed         # proposed | accepted | rejected | deferred(第2.5節)

obsolete_tests:
  - id: test:checkout-postcode-optional
    reason: "asserts behavior removed by this change"
```

`changed_features`のうちtree SHA比較から機械的に導出したエントリは`confidence: 1.0`固定とし、AI proposal由来のエントリのみ1.0未満の値を持ちうる。この区別により、Plan消費者(CI・GUI)が決定論的な結果とAI由来の提案を混同しない。

### 2.4 CLIコマンド(案)

```bash
# base/head間のPlanを生成(既存 changes compute の一般化)
markharness plan --base origin/main --head HEAD --format json \
  --output .markharness/verification-plan.json

# Planをreviewし、new_required_testsのproposalをaccept/reject
markharness plan review .markharness/verification-plan.json

# 既存 verify trace/pending をそのまま利用してevidenceを解決
markharness plan status --plan .markharness/verification-plan.json
```

exit codeは既存の`verify pending --fail-on-pending`と一貫させる。

| exit code | 条件 |
|---|---|
| 0 | required testsがすべてvalid evidenceを持つ |
| 1 | failed testがある |
| 2 | pending/stale/未承認proposalがある |
| 3 | 入力・schema・identity解決エラー |

### 2.5 new_required_testsのdecision状態モデル

設計検討文書第6.3節の状態モデルを、markharnessの既存語彙(`verify pending`のpending/stale)と衝突しない形で採用する。

```text
proposed ── human accepts ── accepted(TestCase作成後、通常のTESTCASEとして扱われる)
    │                             │
    ├── rejects ── rejected       └── execution待ちの間は pending(既存verify pendingの判定)
    └── defers  ── deferred
```

`proposal decision`(accepted/rejected/deferred)と`execution status`(pending/passed/failed/stale)を混同しない。new required testはacceptされて初めてTestCase化され、その後は既存の`verify trace`/`verify pending`の対象になる(第2段の`Evidence resolution`と同じロジックを再利用する)。

---

## 3. Bounded Components(実装分割の指針)

| Component | 責務 | 既存実装との対応 |
|---|---|---|
| Importer SDK | 外部形式の読取、identity/provenance付与 | 新規(Stage 1) |
| Canonical Store | canonical filesとschema migration | 新規(Stage 1)。ただしmarkharness-native importerの出力先は既存`knowledge/`ディレクトリそのもの |
| Change Engine | version/snapshot間のsemantic diff | 既存`markharness changes compute`を一般化(第2.2節) |
| Impact Engine | stored/derived traceからaffectedを導出 | 既存`generates`関係(統合版第3.2節(A))を再利用 |
| Gap Analyzer | missing/new/obsolete test proposal | 新規(Stage 2、optional variant) |
| Evidence Engine | result ingestion、version bind、freshness判定 | 既存`verify trace`/`verify pending`を再利用(統合版第3.7節) |
| Plan Engine | required setとstatusを組立 | 新規(Stage 2)。上記各Engineの出力を合成する薄い層 |
| Presentation | CLI、(Stage 3で)GUI、CI summary | 既存CLI出力形式を拡張 |

この分割の要点は、**Change EngineとEvidence Engineは既存実装をそのまま再利用し、新規実装はImporter SDK・Gap Analyzer・Plan Engineに限定する**ことである。これにより、Stage 1〜2の実装が既存の`ChangeEvent`・`verified_feature_tree_shas`モデル(統合版第3章、実証未了のRQ1評価対象そのもの)と分岐するリスクを構造的に下げる。

## 4. Invariants

1. 同一canonical input・rule version・base/headから同一の決定論的出力が得られる(既存`markharness changes compute`の`historical`モードと同じ要件、統合版第3.5節)。
2. derived artifactはprovenanceとgenerator versionを持つ(既存`generates`関係のrule_versionを流用)。
3. evidenceは少なくともtest identity・result・execution context・検証対象versionを持つ(既存`verified_feature_tree_shas`と同じ要件)。
4. Plan上の`valid`は「最後にPASSした」ではなく、「現在のbase/head区間で必要なversion集合に十分なevidenceがある」ことを意味する(既存`verify pending`のstale判定と同じ意味論)。
5. identityの曖昧さ(rename/split/merge)を暗黙に確定しない。`identity_resolution: proposed`として人間のreviewを経る(統合版で未対応、設計検討文書第3.6節)。
6. Plan項目はreason/source/confidenceを追跡できる(第2.3節のスキーマ)。
7. cache/index/GUI stateはGitと外部snapshotから再構築可能である([decisions/0008](../decisions/0008-verification-plan-product-roadmap.md)第3節「GUI専用DBをsource of truthにしない」)。

---

## 5. Stage 0で確定すべき事項(本資料の前提)

本資料はStage 1〜2の設計であり、[decisions/0008](../decisions/0008-verification-plan-product-roadmap.md)が定めるStage 0を前提とする。ChangeEventは`tests/fixtures/stage0/changes-m1-m2.golden.yml`、canonical snapshotは`tests/fixtures/stage1/junit-import.golden.json`、Planは`tests/fixtures/stage2/verification-plan.golden.json`をgolden contractとする。`historical-pr.json`との比較でrule-based proposalのprecision/recallを再現する。

CLI JSON契約は次の方針でversioningする。

- 新たにstable contractとして公開するJSON objectは、top-levelに整数の`schema_version`を必須とし、初版を`1`とする。
- 同じversionではフィールドの意味・型・必須性を変更しない。optional fieldの追加のみ許容する。
- フィールド削除、rename、型変更、意味変更は`schema_version`を増やし、少なくとも1 minor releaseの間は旧versionを読み取れるようにする。0.x期間中もこのデータ契約規則は維持する。
- Phase 2で共通Presenterへ移行した`generate`・`verify pending`は`schema_version: 1`のversioned envelopeを返す。未移行コマンドの`--json`出力はunversioned legacy contractとして扱い、各Presenter移行までは形を変更しない。
- golden testは時刻、一時パス、commit SHAのような環境依存値を正規化したうえで、残りのJSON/YAML全体を完全一致比較する。

## 6. 検討したが採用しない選択肢

- **`ChangeEvent`とは別の新エンティティとして`Change`を実装する**：設計検討文書は概念モデル上`Change`という名称を使うが、markharness既存実装の`ChangeEvent`と機能的に同一(tree SHA比較による差分)であるため、別エンティティとして実装せず、`ChangeEvent`のfrom/toをmilestoneタグ限定から任意refへ一般化する(第2.2節)。エンティティを分けると、統合版第3章の実証対象(RQ1)とVerification Planの評価が別モデルを見ることになり、Stage 2の評価結果を論文側のモデルへフィードバックできなくなる。
- **SaaS API由来のcanonical_hashをStage 1で先行実装する**：TestRail等のAPI認証・pagination・rate limit対応はfile-based importerより実装コストが高く、Stage 1の目的(canonical schemaの検証)を遅らせる。Stage 4まで持ち越す([decisions/0008](../decisions/0008-verification-plan-product-roadmap.md)第4節)。
- **`new_required_tests`のconfidenceスコアを`generated_by`/`verified_by`(統合版第3.5節の製品化提案フィールド)へ統合する**：`generated_by`/`verified_by`はExpectedResultという確定済みknowledgeに対するメタデータであり、`proposed`状態のproposalとは意味論が異なる(確定知識 vs 未承認候補)。両者を混在させると、統合版が明記する「`knowledge/`配下は検証済みの確定知識である」という前提(第3.5節)が崩れるため、proposalは`.markharness/verification-plan.json`側にのみ保持し、acceptされて初めて通常のknowledgeへ反映する。

## 7. `bound_versions`の型付け([decisions/0017](../decisions/0017-scenario-case-revision-and-execution-evidence.md)の本番経路への適用、Issue #43)

[decisions/0017](../decisions/0017-scenario-case-revision-and-execution-evidence.md) §3はCase UID・Case revisionを意味の異なる型として扱うことを要求するが、Step 1a(型の新設)の時点では`ExecutionEntry`・`PlanEvidence`・`CaseVersion`・`TestCase`・`CaseDefinition`等の本番経路は引き続き`String`/`BTreeMap<String, String>`のままだった。この節はその契約を本番経路のRust型システムへ実際に反映する際の設計を記録する。

### 7.1 `PlanEvidence.bound_versions`の構造体化と、`CanonicalEvidence.bound_versions`との分離

第1.4節の表は`bound_versions`を単に「Evidence領域のフィールド」として一括りに扱っていたが、実装を追うと2つの異なる性質のものが同じ型(`BTreeMap<String, String>`)を共有していたことが分かった。

- `plan::PlanEvidence.bound_versions`・`derived_index`の実行インデックスが持つ`bound_versions`：`execution::ExecutionEntry`から、`case_uid`/`case_revision`/`target_revision`という必須3項目と、値がある場合だけ保持する任意項目`environment`を詰めて作られる。項目の意味と集合は固定されており、任意キーを追加できるマップである必然性がない。
- `canonical::CanonicalEvidence.bound_versions`：`markharness import --bind ARTIFACT_ID=VERSION`(任意個・任意キー)がそのまま入る、外部importer向けの汎用バインディング。`application::build_verification_plan_value`はこの経路のevidenceを計画の証跡候補から意図的に除外している(第2.1節図の`Evidence resolution`が対象とするのは`ExecutionEntry`由来のみ)。

前者だけを`struct BoundVersions { case_uid: CaseUid, case_revision: CaseRevision, target_revision: TargetRevision, environment: Option<Environment> }`に置き換え、キーのtypo・欠落をコンパイル時に排除する。`CanonicalEvidence.bound_versions`は汎用`BTreeMap<String, String>`のまま据え置く。両者が同名・同型でありながら異なる契約を持つのは意図的な設計であり、統合を目指さない。

### 7.2 新規の型付き識別子

`target_revision`・`environment`は[decisions/0017](../decisions/0017-scenario-case-revision-and-execution-evidence.md) §3が列挙する型に含まれないが、`execution::record_execution`が既に空文字を拒否する専用の検証ロジック(`EmptyTargetRevision`・`EmptyEnvironment`)を持っていた。`identity::typed_uid`の既存マクロ(`define_typed_uid!`)をそのまま使って`TargetRevision`・`Environment`を追加し、この検証をCase UID等と同じ形へ揃える。入力境界での検証は`record_execution`の関数境界そのもので行い(CLI層での事前変換は行わない)、`RecordError::EmptyTargetRevision`/`EmptyEnvironment`はCLIのエラーメッセージ互換のため維持する(内部で`BlankValueError`をラップする)。

### 7.3 意図的にスコープ外とした箇所

- `canonical::CanonicalArtifact.uid`(Feature/Scenario/TestCaseで多態的に使われる`Option<String>`)の型分離：既存フィールドの構造変更であり、今回の「本番経路への伝播」より大きい別の設計判断のため、将来の別Issue/ADRに委ねる。
- `generate::GeneratedFrom.feature_uid`/`requirement_uids`：情報用途のみで照合キーに使われないため、取り違えリスクが実在しない。
- `identity/migration_manifest.rs`(レガシー`case_id`→`case_uid`のロゼッタストーン)：証跡・計画照合パスとは別の関心事であり、新規Issueとして切り出す。破壊的変更が自由なプロトタイプ期であることは、無関係な関心事を1つのIssueへ混ぜてよい理由にはならない。

### 7.4 `CanonicalArtifact.uid`から`CaseUid`を再構築する箇所の失敗方針

`application::build_verification_plan_value`が`case_versions`を組み立てる際、`CanonicalArtifact.uid`(§7.3の通り型分離のスコープ外、`Option<String>`のまま)から`CaseVersion.case_uid: CaseUid`を再構築する必要がある。この変換には`CaseUid::new(value).expect(...)`を使い、`io::Error`等への伝播は行わない。

安全性の根拠は「上流のどこかを信頼する」という間接的なものではなく、`identity::derived_uid::case_uid`(`derive()`)の構造そのものにある。`derive()`はSHA1ハッシュを`format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}", ...)`という固定書式で文字列化するため、入力(`scenario_uid`)の値によらず常に36文字の非空文字列を返す。したがって、この関数の出力を`CaseUid::new(...)`に通す限り、失敗するケースが構造的に存在しない。`filter_map`内の`?`による`None`除外(`uid`自体が欠落した未移行Scenarioを除外する処理)とは別の話であり、`Some`の中身が空文字になり得ないことの根拠がこちらである。

`io::Error`への伝播は到達不能なエラーパスを追加することになり、「起こり得ないシナリオに対するエラーハンドリングを追加しない」という設計方針(CLAUDE.md)に反するため採用しない。この保証を将来のリファクタリングで壊さないよう、`derive()`の出力が入力によらず常に36文字・非空であることを固定するユニットテスト(空文字入力を含む)を`identity/derived_uid.rs`に追加する**予定である(未実装。`checklist-issue-43-typed-identifiers.md` Step 6で実施する)**。
