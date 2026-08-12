# markharness CLI マニュアル

**Status**: Implemented(実装済みコマンドは1章)/ Draft(未実装コマンドの暫定案は2章)
**関連ドキュメント**: [product-operation.md](./product-operation.md)(ユースケース対応)、[testcase-generation-design.md](./testcase-generation-design.md)(`generate`の生成規則)、[knowledge-apply-cli-spec.md](./knowledge-apply-cli-spec.md)(`knowledge validate`/`apply`の詳細設計)

**位置づけ**：本資料は `markharness` CLI の使用方法を、**実装済みコマンド**と**未実装(今後実装予定)のコマンド**に分けてまとめたものです。ユースケース(UC1〜UC8)の対応は `docs/product-operation.md` の「3. ユースケース記述」表に基づきます。実装済みコマンドの具体的な生成規則は `docs/testcase-generation-design.md` を参照してください(ただし `generate`/`verify` の現行実装は、同ドキュメント作成後に `feature → behavior → condition → expected` の4階層モデルへ刷新されており、詳細は本マニュアル 1.5/1.6 節を正としてください)。`knowledge validate`/`apply`(非対話・TTY非依存版、1.3/1.4節)の詳細設計は `docs/knowledge-apply-cli-spec.md` を正としてください。

---

## 1. 実装済みコマンド

### 1.1 `markharness init` — プロジェクトの初期化(UC1〜UC8 の前提)

```text
markharness init
```

**用途**: UC1〜UC8を支える物理ディレクトリ構成(論文 §3.5, 244-273行目)のうち、対象リポジトリ上に作成が必要な6ディレクトリを作成し、以降のコマンドが動作できる状態にする。

| ディレクトリ  | 対応UC                                                              |
| ------------- | ------------------------------------------------------------------- |
| `knowledge/`  | UC1(知識を記述する)/ UC1b(forked_from を手動記述する)               |
| `axes/`       | UC1(横断的観点 Axis のレジストリ、§3.1)                             |
| `generated/`  | UC2(TestCaseを決定的生成する)/ UC3(生成物をレビュー・マージする)    |
| `executions/` | UC4(マイルストーンをタグ付けする、実行結果の記録先)                 |
| `changes/`    | UC5(ChangeEventを自動計算する)/ UC6(バックフィルを非同期実行する)   |
| `schema/`     | UC7(idキャッシュを破棄・再構築する。フォーマット・正規化ルール定義) |

UC8(既存ツールからのインポート)は専用ディレクトリを持たず、変換結果を `knowledge/` に書き込む想定のため対象外。

**動作**

- 各ディレクトリについて、存在しなければ作成し、既に存在すればそのまま(中身も含めて)何もしない冪等な処理。すでに初期化済みのプロジェクトで再実行してもエラーにはならず、不足しているディレクトリだけが追加で作成される。
- 成功すると作成先のパスを標準出力に表示する。

**使用例**

```console
$ markharness init
initialized knowledge/, axes/, generated/, executions/, changes/, schema/ under /path/to/project

$ markharness init
initialized knowledge/, axes/, generated/, executions/, changes/, schema/ under /path/to/project
```

**ユースケース対応**: どのUCにも明示的には現れないが、UC1〜UC8の全ユースケースを開始する前提条件を満たすための補助コマンド。

---

### 1.2 `markharness knowledge add` — 知識の対話的記述(UC1: 知識を記述する。Requirement → Feature → Behavior → Condition → ExpectedResult の順)

```text
markharness knowledge add [--dir <path>]
```

**用途**: Test Designer が `Requirement` → `Feature` → `Behavior` → `Condition` → `ExpectedResult` の5階層を対話形式(標準入力への逐次プロンプト)で記述し、`knowledge/` 配下に `.yml` ファイルを作成する。`Requirement` は Feature の親となる要求単位で、`Feature` は自身の `requirement:` フィールドで親を参照する。`Behavior` は「機能がどう振る舞うか」を表す必須の中間階層で、`generate` が組み立てる TestCase の `steps`(手順)の元になる。

**オプション**

| オプション         | 説明                                                                                                  |
| ------------------ | ----------------------------------------------------------------------------------------------------- |
| `-d, --dir <path>` | 対象プロジェクトディレクトリ(`knowledge/` の親)を指定する。省略時はカレントディレクトリを対象にする。 |

**使用例(カレントディレクトリ以外を対象にする)**

```console
$ markharness knowledge add --dir tmp/todo-sample
Requirement name (e.g. task-management): task-management
Requirement axis (comma separated, e.g. ui, validation): workflow
Feature name (e.g. add-todo): add-todo
Axis (comma separated, e.g. ui, validation): ui, validation
Behavior name (e.g. add-task): add-task
Behavior axis (comma separated, e.g. ui, validation): ui
Behavior description (e.g. User adds a new task to the list.): User adds a new task to the list.
Condition name (e.g. empty-title): empty-title
Scenario (e.g. Submit the todo form with an empty title): Submit the todo form with an empty title
Expected result (e.g. shows a validation error): shows a validation error
```

→ `tmp/todo-sample/knowledge/task-management/add-todo/...` にファイルが作成される。

**アクター**: Test Designer(`docs/product-operation.md` UC1)

**フロー**

1. `Requirement name (e.g. task-management):` — Requirement の slug(小文字英数字とハイフンのみ)、または日本語ラベルを入力
   - `knowledge/` 配下に既存の Requirement が1件以上あれば、プロンプトの前に `N) id` 形式で番号付き一覧を表示する。番号を入力すると対応する Requirement を選択でき、既存の id をそのまま直接入力しても再利用できる。候補が0件の場合は一覧を表示しない。
   - 既存の `knowledge/<requirement_id>/requirement.yml` があれば再利用し、次のプロンプトへスキップする
   - 新規の場合のみ `Requirement axis (comma separated, e.g. ui, validation):` で観点をカンマ区切りで入力し、`requirement.yml` を新規作成する
2. `Feature name (e.g. add-todo):` — Feature の slug(小文字英数字とハイフンのみ)、または日本語ラベルを入力
   - 選択した Requirement 配下に既存の Feature が1件以上あれば、同様に番号付き一覧を表示し、番号選択・直接入力のいずれも可能。
   - 既存の `knowledge/<requirement_id>/<feature_id>/feature.yml` があれば再利用し、次のプロンプトへスキップする
   - 新規の場合のみ `Axis (comma separated, e.g. ui, validation):` で観点をカンマ区切りで入力し、`feature.yml` を新規作成する(`requirement:` フィールドには選択・作成した Requirement の id が自動的に記録される)
