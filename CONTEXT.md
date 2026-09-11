# markharness

Gitネイティブなテスト知識管理と、外部の仕様(StrictDoc)・テストケースの対応関係・修正漏れ検知を扱う。2026-09-11のgrillingセッション（対象: [markharness-v2-design.md](docs/ja/design/markharness-v2-design.md)の再設計）で確定した用語を記録する。

## Language

**Feature**：利用者に提供する能力を表す、`knowledge/`配下で管理する仕様上の概念。実装コードそのものではない。複数のRequirementへ`contributes_to`で関連付けられる。

**Behavior**：特定条件下で観測可能な振る舞い。一つのFeatureに所属し、共通手順を定義する。

**Scenario**：前提・操作・期待結果が具体化された一つの検証例。一つのBehaviorに所属し、一つのTestCaseと1対1で対応する（1 Scenario = 1 TestCase）。

**TestCase**：Scenarioから決定的に生成される、検証すべき対象の単位。

**Case revision**：TestCaseの検証内容（操作・前提・期待結果・順序・データ）の版。対象ビルドや実行環境の版とは別である。

**Axis**：Feature・TestCase等を横断して分類・検索する固定された観点。実行条件とは区別する。

**ChangeEvent**：base/head間のFeature版比較から導出する変更記録。何が変わったかを機械的に示す。

**Requirement**：検証対象の要件。`source: native`ではmarkharnessが`label`/`description`の正本を持ち、`source: external`では仕様書(StrictDoc、`.sdoc`としてGit管理)が正本で、markharnessは内容を編集・複製せず固定参照(id・版)だけを保持する(ADR 0023)。StrictDocを導入しない運用ではnativeのみで完結する。
_Avoid_：仕様、Spec（用語を`Requirement`に統一する）。

**Contributes-to関連**：FeatureからRequirementへの多対多の関連。「実現に寄与する」ことを示すのみで、検証済みの証明ではない。正本はFeature側が持ち、実体は現行の`feature.requirement_uids`(新しい型・格納先は作らない)。
_Avoid_：`implements`／`verifies`（厳密な検証済み証明という誤解を招くため使わない）。

**対応確認（Alignment check）**：仕様(Requirement)またはTestCaseの一方が変更されたとき、他方が追随したか、追随不要かを検出する機能。両方向（仕様→TestCase、TestCase→仕様）を対象とする。
_Avoid_：Alignment Obligation／Alignment Decision（独立したDomain型・承認ワークフローは作らないため使わない）。

**Spec-Reviewedトレーラー**：対応確認の結果、「変更不要」と判断したことをコミットメッセージの末尾に記録するcommit trailer（例：`Spec-Reviewed: no-change-required`）。変更を加える本人が、変更のコミットと同時に書き添える運用を前提とする。git notesは使わない（push/fetchのデフォルト対象外、GitHub上で不可視、rebase非追随という理由で不採用）。

**Execution status**：TestCaseに付与する軽量な記録で、**検証手段(`automated`／`manual`)とその参照先（テストコードへのパスやURL）**を表す。値の存在は「最新版で実行済み」を意味しない。pass/fail等の詳細結果・実行日時・実行者・証跡本体・実行環境（ブラウザ/OS等）は持たない。
_Avoid_：Evidence、Verification Target、Environment matrix（証跡管理・環境ごとの区別は別ツールの責務であり、markharnessのドメインに含めない）。

**Change Impact**：base/head間の差分から算出する、影響を受けるFeature・Requirement・TestCaseの一覧。PR単位の日常的な確認に使う。
_Avoid_：Verification Plan（重量級の契約オブジェクトという誤解を招くため使わない）。

**Release scope(選定スコープ)**：あるリリースで検証対象に選んだTestCaseの一覧。`release_id`とCase UIDの配列だけを持ち、日時・担当者・承認状態・合否は持たない。人が記録する「選んだ」という宣言であり、実行証跡ではない(ADR 0024)。
_Avoid_：Verification Plan、Evidence Selection(重量級の契約・証跡選択という誤解を招くため使わない)。

**Release Coverage**：指定したRequirement/Feature集合全体について、TestCaseとの対応関係およびExecution statusの有無を一覧化したもの。リリース判断時の補助情報として、Change Impactと併用する。

**Retire（退役）**：TestCaseまたはFeatureを現在の対象から外すこと。UIDの再利用保証や、同一UIDでの明示的な復元・ID予約解除の仕組みは持たない。退役後に同じ内容が再登場した場合は新規の別要素として扱う。
_Avoid_：Restore／Release（ID予約解除）（厳密な同一性保証の仕組みとしては導入しないため、退役に統合する）。
