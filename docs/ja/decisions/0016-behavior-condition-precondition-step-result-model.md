# 0016: Behavior/Condition/ExpectedResultへの前提・手順・結果モデルの導入([0015](./0015-behavior-step-model.md)を置き換え)

## ステータス

Proposed (2026-08-31)。[0015](./0015-behavior-step-model.md)を`Superseded`に変更する。

## 背景

[0015](./0015-behavior-step-model.md)のPhase 1は、`behavior.yml`に`steps: Vec<String>`(必須・順序付き配列)を導入し、同一Behavior配下の全Conditionがこの`steps`を共有する設計を決定・実装した(commit `5480b85`)。

Phase 1実装後、実際にこのモデルでテストケースを組み立てたところ、次の問題が発生した。

1. **条件ごとに操作内容が異なる**。例えば「TODO追加」というBehaviorに対し、「空白のみのテキストを入力する」条件と「有効なテキストを入力する」条件では、入力するテキスト自体が異なる。behavior.steps一本では、この差分を表現できず、Test Designerは無理にどちらか一方に寄せた`steps`を書くか、条件間の差分を`description`の自然文に逃がすしかなかった。
2. **手順だけでは到達できない前提が条件ごとに存在する**。例えば「対象のTODOが既に削除されている」ことを前提とする条件は、behavior共通の`steps`だけでは再現できない。
3. **期待結果が単一の文([0015](./0015-behavior-step-model.md)以前からの`expected_result.description`)にしか対応しておらず、複数の観測可能な結果(例:「一覧に追加される」「入力欄がリセットされる」「入力欄にフォーカスが戻る」)を1つのExpectedResultにまとめて書けない、または実装詳細の理由づけとユーザー観点の結果が同じ1文に混在してしまう。**
4. **「追加の操作を挟んでから確認する」検証(例: 永続化確認のためページを再読み込みしてから確認する)を表現する手段がない。**現行の`generate.rs::load_knowledge_snapshot`は1つのConditionに紐づく`expected/*.yml`を全てフラットな`Vec<ExpectedSnapshot>`へ合流させ、`TestCase`は単一の`steps: Vec<String>`と単一の`expected: Vec<String>`しか持たない(`src/generate.rs`)。そのため「reloadしてから見えるはずの結果」を、reload不要な他の結果と区別して表現できない。

これらは[0015](./0015-behavior-step-model.md)がPhase 2として想定していた「同じ手順の重複・コピー更新漏れ」問題とは異なる。Phase 2は共有Stepレジストリの必要性を扱うものだったが、今回顕在化したのは**behavior単位の共有粒度そのものが、条件ごとの実際のばらつきに対して粗すぎる**という問題であり、[0015](./0015-behavior-step-model.md)のPhase 1で確定した「Behavior.stepsは全Conditionで共有する」という前提そのものが実データで崩れた。

この検討にあたり、`.markharness`本来のschemaを一時的に無視し、Gherkin(Given/When/Then/Background)の考え方をディレクトリ構造にそのまま反映したスクラッチサンプル(`examples/bdd-sample/`、本ADR確定後に削除)を作成し、何が過不足なく必要かを検討した。本ADRはその検討結果を反映する。

## 決定内容

### 1. schema変更

`behavior.schema.json`の`steps`を`preconditions`に改名し、意味を「全Conditionに共通する前提」に変更する。実際の操作手順はConditionへ完全に移す。

```yaml
# behavior.yml
id: add-todo
feature: todo-management
label: TODOの追加
axis: [ui]
description: |
  フォーム送信時に入力テキストからTODOを追加する
preconditions:
  - TODOアプリを開く
  - 入力欄が空であることを確認する
```

```yaml
# condition.yml (valid-text)
id: valid-text
behavior: add-todo
label: 有効なテキスト
description: |
  空でない有効なテキストを入力欄に入力して送信した場合
additional_preconditions: []
steps:
  - 入力欄に "牛乳を買う" と入力する
  - 「追加」ボタンをクリックする
```

