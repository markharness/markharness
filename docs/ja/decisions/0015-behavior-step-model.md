# 0015: Behaviorへのstep概念の導入(段階的アプローチ)

## ステータス

Superseded by [0016](./0016-behavior-condition-precondition-step-result-model.md) (2026-08-31)。Phase 1(inline `Behavior.steps`の導入)は実装され「Acceptedへ変更する条件」を一度満たしたが、実データでの利用により`behavior.steps`をConditionの共通粒度とする前提そのものが崩れたため、[0016](./0016-behavior-condition-precondition-step-result-model.md)がこのADRを置き換える。

## 背景

`generate.rs::generate_testcases`が生成する`TestCase.steps`は、実装上は配列型だが実際には常に`[behavior.description]`という単一要素にしかならない([testcase-generation-design.md §3.3](../design/testcase-generation-design.md#33-preconditions--phases-のテキスト組み立て)(現行の見出しは[0016](./0016-behavior-condition-precondition-step-result-model.md)後の内容に更新済みだが、当時の`title = condition.description`/`steps = behavior.steps`という単純転記方式の記述はそのまま残っている))。`behavior.yml`(`behavior.schema.json`)自体には`description`という1つの自由文フィールドしかなく、Test Designerは本来順序立てて書きたい複数の操作手順を1つの文字列に無理に詰め込んでいる。[testcase-generation-design.md §7](../design/testcase-generation-design.md#7-将来課題との切り分け)は「Behavior階層を使ったより高度なグルーピング・Axisの多段管理などのモデル拡張」を将来課題として明示していたが、steps自体の複数要素化には言及していなかった。

同時に、複数のBehaviorが同じ操作手順(例: ログイン手順)を繰り返し記述する再利用ニーズが将来生じうる。`description`を都度コピーすると内容の乖離(あるBehaviorだけ手順が更新され、他が古いまま)を検出できない、という懸念がある。

当初案では、Behaviorの複数要素化・共有Stepレジストリ・UID・ハッシュ整合性検証・acceptコマンドを一度に導入する設計を検討した。しかしレビューにより、この一括導入は過剰であると判断した。共有ニーズは現時点で実データに基づく確認ができておらず(本ADR時点でリポジトリ内に`behavior.yml`の実データは存在しない)、共有Step変更時の影響範囲(全参照Behavior一括更新か個別承認か、原子性、復旧)や、Stepを第6の`EntityKind`として[identity lifecycle基盤](./0013-immutable-identity-model.md)に統合する必要性も未検証のまま、hash不一致のfail-closed運用だけを先に確定させることはリスクが大きい。

そのため本ADRは、複数要素化そのもの(Phase 1)だけを決定事項とし、共有レジストリ以降(Phase 2〜4)は実データで需要を確認したうえで改めて設計する方針とする。

## 前提

- 本ADR時点で、markharnessを使用している外部ユーザーはいない。
- そのため`behavior.yml`の形式変更に伴う既存データのマイグレーションは行わない。本ADR時点でリポジトリ内(`samples/`含む)に`behavior.yml`の実データは存在しないため、一括書き換えや移行コマンドは不要である。
- Knowledge schema versionは変更せず、v1のまま維持する。`steps`必須フィールド追加は[ADR 0014](./0014-knowledge-schema-version-persistence.md)が定める`schema_version`更新の対象としない。
- ただし、将来v1が外部互換性を持つようになった場合は、この方針(マイグレーション不要・schema_version不変)を再評価する。
- Phase 1前後のrefをchanges computeや履歴比較の対象にする必要が生じた場合は、schema versionの扱いを再評価する。

## 決定内容

### Phase 1(本ADRで決定): inline `Behavior.steps`を導入する

1. `behavior.yml`に`steps: Vec<String>`(必須、順序付き配列)を追加する。各要素はその場に直接書くインライン文字列であり、共有レジストリへの参照は行わない。
2. Step粒度は「`steps`配列の1要素 = 1操作」に統一する。複数操作をまとめて1要素に書くことは許容しない。
3. `description`は人間向けの1文要約として残すが、テストケース生成には一切使わない。
4. `generate.rs::generate_testcases`の`steps = [behavior.description]`を`steps = behavior.steps`に置き換える。`behavior.description`は生成ロジックから完全に除外され、`knowledge/`上の人間向けドキュメントとしての役割のみを持つ。
5. `steps`が空配列である場合、および各要素が空文字列である場合は、Knowledge検証でエラーとする。
6. 共有Stepレジストリ、UID、hash整合性検証、`steps accept`等の復旧コマンドは、本Phaseでは導入しない。
7. Behaviorを作成できる唯一のサポート経路である`knowledge add --edit`(`KnowledgeDraft`/`BehaviorDraft`、`src/knowledge_draft.rs`)を、新設の必須`steps`を入力・検証できるように更新する。具体的には、`BehaviorDraft`へ`steps`フィールドを追加し、`knowledge add --edit`が開く空draftテンプレートおよび非対話呼び出し用のテンプレート出力(`markharness knowledge add --edit --print-template`相当、`cli.rs`)にも`steps:`の記入欄を含める。`push_missing_description`と同様に、draft側でも`steps`が空・全要素空文字列の場合は検証エラーとする。この更新を行わない限り、唯一の作成経路が新しい必須フィールドを満たせず、Behaviorを新規作成できなくなる。

```yaml
# behavior.yml
id: todo-add-task
feature: todo
label: Add Task
axis: [ui]
description: "User adds a task."
steps:
  - "タイトル欄をクリックする"
  - "何も入力しない"
  - "送信ボタンを押す"
```

#### 実装時の留意事項(本ADRでは決定しない)

- `generate.rs::generate_testcases`の変更は、`steps = [behavior.description]`という前提で書かれている既存テストヘルパー・アサーション(例: `steps: vec![case.behavior_description.clone()]`を組み立てるテストケース構築箇所、`tc.steps`が単一要素であることを前提にした`assert_eq!`群)を洗い出し、複数要素`steps`に対応させる書き換えが必要になる。影響箇所の網羅的な洗い出しは実装着手時のchecklist化(`checklist-<task>.md`)で行う。
- `behavior.schema.json`への`steps`必須フィールド追加、`serialize_behavior`、fixture、schemaテストの更新が必要になる。schema versionはv1のまま変更しない(上記「前提」参照)。
- `knowledge_draft.rs`の`KnowledgeDraft`/`BehaviorDraft`および関連する検証(`push_missing_description`相当のsteps版)、draftテンプレート文字列、`knowledge_apply::apply_draft`のBehavior書き出し処理、既存の draft parse/validate テスト(`description: null`等の欠落パターンを検証しているテスト群)を、`steps`必須化に合わせて更新する必要がある。
- 決定内容5(空配列・空文字列要素の拒否)をどの層で実装するかは本ADRでは決定しない。`schema/behavior.schema.json`は`validate.rs`から実行時バリデーションとして実際に使われており、`description`は`minLength: 1`でschema層に検証を寄せている。一方このリポジトリには配列長を制約する`minItems`の前例がなく、`axis: []`のように空配列を許容してきた既存フィールドしかない。`steps`について`minItems`/`items.minLength`をschema側に追加して宣言的に弾くか、`push_missing_description`同様にRust側の手続き的チェックに寄せるかは、実装着手時に決め打ちしておく。
- 決定内容2の「1要素=1操作」という粒度規約は自由文字列である`steps`の性質上、機械的な検証では強制できない(例: 「Aする。Bする。」を1要素に書いても`knowledge validate`は通過する)。この規約はTest Designerのレビュー運用に委ねられるものであり、Knowledge検証の対象外であることを実装時に明記しておく。

### Phase 2(将来の方向性、未確定): 実データで共有需要を確認する

Phase 1導入後、しばらく実データを蓄積した上で次を確認する。

- 同じ手順が複数Behaviorで繰り返されるか
- コピーの更新漏れが実際に起きるか
- 共有したい単位が1操作か、複数操作をまとめた手順ブロックか
- 共有Stepの変更影響を利用者が理解できるか

判断基準は定量的な閾値を設けず、定性的に「重複・更新漏れが実際に発生したら」次のPhaseへ進む。実際に発生しなければ、inline `Behavior.steps`(Phase 1)のみで完了とする。

### Phase 3(将来の方向性、未確定): 共有Stepレジストリを別ADRとして設計する

Phase 2で共有需要が確認できた場合に限り、別ADR(または本ADRの改訂版)として次を設計する。詳細は本ADRでは決定しない。

- `.markharness/steps/`のデータモデル
- UIDを必要とする理由の確認
- Stepを[ADR 0013](./0013-immutable-identity-model.md)の第6の`EntityKind`にするか、より単純な共有レジストリで足りるかの判断
- rename・retire・restoreの扱い
- 共有Step変更時に、参照している全Behaviorへの影響範囲をどう表示するか

### Phase 4(将来の方向性、未確定): ハッシュ検証とaccept運用を追加する

Phase 3で共有レジストリを導入する場合に限り、次を設計する。詳細は本ADRでは決定しない。

- hash不一致の検出と、`knowledge validate`/`generate`のfail-closed動作
- `steps add`/`steps accept`の挙動(全参照Behaviorを一括更新するか、個別承認するか、Step変更と参照側更新の原子性、失敗時の復旧、変更対象Behavior一覧の表示)
- ハッシュの正規化規則(UTF-8・改行・末尾改行・YAML block scalarの扱い)の明文化

## Acceptedへ変更する条件

- Phase 1の実装(`Behavior.steps: Vec<String>`の追加、schemaテスト・fixture・`generate_testcases`・関連テストの更新)が完了すること。
- knowledge add --editのBehaviorDraft、テンプレート、steps検証、apply処理、および関連テストの更新が完了すること。
- Phase 2以降(共有レジストリ・UID・hash・accept)は、需要が確認された時点で別ADRとして提案・決定するため、本ADRのAccepted判定には含めない。

## 対象外

- Phase 2〜4の詳細設計(共有Stepレジストリのデータモデル、UID要否、EntityKind化、rename/retire/restore、hash正規化規則、`steps add`/`steps accept`の挙動)。共有需要が確認された時点で別途ADRを起こす。
- 共有Stepレジストリからの選択・新規作成UX(Phase 3以降が導入された場合の`knowledge add --edit`拡張)。なお、Phase 1で`knowledge add --edit`のdraftテンプレートに`steps`の記入欄を追加すること自体は上記Phase 1決定事項に含まれ、対象外ではない。
- ExecutionResult側でのstep単位の実行結果記録(本ADRの動機には含まれない)。
