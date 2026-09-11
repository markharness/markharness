# markharness v2 の用語

markharnessは、Gitネイティブなテスト知識(`knowledge/`)と、外部の仕様(StrictDoc)との対応関係・修正漏れ・実行実績を軽量に追跡する。用語の一次情報は[CONTEXT.md](../../../CONTEXT.md)であり、本書はその抜粋・補足である。定義が食い違う場合はCONTEXT.mdを正とする。

## Language

**Feature**：利用者に提供する能力を表す、`knowledge/`配下で管理する仕様上の概念。複数のRequirementへ`contributes_to`で関連付けられる。

**Behavior**：特定条件下で観測可能な振る舞い。一つのFeatureに所属する。

**Scenario**：前提・操作・期待結果が具体化された一つの検証例。一つのBehaviorに所属し、一つのTestCaseと1対1で対応する。

**TestCase**：Scenarioから決定的に生成される、検証すべき対象の単位。

**Case revision**：TestCaseの検証内容の版。対象ビルドや実行環境の版とは別。

**Axis**：Feature・TestCase等を横断して分類・検索する固定された観点。

**ChangeEvent**：base/head間のFeature版比較から導出する変更記録。

**Requirement**：検証対象の要件。`source: native`ではmarkharnessが`label`/`description`の正本を持ち、`source: external`ではStrictDocが正本で、markharnessは固定参照(id・版)だけを保持し本文を複製・編集しない([0023](../decisions/0023-requirement-native-and-external-source.md)、設計書§5.2.1)。現行実装は`knowledge/requirements/`にnative実体として`label`/`description`まで保持しているため、v2では固定参照へ移す変更になる(設計書§5.2.1)。

**Contributes-to関連**：FeatureからRequirementへの多対多の関連。「実現に寄与する」ことを示すのみで、検証済みの証明ではない。実体は現行の`feature.requirement_uids`であり、新しい型・格納先は作らない。実体は現行の`feature.requirement_uids`であり、新しい型・格納先は作らない。

**対応確認(Alignment check)**：仕様(Requirement)またはTestCaseの一方が変更されたとき、他方が追随したか、追随不要と確認されたかを検出する機能。両方向を対象とする。

**Spec-Reviewedトレーラー**：対応確認で「変更不要」と判断したことを記録するcommit trailer。変更のコミットと同時に書き添える。

**Execution status**：TestCaseに付与する軽量な記録で、**検証手段(`automated`／`manual`)とその参照先**を表す。pass/fail等の詳細・実行日時・証跡本体・実行環境は持たないため、値の存在は「最新版で実行済み」を意味しない。

**Change Impact**：base/head間の差分から算出する、影響を受けるFeature・Requirement・TestCaseの一覧。PR単位の確認に使う。

**Release Coverage**：指定したRequirement/Feature集合全体について、TestCaseとの対応関係およびExecution statusの有無を一覧化したもの。リリース判断の補助情報。

**Retire(退役)**：TestCaseまたはFeatureを現在の対象から外すこと。UIDの再利用保証や、同一UIDでの明示的な復元・ID予約解除の仕組みは持たない。