```yaml
# expected/001.yml(同じ valid-text ディレクトリ配下)
id: valid-text-001
condition: valid-text
generated_by: manual
description: 有効なテキストがTODOとして追加され、入力欄がリセットされる
results:
  - TODO一覧の末尾に "牛乳を買う" という未完了のTODOが表示される
  - 入力欄は空にリセットされる
  - 入力欄にフォーカスが戻る
implementation_note: |
  addTodo() が trim 後のテキストで {id, text, completed:false} を todos に push し、
  render() が呼ばれる。submit ハンドラが input.value = "" と input.focus() を実行する
```

```yaml
# expected/002.yml (追加操作を挟む例)
id: valid-text-002
condition: valid-text
generated_by: manual
description: 再読み込み後もTODOが永続化されている
additional_steps:
  - ページを再読み込みする
results:
  - "牛乳を買う" のTODOが引き続き一覧に表示される
implementation_note: |
  addTodo() 内で saveTodos() が呼ばれ localStorage に保存されるため、
  再読み込み時の loadTodos() で復元される
```

フィールド一覧:

| entity | フィールド | 型 | 必須度 | 意味 |
|---|---|---|---|---|
| behavior | `preconditions`(`steps`から改名) | `Vec<String>` | 空配列許容(minItemsなし) | 全Conditionに共通する前提 |
| condition | `steps`(新規) | `Vec<String>` | `minItems: 1`必須 | 条件固有の操作手順(旧`behavior.steps`の粒度規約を継承) |
| condition | `additional_preconditions`(新規) | `Vec<String>` | 空配列許容 | 条件固有の追加前提(手順だけでは到達できない前提) |
| expected_result | `description`(既存、変更なし) | `String` | 必須 | 人間向け1文要約。生成には使わない |
| expected_result | `results`(新規) | `Vec<String>` | `minItems: 1`必須 | 観測可能な複数の結果。テストケース生成に使う |
| expected_result | `additional_steps`(新規) | `Vec<String>` | Condition内でファイル名順が先頭の`expected_result`のみ省略可。2番目以降は非空(1操作以上)必須 | この結果を確認する前に必要な追加操作 |
| expected_result | `implementation_note`(新規) | `String` | 任意 | 実装根拠メモ。生成には使わない |

**`expected/*.yml`のファイル分割規約**: 同じ操作の後に確認する独立した複数の観測結果は、別ファイルに分割せず同じ`expected_result.results`配列内に複数行として書く。別ファイル(`002.yml`等)を作るのは、新しい操作(`additional_steps`)を挟んでから確認する新しいphaseを表現する場合に限る。この規約を執筆時のレビューだけに委ねず機械的に強制するため、Condition内で2番目以降(ファイル名順)の`expected_result`は`additional_steps`が非空でなければならない(先頭の`expected_result`のみ省略可、または空でよい)。

この制約は`expected_result.schema.json`単体のJSON Schemaでは表現できない——`validate.rs`の`validate_file`は`expected/*.yml`を1ファイルずつ独立に検証しており、あるファイルが同じConditionディレクトリ内で何番目かをJSON Schemaは知り得ない。したがって本制約は、`axis`タグの参照整合性や`forked_from`の参照先実在チェックと同じく、`validate.rs`側のクロスリファレンスチェック(Conditionディレクトリ配下の`expected/*.yml`をファイル名順に列挙し、2番目以降で`additional_steps`が空の場合にエラーとする)として実装する。これにより「追加操作なしに新しいファイルを作る」こと自体がKnowledge検証エラーとなり、[Standards/Specレビュー(2026-09-01)](./0016-review-2026-09-01.md)が指摘した「002が独立した観測結果なのか、操作を再実行するのか、状態を保持したまま追加操作だけ行うのか」という曖昧さを構造的に排除する。

### 2. `TestCase`構造の変更(`generate.rs`)

`TestCase`の粒度は従来通り「1 Condition = 1 TestCase」を維持する。内部構造を次のように変更する。