3. `Behavior name (e.g. add-task):` — Behavior の slug、または日本語ラベルを入力
   - 選択した Feature 配下に既存の Behavior が1件以上あれば、同様に番号付き一覧を表示し、番号選択・直接入力のいずれも可能。
   - 既存の `knowledge/<requirement_id>/<feature_id>/<behavior_id>/behavior.yml` があれば再利用し、次のプロンプトへスキップする
   - 新規の場合のみ `Behavior axis (...)` と `Behavior description (...)` を入力し、`behavior.yml` を新規作成する
4. `Condition name (e.g. empty-title):` — Condition の slug、または日本語ラベルを入力
   - 選択した Behavior 配下に既存の Condition が1件以上あれば、同様に番号付き一覧を表示し、番号選択・直接入力のいずれも可能。
   - 新規に作成する Condition id が `{behavior_id}-` で始まる場合(Behavior id を重複して含めてしまった場合)、その接頭辞を自動的に除去してから作成し、その旨を通知する(例: Behavior `add-task` に Condition id `add-task-empty-title` と入力すると `empty-title` として作成される)。ただし、入力された id そのままのディレクトリが既に存在する場合は除去せずそのまま再利用する(過去に手動で重複した名前のまま作成されたデータを壊さないため)。
   - 既存の `knowledge/<requirement_id>/<feature_id>/<behavior_id>/<condition_id>/condition.yml` があれば(除去後の id で判定し)再利用し、次のプロンプトへスキップする
   - 新規の場合のみ `Scenario (e.g. Submit the todo form with an empty title):` で条件の説明を入力し、`condition.yml` を新規作成する
5. `Expected result (e.g. shows a validation error):` — 期待結果のテキストを入力し、`expected/NNN.yml`(3桁連番、既存ファイル数+1)を作成する

**プロンプト文言について**: 各プロンプトは内部的には Feature/Behavior/Condition の `id`(ディレクトリ名・YAMLの `id` フィールド)を決めるものだが、人間が入力する際に「id」という抽象的な概念に迷わないよう、`Feature name` / `Behavior name` / `Condition name` のように分かりやすい英語表現と入力例を添えている。内部データモデル(`id`フィールド、通知メッセージ、コード上の変数名)は変更していない。

**日本語ラベル入力(Feature name / Behavior name / Condition name)**

各 name プロンプトでは、ASCII以外の文字を含む入力(日本語ラベルなど)を渡すと、id直接入力の代わりに以下のローマ字変換フローに切り替わる。ExpectedResult の id は自動連番のため対象外。

