# markharness CLI マニュアル

**Status**: Implemented(実装済みコマンドは1章)/ Draft(未実装コマンドの暫定案は2章)
**関連ドキュメント**: [product-operation.md](./product-operation.md)(ユースケース対応)、[testcase-generation-design.md](./design/testcase-generation-design.md)(`generate`の生成規則)、[decisions/0027](./decisions/0027-declarative-knowledge-reconciliation.md)(`knowledge reconcile`の設計)

**位置づけ**：本資料は `markharness` CLI の使用方法を、**実装済みコマンド**と**未実装(今後実装予定)のコマンド**に分けてまとめたものです。ユースケース(UC1〜UC8)の対応は `docs/product-operation.md` の「3. ユースケース記述」表に基づきます。実装済みコマンドの具体的な生成規則は `docs/design/testcase-generation-design.md` を参照してください(ただし `generate`/`verify` の現行実装は、同ドキュメント作成後に `feature → behavior → condition → expected` の4階層モデルへ刷新されており、詳細は本マニュアル 1.3/1.4 節を正としてください)。

---

## 1. 実装済みコマンド

### 1.1 `markharness init` — プロジェクトの初期化(UC1〜UC8 の前提)

```text
markharness init
```

**用途**: UC1〜UC8を支える物理ディレクトリ構成(論文 §3.5, 244-273行目)のうち、対象リポジトリ上に作成が必要な6ディレクトリを作成し、以降のコマンドが動作できる状態にする。

6ディレクトリはすべて単一の `.markharness/` 名前空間の下に作成され、対象プロジェクトに既存のトップレベル `knowledge/` や `schema/` と衝突しない:

```text
.markharness/
├── knowledge/
├── axes/
├── generated/
├── executions/
├── changes/
└── schema/
```

| ディレクトリ               | 対応UC                                                              |
| --------------------------- | ------------------------------------------------------------------- |
| `.markharness/knowledge/`  | UC1(知識を記述する)/ UC1b(forked_from を手動記述する)               |
| `.markharness/axes/`       | UC1(横断的観点 Axis のレジストリ、§3.1)                             |
| `.markharness/generated/`  | UC2(TestCaseを決定的生成する)/ UC3(生成物をレビュー・マージする)    |
| `.markharness/executions/` | UC4(マイルストーンをタグ付けする、実行結果の記録先)                 |
| `.markharness/changes/`    | UC5(ChangeEventを自動計算する)/ UC6(バックフィルを非同期実行する)   |
| `.markharness/schema/`     | UC7(idキャッシュを破棄・再構築する。フォーマット・正規化ルール定義) |

UC8(既存ツールからのインポート)は専用ディレクトリを持たず、変換結果を `.markharness/knowledge/` に書き込む想定のため対象外。

**動作**

- 各ディレクトリについて、存在しなければ作成し、既に存在すればそのまま(中身も含めて)何もしない冪等な処理。すでに初期化済みのプロジェクトで再実行してもエラーにはならず、不足しているディレクトリだけが追加で作成される。
- `.markharness/config.toml`(`schema_version = 1` と `[knowledge]\nschema_version = 1` を含む、[decisions/0014](./decisions/0014-knowledge-schema-version-persistence.md))を作成する。トップレベルの`schema_version`は`init` 以外の全コマンドがプロジェクトルートを見つけるための目印であり、`--dir` 省略時は上位ディレクトリを遡って探索し、`--dir` 明示時もその実在を検証する(見つからなければ `markharness init` を促すエラーになる)。`[knowledge].schema_version`はこれとは独立したスコープを持つ値で、`changes compute`(1.9節)がKnowledgeスキーマの移行を検出するためにref単位で解決する。リポジトリにコミットする(`.gitignore` の対象にしない)。既に存在する場合は上書きしない。
- 成功すると作成先のパスを標準出力に表示する。

**使用例**

```console
$ markharness init
initialized .markharness/{knowledge,axes,generated,executions,changes,schema}/ under /path/to/project

$ markharness init
initialized .markharness/{knowledge,axes,generated,executions,changes,schema}/ under /path/to/project
```

**ユースケース対応**: どのUCにも明示的には現れないが、UC1〜UC8の全ユースケースを開始する前提条件を満たすための補助コマンド。

---

### 1.2 `markharness knowledge reconcile` — Knowledge Intentの宣言的反映(UC1: 知識を記述する)

```text
markharness knowledge reconcile <intent-file> [--check] [--json] [-d, --dir <path>]
markharness knowledge reconcile --print-template
```

**用途**: Knowledge Intent(望ましい状態を記述したYAML)を1ファイルで与え、現在のリポジトリ状態と突き合わせて、Requirement・Feature・Behavior・Scenarioの作成・更新・renameを単一トランザクションで反映する。Knowledge authoringの書込みInterfaceはこのコマンドだけであり、人もAIも同じ経路を使う([decisions/0027](./decisions/0027-declarative-knowledge-reconciliation.md)・[decisions/0028](./decisions/0028-consolidate-knowledge-authoring-commands.md))。

Intentは**手続きではなく望ましい状態**を記述する。新規要素はドキュメント内ローカルな `key` で相互参照し(`key` は保存されない)、既存要素は `uid` で選択する。同じIntentを再実行しても、内容が一致する要素は `unchanged` となり書込みは発生しない。

**オプション**

| オプション         | 説明                                                                                                     |
| ------------------ | -------------------------------------------------------------------------------------------------------- |
| `<intent-file>`    | Knowledge Intent YAMLのパス。`--print-template` と排他(いずれか一方が必須)                               |
| `--print-template` | 空のKnowledge Intent雛形を標準出力へ出す。他のオプションとは併用できない                                  |
| `--check`          | 解析・照合・検証・mutation plan生成まで通常実行と同じ実装で行い、**書込みだけを行わない**                 |
| `-d, --dir <path>` | 対象プロジェクトディレクトリ(`.markharness/knowledge/` の親)。省略時はプロジェクトルート(cwdから上位探索) |
| `--json`           | 結果・診断を1行のJSONで出力する。省略時は人間可読なテキストを出力する                                     |

**終了コード**

| コード | 意味                                                                                     |
| ------ | ---------------------------------------------------------------------------------------- |
| 0      | 成功(通常実行は反映完了、`--check` は「変更不要」)                                       |
| 1      | 検証エラー(診断を出力する)                                                               |
| 3      | 他のidentity操作が進行中、または前回の操作の回復が保留されている                         |
| 4      | `--check` で、反映すれば変更が生じる状態(スクリプトが「変更あり」を出力解析なしに判定可能) |

`--check` は計画までを共有実装で行うが、その結果を後続書込みの許可証としては扱わない。通常実行はコミット直前に現在状態を再確認し、入力状態が変化していれば stale plan として停止する。

**Knowledge Intentの形式**

雛形は `markharness knowledge reconcile --print-template` で取得できる。

```yaml
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: req_todo # ドキュメント内ローカルな参照名。保存されない
    id: todo # 必須。ASCII slug
    source: native # native | external
    label: TODO management # source: native では必須
    axis: [functional] # 登録済みaxisのみ。未登録は unknown_axis エラー
    description: null # 省略可
    related_issues: [] # 省略可

features:
  - key: feature_todo_add
    id: todo-add
    contributes_to: [req_todo] # Requirementの key(新規)または uid(既存)
    label: Add a TODO
    axis: [functional]
    description: null # 省略可
    forked_from: null # 省略可。概念的な派生元Featureのid(§3.1)。実在するFeatureのidであること
    behaviors:
      - id: add
        label: Add
        axis: [functional]
        description: The user adds a TODO item. # Behaviorは新規作成時にdescription必須
        procedures: # 省略可。このBehaviorが宣言する共通手順(ADR 0017)
          - name: open-app
            steps:
              - Launch the application.
        scenarios:
          - id: empty-list
            label: Empty list
            description: Adding to an empty list
            phases:
              - steps:
                  - use: open-app # procedures で宣言した共通手順の呼び出し
                  - action: Type a title and submit. # 1要素=1操作
                results:
                  - The item appears in the list. # 1要素=1つの観測可能な結果
            implementation_note: null # 省略可。実装根拠メモ。生成には使わない(ADR 0016)
```

`mode` は初期版では `merge` のみを受け付ける(既存要素を削除しない)。`format` が `markharness/knowledge-intent/v1` 以外なら `invalid_format` エラーになる。

文字列fieldを空文字列(または空白のみ)で与えることは、省略とは区別して `missing_required_field` で拒否される。`label: ""` は `label: ` として書き出されYAML nullとして読み戻るため保存ファイルが壊れ、空の `action` や `results` はTest Executorが実施・観測できない記述になるため。

Requirementの`native`と`external`はfieldの集合が排他である(ADR 0023)。`native`は自身の内容を所有するため`label`を必須とし、`source_locator`を持てない。`external`は外部ドキュメントが内容を所有するため`label`と`description`のどちらも持てず、`source_locator`と`source_revision: current`が必須で、後者は実行時に現在のblob OIDへ解決される。`source`を切り替えるpatchでは、新しいmodeが持てないfieldは破棄され、新しいmodeが必須とするfieldがIntentにも現在値にも無ければ`missing_required_field`で停止する(例: externalからnativeへ切り替えるIntentは`label`を与える必要がある)。

**既存要素の更新・rename**

既存要素は `key`/`id` ではなく `uid` で選択する。UIDは反映成功時の出力、または `--json` のsnapshotから得る。

```yaml
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: 01J8Z... # 既存Featureを選択
    id: todo-create # display IDを変えるとrenameになる(uidは保持される)
    label: Create a TODO # 省略したfieldは現在値を保つ
    contributes_to: [01J8A..., 01J8B...] # collectionは全置換
```

- 値のcollection(`axis`・`contributes_to`・`procedures`)は、記述すると**全置換**される。省略すると現在値を保つ。空配列を明示すれば空になる。
- renameは `uid` で選択した要素の `id` を変えるだけで行う。uidとidentity eventは保持される。
- Scenarioの親Behaviorを変えるreparentは、`uid` で選択したScenarioを別のBehaviorの下に記述して行う。ファイルの移動は反映結果の `previous_path` で報告される。
- FeatureとRequirementの関連の追加・削除は、`contributes_to` の全置換で表現する。
- external Requirementの固定参照更新は `source_revision: current` で行う。現在のblob OIDへ再固定される。`source: native` に対して指定するとエラーになる。