- `TestCase.preconditions: Vec<String>` — `behavior.preconditions` + `condition.additional_preconditions`を連結したもの。`phases`とは独立したフィールドとする。§4の通り`preconditions`と`steps`は実行可能な操作として意味の種類に違いはないため、この分離は実行上の必然ではなく、人間が生成物を読む際に「共通のセットアップ」と「このConditionが実際に検証する操作」を視覚的に区別できるようにするための構造化であり、§5で述べる「人間向けの手順書」という位置づけを反映したものである。
- `TestCase.phases: Vec<Phase>` — `expected/*.yml`をファイル名順に走査して1ファイルにつき1つの`Phase { steps: Vec<String>, results: Vec<String> }`を生成する順序列。各phaseの`steps`は「そのphaseが先頭(=ファイル名順で最初の`expected/*.yml`)なら`condition.steps`を先頭に置き、それ以外は置かない」うえで、その`expected/*.yml`自身の`additional_steps`(先頭は空でもよいが、2番目以降は前述の通り`validate.rs`のクロスリファレンスチェックにより非空が必須)を末尾に連結したものとする。`results`はその`expected/*.yml`の`results`。つまり先頭phase以外の`steps`は非空の`additional_steps`のみからなり、先頭phaseの`steps`はそれ自身が`additional_steps`を持つ場合`condition.steps`の後ろに連結する(先頭の`expected/*.yml`が`additional_steps`を持たないことを前提にしない)。

これにより、`title`/`steps`/`expected`という既存のフラットな3フィールドは廃止し、`preconditions`/`phases`に置き換える。

### 3. 命名方針

Gherkin用語(Given/When/Then/Background)は採用せず、既存スキーマの命名慣習(`id`/`label`/`description`/`axis`のような非BDD用語)に寄せる。`preconditions`/`steps`/`additional_preconditions`/`additional_steps`/`results`/`implementation_note`はいずれもGherkin固有語彙ではない。

### 4. 粒度規約

[0015](./0015-behavior-step-model.md)が`behavior.steps`に定めた「1要素=1事実」の規約を、対象を変えて全ての新規配列フィールドへ引き継ぐ。ただし各フィールドの「1事実」の単位はフィールドの性質によって異なることを明記する。

- `behavior.preconditions` / `condition.additional_preconditions` / `condition.steps` / `expected_result.additional_steps`: 1要素 = 1操作([0015](./0015-behavior-step-model.md)の規約と同一)。`preconditions`/`additional_preconditions`は「実行される前提状態を確立するための操作」であり、`condition.steps`と同じく実行可能な命令形で書く(「TODOアプリを開く」「対象のTODOを削除する」のように)。「状態の記述であって操作ではない」という区別は設けない——GherkinのBackground/Givenが実際にはステップ定義として実行されるのと同様、preconditionsも実行してその状態を実際に作り出すためのものであり、単なる文書的な前提の言明ではない。両者の違いは意味の種類ではなく、実行される範囲(behaviorに共通する前提として先に実行されるか、conditionの主手順か)と共有スコープ(behavior共通かcondition固有か)だけである。
- `expected_result.results`: 1要素 = 1つの観測可能な結果

[0015](./0015-behavior-step-model.md)同様、この規約はKnowledge検証では機械的に強制せず、Test Designerのレビュー運用に委ねる。

### 5. 実行モデルとの関係(自動実行エンジンではない)

`execution_result.schema.json`が定義する通り、markharnessに自動実行エンジンはなく、テストは人間のTest Executorが手順書を読んで手動で実施し、TestCase全体につき1つの`pass`/`fail`/`skip`のみを記録する(この点は0016前から変わらない)。したがって本ADRが導入する`phases`は、人間が上から順に読んで実施する一続きの手順書であり、自動実行を前提にしたステートマシン(Phase単位の合否記録、失敗時の分岐・リトライ)ではない。Phase間の状態(reload後の永続化確認など)は、同じ人間が同じ環境で連続して手順を実施することで自然に引き継がれ、明示的な状態管理機構は不要である。teardown/cleanupの概念は本ADRでは導入しない——具体的な必要性が実データで確認された場合、[0015](./0015-behavior-step-model.md)のPhase 2以降と同じ判断基準(定量閾値を設けず、実際に不便が発生したら検討する)で別途検討する。