1. 入力文字列に非ASCII文字が含まれるかを判定する。
2. 含まれる場合、[`kakasi`](https://crates.io/crates/kakasi) crate で入力をローマ字に変換し、続けて正規化(小文字化・空白のハイフン化・連続ハイフンの圧縮・先頭/末尾ハイフン除去・許可されない記号の除去)を行った id 候補を1件提示する。
3. `id候補: <候補> (Enterで採用、編集する場合は入力):`というプロンプトに対して空入力(Enter)のみを送るとその候補をそのまま id として採用する。何か文字列を入力すると、その入力を同じ正規化ルールに通した上で id として採用する(自由編集)。
4. 正規化後の id が既存候補一覧(番号付き一覧に表示されているもの)と衝突する場合は警告を表示し、その階層の id 入力からやり直しになる(自動的な既存id流用はしない。既存idの意図的な再利用は番号選択で行う)。
5. 日本語ラベル入力を経由して新規作成された Requirement/Feature/Behavior/Condition の `label` フィールドには、入力した日本語文字列がそのまま保存される。ASCII直接入力の場合や番号選択で既存を再利用した場合は入力値そのもの(= id と同じ文字列)が `label` として保存される。ExpectedResult は id が自動連番でありユーザーが名前を入力する対象ではないため、`label` フィールドは存在しない(入力した説明文はそのまま `description` に保存される)。

**入力バリデーション**

- id(Feature id / Behavior id / Condition id)は小文字英数字とハイフンのみ許可。不正な場合は再入力を促す。
- 候補一覧が表示されている場合、1以上・候補件数以下の整数を入力すると対応する候補が選択される。範囲外の整数や非数値は通常のid入力(またはASCII以外を含む場合は日本語ラベル)として扱われる。
- すべてのプロンプトで空入力(trim後に空)は再入力を促す。ただし日本語ラベル変換後の id候補提示に対する空入力は「候補をそのまま採用」を意味し、再入力を促す対象ではない。

**生成されるファイル**(例: `task-management` / `add-todo` / `add-task` / `empty-title` / 1件目)

```
knowledge/task-management/requirement.yml
knowledge/task-management/add-todo/feature.yml
knowledge/task-management/add-todo/add-task/behavior.yml
knowledge/task-management/add-todo/add-task/empty-title/condition.yml
knowledge/task-management/add-todo/add-task/empty-title/expected/001.yml
```

`requirement.yml`:

```yaml
id: task-management
label: task-management
axis: [workflow]
```

`feature.yml`:

```yaml
id: add-todo
requirement: task-management
label: add-todo
axis: [ui, validation]
```

`behavior.yml`:

```yaml
id: add-task
feature: add-todo
label: add-task
axis: [ui]
description: |
  User adds a new task to the list.
```

`condition.yml`:

```yaml
id: empty-title
behavior: add-task
label: empty-title
description: |
  Submit the todo form with an empty title
```

`expected/001.yml`(id は `{condition_id}-{連番3桁}`):

```yaml
id: empty-title-001
condition: empty-title
description: |
  shows a validation error
```

**使用例(初回セッション)**

```console
$ markharness knowledge add
Requirement name (e.g. task-management): task-management
Requirement axis (comma separated, e.g. ui, validation): workflow
Feature name (e.g. add-todo): add-todo
Axis (comma separated, e.g. ui, validation): ui, validation
Behavior name (e.g. add-task): add-task
Behavior axis (comma separated, e.g. ui, validation): ui
Behavior description (e.g. User adds a new task to the list.): User adds a new task to the list.
Condition name (e.g. empty-title): empty-title
Scenario (e.g. Submit the todo form with an empty title): Submit the todo form with an empty title
Expected result (e.g. shows a validation error): shows a validation error
```

**使用例(既存Requirement/Feature/Behavior/Conditionへの2件目のExpectedResult追加、番号選択)**

```console
$ markharness knowledge add
Requirement name (e.g. task-management):
  1) task-management
1
既存のRequirement 'task-management' を再利用します。
Feature name (e.g. add-todo):
  1) add-todo
1
既存のFeature 'add-todo' を再利用します。
Behavior name (e.g. add-task):
  1) add-task
1
既存のBehavior 'add-task' を再利用します。
Condition name (e.g. empty-title):
  1) empty-title
1
既存のCondition 'empty-title' を再利用します。
Expected result (e.g. shows a validation error): highlights the title field in red
```

→ `knowledge/task-management/add-todo/add-task/empty-title/expected/002.yml` が作成される。番号の代わりに `task-management` / `add-todo` / `add-task` / `empty-title` を直接入力しても同じ結果になる。

**使用例(Condition id の重複接頭辞を自動除去)**

```console
$ markharness knowledge add
Requirement name (e.g. task-management): task-management
既存のRequirement 'task-management' を再利用します。
Feature name (e.g. add-todo): add-todo
既存のFeature 'add-todo' を再利用します。
Behavior name (e.g. add-task): add-task
既存のBehavior 'add-task' を再利用します。
Condition name (e.g. empty-title):
  1) empty-title
add-task-max-length
Condition id 'add-task-max-length' から Behavior id 'add-task' と重複する接頭辞を除去し、'max-length' として作成します。
Scenario (e.g. Submit the todo form with an empty title): Submit the todo form with a title longer than 200 characters
Expected result (e.g. shows a validation error): shows a length validation error
```

→ `knowledge/task-management/add-todo/add-task/max-length/condition.yml` と `knowledge/task-management/add-todo/add-task/max-length/expected/001.yml` が作成される(`add-task-max-length/` ディレクトリは作成されない)。

**ユースケース対応**: UC1「知識を記述する」(手動記述、`docs/product-operation.md` 103行目)を対話形式で支援する。

---

### 1.3 `markharness knowledge validate` — ドラフトYAMLの検証(UC1: 知識を記述する。非対話・TTY非依存)

```text
markharness knowledge validate <draft-file> [--json] [-d, --dir <path>]
```

**用途**: `knowledge add`(1.2節)が前提とするTTY上での逐次プロンプトに依存せず、Requirement→Feature→Behavior→Condition→ExpectedResultの1チェーン分を1つのドラフトYAMLファイルとして与え、スキーマ・整合性を検証する。**副作用はなく、ファイルへの書き込みは一切行わない。** Claude Code等のAIエージェントによる非対話呼び出しや、将来のGUI実装からの利用を想定している。詳細な設計意図・バリデーションルール一覧は `docs/knowledge-apply-cli-spec.md` を正とする。

**オプション**

| オプション         | 説明                                                                          |
| ------------------ | ----------------------------------------------------------------------------- |
| `<draft-file>`     | (必須)ドラフトYAMLファイルのパス                                              |
| `-d, --dir <path>` | 対象プロジェクトディレクトリ(`knowledge/` の親)。省略時はカレントディレクトリ |
| `--json`           | エラー・結果を1行のJSONで出力する。省略時は人間可読なテキストを出力する       |

**ドラフトYAMLの形式**(1回の実行で1本のチェーンを検証する。複数チェーンの一括検証は非対応)

```yaml
requirement:
  id: controls # 必須。ASCII slug
  label: controls # 省略可(既存id再利用時は省略可)
  axis: [gameplay] # 新規作成時は必須。既存id再利用時は省略可
  description: null # 省略可

feature:
  id: player-jump
  label: player-jump
  axis: [gameplay, animation]

behavior:
  id: jump
  label: jump
  axis: [gameplay]
  description: Player presses jump. # Behaviorのみdescriptionが必須(新規作成時)

condition:
  id: ground
  label: ground
  description: Jump from the ground and land

expected:
  - description: lands safely
  - description: takes fall damage if height > 3m
```

`axis`/`label`/`description` は、既存id(すでに `knowledge/` 配下にファイルが存在するRequirement/Feature/Behavior/Condition)を再利用する場合は省略できる。省略されたフィールドは既存値との比較対象から除外され、指定されたフィールドのみ既存ファイルの値と突合される(`conflicting_existing_value` エラー)。

**バリデーションルール(概要。詳細は spec §5)**

| エラーコード                 | 内容                                                                                                             |
| ---------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `invalid_slug`               | idが小文字英数字とハイフン以外を含む                                                                             |
| `missing_axis`               | 新規作成のRequirement/Feature/Behaviorで `axis` が空・未指定                                                     |
| `missing_description`        | 新規作成のBehavior/Condition、または各ExpectedResultで `description` が空                                        |
| `unknown_axis`               | `axes/*.yml` レジストリに登録されていない観点値(近似候補があれば `suggestion` に提示)                            |
| `redundant_prefix`           | `condition.id` が `{behavior.id}-` で始まる(`knowledge apply` の `--strip-redundant-prefix` 未指定時。1.4節参照) |
| `conflicting_existing_value` | 既存id再利用時、指定した `label`/`axis`/`description` が既存ファイルの値と不一致                                 |
| `parent_not_found`           | 既存ファイルに記録された親参照(例: `feature.yml` の `requirement:`)がドラフトのチェーンと矛盾                    |

**終了コード**

| コード | 意味                                                                             |
| ------ | -------------------------------------------------------------------------------- |
| 0      | 成功(エラーなし)                                                                 |
| 1      | バリデーションエラーあり(エラー内容はstderr、`--json`指定時はstdoutにJSONで出力) |
| 2      | 使用方法エラー(ファイル不在・YAMLパース不能)                                     |

**使用例(成功・人間可読)**

```console
$ markharness knowledge validate draft.yml --dir tmp/todo-sample
$ echo $?
0
```

(標準出力・標準エラーとも何も出力しない)

**使用例(成功・`--json`)**

```console
$ markharness knowledge validate draft.yml --dir tmp/todo-sample --json
{"ok":true}
```

**使用例(失敗・人間可読)**

```console
$ markharness knowledge validate draft.yml --dir tmp/todo-sample
error: unknown_axis: axis "validdation" is not registered (path=behavior.axis[0])
error: redundant_prefix: condition.id "jump-ground" starts with behavior.id "jump-" prefix (suggested="ground", path=condition.id)
$ echo $?
1
```

**使用例(失敗・`--json`)**

```console
$ markharness knowledge validate draft.yml --dir tmp/todo-sample --json
{"ok":false,"errors":[{"code":"unknown_axis","path":"behavior.axis[0]","value":"validdation","message":"axis \"validdation\" is not registered","suggestion":"validation"}]}
$ echo $?
1
```

**ユースケース対応**: UC1「知識を記述する」(`docs/product-operation.md` 103行目)を、TTYに依存しない形で支援する。1.2節の `knowledge add` と同じ検証ロジックを共有する。

---

### 1.4 `markharness knowledge apply` — ドラフトYAMLの検証+書き込み(UC1: 知識を記述する。非対話・TTY非依存)

```text
markharness knowledge apply <draft-file> [--json] [-d, --dir <path>] [--strip-redundant-prefix] [--dry-run]
```

**用途**: `knowledge validate`(1.3節)と同じ検証を行い、問題がなければ `knowledge/` 配下に**アトミックに**書き込む。5階層(Requirement〜ExpectedResult)のうち一部だけを新規作成する場合でも、全バリデーションが通過した後にまとめて書き込む(一時ファイル+リネーム。書き込み中にI/Oエラーが発生した場合は成功済みファイルも含めてロールバックする)。既存id(再利用)のファイルは上書きしない。

**オプション**

| オプション                 | 説明                                                                                                                                                                                                                                                                                     |
| -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `<draft-file>`             | (必須)ドラフトYAMLファイルのパス。形式は1.3節と共通                                                                                                                                                                                                                                      |
| `-d, --dir <path>`         | 1.3節と同様                                                                                                                                                                                                                                                                              |
| `--json`                   | 1.3節と同様。成功時は書き込んだファイル一覧を出力する(下記参照)                                                                                                                                                                                                                          |
| `--strip-redundant-prefix` | `condition.id` が `{behavior.id}-` で始まる場合、確認なしで接頭辞を除去したidを採用する。未指定の場合は `redundant_prefix` エラーで停止する(1.3節参照)。除去後idと同名のディレクトリが既に存在する(レガシーデータ)場合は、`knowledge add` と同様に除去せず既存のものをそのまま再利用する |
| `--dry-run`                | `knowledge validate` と同義(検証のみ行い書き込まない)。CI等での用途を想定した別名                                                                                                                                                                                                        |

**終了コード**

| コード | 意味                                                                    |
| ------ | ----------------------------------------------------------------------- |
| 0      | 成功(書き込み成功。`--dry-run` 指定時はエラーなし)                      |
| 1      | バリデーションエラーあり(1.3節と同じ形式。ファイルは一切書き込まれない) |
| 2      | 使用方法エラー(ファイル不在・YAMLパース不能)                            |
| 3      | ファイルシステムエラー(書き込み失敗など)                                |

**使用例(成功・`--json`)**

```console
$ markharness knowledge apply draft.yml --dir tmp/todo-sample --json
{"ok":true,"written":["knowledge/controls/player-jump/jump/ground/expected/002.yml"]}
$ echo $?
0
```

`written` には新規に書き込まれたファイルのみ(既存id再利用でスキップしたファイルは含まない)が、対象ディレクトリ(`--dir`)からの相対パスで列挙される。

**使用例(`--strip-redundant-prefix` でCondition idの重複接頭辞を除去)**

```console
$ markharness knowledge apply draft.yml --dir tmp/todo-sample --strip-redundant-prefix
$ echo $?
0
```

`draft.yml` の `condition.id: add-task-max-length`(Behavior id `add-task` と重複)は `max-length` として書き込まれる。`knowledge add`(1.2節)の自動除去と同じ挙動。

**使用例(`--dry-run`)**

```console
$ markharness knowledge apply draft.yml --dir tmp/todo-sample --dry-run --json
{"ok":true}
$ echo $?
0
```

(ファイルは書き込まれない)

**使用例(バリデーションエラーで書き込み拒否)**

```console
$ markharness knowledge apply draft.yml --dir tmp/todo-sample
error: missing_description: behavior.description must not be empty (path=behavior.description)
$ echo $?
1
```

(`knowledge/` 配下には一切ファイルが作成されない)

**ユースケース対応**: UC1「知識を記述する」(`docs/product-operation.md` 103行目)を、TTYに依存しない形で支援する。AIエージェント・将来のGUI実装が知識を確定登録するための共通エントリポイント。人間向けの `$EDITOR` 起動ラッパーは `knowledge add --edit`(1.9節)として実装済み。

---

### 1.5 `markharness generate` — TestCase の決定的生成(UC2: TestCaseを決定的生成する)

```text
markharness generate
```

**用途**: `knowledge/` 配下を決定的に走査し、`Requirement × Feature × Behavior × Condition × ExpectedResult` から `TestCase` を機械的に組み立てて、`generated/testcases/` 配下に **1 Condition = 1 ファイル** の `.yml` として再生成する。実行のたびに `generated/testcases/` を空にしてから書き直すため、削除された Condition に対応する古いファイルも自動的に消える。

**アクター**: 本来は CI Bot(UC2)だが、ローカルでの事前確認用に手動実行も可能。

**アルゴリズム概要**

- `knowledge/` 配下を `requirement.yml` → `feature.yml` → `behavior.yml` → `condition.yml` → `expected/*.yml` の順に、パスのソート順で走査する(実行環境・タイムスタンプに依存しない)。`Behavior` を持たない `Feature` や `expected/` が空(または存在しない)の `Condition` からは `TestCase` は生成されない。
- **集約モデル**: 1つの `Condition` の `expected/` 配下にある全ファイルを、1つの `TestCase` の `expected` 配列に集約する(1 Condition = 1 TestCase。1 expected ファイルごとに別 TestCase を作る旧モデルからの変更)。
- `case_id = "tc-{condition.id}-001"`。連番は将来 1 Condition から複数 TestCase を生成する拡張のために予約されており、現状は常に `001`。
- 出力ファイル名は `generated/testcases/{condition.id}.yml`(`case_id` の `tc-` 接頭辞は付けない)。
- `title` = `condition.description`、`steps` = `[behavior.description]`、`expected` = 各 `expected/*.yml` の `description` をファイル名のソート順で列挙。
- `generated_from` に `requirement` / `feature` / `behavior` / `condition` の各 id と、集約元の `expected_results`(`expected/*.yml` の `id` の一覧)を記録する。
- `axis`: `Requirement` / `Feature` / `Behavior` の `axis` を合成(union、重複除去のうえソート)した観点一覧(§3.4「axisの継承」)。
- 出力は `serde_yaml_ng` によるシリアライズで、同一入力に対して常に同一の出力になる(決定性、CIでの差分検証の前提)。
- `generate` は `generated/testcases/*.yml` に加えて `generated/traceability-index.json`(Requirement → Feature → Behavior → Condition → TestCase の対応関係を持つ機械可読索引。`serde_json` による整形済みJSON)も同時に再生成する。`markharness verify`(1.6節)はこのファイルも差分検証対象に含める。

**使用例**

```console
$ markharness generate
generated 1 testcase(s) into generated/testcases/
```

`generated/testcases/empty-title.yml`:

```yaml
case_id: tc-empty-title-001
generated_from:
  requirement: task-management
  feature: add-todo
  behavior: add-task
  condition: empty-title
  expected_results:
    - empty-title-001
title: |
  Submit the todo form with an empty title
steps:
  - |
    User adds a new task to the list.
expected:
  - |
    shows a validation error
```

`knowledge/` に何も無い場合は `generated/testcases/` が空(0ファイル)になる。

**ユースケース対応**: UC2「TestCaseを決定的生成する」(`docs/product-operation.md` 105行目)。CI上での差分検証(UC3)は 1.6 節の `markharness verify` で行う。

---

### 1.6 `markharness verify` — 生成物の差分検証(UC3: 生成物をレビュー・マージする)

```text
markharness verify
```

**用途**: `knowledge/` から `generate` と同じロジックで TestCase と `traceability-index.json` を再構築し(ディスクへは書き込まない)、コミット済みの `generated/testcases/*.yml` および `generated/traceability-index.json` と比較する。CI上でこのコマンドを実行し、`knowledge/` の変更を `generated/` へ反映し忘れていないかを検証する想定。

**アクター**: Reviewer / CI Bot(UC3)

**動作**

- 差分が無ければ `generated/testcases/ is up to date with knowledge/` を表示し、終了コード `0`。
- 差分があれば、追加・削除・変更されたファイルを `added:` / `removed:` / `changed:` のラベル付きでファイル名のソート順に一覧表示し、終了コード `1` で終了する(内容のunified diffまでは表示しない)。`generated/traceability-index.json` も他の生成物と同じ扱いで一覧に含まれる(ファイル名は `traceability-index.json`)。

**使用例(差分なし)**

```console
$ markharness verify
generated/testcases/ is up to date with knowledge/
```

**使用例(差分あり)**

```console
$ markharness verify
added: generated/testcases/empty-title.yml
changed: generated/testcases/max-length.yml
removed: generated/testcases/duplicate-title.yml
$ echo $?
1
```

**ユースケース対応**: UC3「生成物をレビュー・マージする」(`docs/product-operation.md` 106行目)。差分が検出された場合、その内容が意図したものかどうかを判断してマージするのはReviewerの役割(人間の判断ポイント)。

---

### 1.7 `markharness axes list` — 観点(axis)レジストリの一覧表示

```text
markharness axes list [--json] [-d, --dir <path>]
```

**用途**: `axes/*.yml` に登録済みの観点一覧を、id昇順で出力する。`knowledge validate`/`apply` の `unknown_axis` エラーを事前に回避するための参照コマンド。

**動作**: `--json` 未指定時は `id (label)`(label が id と同じ場合は id のみ)を1行ずつ表示し、登録が0件なら `no axes registered under axes/` と表示する。`--json` 指定時は `[{"id":...,"label":...|null}]` を1行のJSONで出力する。

**使用例**

```console
$ markharness axes list --dir tmp/todo-sample
gameplay (Gameplay)
ui

$ markharness axes list --dir tmp/todo-sample --json
[{"id":"gameplay","label":"Gameplay"},{"id":"ui","label":null}]
```

**ユースケース対応**: どのUCにも明示的には現れない補助コマンド(`docs/knowledge-apply-cli-spec.md` §8)。

---

### 1.8 `forked_from`(UC1b: 別Featureからの概念的派生を手動記述する)

専用コマンドはなく、`feature.yml` の `forked_from` フィールドに派生元Featureのidを直接記述する運用(§3.1)。`knowledge validate`/`apply`(1.3/1.4節)のドラフトYAMLでも `feature.forked_from` を受け付け、参照先のFeatureが `knowledge/` 配下のどこにも存在しない場合は `unknown_forked_from` エラーで停止する。Git履歴からは自動導出できないドメイン知識のため、`derived_from`(同一Featureの版履歴、§3.2〜3.4)とは異なり検証のみ行い自動計算はしない。

```yaml
feature:
  id: player-double-jump
  label: player-double-jump
  axis: [gameplay]
  forked_from: player-jump # 概念的な派生元(既存Feature id)。省略可
```

---

### 1.9 `markharness knowledge add --edit` — ドラフトYAMLの$EDITOR編集(UC1: 知識を記述する)

```text
markharness knowledge add --edit [-d, --dir <path>]
```

**用途**: `knowledge add`(1.2節)の対話プロンプトの代わりに、空のドラフトYAMLテンプレート(1.3節と同じ形式)を一時ファイルに書き出して `$VISUAL`(未設定なら `$EDITOR`)を起動する。保存してエディタを終了すると `knowledge apply`(1.4節)と同じ検証・書き込みを行い、バリデーションエラーがあればエラー内容を表示したうえで同じファイルを再度エディタで開く(ループ)。`$VISUAL`/`$EDITOR` がいずれも未設定の場合はエラーを表示して終了コード `2` で終了する。

**Windows/`code`コマンドについて**: VS Codeの `code` コマンドは実体が `.cmd`(バッチファイル)であり、Rustの `std::process::Command` は拡張子解決(PATHEXT)を行わないため `EDITOR=code --wait` は `program not found` になる。`cmd /c` 経由で起動するよう `EDITOR="cmd /c code --wait"` のように指定すること。

**axisの自動登録**: `requirement.axis` / `feature.axis` / `behavior.axis` に、`axes/*.yml` へ未登録の値が含まれていた場合、以下の条件をすべて満たす値だけを `axes/<value>.yml`(`id`/`label` とも当該値)として自動的に新規登録し、メッセージを表示する。

- 登録済みaxisとの編集距離(levenshtein距離)が2以下の近似候補が無い(タイポの可能性がある値は自動登録せず、従来通り `unknown_axis` エラーとして残し、`suggested="..."` で近似候補を提示する)
- `id`として有効な形式(小文字英数字とハイフンのみ)である

1回の検証で複数の未登録axis値がある場合、axisごとに独立して判定する(一部だけ自動登録され、近似候補がある値だけエラーとして残る)。この自動登録は `knowledge add --edit` 限定であり、対話式 `knowledge add`・`knowledge validate`/`apply`(非対話)では従来通り `unknown_axis` エラーのみで停止する。

**使用例**

```console
$ EDITOR="cmd /c code --wait" markharness knowledge add --edit
axis 'state' を新規登録しました (axes/state.yml)
wrote knowledge/controls/player-jump/jump/ground/expected/001.yml
```

**ユースケース対応**: UC1「知識を記述する」(`docs/product-operation.md` 103行目)。`knowledge apply` の非対話検証ロジックをそのまま再利用する。

---

### 1.10 `markharness cache rebuild` — idキャッシュの破棄(UC7: idキャッシュを破棄・再構築する)

```text
markharness cache rebuild [-d, --dir <path>]
```

**用途**: `.markharness-cache/`(1.11節の `changes compute` が使う、Featureのid→tree SHA解決結果の非コミットキャッシュ。内容アドレス方式のキーで格納されており、`knowledge/`の内容やツールのバージョンが変われば読み込み時に自動的に再計算されるため、通常は明示的な`rebuild`は不要)を丸ごと削除する。即時の再計算は行わない(次回 `changes compute` 実行時に遅延計算される)。キャッシュディレクトリが存在しない場合もエラーにならない(冪等)。

**使用例**

```console
$ markharness cache rebuild
removed .markharness-cache/ under /path/to/project
```

**ユースケース対応**: UC7「idキャッシュを破棄・再構築する」(`docs/product-operation.md`)。id解決の不整合が疑われる場合のフェイルセーフ。

**Featureの`id:`を変更した場合の注意(利用者向け、論文§3.3)**: Feature idは各`feature.yml`の`id:`フィールドを正準ソースとして追跡する。`id:`の値そのものを書き換えると、ツールから見て「元のFeatureが削除され、新しいidのFeatureが追加された」扱いになり、`changes compute`は過去のマイルストーンとの`derived_from`関係を復元できない(版履歴が断絶する)。Featureディレクトリの**リネーム**(パス変更)は`id:`が変わらない限り追跡対象内だが、`id:`自体の変更に対する移行手順(旧id→新idのエイリアス記録等)は本CLIには無く、現状は「`id:`を変更しない」運用を利用者側で徹底する必要がある。検討状況は[decisions/0004](./decisions/0004-feature-id-change-migration.md)を参照。

**キャッシュキーのバージョンフィールドについて**: `.markharness-cache/`のキャッシュキーを構成する`canonicalization_rule_version`/`id_index_schema_version`(論文§3.3)は、実装では現状固定値`"1"`である。これらの値を実際に上げる正規化ルール改訂・id-indexフォーマット改訂はまだ発生していないため、値を上げた場合にキャッシュが正しく破棄されるかは実地検証されていない。

---

### 1.11 `markharness changes compute` — ChangeEventの算出(UC5: ChangeEventを自動計算する)

```text
markharness changes compute <from-milestone> <to-milestone> [--no-cache] [--current-tree] [-d, --dir <path>]
```

**用途**: 2つのマイルストーン(git tag名をそのまま使用。マイルストーン境界の判定はタグ名一致のみで、`executions/*/milestone.yml` との対応は呼び出し側の責務)間で、`knowledge/` 配下の各Featureディレクトリのtree SHAを `git ls-tree -r <tag> -- knowledge` で比較し、変化したFeatureごとに `ChangeEvent` を算出して `changes/<to-milestone>.yaml` に書き込む。Feature idは各`feature.yml`の`id:`フィールド(`git show`で読む)を正準ソースとし、ディレクトリ名とは独立に追跡する(論文§3.3)。

**アクター**: CI Bot(UC5)

**動作**

- Feature単位で `from_blob`/`to_blob` を比較し、一致すれば何もしない。片方にのみ存在すれば追加/削除、両方に存在し値が異なれば変更として `ChangeEvent` を1件生成する。
- `impacted_testcases` は、変更されたFeatureに由来する `TestCase.case_id` を、`generate`(1.5節)と同じ生成グラフ(§3.2(A)の構造的生成グラフ。版履歴は使わない)から列挙したもの。どの時点の `knowledge/` からこの生成グラフを構築するかは2026-08以降2モードに分かれる(2026-08-12時点、[change-event-verification-tracking-spec.md](./change-event-verification-tracking-spec.md) §2.4も参照)。
  - **既定(`--current-tree`未指定)**：`to-milestone`タグが指す`knowledge/`ツリー(一時`git worktree`に展開)から構築する。同じ区間を後日再計算しても常に同じ結果になる。
  - **`--current-tree`指定時**：現在の作業ツリーの`knowledge/`から構築する(従来動作)。作業ツリーが変化し続ける限り、同じ区間の再計算結果も変わりうる。
- `change_type`(仕様変更/バグ修正等)は算出時には `null` のまま出力する。人間が `markharness changes annotate`(1.15節)で事後入力する運用(§3.5)。
- `--no-cache` を指定しない場合、Feature tree SHA解決結果を内容アドレス方式でキー化された `.markharness-cache/` に読み書きする(1.10節)。
- `from-milestone..to-milestone` の区間を `git rev-list --ancestry-path` で走査し、区間内に存在する全ての2親マージコミットそれぞれについて `git merge-base` を用いて1.16節の`lineage`判定ロジックを内部で実行する(古い順)。対象Featureがいずれかのマージで`true_divergence`(真の分岐)と判定されると、`true_divergences` フィールドに `merge_commit`(監査用のマージコミットSHA)と `parent_tree_shas: [P1, P2]` の組を、発生した順に追記する(§3.2)。同一Featureが区間内で複数回真の分岐を起こした場合もすべて記録される。通常の線形履歴、または区間内にマージが無い場合は空配列のまま。
- **ブランチ戦略への依存に注意**：`from_tree_sha`/`to_tree_sha`の差分検出そのものはブランチ戦略(merge/squash/rebase/fast-forward)に依存しないが、`true_divergences`はマイルストーン区間内に2親を持つマージコミットが実際に残っていることが前提であり、squash mergeやrebase・fast-forward mergeでは元ブランチの分岐関係がコミットグラフから失われるため検出されない(空配列のまま。論文§3.4表2)。

**出力例**(`changes/m2.yaml`、線形履歴の場合)

```yaml
- event_id: player-jump--m1--m2
  feature_id: player-jump
  from_milestone: m1
  to_milestone: m2
  from_tree_sha: 1a2b3c...
  to_tree_sha: 4d5e6f...
  impacted_testcases:
    - tc-ground-001
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
  change_type: null
  true_divergences:
    - merge_commit: 9f8e7d...
      parent_tree_shas:
        - 2b3c4d...
        - 5e6f7a...
```

**ユースケース対応**: UC5「ChangeEventを自動計算する」。本モデルの核心的貢献(§3.2〜3.4)の簡易実装。

---

### 1.12 `markharness backfill run` — 過去マイルストーンの一括処理(UC6: バックフィルを非同期実行する)

```text
markharness backfill run [--no-cache] [-d, --dir <path>]
```

**用途**: `executions/*/milestone.yml` が存在するマイルストーンを対象に、対応する git tag のコミット日時(committer date)で新しい順に並べ、隣接する2マイルストーンごとに `changes compute`(1.11節)相当の処理を実行して `changes/<milestone>.yaml` を生成する。1回の実行で全ペアを処理し終了する(常駐デーモンではない。CI等からの定期実行を想定)。

**動作**

- 最も古いマイルストーンは比較対象がないためスキップされる。
- 各マイルストーン(to側)の処理完了は `git notes --ref=markharness-backfill` に記録され、次回実行時に同じペアは再計算されずスキップされる(§4.3)。
- `--no-cache` を指定しない場合、`changes compute` と同じ `.markharness-cache/` を共有する。

**使用例**

```console
$ markharness backfill run
backfilled changes/2026-08-release.yaml
backfill: 1 processed, 2 already up to date
```

**ユースケース対応**: UC6「バックフィルを非同期実行する」(§4.1〜4.3の簡易実装。マイルストーン限定・git notesによる進捗管理は本編どおり、非同期ワーカー化は見送り)。

---

### 1.13 `markharness milestone init` — `executions/<tag>/milestone.yml` の作成(UC4: マイルストーンをタグ付けする、の補助)

```text
markharness milestone init <tag> [--json] [-d, --dir <path>]
```

**用途**: 既存の `git tag <tag>` に対応する `executions/<tag>/milestone.yml` を作成する。UC4そのもの(リリースタイミングの意思決定として `git tag` を打つこと)は引き続き人間の判断ポイントであり本コマンドの対象外だが、そのタグを `backfill run`(1.12節)が認識できる形(`executions/<name>/milestone.yml` というディレクトリ名がタグ名と一致すること、[src/backfill.rs:21-22](../src/backfill.rs#L21-L22))に機械的にスキャフォールドする。

**オプション**

| オプション              | 説明                                                                             |
| ------------------ | ------------------------------------------------------------------------------ |
| `<tag>`            | (必須)対象の `git tag` 名。そのまま `executions/<tag>/` のディレクトリ名として使う(追加の正規化・バリデーションはしない) |
| `-d, --dir <path>` | 対象プロジェクトディレクトリ(gitリポジトリのルート)。省略時はカレントディレクトリ                                    |
| `--json`           | 結果を1行のJSONで出力する。省略時は人間可読なテキストを出力する                                             |

**動作**

- 対象の `tag` が `git tag` として存在しなければ、`git tag <tag>` を先に実行するよう促すエラーメッセージを出して終了コード `2` で終了する(ファイルは作成しない)。
- タグが存在し `executions/<tag>/milestone.yml` が未作成の場合、`id: <tag>` のみを内容として書き込む(committer dateなどはgitから都度取得する既存設計を変えないため保存しない、[src/backfill.rs:41-48](../src/backfill.rs#L41-L48))。
- `executions/<tag>/milestone.yml` が既に存在する場合は中身を変更せず、「既に初期化済み」である旨のメッセージを出して終了コード `0` で終了する(`markharness init` と同じ冪等パターン)。

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
initialized executions/2026-08-release/milestone.yml
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
executions/2026-08-release/milestone.yml is already initialized
$ echo $?
0
```

**ユースケース対応**: UC4「マイルストーンをタグ付けする」(`docs/product-operation.md` 107行目)の実行結果記録先スキャフォールドを補助する。タグ付け自体の意思決定は引き続き人間が行う。

---

### 1.14 `markharness execution record` — TestCase実行結果の記録(UC4: 実行結果の記録先)

```text
markharness execution record <case_id> --milestone <name> --result <pass|fail|skip> --executor <name> [--note <text>] [--json] [-d, --dir <path>]
```

**用途**: `generated/testcases/` 内のいずれかの `TestCase`(`case_id` で識別)について、あるマイルストームでの実行結果1件を `executions/<milestone>/results.yml` に追記する。CIによる自動テスト実行・QAによる手動テストのいずれからも同じインターフェースで呼び出す想定(書き込み先・スキーマは共通)。

**オプション**

| オプション                | 説明                                                                 |
| -------------------- | ------------------------------------------------------------------ |
| `<case_id>`          | (必須)対象TestCaseの `case_id`(`generated/testcases/*.yml` のいずれかに含まれる値) |
| `--milestone <name>` | (必須)記録先のマイルストーム名。対応する `executions/<name>/milestone.yml` が必要        |
| `--result <value>`   | (必須)`pass` / `fail` / `skip` のいずれか                                 |
| `--executor <name>`  | (必須)実行者の自由記述(人名、または `ci-github-actions` のようなCI識別子)                 |
| `--note <text>`      | 任意の自由記述メモ                                                          |
| `-d, --dir <path>`   | 対象プロジェクトディレクトリ。省略時はカレントディレクトリ                                      |
| `--json`             | 結果を1行のJSONで出力する。省略時は人間可読なテキストを出力する                                 |

**動作**

- `executions/<milestone>/milestone.yml` が存在しなければ、`markharness milestone init <milestone>` を先に実行するよう促すエラーメッセージを出して終了コード `2` で終了する。
- `case_id` が現在の(HEAD時点の)`generated/testcases/*.yml` のいずれにも見つからなければ、`markharness generate` を先に実行するよう促すエラーメッセージを出して終了コード `2` で終了する。`generated/testcases/` のファイル名は `condition.id` であり `case_id` とは異なる([1.5節](#15-markharness-generate--testcase-の決定的生成uc2-testcaseを決定的生成する))ため、この検証は各ファイルの中身(`case_id` フィールド)を読んで行う。過去マイルストーン時点の内容までは遡らず、常に現在のHEADに対して検証する。
- 検証を通過すると、`case_id` / `result` / `executor` / `note`(省略時は出力しない)/ `executed_at`(ISO8601, UTC)を1エントリとして `executions/<milestone>/results.yml` に追記する。既存のエントリは変更せず、末尾に追加する(過去の実行履歴・再実行の記録も保持する)。
- 書き込みは `knowledge apply`(1.4節)と同じ「一時ファイル+リネーム」のアトミック方式(全エントリを読み直してまとめて書く)。

**終了コード**

| コード | 意味                                     |
| --- | -------------------------------------- |
| 0   | 成功(エントリを追記)                            |
| 2   | 指定したマイルストームが未初期化、または `case_id` が見つからない |
| 3   | ファイルシステムエラー                            |

**使用例**

```console
$ markharness execution record tc-ground-001 --milestone 2026-08-release --result pass --executor yamada
recorded pass for tc-ground-001 into executions/2026-08-release/results.yml
```

`executions/2026-08-release/results.yml`:

```yaml
- case_id: tc-ground-001
  result: pass
  executor: yamada
  executed_at: 2026-08-08T03:15:00Z
```

**使用例(未初期化のマイルストームを指定してエラー)**

```console
$ markharness execution record tc-ground-001 --milestone 2099-01-01 --result pass --executor yamada
error: milestone '2099-01-01' not found. Run `markharness milestone init 2099-01-01` first.
$ echo $?
2
```

**ユースケース対応**: UC4「マイルストーンをタグ付けする、実行結果の記録先」(`docs/cli-manual.md` の `executions/` ディレクトリ対応表、および `docs/テスト知識管理のGit-nativeモデル_統合版.md` §3.1の `TESTEXECUTION`)。結果の集計・レポート表示、CIテストレポート形式からの一括投入(`--from-report`)、過去マイルストーン時点の `generated/testcases/` に対する検証は未実装(将来課題)。

---

### 1.15 `markharness changes annotate` — change_type / related_eventsの事後入力(§3.5)

```text
markharness changes annotate <event_id> [--type <spec-change|bug-fix|refactor|other>] [--related <event_id>]... [-d, --dir <path>]
```

**用途**: `changes compute`(1.11節)が算出した `ChangeEvent` の `change_type` と `related_events` を、人間が事後に設定する。`changes/` 配下の全 `*.yaml` ファイルを `event_id` で横断検索するため、呼び出し側はどのマイルストーン区間のファイルに含まれるかを事前に知る必要がない。

**動作**

- `--type` と `--related` は互いに独立した加算的フィールドであり、どちらか一方だけを指定してもよい(両方省略した場合はエラー、少なくとも一方の指定が必須)。
- `--type` を指定すると、一致する `event_id` を持つ最初のファイルの `change_type` を書き換える。同じファイル内の他の `ChangeEvent` は変更しない。
- `--related <event_id>` は複数回指定でき、それらを対象イベントの `related_events` に追記する(既存の値は保持、上書きではなく追加)。
- `--related` を指定した場合、対象の `event_id` と `--related` に指定した全ての `event_id` が `changes/*.yaml` のどこかに存在するかを、書き込みより前に検証する。いずれかが見つからなければ、`--type` を指定していてもその書き込みは行われずに終了コード `3` でエラーになる(`--type`・`--related` は独立した加算的フィールドだが、コマンド全体としては全て書き込むか何も書き込まないかのいずれかになる)。
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

**用途**: 指定したマージコミットについて、その2親(P1・P2)と `git merge-base` によるマージベース(B)のtree SHAを比較し、各Feature idごとに§3.2の場合分け(`linear` / `true_divergence` / `single_parent`)を判定して出力する監査専用コマンド。`changes compute`(1.11節)は、`from-milestone..to-milestone`区間内に存在する全ての2親マージコミットについて本コマンドと同じ判定ロジックを内部で呼び出し、結果を`true_divergences`に反映する。個別のマージコミット単体を人手で監査・確認したい場合は、本コマンドを独立に実行する。本コマンド自体は `changes/*.yaml` への書き込みを行わない(読み取り専用の監査コマンド)。squash mergeやrebase・fast-forward mergeで運用されたリポジトリでは、そもそも対象となる2親マージコミットがコミットグラフ上に存在しないため、本コマンドで監査できる対象自体が無い(論文§3.4表2)。

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

### 1.17 `markharness validate` — knowledge/・axes/・executions/ の構造検証(§3.5/§3.6)

```text
markharness validate [--json] [-d, --dir <path>]
```

**用途**: `knowledge/` 配下の全YAML(`requirement.yml` / `feature.yml` / `behavior.yml` / `condition.yml` / `expected/*.yml`)と `axes/*.yml`、および `executions/<milestone>/results.yml` を、対応する `schema/*.schema.json`(`markharness init` が既定一式を配置。1.1節)でJSON Schema検証する。加えて、JSON Schema単体では表現できない相互参照制約を検証する: `axis` タグが `axes/*.yml` に登録されているか、`feature.yml` の `forked_from` が実在するFeature idを指しているか。

**`executions/*/results.yml`のスキーマ**: `execution_result.schema.json` は `case_id` / `result`(`pass`/`fail`/`skip`) / `executor` / `executed_at` を必須、`note` / `verified_feature_tree_shas` を任意フィールドとする(1.14節)。`verified_feature_tree_shas` は本仕様導入前に書かれた実行記録には存在しないが、任意フィールドとして定義しているため過去の記録もそのままスキーマ検証を通る。この場合、`verify trace`/`verify pending`(change-event-verification-tracking-spec.md §6)は当該レコードを遡及的に補完せず「不明」として扱う。

**動作**

- 問題が1件もなければ終了コード `0`。人間可読モードでは `knowledge/ and axes/ are valid`、`--json` では `{"ok":true}` を出力する。
- 問題があれば、ファイルごとのメッセージを列挙して終了コード `1` で終了する。

**使用例**

```console
$ markharness validate
knowledge/controls/player-jump/feature.yml: axis 'not-registered' is not registered under axes/
$ echo $?
1
```

**ユースケース対応**: §3.5の「`axes/*.yml`に定義されていない値をfront matterで使えないようスキーマバリデーションで縛る」制約の実装。

---

## 2. 未実装(今後実装予定)のコマンド

以下は `docs/product-operation.md` のユースケース図・ユースケース記述に基づく、今後実装予定のコマンドです。コマンド名・オプションは暫定案であり、実装時に変更され得ます。

| #   | ユースケース                 | 想定コマンド(暫定)                                                  | アクター                | 概要                                                                                      |
| --- | ---------------------------- | ------------------------------------------------------------------- | ----------------------- | ----------------------------------------------------------------------------------------- |
| UC4 | マイルストーンをタグ付けする | 専用コマンドなし(`git tag <milestone>` を直接使用)                  | Release Manager         | リリースタイミングの意思決定そのものであり、人間の判断ポイント(図3)。                     |
| UC8 | 既存ツールからインポートする | `markharness import --from <testrail\|xray\|testlink> <file>`(暫定) | Data Migration Operator | 既存TMSのエクスポートファイルを `knowledge/` 構造に変換する(§4.5)。今回の実装スコープ外。 |

これらは現時点で未着手であり、実装順序は別途チェックリスト(`/plan-checklist`)で管理する。

---

## 3. 動作確認・テスト

実装済みコマンドの単体テストは `cargo test` で実行できる(`src/init.rs` / `src/knowledge.rs` / `src/interactive.rs` / `src/knowledge_draft.rs` / `src/knowledge_apply.rs` / `src/knowledge_edit.rs` / `src/generate.rs` / `src/verify.rs` / `src/axes.rs` / `src/traceability.rs` / `src/git.rs` / `src/id_cache.rs` / `src/changes.rs` / `src/backfill.rs` の `#[cfg(test)] mod tests`、および `knowledge validate`/`apply` の終了コード・出力を検証する `tests/knowledge_cli.rs` を参照)。`git.rs`/`id_cache.rs`/`changes.rs`/`backfill.rs` のテストは実際に一時ディレクトリ上で `git init`/`commit`/`tag` を行うため、テスト実行環境に `git` コマンドが必要。Pre-PR チェックリスト(`PROJECT.md`)に従い、コミット前に以下を実行すること:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo audit
```
