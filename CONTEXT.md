# markharness

Gitネイティブなテスト知識管理と、外部の仕様(StrictDoc)・テストケースの対応関係・修正漏れ検知を扱う。2026-09-11のgrillingセッション(対象: [markharness-v2-design.md](docs/ja/design/markharness-v2-design.md)の再設計)で確定した用語を記録する。

## Language

**Feature**：利用者に提供する能力を表す、`knowledge/`配下で管理する仕様上の概念。実装コードそのものではない。複数のRequirementへ`contributes_to`で関連付けられる(Scenario側にも同じ関連を持てる、ADR 0031)。

**Behavior**：特定条件下で観測可能な振る舞い。一つのFeatureに所属し、共通手順を定義する。

**Scenario**：前提・操作・期待結果が具体化された一つの検証例。一つのBehaviorに所属し、一つのTestCaseと1対1で対応する(1 Scenario = 1 TestCase)。Featureと同様、複数のRequirementへ`contributes_to`で関連付けられる(ADR 0031)。

**TestCase**：Scenarioから決定的に生成される、検証すべき対象の単位。

**Case revision**：TestCaseの検証内容(操作・前提・期待結果・順序・データ)の版。対象ビルドや実行環境の版とは別である。

**Axis**：Feature・TestCase等を横断して分類・検索する固定された観点。実行条件とは区別する。

**ChangeEvent**：base/head間のFeature版比較から導出する変更記録。何が変わったかを機械的に示す。

**Requirement**：検証対象の要件。`source: native`ではmarkharnessが`label`/`description`の正本を持ち、`source: external`では仕様書(StrictDoc、`.sdoc`としてGit管理)が正本で、markharnessは内容を編集・複製せず固定参照(id・版)だけを保持する(ADR 0023)。StrictDocを導入しない運用ではnativeのみで完結する。
_Avoid_：仕様、Spec(用語を`Requirement`に統一する)。

**外部key(`source_key`)**：`source: external`のRequirementが保持する、StrictDoc側の識別子をそのまま複製した付随情報。生値のまま保持し、大文字小文字の変換は行わない。推奨する値はStrictDocのMID(各ノードに自動生成される、常に小文字16進の機械生成識別子)であり、事故の原因になった自由記述の`UID:`フィールドではない(MIDは表記ゆれが構造的に発生しない)。`uid`(ADR 0013)とは独立しており、Requirementの同一性判定・rename耐性のロジックには一切使わない。比較(重複検出・検索)が必要になった場合は大文字正規化して比較する方針のみ定め、実装は将来の課題とする(ADR 0030)。
_Avoid_：`external_id`(`src/canonical.rs`のJUnitインポート等に使われる無関係な概念と紛らわしいため使わない)。

**Contributes-to関連**：FeatureまたはScenarioからRequirementへの多対多の関連。「実現に寄与する」ことを示すのみで、検証済みの証明ではない。Feature側は`feature.requirement_uids`、Scenario側は`scenario.requirement_uids`が実体で(新しい型・格納先は作らない)、markharnessはRequirement同士の階層(StrictDoc側のHLR/LLR・Parent関係)を一切知らずフラットな集合として扱う(ADR 0031、P5)。生成されるTestCaseの関連は、Scenario側に1件以上あればそちらだけを使い、無ければFeature側にフォールバックする(和集合はしない)。両者の内容が食い違っていても`validate`は検出しない(著者の責任)。
_Avoid_：`implements`／`verifies`(厳密な検証済み証明という誤解を招くため使わない)。

**対応確認(Alignment check)**：仕様(Requirement)またはTestCaseの一方が変更されたとき、他方が追随したか、追随不要かを検出する機能。両方向(仕様→TestCase、TestCase→仕様)を対象とする。
_Avoid_：Alignment Obligation／Alignment Decision(独立したDomain型・承認ワークフローは作らないため使わない)。

**Spec-Reviewedトレーラー**：対応確認の結果、「変更不要」と判断したことをコミットメッセージの末尾に記録するcommit trailer(例：`Spec-Reviewed: no-change-required`)。変更を加える本人が、変更のコミットと同時に書き添える運用を前提とする。git notesは使わない(push/fetchのデフォルト対象外、GitHub上で不可視、rebase非追随という理由で不採用)。

**Execution Binding**：TestCaseと検証手段(`automated`／`manual`)およびその参照先(テストコードへのパスやURL)の対応宣言。値の存在は「最新版で実行済み」を意味しない。pass/fail等の詳細結果・実行日時・実行者・証跡本体・実行環境(ブラウザ/OS等)は持たない。
_Avoid_：Execution Status(実行された状態と誤解されるため使わない)、Execution FactをExecution Bindingの同義語として使うこと(将来追加し得る実行事実とは別概念)。
_Avoid_：Evidence、Verification Target、Environment matrix(証跡管理・環境ごとの区別は別ツールの責務であり、markharnessのドメインに含めない)。

**Change Impact**：base/head間の差分から算出する、影響を受けるFeature・Requirement・TestCaseの一覧。PR単位の日常的な確認に使う。
_Avoid_：Verification Plan(重量級の契約オブジェクトという誤解を招くため使わない)。

**Release scope(選定スコープ)**：あるリリースで検証対象に選んだTestCaseの一覧。`release_id`とCase UIDの配列だけを持ち、日時・担当者・承認状態・合否は持たない。人が記録する「選んだ」という宣言であり、実行証跡ではない(ADR 0024)。
_Avoid_：Verification Plan、Evidence Selection(重量級の契約・証跡選択という誤解を招くため使わない)。

**Release Coverage**：指定したRequirement/Feature集合全体について、TestCaseとの対応関係およびExecution Bindingの有無を一覧化したもの。リリース判断時の補助情報として、Change Impactと併用する。

**Retire(退役)**：TestCaseまたはFeatureを現在の対象から外すこと。UIDの再利用保証や、同一UIDでの明示的な復元・ID予約解除の仕組みは持たない。CLIによる新規作成では新UIDを発行し、内容一致から旧UIDを推定しない。Git履歴からUIDを含むファイルを復元した場合は元UIDが戻るため、すべての再登場が別要素になるとは保証しない。
_Avoid_：Restore／Release(ID予約解除)(厳密な同一性保証の仕組みとしては導入しないため、退役に統合する)。