「自動実行のステートマシンではない」ことは、「phaseの境界に意味がない」ことを意味しない。§1で`expected/*.yml`の2番目以降に`additional_steps`を必須化したのは、自動実行の状態遷移を厳密化するためではなく、**人間が手順書を読んだときに一意に読める**ようにするためである。曖昧な手順書は、実行を自動化していないからこそ人間の誤読・誤実施に直結する(自動実行なら仕様の曖昧さはプログラムの分岐として顕在化するが、手動実行では読み手ごとに異なる解釈をされたまま気づかれない)。したがって`additional_steps`の必須化は、実行時のステートマシンではなく**執筆時の一意性(手順書としての明確さ)**を担保するための規約であり、本節の「自動実行エンジンではない」という前提とは矛盾しない。

## Acceptedへ変更する条件

- schema変更(`behavior.schema.json`の`steps`→`preconditions`改名、`condition.schema.json`への`steps`/`additional_preconditions`追加、`expected_result.schema.json`への`results`/`additional_steps`/`implementation_note`追加)が完了すること。
- `generate.rs`の`TestCase`構造変更(`preconditions`/`phases`への置き換え)と、影響を受ける既存テスト・fixtureの更新が完了すること。
- `knowledge add --edit`(`KnowledgeDraft`、`BehaviorDraft`、新設`ConditionDraft`/`ExpectedResultDraft`相当)が新フィールドを入力・検証できるよう更新され、関連テストが更新されること。
- `examples/bdd-sample/`を削除し、本ADR本文の例と重複しない状態にすること。
- `validate.rs`のクロスリファレンスチェック(§1)について、少なくとも次のケースを検証するテストが追加されていること: Condition内で先頭の`expected_result`(例: `001.yml`)は`additional_steps`を省略しても成功する、2番目以降の`expected_result`(例: `002.yml`)で`additional_steps`を省略(または空配列)にすると検証エラーになる、2番目以降の`expected_result`に`additional_steps`が1操作以上あれば成功する。

## 対象外