**出力**

人間可読モードでは、要素ごとに `created` / `updated` / `unchanged` の行を出力する(変化が一切なければ `no changes`)。

```text
created requirement 'todo' (uid 01J8A...) .markharness/knowledge/requirements/todo.yml
updated feature 'todo-create' (uid 01J8Z...) .markharness/knowledge/features/todo-add.yml -> .markharness/knowledge/features/todo-create.yml
unchanged behavior 'add' (uid 01J8C...) .markharness/knowledge/features/todo-create/behaviors/add.yml
```

`--json` では `{"ok":true,"created":[...],"updated":[...],"unchanged":[...]}` を1行で出力する。各要素は `kind` / `uid` / `id` / `path` を持ち、ファイルが実際に移動した場合のみ `previous_path` が付く。検証エラー時は `{"ok":false,...}` 形式で診断コード・位置・メッセージを返し、人間可読モードでは `error[<code>]: <message> (<location>)` を標準エラーへ出力する。

**原子的保存**: Knowledgeファイル群とidentity eventの書込みは単一トランザクションで行われ、途中で中断してもUIDなしKnowledgeや片側だけ更新された状態を後続コマンドへ公開しない。中断が検出された場合は終了コード3で回復が保留されている旨を報告するので、`--check` を付けずに一度実行して回復を完了させる。

**前提**: Intentが参照するaxisは事前に `axes add`(1.4節)で登録しておく。未登録axisは `unknown_axis` エラーとして反映前に弾かれる。

### 1.3 `markharness generate` — TestCase の決定的生成(UC2: TestCaseを決定的生成する)

```text
markharness generate [--json] [-d, --dir <path>]
```

**用途**: `.markharness/knowledge/` 配下を決定的に走査し、`Requirement × Feature × Behavior × Condition × ExpectedResult` から `TestCase` を機械的に組み立てて、`.markharness/generated/testcases/` 配下に **1 Condition = 1 ファイル** の `.yml` として再生成する。実行のたびに `.markharness/generated/testcases/` を空にしてから書き直すため、削除された Condition に対応する古いファイルも自動的に消える。

**アクター**: 本来は CI Bot(UC2)だが、ローカルでの事前確認用に手動実行も可能。

**アルゴリズム概要**

- `.markharness/knowledge/` 配下を `requirement.yml` → `feature.yml` → `behavior.yml` → `condition.yml` → `expected/*.yml` の順に、パスのソート順で走査する(実行環境・タイムスタンプに依存しない)。`Behavior` を持たない `Feature` や `expected/` が空(または存在しない)の `Condition` からは `TestCase` は生成されない。
- **集約モデル**: 1つの `Condition` の `expected/` 配下にある全ファイルを、1つの `TestCase` の `phases` 配列に集約する(1 Condition = 1 TestCase)。
- `case_id = "tc-{requirement.id}-{feature.id}-{behavior.id}-{condition.id}"`。`requirement`/`feature`/`behavior`/`condition` の4つのidをすべて連結することで、`condition.id` が別の Behavior で再利用されても `case_id` の衝突が構造的に起こり得ないようにしている。
- 出力ファイルは `.markharness/generated/testcases/{requirement.id}/{feature.id}/{behavior.id}/{condition.id}.yml` に、`.markharness/knowledge/` と同じ階層でフルミラーして書き込まれる(旧版は `.markharness/generated/testcases/{condition.id}.yml` というフラットな命名で、異なる Behavior 配下で同じ `condition.id` が再利用されると無言で上書きされる欠陥があった)。
- [ADR 0016](decisions/0016-behavior-condition-precondition-step-result-model.md)により、`title`/`steps`/`expected` という旧フィールドは廃止され `preconditions`/`phases` に置き換わっている。`preconditions` = `behavior.preconditions` + `condition.additional_preconditions` を連結したもの(`behavior.description`/`condition.description` は生成には使わない人間向け要約のまま)。`phases` は `expected/*.yml` をファイル名順に走査して1ファイルにつき1つの `Phase { steps, results }` を生成した配列で、先頭のphaseの `steps` は `condition.steps` の後ろにその `expected/*.yml` 自身の `additional_steps`(あれば)を連結したもの、2番目以降のphaseの `steps` はその `expected/*.yml` の `additional_steps` のみ(Condition内で2番目以降は`markharness validate`により非空が必須)。各phaseの `results` はその `expected/*.yml` の `results`。
- `generated_from` に `requirement` / `feature` / `behavior` / `condition` の各 id と、集約元の `expected_results`(`expected/*.yml` の `id` の一覧)を記録する。
- `axis`: `Requirement` / `Feature` / `Behavior` の `axis` を合成(union、重複除去のうえソート)した観点一覧(§3.4「axisの継承」)。
- 出力は `serde_yaml_ng` によるシリアライズで、同一入力に対して常に同一の出力になる(決定性、CIでの差分検証の前提)。
- `generate` は `.markharness/generated/testcases/*.yml` に加えて `.markharness/generated/traceability-index.json`(Requirement → Feature → Behavior → Condition → TestCase の対応関係を持つ機械可読索引。`serde_json` による整形済みJSON)も同時に再生成する。`markharness verify`(1.4節)はこのファイルも差分検証対象に含める。
- `--dir` を省略すると、カレントディレクトリから上位へ `.markharness/config.toml` を探索して見つかったプロジェクトルートを対象にする(他のコマンドと同じ規約。以前は `generate` だけこのオプションを持たず常にカレントディレクトリ固定だった)。
- `--json` を指定すると、人間可読メッセージの代わりに `{"ok":true,"generated":<件数>,"written":[<書き込んだファイルパスの一覧(traceability-index.jsonを含む)>]}` を出力する。表示上の件数と実際に書き込まれたファイル数が食い違っていないかを、呼び出し側が機械的に突き合わせられるようにするための出力。

**使用例**

```console
$ markharness generate
generated 1 testcase(s) into .markharness/generated/testcases/
$ markharness generate --json
{"ok":true,"generated":1,"written":[".markharness/generated/testcases/req-todo/todo/todo-add-task/todo-add-task-empty-input.yml",".markharness/generated/traceability-index.json"]}
```

`.markharness/generated/testcases/task-management/add-todo/add-task/empty-title.yml`:

```yaml
case_id: tc-task-management-add-todo-add-task-empty-title
generated_from:
  requirement: task-management
  feature: add-todo
  behavior: add-task
  condition: empty-title
  expected_results:
    - empty-title-001
preconditions:
  - "Open the todo app."
phases:
  - steps:
      - "Click the title field."
      - "Press the add button."
    results:
      - "A validation error is shown under the title field."
```

`.markharness/knowledge/` に何も無い場合は `.markharness/generated/testcases/` が空(0ファイル)になる。

**ユースケース対応**: UC2「TestCaseを決定的生成する」(`docs/product-operation.md` 105行目)。CI上での差分検証(UC3)は 1.4 節の `markharness verify` で行う。

---

### 1.4 `markharness verify` — 生成物の差分検証(UC3: 生成物をレビュー・マージする)

```text
markharness verify [--json] [-d, --dir <path>]
```

**用途**: `.markharness/knowledge/` から `generate` と同じロジックで TestCase と `traceability-index.json` を再構築し(ディスクへは書き込まない)、コミット済みの `.markharness/generated/testcases/*.yml` および `.markharness/generated/traceability-index.json` と比較する。CI上でこのコマンドを実行し、`.markharness/knowledge/` の変更を `.markharness/generated/` へ反映し忘れていないかを検証する想定(`generate --check` に相当する用途はこのコマンドが既に担っている)。

**アクター**: Reviewer / CI Bot(UC3)

**オプション**

| オプション          | 説明                                                                          |
| -------------------- | ----------------------------------------------------------------------------- |
| `-d, --dir <path>`   | 対象プロジェクトディレクトリ。省略時はプロジェクトルート(cwdから上位探索で自動検出)                    |
| `--json`             | 人間可読メッセージの代わりに構造化JSONを出力する(下記参照)                    |

**動作**

- 差分が無ければ `.markharness/generated/testcases/ is up to date with .markharness/knowledge/` を表示し、終了コード `0`。
- 差分があれば、追加・削除・変更されたファイルを `added:` / `removed:` / `changed:` のラベル付きでファイル名のソート順に一覧表示し、終了コード `1` で終了する(内容のunified diffまでは表示しない)。`.markharness/generated/traceability-index.json` も他の生成物と同じ扱いで一覧に含まれる(ファイル名は `traceability-index.json`)。
- `--json` 指定時は差分の有無にかかわらず `{"would_change":<bool>,"added":[...],"changed":[...],"removed":[...]}` を出力する。各パスは `.markharness/generated/` からの相対パスで、TestCaseファイルは `testcases/` 接頭辞付き(例: `testcases/task-management/add-todo/add-task/empty-title.yml`)、`traceability-index.json` はそのままの名前(`.markharness/generated/testcases/` 配下ではなく `.markharness/generated/` 直下にあるため)。差分が無ければ終了コード `0`(`would_change:false`)、あれば `1`(`would_change:true`)。

**使用例(差分なし)**

```console
$ markharness verify
.markharness/generated/testcases/ is up to date with .markharness/knowledge/
$ markharness verify --json
{"would_change":false,"added":[],"changed":[],"removed":[]}
```

**使用例(差分あり)**

```console
$ markharness verify
added: .markharness/generated/testcases/task-management/add-todo/add-task/empty-title.yml
changed: .markharness/generated/testcases/task-management/add-todo/add-task/max-length.yml
removed: .markharness/generated/testcases/task-management/add-todo/add-task/duplicate-title.yml
$ echo $?
1

$ markharness verify --json
{"would_change":true,"added":["testcases/task-management/add-todo/add-task/empty-title.yml"],"changed":["testcases/task-management/add-todo/add-task/max-length.yml"],"removed":["testcases/task-management/add-todo/add-task/duplicate-title.yml"]}
$ echo $?
1
```

**ユースケース対応**: UC3「生成物をレビュー・マージする」(`docs/product-operation.md` 106行目)。差分が検出された場合、その内容が意図したものかどうかを判断してマージするのはReviewerの役割(人間の判断ポイント)。