- サンプルの`condition.yml`にあった`examples:`(Scenario Outline風のデータパラメータ化)。実際の生成ロジックとの結合設計を含め、需要が確認された時点で別途検討する([0015](./0015-behavior-step-model.md)のPhase 2以降と同じ判断基準: 定量閾値を設けず、実際に重複・不便が発生したら検討する)。
- [0013](./0013-immutable-identity-model.md)の識別子モデルへの変更。本ADRはBehavior/Condition/ExpectedResultの`uid`運用に一切影響しない。新規フィールドは全て既存entityへの追加であり、新しい`EntityKind`を導入しない。
- [0015](./0015-behavior-step-model.md)のPhase 3・Phase 4(共有Stepレジストリ、UID、hash整合性検証)。本ADRはこれらを不要にするものではなく、判断を継続保留する。
- **Gherkin(`.feature`)との連携機能の実装**。本ADRが導入したフィールドにより、素直なFeature+Background+Scenario+Given/When/Thenの大部分は表現可能になったが、これは連携機能そのものの設計・実装ではない。今後必ず実装する機能として[product-operation.md UC8](../product-operation.md)および[論文§7 Future Work](../テスト知識管理のGit-nativeモデル_統合版.md#7-future-work)に切り出し済み。連携は**双方向のラウンドトリップとしてではなく、独立した2つの一方向機能**として設計する方針である。
  - **インポート(Gherkin→markharness)**: `.feature`ファイルを人間レビューを介して(`knowledge add --edit`相当のドラフトフロー)markharness YAMLへ変換する。`Scenario Outline`+`Examples`・`Data Table`/`Doc String`・`Rule:`キーワード・タグと`axis`の対応・非正準順序のGiven・`When`を持たないScenarioのように、markharnessの意味モデルに構造的な受け皿がない構文は自動変換の対象とせず、変換ツールが人間に警告し手動対応を求める(常に人間監督下で行う変換であるため、無条件の自動無損失変換までは要求しない)。この変換を1回限りの移行作業として使うか、`.feature`ファイルを引き続き編集し繰り返し変換にかけるかは本ADRでは規定せず、利用者の運用に委ねる。変換元へ最低限遡れるよう、変換で生成した`behavior.yml`/`condition.yml`に元の`.feature`ファイルパスと対象Scenario名(Behaviorの場合はFeature名)を記録する`source`フィールド(例: `source: { path: features/todo/add-todo.feature, scenario: 空白のみのテキスト }`)を持たせる。この`source`はトレース専用の参照情報であり`generate.rs`のテストケース生成には使わないが、同じ`.feature`ファイルが繰り返し変換にかけられた場合、変換ツールが「この`source`を持つCondition/Behaviorは既存の`uid`を維持して更新する」という照合キーとしても使える(運用として繰り返し変換を選んだ場合に限り必要になる仕組みであり、詳細は実装着手時に決定する)。
  - **エクスポート(markharness→Gherkin、都度再生成可能)**: `TestCase`から`.feature`ファイルを生成するレンダリング機能。`generated/testcases/*.yml`と同じ「生成物(コミット対象、CIで再生成一致を検証)」パターンに従う。
  実装着手時に別ADRで方針を決定する。

## 実装時の留意事項(本ADRでは決定しない)

- 各新規配列フィールドの空配列・空文字列要素の拒否をどの層(JSON Schema `minItems`/`items.minLength` か、Rust側の手続き的検証か)で実装するかは、[0015](./0015-behavior-step-model.md)が残した論点と同じく実装時に決定する。ただしPhase 1実装により`behavior.schema.json`に`minItems`/`items.minLength`を使う前例が既にできているため、今回は特段の事情がない限りこの前例に揃える。ただし`expected_result.additional_steps`の「Condition内で2番目以降は非空」という制約だけは例外で、単一ファイル検証の`expected_result.schema.json`では原理的に表現できないため、選択の余地なく`validate.rs`側のクロスリファレンスチェックとする(§1参照)。
- `expected/*.yml`のファイル名順(`001`, `002`, ...)は、従来は表示上の整列に過ぎなかったが、本ADRの`phases`導入によって**実行順序そのものを決める契約**に格上げされる。ファイル名の並び替えが静かにテストの意味を変えてしまうリスクがあるため、この契約を`condition.schema.json`または`expected_result.schema.json`のドキュメントコメント、あるいはCLIのvalidateメッセージとして明文化する必要がある。
- `case_uid`は[0013](./0013-immutable-identity-model.md)の定義通り`requirement_uid`/`feature_uid`/`behavior_uid`/`condition_uid`/`expected_result_uid`集合のみから決定的に導出され、本ADRが追加する`preconditions`/`condition.steps`/`additional_preconditions`/`additional_steps`/`results`等のテキスト内容はハッシュ入力に含めない。これらのフィールドはUID集合に影響しないため、`compute_case_uid`の計算方法自体に変更は不要である(内容変更ではTestCase identityを維持するという[0013](./0013-immutable-identity-model.md)の設計通り)。内容変更の検知は既存の`changes compute`(Featureディレクトリのtree SHA差分、論文§3.2〜3.4)が引き続き担い、本ADRのために新たな`content_hash`のような仕組みを追加する必要はない。
- `knowledge_draft.rs`の`BehaviorDraft`(`steps`→`preconditions`)、および新設が必要な`ConditionDraft`の`steps`/`additional_preconditions`欄、`ExpectedResultDraft`の`results`/`additional_steps`/`implementation_note`欄と、それぞれの空値検証・draftテンプレート文字列の更新が必要。
- `generate.rs`の`compile_testcases`が前提とする「`TestCase.steps`/`expected`は単一のフラット配列」という既存テストヘルパー・アサーションの洗い出しと書き換えが必要(影響範囲の網羅的な洗い出しは実装着手時の`checklist-<task>.md`で行う)。
- `[knowledge].schema_version`(ADR 0014)はv1のまま維持する。[ADR 0014](./0014-knowledge-schema-version-persistence.md)の決定内容11(「プロトタイプ段階(0.x)では本ADRのバージョニング契約を適用しない」)が定める通り、本プロジェクトが1.0の基準(`PROJECT.md`が定める、スキーマ安定化を含む)を満たすまでは、Knowledgeスキーマの破壊的変更があっても`[knowledge].schema_version`を上げない。この方針は本ADR固有のものではなくADR 0014自身の契約の一部として記録されているため、ここでは単にその適用を確認するにとどめる。あわせて、実データが`examples/bdd-sample/`以外に存在しないため、[0015](./0015-behavior-step-model.md)と同じ理由で今この時点での旧形式実データとの誤比較リスクもない。