---

### 1.5 `markharness axes list` — 観点(axis)レジストリの一覧表示

```text
markharness axes list [--json] [-d, --dir <path>]
```

**用途**: `.markharness/axes/*.yml` に登録済みの観点一覧を、id昇順で出力する。`knowledge reconcile` の `unknown_axis` エラーを事前に回避するための参照コマンド。

**動作**: `--json` 未指定時は `id (label)`(label が id と同じ場合は id のみ)を1行ずつ表示し、登録が0件なら `no axes registered under .markharness/axes/` と表示する。`--json` 指定時は `[{"id":...,"label":...|null}]` を1行のJSONで出力する。

**使用例**

```console
$ markharness axes list --dir tmp/todo-sample
gameplay (Gameplay)
ui

$ markharness axes list --dir tmp/todo-sample --json
[{"id":"gameplay","label":"Gameplay"},{"id":"ui","label":null}]
```

**ユースケース対応**: どのUCにも明示的には現れない補助コマンド。

---

### 1.6 `markharness axes add` — 観点(axis)の非対話登録

```text
markharness axes add <id> [--label <label>] [--json] [-d, --dir <path>]
```

**用途**: `.markharness/axes/<id>.yml` を新規作成する。Knowledge Intentが参照するaxisは登録済みである必要があり(未登録axisは `unknown_axis` エラーとして反映前に弾かれる)、`axes add` はそのための、他のリソース(Requirement/Feature/Behavior/Scenario)と対称的な単体の書き込みコマンド。

**動作**

- `<id>` は `condition.id` 等と同じスラッグ制約(小文字英数字とハイフンのみ)。不正な場合は終了コード `2`。
- `--label` を省略すると `label` は `<id>` と同じ値になる(他コマンドと同じ「省略時はidをlabelにも使う」規約)。
- `.markharness/axes/<id>.yml` が既に存在する場合は**上書きしない**。エラーメッセージを表示して終了コード `2` で終了する(既存リソースを触りたい場合は現状ファイルを直接編集する運用)。
- `--json` 指定時は `{"ok":true,"written":[".markharness/axes/<id>.yml"]}` を出力する。

**使用例**

```console
$ markharness axes add persistence --dir tmp/todo-sample
created tmp/todo-sample/.markharness/axes/persistence.yml

$ markharness axes add persistence --dir tmp/todo-sample
error: axis 'persistence' already exists under .markharness/axes/
$ echo $?
2

$ markharness axes add security --label Security --dir tmp/todo-sample --json
{"ok":true,"written":["tmp/todo-sample/.markharness/axes/security.yml"]}
```

**ユースケース対応**: `markharness axes list`(1.5節)と同じく、どのUCにも明示的には現れない補助コマンド。

---

### 1.7 `forked_from`(UC1b: 別Featureからの概念的派生を手動記述する)

`knowledge reconcile`(1.2節)のKnowledge Intentで、Featureの `forked_from` に派生元Featureのidを記述する(§3.1)。参照先のFeatureが `.markharness/knowledge/` 配下のどこにも存在しない場合は `unknown_forked_from` エラーで停止する。Git履歴からは自動導出できないドメイン知識のため、`derived_from`(同一Featureの版履歴、§3.2〜3.4)とは異なり検証のみ行い自動計算はしない。

```yaml
feature:
  id: player-double-jump
  label: player-double-jump
  axis: [gameplay]
  forked_from: player-jump # 概念的な派生元(既存Feature id)。省略可
```

---

### 1.8 `markharness cache rebuild` — idキャッシュの破棄(UC7: idキャッシュを破棄・再構築する)

```text
markharness cache rebuild [-d, --dir <path>]
```

**用途**: `.markharness-cache/`(1.9節の `changes compute` が使う、Featureのid→tree SHA解決結果の非コミットキャッシュ。内容アドレス方式のキーで格納されており、`.markharness/knowledge/`の内容やツールのバージョンが変われば読み込み時に自動的に再計算されるため、通常は明示的な`rebuild`は不要)を丸ごと削除する。即時の再計算は行わない(次回 `changes compute` 実行時に遅延計算される)。キャッシュディレクトリが存在しない場合もエラーにならない(冪等)。

**使用例**

```console
$ markharness cache rebuild
removed .markharness-cache/ under /path/to/project
```

**ユースケース対応**: UC7「idキャッシュを破棄・再構築する」(`docs/product-operation.md`)。id解決の不整合が疑われる場合のフェイルセーフ。

**Featureの`id:`を変更した場合の注意(利用者向け、論文§3.3)**: Feature idは各`feature.yml`の`id:`フィールドを正準ソースとして追跡する。`id:`の値そのものを書き換えると、ツールから見て「元のFeatureが削除され、新しいidのFeatureが追加された」扱いになり、`changes compute`は過去のマイルストーンとの`derived_from`関係を復元できない(版履歴が断絶する)。Featureディレクトリの**リネーム**(パス変更)は`id:`が変わらない限り追跡対象内だが、`id:`自体の変更に対する移行手順(旧id→新idのエイリアス記録等)は本CLIには無く、現状は「`id:`を変更しない」運用を利用者側で徹底する必要がある。検討状況は[decisions/0004](./decisions/0004-feature-id-change-migration.md)を参照。

**キャッシュキーのバージョンフィールドについて**: `.markharness-cache/`のキャッシュキーを構成する`canonicalization_rule_version`/`id_index_schema_version`(論文§3.3)は、実装では現状固定値`"1"`である。これらの値を実際に上げる正規化ルール改訂・id-indexフォーマット改訂はまだ発生していないため、値を上げた場合にキャッシュが正しく破棄されるかは実地検証されていない。

---

### 1.9 `markharness changes compute` — ChangeEventの算出(UC5: ChangeEventを自動計算する)

```text
markharness changes compute <from-milestone> <to-milestone> [--no-cache] [--current-tree] [--granularity <feature|behavior|condition>] [-d, --dir <path>]
```

**用途**: 2つのマイルストーン(git tag名をそのまま使用。マイルストーン境界の判定はタグ名一致のみで、`.markharness/executions/*/milestone.yml` との対応は呼び出し側の責務)間で、`.markharness/knowledge/` 配下の各Featureディレクトリのtree SHAを `git ls-tree -r <tag> -- .markharness/knowledge` で比較し、変化したFeatureごとに `ChangeEvent` を算出して `.markharness/changes/<to-milestone>.yaml` に書き込む。Feature idは各`feature.yml`の`id:`フィールドを正準ソースとし、ディレクトリ名とは独立に追跡する(論文§3.3)。

対象プロジェクトディレクトリ(`-d`/`--dir`、`.markharness/knowledge/` の親)は、gitリポジトリ内の任意のディレクトリでよい(リポジトリ自体のルートである必要はない)。かつては`git show <ref>:<path>`構文の仕様上の制約により、プロジェクトディレクトリがリポジトリのサブディレクトリの場合に本コマンドが失敗する既知の問題があったが、`ls-tree`/`cat-file`ベースの実装に切り替えて解消済み(詳細: [decisions/0006](./decisions/0006-nested-project-directory-support.md))。

**アクター**: CI Bot(UC5)

**動作**

- 比較を始める前に、`from-milestone`・`to-milestone`双方の`.markharness/config.toml`から`[knowledge].schema_version`を解決する([decisions/0014](./decisions/0014-knowledge-schema-version-persistence.md))。バージョンが記録されていないrefはlegacyスキーマバージョン1とみなし、その旨をwarningとして出力に含める(後述)。解決した2つのバージョンが異なる場合、またはいずれかがこのCLIビルドが知らない未来のバージョンである場合は、何も書き込まずエラー終了する — ChangeEventは生成されず、既存の`.markharness/changes/<to-milestone>.yaml`も変更されない(fail closed。スキーマバージョンをまたいだ生のtree SHA差分は、schema-only migrationをFeatureの変更として誤検出しうるため)。
- Feature単位で `from_blob`/`to_blob` を比較し、一致すれば何もしない。片方にのみ存在すれば追加/削除、両方に存在し値が異なれば変更として `ChangeEvent` を1件生成する。
- `impacted_testcases` は、変更されたFeatureに由来する `TestCase.case_id` を、`generate`(1.3節)と同じ生成グラフ(§3.2(A)の構造的生成グラフ。版履歴は使わない)から列挙したもの。どの時点の `.markharness/knowledge/` からこの生成グラフを構築するかは2026-08以降2モードに分かれる(2026-08-12時点、[change-event-verification-tracking-spec.md](./design/change-event-verification-tracking-spec.md) §2.4も参照)。
  - **既定(`--current-tree`未指定)**：`to-milestone`タグが指す`.markharness/knowledge/`ツリーをGit blobから直接読み込んで構築する。同じ区間を後日再計算しても常に同じ結果になる。
  - **`--current-tree`指定時**：現在の作業ツリーの`.markharness/knowledge/`から構築する(従来動作)。作業ツリーが変化し続ける限り、同じ区間の再計算結果も変わりうる。
- **`--granularity <feature|behavior|condition>`(既定: `feature`)**：`impacted_testcases`を絞り込む単位を選択する(issue #15)。
  - **`feature`(既定)**：従来通り、変更が検出されたFeatureに由来する全TestCaseを候補として含める(保守的・安全側)。
  - **`behavior`**：Feature配下のBehaviorディレクトリ(`behavior.yml`)ごとにtree SHAを比較し、実際に変化した(または追加/削除された)Behaviorに由来するTestCaseのみを候補に含める。変化していない兄弟Behaviorの分は除外される。
  - **`condition`**：同様にConditionディレクトリ(`condition.yml`)単位でさらに絞り込む。
  - `behavior`/`condition`はFeature単位の変更検出そのもの(どのFeatureに`ChangeEvent`を1件生成するか、rename追跡、`true_divergences`判定)には影響しない。影響するのは`impacted_testcases`の絞り込みのみ。
  - **注意(false negativeのリスク)**: Behavior/Conditionのスキーマには兄弟間の依存関係を表すフィールドが存在せず、本コマンドはそれを検出・推論しない。Feature境界には著者が暗黙に込めた関連性(共有のセットアップ、前提条件等)が含まれている可能性があり、`behavior`/`condition`はその関連性を意図的に無視した上で再現率(recall)を精度(precision)と引き換える機能である。この判断はツール側では保証できないため、利用者が個々のプロジェクトの実情に応じて選択する必要がある。
  - 選択した粒度と、絞り込みの根拠は算出された各`ChangeEvent`の`impact_reason`フィールド(`granularity`と`changed_paths`)に記録される(後述の出力例を参照)。`changed_paths`は`behavior`/`condition`のときのみ、実際にtree SHAが変化した(または追加/削除された)Behavior/Conditionのマーカーファイルパス(`behavior.yml`/`condition.yml`)の一覧であり、`feature`のときは空配列になる(Feature単位では個々のBehavior/Conditionを解決しないため)。
- `change_type`(仕様変更/バグ修正等)は算出時には `null` のまま出力する。人間が `markharness changes annotate`(1.15節)で事後入力する運用(§3.5)。
- `--no-cache` を指定しない場合、Feature tree SHA解決結果を内容アドレス方式でキー化された `.markharness-cache/` に読み書きする(1.8節)。
- 成功時、人間向け出力にはlegacyスキーマバージョン1へフォールバックした側ごとに`warning: ...`行が追加される。`--json`出力では同じメッセージが既存のJSON envelope内の`"warnings"`配列として含まれる。両refが`[knowledge].schema_version`を記録している場合はどちらも出力されない — JSON側の`"warnings"`キーは`[]`としてではなく、キー自体を省略する。同一`schema_version`内での追加はoptionalなフィールドに限られるため([verification-plan-canonical-model-design.md](./design/verification-plan-canonical-model-design.md)§5)。
- `from-milestone`・`to-milestone`のいずれかに`.markharness/executions/<name>/milestone.yml`が存在し、その記録された`commit_oid`/`knowledge_schema_version`がそのtagの現在の解決結果と食い違っている場合(tagの移動、または手編集)、何も計算せずエラー終了する([decisions/0014](./decisions/0014-knowledge-schema-version-persistence.md))。これらのフィールドを持たない`milestone.yml`は検証対象外。
- `from-milestone..to-milestone` の区間を `git rev-list --ancestry-path` で走査し、区間内に存在する全ての2親マージコミットそれぞれについて `git merge-base` を用いて1.16節の`lineage`判定ロジックを内部で実行する(古い順)。対象Featureがいずれかのマージで`true_divergence`(真の分岐)と判定されると、`true_divergences` フィールドに `merge_commit`(監査用のマージコミットSHA)と `parent_tree_shas: [P1, P2]` の組を、発生した順に追記する(§3.2)。同一Featureが区間内で複数回真の分岐を起こした場合もすべて記録される。通常の線形履歴、または区間内にマージが無い場合は空配列のまま。
- **ブランチ戦略への依存に注意**：`from_tree_sha`/`to_tree_sha`の差分検出そのものはブランチ戦略(merge/squash/rebase/fast-forward)に依存しないが、`true_divergences`はマイルストーン区間内に2親を持つマージコミットが実際に残っていることが前提であり、squash mergeやrebase・fast-forward mergeでは元ブランチの分岐関係がコミットグラフから失われるため検出されない(空配列のまま。論文§3.4表2)。

**出力例**(`.markharness/changes/m2.yaml`、線形履歴の場合)

```yaml
- event_id: player-jump--m1--m2
  feature_id: player-jump
  from_milestone: m1
  to_milestone: m2
  from_tree_sha: 1a2b3c...
  to_tree_sha: 4d5e6f...
  impacted_testcases:
    - tc-ground-001
  impact_reason:
    granularity: feature
    changed_paths: []
  change_type: null
  true_divergences: []
```

**出力例**(区間内のマージで真の分岐が検出された場合)

```yaml
- event_id: player-jump--m1--m2
  feature_id: player-jump
  from_milestone: m1
  to_milestone: m2
  from_tree_sha: 1a2b3c...
  to_tree_sha: 7c8d9e...
  impacted_testcases:
    - tc-ground-001
  impact_reason:
    granularity: feature
    changed_paths: []
  change_type: null
  true_divergences:
    - merge_commit: 9f8e7d...
      parent_tree_shas:
        - 2b3c4d...
        - 5e6f7a...
```

**出力例**(`--granularity behavior`指定時、Feature配下の一部Behaviorのみが変更された場合)

```yaml
- event_id: player-jump--m1--m2
  feature_id: player-jump
  from_milestone: m1
  to_milestone: m2
  from_tree_sha: 1a2b3c...
  to_tree_sha: 4d5e6f...
  impacted_testcases:
    - tc-ground-001
  impact_reason:
    granularity: behavior
    changed_paths:
      - .markharness/knowledge/controls/player-jump/jump/behavior.yml
  change_type: null
  true_divergences: []
```

**ユースケース対応**: UC5「ChangeEventを自動計算する」。本モデルの核心的貢献(§3.2〜3.4)の簡易実装。

---

### 1.10 `markharness backfill run` — 過去マイルストーンの一括処理(UC6: バックフィルを非同期実行する)

```text
markharness backfill run [--no-cache] [--max-pairs <count>] [--time-budget <duration>] [-d, --dir <path>]
```

**用途**: `.markharness/executions/*/milestone.yml` が存在するマイルストーンを対象に、対応する git tag のコミット日時(committer date)で新しい順に並べ、隣接する2マイルストーンごとに `changes compute`(1.9節)相当の処理を実行して `.markharness/changes/<milestone>.yaml` を生成する。1回の実行で全ペアを処理し終了する(常駐デーモンではない。CI等からの定期実行を想定)。

**動作**

- 最も古いマイルストーンは比較対象がないためスキップされる。
- 各マイルストーン(to側)の処理完了は `git notes --ref=markharness-backfill` に記録され、次回実行時に同じペアは再計算されずスキップされる(§4.3)。
- Knowledgeスキーマバージョンを安全に比較できないペア(`changes compute`と同じfail closedの判定、1.9節、[decisions/0014](./decisions/0014-knowledge-schema-version-persistence.md))は、run全体を中断せずそのペアだけスキップする — 残りのペアは処理が続く。このスキップは`git notes`には記録されないため、後続のrunで自動的に再試行される(例: 該当スキーマバージョンのconverterが実装された時点で)。スキップされた各ペアは`skipped <to-milestone>: <reason>`として出力される。`<reason>`はそのペアに対して`changes compute`が表示するのと同じfail-closedエラーそのもの(両側のスキーマバージョンと、CLI更新またはmigrationが必要であること — issue #29 §5)であり、汎用メッセージではない。手動で`changes compute`を再実行しなくても理由が分かるようにするため。1件でもスキップがあればコマンドは終了コード`1`で終了する — 非互換ペアを未処理のまま残したrunを、クリーンな成功として報告しない。
- `milestone.yml`の記録された`commit_oid`/`knowledge_schema_version`が、そのtagの現在の解決結果と食い違っている場合(tagの移動、または手編集)は、そのペアに関してハードエラーとなる。上記のfail-closedスキップとは異なりrun全体が停止する — 古い・改ざんされた監査コピーは自動リトライではなく人間の確認を必要とするため。
- ペア処理中に検出されたlegacyスキーマバージョンのwarning(そのrefに対して`changes compute`が表示するのと同じwarning)は`warning: ...`行として出力される。
- `--no-cache` を指定しない場合、`changes compute` と同じ `.markharness-cache/` を共有する。
- `--max-pairs`は1回の実行で新規処理するペア数を制限する。既処理としてスキップしたペアは件数に含めない。
- `--time-budget`は未処理ペアの開始前に時間予算を判定する。単位は`ms`、`s`、`m`、`h`(例: `30s`、`5m`)。ペア処理中の強制中断は行わない。

対象プロジェクトディレクトリ(`-d`/`--dir`)がgitリポジトリのサブディレクトリの場合の制約は、1.9節と同じく解消済み([decisions/0006](./decisions/0006-nested-project-directory-support.md))。

**終了コード**

| コード | 意味                                                       |
| ------ | ------------------------------------------------------------ |
| 0      | 成功 — 全ペアが処理済みか既に最新                           |
| 1      | Knowledgeスキーマ非互換により1件以上のペアがスキップされた |

**使用例**

```console
$ markharness backfill run
backfilled .markharness/changes/2026-08-release.yaml
backfill: 1 processed, 2 already up to date
```

**ユースケース対応**: UC6「バックフィルを非同期実行する」(§4.1〜4.3の簡易実装。マイルストーン限定・git notesによる進捗管理は本編どおり、非同期ワーカー化は見送り)。

---

### 1.11 `markharness milestone init` — `.markharness/executions/<tag>/milestone.yml` の作成(UC4: マイルストーンをタグ付けする、の補助)

```text
markharness milestone init <tag> [--json] [-d, --dir <path>]
```

**用途**: 既存の `git tag <tag>` に対応する `.markharness/executions/<tag>/milestone.yml` を作成する。UC4そのもの(リリースタイミングの意思決定として `git tag` を打つこと)は引き続き人間の判断ポイントであり本コマンドの対象外だが、そのタグを `backfill run`(1.10節)が認識できる形(`.markharness/executions/<name>/milestone.yml` というディレクトリ名がタグ名と一致すること、[src/backfill.rs:21-22](../../src/backfill.rs#L21-L22))に機械的にスキャフォールドする。

**オプション**

| オプション              | 説明                                                                             |
| ------------------ | ------------------------------------------------------------------------------ |
| `<tag>`            | (必須)対象の `git tag` 名。そのまま `.markharness/executions/<tag>/` のディレクトリ名として使う(追加の正規化・バリデーションはしない) |
| `-d, --dir <path>` | 対象プロジェクトディレクトリ(gitリポジトリ内の任意のディレクトリ。リポジトリ自体のルートである必要はない)。省略時はプロジェクトルート(cwdから上位探索で自動検出)         |
| `--json`           | 結果を1行のJSONで出力する。省略時は人間可読なテキストを出力する                                             |

**動作**

- 対象の `tag` が `git tag` として存在しなければ、`git tag <tag>` を先に実行するよう促すエラーメッセージを出して終了コード `2` で終了する(ファイルは作成しない)。
- タグが存在し `.markharness/executions/<tag>/milestone.yml` が未作成の場合、`id: <tag>` に加えて、タグ自体から解決した監査用の2フィールド `commit_oid`(タグが指すコミットのフルSHA)と `knowledge_schema_version`(タグ自身の`.markharness/config.toml`に記録された`[knowledge].schema_version`。そのタグがこのフィールドより古い場合は`1`、[decisions/0014](./decisions/0014-knowledge-schema-version-persistence.md))を書き込む。どちらも`changes compute`/`backfill run`からは正本として扱われず、比較対象refから常に再解決される — これらは`init`時点で何が記録されたかを人間が`milestone.yml`を見て確認できるようにするためだけに存在する。
- `.markharness/executions/<tag>/milestone.yml` が既に存在する場合は中身を変更せず、「既に初期化済み」である旨のメッセージを出して終了コード `0` で終了する(`markharness init` と同じ冪等パターン)。

**終了コード**

| コード | 意味                        |
| --- | ------------------------- |
| 0   | 成功(新規作成、または既に初期化済みでの冪等終了) |
| 2   | 対象の `git tag` が存在しない      |
| 3   | ファイルシステムエラー               |

**使用例(新規作成)**

```console
$ git tag 2026-08-release
$ markharness milestone init 2026-08-release
initialized .markharness/executions/2026-08-release/milestone.yml
```

**使用例(タグ未作成でエラー)**

```console
$ markharness milestone init 2026-08-release
error: git tag '2026-08-release' not found. Run `git tag 2026-08-release` first, then retry.
$ echo $?
2
```

**使用例(冪等)**

```console
$ markharness milestone init 2026-08-release
.markharness/executions/2026-08-release/milestone.yml is already initialized
$ echo $?
0
```

**ユースケース対応**: UC4「マイルストーンをタグ付けする」(`docs/product-operation.md` 107行目)の実行結果記録先スキャフォールドを補助する。タグ付け自体の意思決定は引き続き人間が行う。

---

### 1.12 `markharness binding set` / `list` — TestCaseの検証手段の宣言(ADR 0020・ADR 0025)

```text
markharness binding set --case-uid <case-uid> --mode <automated|manual> [--reference <text>] [--json] [-d, --dir <path>]
markharness binding list [--json] [-d, --dir <path>]
```

**用途**: あるTestCaseが「自動テストで検証されるのか、手動で検証されるのか」と「その検証実体がどこにあるか」を宣言する。`.markharness/bindings/<case-uid>.yml` に1 Case 1ファイルで保存する。

**`ExecutionBinding`は実行の記録ではない**。実行日時・結果(pass/fail)・Case revision・対象ビルド・実行環境・試行回数・証跡のいずれも持たず、**その存在を「実行済み」「合格」と読んではならない**(ADR 0025 §1・§2)。詳細な実行証跡の管理はmarkharnessの責務外であり、`reference` が指す先(テストコード、別ツール)に委ねる。

**オプション**

| オプション | 説明 |
| --- | --- |
| `--case-uid <case-uid>` | (必須)対象TestCaseのCase UID。表示idではない(ADR 0013。表示idのrenameで記録が切れないようにするため) |
| `--mode <value>` | (必須)`automated` / `manual` のいずれか |
| `--reference <text>` | 検証実体への自由記述の参照(テストファイルパス、URL等)。markharnessは内容を解釈しない |
| `-d, --dir <path>` | 対象プロジェクトディレクトリ。省略時はプロジェクトルート(cwdから上位探索で自動検出) |
| `--json` | 結果をJSONで出力する |

**動作**

- `set` は同じCase UIDの既存bindingを**置換**する。bindingは現時点の宣言であって追記型のログではないため、1 Case 1ファイルで上書きする。
- 保存形式は `schema_version: 1`・`record_kind: execution_binding`・`case_uid`・`mode`・`reference`(任意)のみ。`schema_version` は全レコード種別で `1` に固定し、今後も上げない(ADR 0026 §7)。
- 未知のフィールドを持つbindingファイルは**読み取り時に拒否**する。`result`・`executed_at`・`build`・`environment` 等の実行事実フィールドを手で書き足しても、黙って無視されることはない(ADR 0025 §2)。
- ファイル名が示すCase UIDと、ファイル内の `case_uid` が食い違う場合も**読み取り時に拒否**する。1 Case 1ファイルという同一性の前提が崩れると、あるCaseの検証宣言が別のCaseのものとして読まれるため。
- `schema_version` が `1` でないbindingファイルも拒否する。版は固定であり、別の値は手編集かこのreaderが知らないレコード種別を意味する。
- Case UIDはファイル名の唯一の構成要素になるため、空文字・`.`・`..`・先頭ドット・パス区切り(`/`・`\`)・ドライブ指定を含む値は**ファイルを作る前に**拒否する。`generate` が `id:` に課す検証と同じ扱い。
- 書き込みは `src/fs_safety.rs` の原子的置換経路を用いる。

**終了コード**

| コード | 意味 |
| --- | --- |
| 0 | 成功 |
| 2 | Case UIDがファイル名として不正、または保存済みbindingが壊れている(未知フィールドを含む等) |
| 3 | ファイルシステムエラー |

**使用例**

```console
$ markharness binding set --case-uid 01ARZ3NDEKTSV4RRFFQ69G5FAV --mode automated --reference tests/login.spec.ts
bound 01ARZ3NDEKTSV4RRFFQ69G5FAV as automated in .markharness/bindings/01ARZ3NDEKTSV4RRFFQ69G5FAV.yml
```

`.markharness/bindings/01ARZ3NDEKTSV4RRFFQ69G5FAV.yml`:

```yaml
schema_version: 1
record_kind: execution_binding
case_uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
mode: automated
reference: tests/login.spec.ts
```

**ユースケース対応**: [markharness v2設計書](./design/markharness-v2-design.md)§5.2の`ExecutionBinding`。リリースごとの検証スコープは`ReleaseScope`(別コマンド)が持つ。

---

### 1.13 `markharness impact` — Change Impactと対応確認(ADR 0019、設計書§5.3・§6.1)

```text
markharness impact --base <git-ref> --head <git-ref> [--format json] [--fail-on-findings] [-d, --dir <path>]
```

**用途**: `base..head` の区間で変更されたRequirementごとに、関連するFeature・TestCaseと、両者の対応が人によって確認されたかを出力する。

**`--base` は必須**。ローカルのbranch配置からbaseを推測すると、同じ区間でも実行環境によって結果が変わり、算出の再現性(設計原則P3、AC37)が壊れるため。CIでは `origin/main` などを明示的に渡す。

**対応確認の記録: `Spec-Reviewed` commit trailer**

```text
Spec-Reviewed: requirement=<requirement-id> case=<case-id> reason=no-change-required
```

- 変更のコミット本文の末尾に、**行頭から**書き添える。**1組につき1行**。インデントされた行は採用しない — コードブロックや引用の中で書式を説明しているだけの行を宣言と取り違えないため(gitのtrailer解釈と同じ立場)。行末の空白は無視する。複数組を確認した場合は行を複数書く(1行カンマ区切りは、片方だけが後から無効化される状況を表現できないため採らない)。
- `requirement=` と `case=` の**両方が必須**。片方だけ、あるいは対象を書かないtrailerは採用しない。1つのコミットが複数のRequirement/TestCaseに触れる場合、どの対応確認が済んだのか判定できないため(AC12・AC16)。
- 識別子は**表示ID**で書く。そのtrailerを含むコミット時点のKnowledgeでUIDへ解決する。解決できない場合は採用せず、`rejected_trailers` に理由付きで出力する。
- `reason` は省略可(既定 `no-change-required`)。未知の値は採用しない。
- 判定は `git log base..head` の各コミット**本文全体**を走査する。squash mergeでは元のtrailerがmerge commit本文の途中に埋め込まれるため、最終行だけを見る実装にはしていない。

**出力の三値**(設計書§5.3)

| status | 意味 |
| --- | --- |
| `confirmed` | その組に対する有効な `Spec-Reviewed` がある |
| `followed_up` | 区間内で両側が変更されたが、確認の記録は無い。TestCaseが動いたことは作業の証拠であって、人が意味の整合を判断した証拠ではない |
| `unconfirmed` | 仕様側が変更され、TestCaseが追随した形跡も確認の記録も無い |

**確認の失効**: 同一区間内の後続コミットで、組のどちらかの実効内容(TestCaseはCase revision、Requirementは `requirement.yml` または `.sdoc` blob)が変更されると、その組の確認は無効になる(AC14・AC29)。別の組への流用や、後から追加されたケースへの拡張は行わない(AC30・AC31)。

**仕様側変更の検知**: `source: native` は `requirement.yml` 自体のbase/head差分、`source: external` は `source_locator` が指す `.sdoc` blobのbase/head差分で判定する(ADR 0023)。固定参照がhead時点のblob OIDと一致しない場合は `stale_pins` として**別に**出力し、仕様側変更としては報告しない(AC10c)。repinは検知を打ち消さない(AC18・AC19)。

**履歴が取得できない場合**: shallow cloneや到達不能なrefで `base..head` を走査できないときは、診断付きで終了コード `2` を返す。履歴不足を「確認済み」や「変更なし」として出力しない(AC17)。

**終了コード**

| コード | 意味 |
| --- | --- |
| 0 | 正常終了(所見の有無によらず。`--fail-on-findings` 指定時は所見なし) |
| 2 | 所見あり(`--fail-on-findings` 指定時のみ)、または履歴不足・入力不正 |
| 3 | ファイルシステムエラー |

`--fail-on-findings` は既定OFF。未確認が1件あればCIを落とすかはチームの運用方針であり、ツールが決め打ちすべきではないため。所見には `confirmed` 以外の組、`stale_pins`、`rejected_trailers` を含む。

**出力**: `schema_version: 1`・`record_kind: change_impact`・`rule_version`・解決済みの完全なcommit ID(`base_commit`/`head_commit`)を含む。同じ入力と同じ `rule_version` なら同じ判定を再現する(AC06・AC37)。

---

### 1.14 `markharness release scope` / `markharness coverage` — リリース選定リストとRelease Coverage(ADR 0024、設計書§6.2)

```text
markharness release scope set --release <release-id> --case-uid <case-uid> [--case-uid ...] [-d, --dir <path>]
markharness release scope show --release <release-id> [--at <ref>] [--format json] [-d, --dir <path>]
markharness coverage --requirements <ids-or-all> [--release <release-id>] [--at <ref>] [--format json] [-d, --dir <path>]
```

**用途**: 「そのリリースで何を検証対象に選んだか」を記録し、指定したRequirement集合について「検証する手段が存在するか」「選定漏れは無いか」を一覧する。

**`ReleaseScope` が持たないもの**(ADR 0024 §2): 選定日時、選定者、承認状態、合否、実行結果、対象ビルド、実行環境、選定理由の構造化フィールド。選定の経緯はGit履歴が記録する。**選定リストに入っていることは「選んだ」という宣言であり、実行された事実でも合格した事実でもない**(ADR 0024 §5)。出力でも「選定済み」と「実行済み」を同一視しない。

**保存場所**: `.markharness/releases/<release-id>.yml`。Git管理下に置くことで `--at <ref>` による過去時点の再現(設計書P3)が成り立つ。`release-id` はこのパスの唯一の構成要素になるため、**ASCII小文字英数字・ハイフン・ドットのみ**を許し、空文字・`.`・`..`・先頭ドット・パス区切り・大文字を含む値は**ファイルを作る前に**拒否する。`v1.2.0` や `2026-08-release` のような一般的なtag名は通る。

**`coverage` が読むものはすべて `--at` の時点**: Knowledge・選定リスト・binding のいずれも指定refのコミットから読む。未コミットの変更は反映されない(既定の `--at HEAD` でも同様)。片方だけを working tree から読むと、過去refへの問い合わせが今日の作業内容で変わってしまい、算出の再現性(設計書P3、AC11)が成り立たないため。

**`--requirements` が判定範囲を決める**: 選定漏れ候補(`unselected_case_uids`)は「指定したRequirement集合から辿れるTestCaseのうち、選定リストに無いもの」である。選定リストの中身から範囲を逆算する方式は採らない — Requirementをまるごと選定し忘れた場合に、その最も危険な漏れを検出できなくなるため。全件を見たい場合は `--requirements all` を渡す。

**出力**

| フィールド | 意味 |
| --- | --- |
| `requirements[].cases[].binding_mode` / `binding_reference` | そのTestCaseの検証手段(1.12節)。**存在することは「実行済み」を意味しない** |
| `requirements[].cases[].selected` | `--release` 指定時のみ。選定リストに含まれるか |
| `gaps[].kind = requirement_has_no_feature` | `contributes_to` するFeatureが1つも無い(AC08) |
| `gaps[].kind = feature_has_no_case` | Featureは関連付いているが、その配下にScenarioが1つも無い(AC21) |
| `release.selected_case_uids` | 選定されており、その時点のKnowledgeにも存在するCase UID |
| `release.unselected_case_uids` | 対象範囲にあるが選定リストに無い(選定漏れ候補、AC25) |
| `release.absent_case_uids` | 選定リストにあるが、その時点のKnowledgeに存在しない(AC26)。選定リストを自動的に書き換えない |

**選定リストが無いリリース**: `--release` に渡したリリースの選定リストが記録されていない場合、`release` フィールドは出力されず、登録状態の一覧だけを返す。存在しない選定を推測しない(ADR 0024 §4)。

**終了コード**

| コード | 意味 |
| --- | --- |
| 0 | 成功 |
| 2 | `release-id` がファイル名として不正、指定したRequirementが存在しない、保存済みレコードが壊れている |
| 3 | ファイルシステムエラー |

**使用例**

```console
$ markharness release scope set --release v1.2.0 --case-uid 01ARZ... --case-uid 01BRZ...
recorded 2 case(s) for v1.2.0 in .markharness/releases/v1.2.0.yml

$ markharness coverage --requirements all --release v1.2.0 --at v1.2.0
```

**ユースケース対応**: 設計書§1の問い3「前回リリースにおいて、どのテストが検証スコープに入っていたか」。選定リストが記録されているリリースについてのみ「何を選んだか」まで答えられる。記録の無いリリースでは、その時点の登録状態の再現までである。

---

### 1.15 `markharness changes annotate` — change_type / related_eventsの事後入力(§3.5)

```text
markharness changes annotate <event_id> [--type <spec-change|bug-fix|refactor|other>] [--related <event_id>]... [-d, --dir <path>]
```

**用途**: `changes compute`(1.9節)が算出した `ChangeEvent` の `change_type` と `related_events` を、人間が事後に設定する。`.markharness/changes/` 配下の全 `*.yaml` ファイルを `event_id` で横断検索するため、呼び出し側はどのマイルストーン区間のファイルに含まれるかを事前に知る必要がない。

**動作**

- `--type` と `--related` は互いに独立した加算的フィールドであり、どちらか一方だけを指定してもよい(両方省略した場合はエラー、少なくとも一方の指定が必須)。
- `--type` を指定すると、一致する `event_id` を持つ最初のファイルの `change_type` を書き換える。同じファイル内の他の `ChangeEvent` は変更しない。
- `--related <event_id>` は複数回指定でき、それらを対象イベントの `related_events` に追記する(既存の値は保持、上書きではなく追加)。
- `--related` を指定した場合、対象の `event_id` と `--related` に指定した全ての `event_id` が `.markharness/changes/*.yaml` のどこかに存在するかを、書き込みより前に検証する。いずれかが見つからなければ、`--type` を指定していてもその書き込みは行われずに終了コード `3` でエラーになる(`--type`・`--related` は独立した加算的フィールドだが、コマンド全体としては全て書き込むか何も書き込まないかのいずれかになる)。
- `--type` のみを指定した場合(`--related` を指定しない場合)は、対象の `event_id` が見つからなければ終了コード `3` でエラーになる。

**使用例**

```console
$ markharness changes annotate player-jump--m1--m2 --type spec-change
set change_type on player-jump--m1--m2

$ markharness changes annotate player-jump--m2--m3 --related player-jump--m1--m2
set related_events on player-jump--m2--m3
```

**ユースケース対応**: UC5「ChangeEventを自動計算する」の一部(§3.5、`change_type`/`related_events`はいずれも計算ではなく人間の事後入力とする設計意図に対応)。

---

### 1.16 `markharness changes lineage` — merge-base祖先探索による系譜監査(§3.2、副次機能)

```text
markharness changes lineage --commit <merge-commit-sha> [--json] [-d, --dir <path>]
```

**用途**: 指定したマージコミットについて、その2親(P1・P2)と `git merge-base` によるマージベース(B)のtree SHAを比較し、各Feature idごとに§3.2の場合分け(`linear` / `true_divergence` / `single_parent`)を判定して出力する監査専用コマンド。`changes compute`(1.9節)は、`from-milestone..to-milestone`区間内に存在する全ての2親マージコミットについて本コマンドと同じ判定ロジックを内部で呼び出し、結果を`true_divergences`に反映する。個別のマージコミット単体を人手で監査・確認したい場合は、本コマンドを独立に実行する。本コマンド自体は `.markharness/changes/*.yaml` への書き込みを行わない(読み取り専用の監査コマンド)。squash mergeやrebase・fast-forward mergeで運用されたリポジトリでは、そもそも対象となる2親マージコミットがコミットグラフ上に存在しないため、本コマンドで監査できる対象自体が無い(論文§3.4表2)。

**動作**

- `<merge-commit-sha>` が2親を持たない(マージコミットでない)場合、終了コード `2` でエラーになる。
- 判定結果は人間可読なテキスト(`<feature_id>: <kind>`)または `--json` でのJSON配列として出力する。

**使用例**

```console
$ markharness changes lineage --commit a1b2c3d
player-jump: linear
```

**ユースケース対応**: §3.2の「詳細系譜ツール(監査用、副次機能)」の実装。RQ1の評価対象(主系譜)には含まれない(§1.3の注記)。

---

### 1.17 `markharness validate` — .markharness/knowledge/・.markharness/axes/・.markharness/bindings/ の構造検証(§3.5/§3.6)

```text
markharness validate [--json] [-d, --dir <path>]
```

**用途**: `.markharness/knowledge/` 配下の全YAML(`requirement.yml` / `feature.yml` / `behavior.yml` / `condition.yml` / `expected/*.yml`)と `.markharness/axes/*.yml`、および `.markharness/executions/<milestone>/results.yml` を、対応する `.markharness/schema/*.schema.json`(`markharness init` が既定一式を配置。1.1節)でJSON Schema検証する。加えて、JSON Schema単体では表現できない相互参照制約を検証する: `axis` タグが `.markharness/axes/*.yml` に登録されているか、`feature.yml` の `forked_from` が実在するFeature idを指しているか。

**bindingの検証**: `.markharness/bindings/*.yml` は `ExecutionBinding` として読み取り可能であることを検証する(1.12節)。`result`・`executed_at`・`build`・`environment` のような実行事実フィールドを持つbindingは、未知フィールドとして拒否される(ADR 0025 §2)。

**UID modeでの追加検証(ADR 0013、design doc §13 Phase 5)**: `.markharness/config.toml`の`[identity]`markerが`mode = "uid"`(1.21節`identity migrate`が全種類の移行完了時に書き込む)であるプロジェクトでは、Requirement/Feature/Behavior/Condition/ExpectedResultのいずれかが`uid:`を持たない場合、そのファイルパスと`markharness identity migrate`の実行を促すメッセージを検証issueとして報告する。copy/import/手編集でcutover後にuidなし要素が紛れ込んだことを検出するためのガードであり、cutover前(markerなし)のプロジェクトでは適用されない。

**動作**

- 問題が1件もなければ終了コード `0`。人間可読モードでは `.markharness/knowledge/ and .markharness/axes/ are valid`、`--json` では `{"ok":true}` を出力する。
- 問題があれば、ファイルごとのメッセージを列挙して終了コード `1` で終了する。

**使用例**

```console
$ markharness validate
.markharness/knowledge/controls/player-jump/feature.yml: axis 'not-registered' is not registered under .markharness/axes/
$ echo $?
1
```

**ユースケース対応**: §3.5の「`.markharness/axes/*.yml`に定義されていない値をfront matterで使えないようスキーマバリデーションで縛る」制約の実装。

---

### 1.18 `markharness --version` / `-V` — バージョン表示

```text
markharness --version
markharness -V
```

**用途**: `Cargo.toml` の `version`(ビルド時に `CARGO_PKG_VERSION` として埋め込まれる)を表示する。バージョン番号は `Cargo.toml` を唯一の情報源とする(CLAUDE.mdの運用ルール)。

**使用例**

```console
$ markharness --version
markharness 0.3.1
```

---

### 1.19 `markharness axes prune` — 未使用axisの検出・削除

```text
markharness axes prune [--delete] [--json] [-d, --dir <path>]
```

**用途**: `.markharness/axes/*.yml` に登録されているが、`.markharness/knowledge/` 配下のどのRequirement/Feature/Behaviorの `axis:` 配列からも参照されていない(孤立した)axisを検出する。`condition.yml`/`expected/*.yml` には `axis` フィールドがないため走査対象外。

**動作**

- デフォルトはレポートのみ(`--delete` 未指定時は `.markharness/axes/*.yml` を一切削除しない)。
- `--delete` を指定すると、検出された未使用axisの `.markharness/axes/<id>.yml` を実際に削除する。二段階確認(追加の`--yes`等)は要求しない——`--delete` フラグの指定自体を明示的同意とみなす(参照されていない孤立axisのみが対象で、重要データを誤って失うリスクが低いため)。
- `--json` 指定時は `{"axes":[<未使用axisのid配列>],"deleted":<bool>}` を出力する。`deleted` は `--delete` を指定したかどうかを表し、`axes` のキー名・構造は `--delete` の有無によらず同じ(呼び出し側がモードごとに別のパースロジックを書かずに済むようにするため)。

**使用例(レポートのみ)**

```console
$ markharness axes prune --dir tmp/todo-sample --json
{"axes":["legacy-ui"],"deleted":false}
```

**使用例(削除)**

```console
$ markharness axes prune --delete --dir tmp/todo-sample --json
{"axes":["legacy-ui"],"deleted":true}
$ markharness axes list --dir tmp/todo-sample --json
```

(`legacy-ui` が `.markharness/axes/` から削除され、以降 `axes list` に現れなくなる)

**ユースケース対応**: `markharness axes add`(1.6節)と対になる補助コマンド。どのUCにも明示的には現れない。

---

### 1.20 `markharness import` — canonical snapshotの生成

```text
markharness import --source <native|junit> [--input <junit.xml>] [--git-ref <ref>] [--bind <artifact-id=version>]... --format json [-d, --dir <path>]
```

`native`は対象Git refの`.markharness/knowledge/`をFeature tree SHA付きartifactとderived traceへ正規化する。`junit`はJUnit XMLのTestCaseとPASS/FAIL/SKIPをevidenceへ正規化し、`--bind`で検証対象versionを付与する。JUnitの`markharness.condition` propertyはstored traceになる。出力は`schema_version: 1`を持ち、`.markharness/schema/canonical_snapshot.schema.json`に従う。入力ファイルや`.markharness/knowledge/`は変更しない。

---

### 1.21 `markharness identity migrate` — 全種類のKnowledge要素へuidを一括発行する(ADR 0013、design doc §12・§13 Phase 4/5)

```text
markharness identity migrate [--json] [--dry-run] [-d, --dir <path>]
```

**用途**: `.markharness/knowledge/`配下のRequirement/Feature/Behavior/Condition/ExpectedResultのうち、まだ`uid:`を持たない要素全てへ新規UIDを発行し、root `Issued` identity eventを記録する。冪等な操作であり、copy/import/手編集で後からuidなし要素が混入した場合も安全に再実行できる。TestCaseの`case_id`→`case_uid`対応(migration manifest、`.markharness/identity-migration-manifest.yml`)もあわせて記録する。

5種類全てにuidなし要素が0件になった時点で、`.markharness/config.toml`の`[identity]`markerへ`schema_version = 1`・`mode = "uid"`を書き込み、UID modeへの公開cutoverを完了する(design doc §13 Phase 5)。cutover完了の判定は`schema_version`ではなく`mode`のみで行う(ADR 0018)。cutover後は`markharness validate`(1.17節)が、uidなし要素の新規混入を検証issueとして報告するようになる。

**前提条件**: 対象ディレクトリがgitリポジトリであること。legacy snapshot identityとして`.markharness/knowledge`のtree SHAをmigration manifestへ記録するため、内部で一時indexを使った`git write-tree`相当の処理を行う(実リポジトリのstaging areaは変更しない)。

**動作**

- `--dry-run`: lock・staging・identity event・Knowledge fileのいずれも書き込まず、予定するUID割当と変更対象ファイルの一覧のみ表示する。
- 通常実行: 全kindを2パスで処理する(id/uid重複検出→問題なければ全kind分のIssued eventを1つのbatchとしてcrash-recoverableに記録)。kind間で同じidを使うのは許容されるが、kind内の重複idは競合として拒否される(終了コード `2`)。
- 同時実行中の別identity operationを検知した場合: 終了コード `2`。
- ファイルシステムエラー: 終了コード `3`。
- `--json`: `{"audit_scope":"working_tree","dry_run":bool,"migrated":[{"kind","id","uid"}],"conflicts":[string],"changed_files":[string]}` を出力する。`audit_scope`は、`changes compute`・`verify`系(1.4/1.9節)の`"two_snapshot"`や`identity audit`(1.23節)の`"full_history"`と対比される値で、`identity migrate`が working tree 1点のみを検査する操作であることを示す機械可読フィールド(design doc §11)。

**使用例**

```console
$ markharness identity migrate --dry-run
would migrate requirement 'req-todo' -> uid 01M0M862TX3X878T44WXBCQDQF
would migrate feature 'todo' -> uid 01M0M862TYP26CAAB5RWHKWC2B
would migrate behavior 'todo-add-task' -> uid 01M0M862TYD5B5H95VGXAXYKN3
would migrate condition 'todo-add-task-empty-input' -> uid 01M0M862TYDND4EQJT6A25KAG4
would migrate expected_result 'todo-add-task-empty-input-001' -> uid 01M0M862TYQKXDCTGDYPE8BBWY
would change .markharness/knowledge/req-todo/requirement.yml
would change .markharness/identity-events/requirements/01M0M862TX3X878T44WXBCQDQF/01M0M862TYKGXCZV3TECPDQGWS.yml
... (以下、変更対象ファイルを全kind分列挙)

$ markharness identity migrate
migrated requirement 'req-todo' -> uid 01M0M8632NP9SY6T1X1NK7Z9XE
migrated feature 'todo' -> uid 01M0M8632N73PB010A2TQQYG84
migrated behavior 'todo-add-task' -> uid 01M0M8632N0KDPK15MAK34TZKC
migrated condition 'todo-add-task-empty-input' -> uid 01M0M8632N94PXJREJEJNMKETY
migrated expected_result 'todo-add-task-empty-input-001' -> uid 01M0M8632NWZAW5VM0HZ2AWNMV

$ markharness identity migrate --json
{"audit_scope":"working_tree","changed_files":[],"conflicts":[],"dry_run":false,"migrated":[]}
```

(2回目の`--json`実行は全要素が既にmigrate済みのため、`migrated`が空のno-op応答になっている。)

**ユースケース対応**: ADR 0013「移行」節、design doc §12(migration時のrecorded_at・crash-recovery)・§13 Phase 4(全要素migration)/Phase 5(UID modeへの公開cutover)。

---

### 1.22 `markharness identity resolve` — branch divergenceを明示的に解決する(ADR 0013、design doc §7)

```text
markharness identity resolve <KIND> <UID> --keep <EVENT_UID> [-d, --dir <path>]
```

`<KIND>` は `requirement` / `feature` / `behavior` / `condition` / `expected-result` のいずれか。

**用途**: 同一entityに対し、同じ先行eventから分岐した複数のidentity event(branch divergence、design doc §7)が存在する場合に、どちらの結果(id)を正とするかを明示的に選び、`Resolved` identity eventを記録する。divergenceは、複数branchで独立にidentity操作(rename等)が行われた履歴をmergeした場合などに発生しうる。branch divergence自体は通常の単一branch運用では発生しにくく、本コマンドは複数branchでの並行identity操作をmergeした場合の復旧手段として用意されている。

**動作**

- 成功時: `resolved divergence for <uid>, keeping <keep>` を出力し終了コード `0`。
- 対象entityに未解決のdivergenceが無い、`--keep`に指定したevent uidがdivergent headのいずれでもない(候補一覧をエラーメッセージに表示)、同時実行中の別identity operationを検知、のいずれも終了コード `2`。
- ファイルシステムエラー: 終了コード `3`。

**ユースケース対応**: ADR 0013 design doc §7(branch divergenceの解決)。

---

### 1.23 `markharness identity audit` — commit history全体の同一性監査(IdentityAuditor、ADR 0013、design doc §11)

```text
markharness identity audit [--json] [--ref <ref>] [-d, --dir <path>]
```

**用途**: `<ref>`(既定`HEAD`)のfirst-parent history全体を走査し、`.markharness/identity-events/`が持つべき2つの性質を検証する: (1) identity eventはappend-onlyであること(一度commitされたeventファイルが後のcommitで消失・内容変更されていないか)、(2) 各commit時点のevent集合が矛盾なくreplayできること(causal chain contradiction)。`changes compute`・`verify`・`identity migrate`(1.4/1.9/1.21節)がいずれも高々2つの`.markharness` snapshotしか見ない軽量な比較であるのに対し、`identity audit`だけがGit commit history全体を走査する重い処理であり、独立したトップレベルコマンドとして分離されている(design doc §11)。

走査は現在checkoutしているbranchのfirst-parent history(`git log --first-parent`相当)に限定される。まだmergeされていないside branch上の変更はこのプロジェクトの公開履歴ではないため、対象に含めない。

**動作**

- 違反が1件もなければ終了コード `0`。人間可読モードでは`no identity-history violations found (<N> commits scanned)`、`--json`では`violations`が空配列。
- 違反があれば違反ごとに1行出力して終了コード `1`。
- `--json`: `{"audit_scope":"full_history","commits_scanned":<N>,"violations":[...]}`。各`violations`要素は`type`フィールド(`event_disappeared` / `event_content_changed` / `causal_chain_contradiction`)でタグ付けされる。
- Gitオブジェクトの読取失敗などの基盤障害が発生した場合はコマンド自体がエラー終了する(監査対象の矛盾としては報告しない)。

**使用例**

```console
$ markharness identity audit
no identity-history violations found (3 commits scanned)

$ markharness identity audit --json
{"audit_scope":"full_history","commits_scanned":3,"violations":[]}
```

identity eventファイルが後から削除されるなど、履歴が改ざんされた場合:

```console
$ markharness identity audit
event disappeared: feature '01M0M8632N73PB010A2TQQYG84' event '01M0M8632N666JSS1BXY1NCH30' is missing as of commit a021aed5d2159dbe718b111e9aaf679130ee823b (.markharness/identity-events/features/01M0M8632N73PB010A2TQQYG84/01M0M8632N666JSS1BXY1NCH30.yml)
causal chain contradiction: feature '01M0M8632N73PB010A2TQQYG84' at commit a021aed5d2159dbe718b111e9aaf679130ee823b: NoRootEvent
$ echo $?
1
```

**ユースケース対応**: ADR 0013 検証規則(「`IdentityAuditor`だけがGit commit history全体を走査し、repository全体のevent append-only性と、選択2 snapshotの外側にある削除・過去改変を検証すること」)、design doc §11。

---

### 1.24 `markharness identity sync` — Knowledge fileのid:/uid:をidentity event logから再同期する

```text
markharness identity sync <KIND> <UID> [-d, --dir <path>]
```

**用途**: `<UID>`のidentity eventを現在の状態までreplayし、その結果の`id`を持つKnowledge fileへ`uid:`を書き戻す(欠けていれば追加、古ければ訂正)。新しいidentity eventは一切記録しない — 既にdurableなevent logからファイル状態を再導出するだけの操作。`identity migrate`をはじめ他の全identity操作が内部で行っている「roll-forwardによるKnowledge file同期」を、単体で呼び出せるようにしたもの。

**前提条件**: Knowledge fileをGit履歴から復元・再作成した場合など、他の操作の副作用としては同期が起きなかったケースを埋めるためのコマンド。`knowledge reconcile`(1.2節)のrenameは対象を`uid`で選択するため、`uid:`を持たないファイルの再同期手段にはならない — `identity sync`は5種類全kindに対応し、ファイルがuidを持っているかどうかを問わない。

**動作**

- 成功時: `synced <uid>` を出力し終了コード `0`。
- 対象entityに`uid`が無い、同時実行中の別identity operationを検知、のいずれも終了コード `2`。
- ファイルシステムエラー: 終了コード `3`。

**使用例**

```console
$ markharness identity sync feature 01M0MJQ5C4CJ3HHVG7PBYAQEBR
synced 01M0MJQ5C4CJ3HHVG7PBYAQEBR
$ cat .markharness/knowledge/req-todo/todo/feature.yml
id: todo
requirement: req-todo
label: todo
axis: []
uid: 01M0MJQ5C4CJ3HHVG7PBYAQEBR
```

**ユースケース対応**: Knowledge fileをGit履歴から復元した場合などの一般的な後始末。

---

### 1.25 `markharness traceability` — Requirement・Feature・Behavior・Scenario・TestCaseの関係を読む(ADR 0032・0033、設計書 cli-read-model-design.md §5)

```text
markharness traceability [--at <git-ref>] [--format json] [-d, --dir <path>]
```

**用途**: Knowledgeと生成済みTestCaseから、Requirement・Feature・Behavior・Scenario・TestCaseの関係を読み取り専用で出力する。`markharness-view`などの外部ツールが、Knowledgeや`.markharness/`を直接読まずにこの出力だけを入力にできるようにする(ADR 0032)。`impact`・`coverage`と同じく`CommandOutcome`/`Presenter`を経由せず、専用モジュールの構造体を直接JSONへシリアライズする。

**`--at`は省略可。省略時は作業ツリー(コミット前の現在の内容)を読む**(`generate`・`verify`と同じ経路。ADR 0033)。`--at <ref>`を指定した場合は、そのGit ref時点のコミット内容を読む。`impact`・`coverage`と異なり`traceability`には2点比較やリリース監査の要件がないため、コミットを要求しない。`generate`のように生成物を書き込むことはない。

**出力**: `schema_version: 1`・`record_kind: traceability`・`at`(`--at`省略時は固定値`"working-tree"`、指定時は指定文字列そのまま。ADR 0033)に加え、`requirements`(`requirement_id`・`requirement_uid`・`source`・`label`・`source_locator`・`source_key`。`label`は`source: "native"`の場合のみ値を持ち、`source_locator`・`source_key`は`source: "external"`の場合のみ値を持つ。互いに反対の値を持ち、片方が`null`の時もう片方は値を持つ)、`features`(`feature_id`・`feature_uid`・`label`)、`behaviors`(`behavior_id`・`feature_id`・`label`。`behavior_uid`は現状のKnowledge読み取り経路では取得できず常に`null`)、`scenarios`(`scenario_id`・`scenario_uid`・`behavior_id`・`label`)、`test_cases`(`case_id`・`case_uid`・`case_revision`・`relative_path`・`scenario_id`)、`relations`(`from_uid`・`to_uid`・`kind`。`kind`は`contributes_to`(FeatureまたはScenarioからRequirementへ)と`generated_from`(TestCaseからScenarioへ)の2種類)を含む。UIDを持たない要素(`identity migrate`未実行)は、Nodeとしては出力されるが`relations`には現れない。TestCaseの本文(`phases`・`axis`)は含まない。`relative_path`が指す`.markharness/generated/testcases/<relative_path>`を直接読むことで得られる。

**動作**

- Feature・Requirementは`generate`が生成するTestCaseの有無に関わらず、Knowledgeに存在する全件を出力する(`coverage`のAC21と同じ理由で、対応するTestCaseが無いFeatureも可視化する)。
- Behavior・Scenario・TestCaseは、生成される全TestCaseから導出する(空のPhaseを持つScenarioは`generate`が拒否するため、実在するScenarioは必ず1件のTestCaseに対応する)。
- `source`(native/external)と、それぞれが排他的に持つフィールド(`label`・`source_locator`・`source_revision`・`source_key`。externalは`description`も)が矛盾するRequirement(例: `source: native`なのに`source_locator`を持つ、`source: external`なのに`label`を持つ)は拒否する(終了コード2)。`validate`と同じ制約(ADR 0023)だが、`traceability`は`validate`が実行済みであることを前提にできないため、読み取り時に自前で確認する。

**終了コード**

| コード | 意味 |
| --- | --- |
| 0 | 成功 |
| 2 | Knowledgeファイルの構文・内容エラー |
| 3 | ファイルシステムエラー |

**使用例**

```console
$ markharness traceability
{
  "schema_version": 1,
  "record_kind": "traceability",
  "at": "working-tree",
  ...
}
```

コミット済みの特定時点を見たい場合は`--at`を指定する:

```console
$ markharness traceability --at HEAD
{
  "schema_version": 1,
  "record_kind": "traceability",
  "at": "HEAD",
  "requirements": [
    { "requirement_id": "controls", "requirement_uid": "01ARZ3NDEKTSV4RRFFQ69G5FAV", "source": "native", "label": "controls", "source_locator": null, "source_key": null }
  ],
  "features": [
    { "feature_id": "player-jump", "feature_uid": null, "label": "player-jump" }
  ],
  "behaviors": [
    { "behavior_id": "jump", "behavior_uid": null, "feature_id": "player-jump", "label": "jump" }
  ],
  "scenarios": [
    { "scenario_id": "ground", "scenario_uid": "01ARZ3NDEKTSV4RRFFQ69G5FB1", "behavior_id": "jump", "label": "ground" }
  ],
  "test_cases": [
    { "case_id": "tc-player-jump-jump-ground", "case_uid": "...", "case_revision": "...", "relative_path": "player-jump/jump/ground.yml", "scenario_id": "ground" }
  ],
  "relations": [
    { "from_uid": "01ARZ3NDEKTSV4RRFFQ69G5FB1", "to_uid": "01ARZ3NDEKTSV4RRFFQ69G5FAV", "kind": "contributes_to" }
  ]
}
```

**ユースケース対応**: [cli-read-model-design.md](./design/cli-read-model-design.md)§5の`TraceabilityReadModel`、[ADR 0032](./decisions/0032-cli-read-model-seam.md)・[ADR 0033](./decisions/0033-traceability-defaults-to-working-tree.md)。

---

## 2. 未実装(今後実装予定)のコマンド

以下は `docs/product-operation.md` のユースケース図・ユースケース記述に基づく、今後実装予定のコマンドです。コマンド名・オプションは暫定案であり、実装時に変更され得ます。

| #   | ユースケース                 | 想定コマンド(暫定)                                                  | アクター                | 概要                                                                                      |
| --- | ---------------------------- | ------------------------------------------------------------------- | ----------------------- | ----------------------------------------------------------------------------------------- |
| UC4 | マイルストーンをタグ付けする | 専用コマンドなし(`git tag <milestone>` を直接使用)                  | Release Manager         | リリースタイミングの意思決定そのものであり、人間の判断ポイント(図3)。                     |

これらは現時点で未着手であり、実装順序は別途チェックリスト(`/plan-checklist`)で管理する。

---

## 3. 動作確認・テスト

実装済みコマンドの単体テストは `cargo test` で実行できる(`src/init.rs` / `src/knowledge.rs` / `src/knowledge_reconcile/` / `src/generate.rs` / `src/verify.rs` / `src/axes.rs` / `src/traceability_index.rs` / `src/git.rs` / `src/id_cache.rs` / `src/changes.rs` / `src/backfill.rs` の `#[cfg(test)] mod tests`、および `knowledge reconcile` の終了コード・出力を検証する `tests/knowledge_reconcile_cli.rs` を参照)。`git.rs`/`id_cache.rs`/`changes.rs`/`backfill.rs` のテストは実際に一時ディレクトリ上で `git init`/`commit`/`tag` を行うため、テスト実行環境に `git` コマンドが必要。Pre-PR チェックリスト(`CONTRIBUTING.md`)に従い、コミット前に以下を実行すること:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo audit
```
