# markharness CLI マニュアル

本資料は `markharness` CLI の使用方法を、**実装済みコマンド**と**未実装(今後実装予定)のコマンド**に分けてまとめたものです。ユースケース(UC1〜UC8)の対応は `docs/product-operation.md` の「3. ユースケース記述」表に基づきます。実装済みコマンドの具体的な生成規則は `docs/testcase-generation-design.md` を参照してください(ただし `generate`/`verify` の現行実装は、同ドキュメント作成後に `feature → behavior → condition → expected` の4階層モデルへ刷新されており、詳細は本マニュアル 1.5/1.6 節を正としてください)。`knowledge validate`/`apply`(非対話・TTY非依存版、1.3/1.4節)の詳細設計は `docs/knowledge-apply-cli-spec.md` を正としてください。

---

## 1. 実装済みコマンド

### 1.1 `markharness init` — プロジェクトの初期化(UC1〜UC8 の前提)

```text
markharness init
```

**用途**: UC1〜UC8を支える物理ディレクトリ構成(論文 §3.5, 244-273行目)のうち、対象リポジトリ上に作成が必要な7ディレクトリを作成し、以降のコマンドが動作できる状態にする。

| ディレクトリ | 対応UC |
|---|---|
| `knowledge/` | UC1(知識を記述する)/ UC1b(forked_from を手動記述する) |
| `axes/` | UC1(横断的観点 Axis のレジストリ、§3.1) |
| `generated/` | UC2(TestCaseを決定的生成する)/ UC3(生成物をレビュー・マージする) |
| `executions/` | UC4(マイルストーンをタグ付けする、実行結果の記録先) |
| `changes/` | UC5(ChangeEventを自動計算する)/ UC6(バックフィルを非同期実行する) |
| `schema/` | UC7(idキャッシュを破棄・再構築する。フォーマット・正規化ルール定義) |
| `tools/` | UC2/UC5/UC6/UC7 で使う生成・検証スクリプト置き場 |

UC8(既存ツールからのインポート)は専用ディレクトリを持たず、変換結果を `knowledge/` に書き込む想定のため対象外。

**動作**

- 各ディレクトリについて、存在しなければ作成し、既に存在すればそのまま(中身も含めて)何もしない冪等な処理。すでに初期化済みのプロジェクトで再実行してもエラーにはならず、不足しているディレクトリだけが追加で作成される。
- 成功すると作成先のパスを標準出力に表示する。

**使用例**

```console
$ markharness init
initialized knowledge/, axes/, generated/, executions/, changes/, schema/, tools/ under /path/to/project

$ markharness init
initialized knowledge/, axes/, generated/, executions/, changes/, schema/, tools/ under /path/to/project
```

**ユースケース対応**: どのUCにも明示的には現れないが、UC1〜UC8の全ユースケースを開始する前提条件を満たすための補助コマンド。

---

### 1.2 `markharness knowledge add` — 知識の対話的記述(UC1: 知識を記述する。Requirement → Feature → Behavior → Condition → ExpectedResult の順)

```text
markharness knowledge add [--dir <path>]
```

**用途**: Test Designer が `Requirement` → `Feature` → `Behavior` → `Condition` → `ExpectedResult` の5階層を対話形式(標準入力への逐次プロンプト)で記述し、`knowledge/` 配下に `.yml` ファイルを作成する。`Requirement` は Feature の親となる要求単位で、`Feature` は自身の `requirement:` フィールドで親を参照する。`Behavior` は「機能がどう振る舞うか」を表す必須の中間階層で、`generate` が組み立てる TestCase の `steps`(手順)の元になる。

**オプション**

| オプション | 説明 |
|---|---|
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

| オプション | 説明 |
|---|---|
| `<draft-file>` | (必須)ドラフトYAMLファイルのパス |
| `-d, --dir <path>` | 対象プロジェクトディレクトリ(`knowledge/` の親)。省略時はカレントディレクトリ |
| `--json` | エラー・結果を1行のJSONで出力する。省略時は人間可読なテキストを出力する |

**ドラフトYAMLの形式**(1回の実行で1本のチェーンを検証する。複数チェーンの一括検証は非対応)

```yaml
requirement:
  id: controls          # 必須。ASCII slug
  label: controls        # 省略可(既存id再利用時は省略可)
  axis: [gameplay]        # 新規作成時は必須。既存id再利用時は省略可
  description: null       # 省略可

feature:
  id: player-jump
  label: player-jump
  axis: [gameplay, animation]

behavior:
  id: jump
  label: jump
  axis: [gameplay]
  description: Player presses jump.   # Behaviorのみdescriptionが必須(新規作成時)

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

| エラーコード | 内容 |
|---|---|
| `invalid_slug` | idが小文字英数字とハイフン以外を含む |
| `missing_axis` | 新規作成のRequirement/Feature/Behaviorで `axis` が空・未指定 |
| `missing_description` | 新規作成のBehavior/Condition、または各ExpectedResultで `description` が空 |
| `unknown_axis` | `axes/*.yml` レジストリに登録されていない観点値(近似候補があれば `suggestion` に提示) |
| `redundant_prefix` | `condition.id` が `{behavior.id}-` で始まる(`knowledge apply` の `--strip-redundant-prefix` 未指定時。1.4節参照) |
| `conflicting_existing_value` | 既存id再利用時、指定した `label`/`axis`/`description` が既存ファイルの値と不一致 |
| `parent_not_found` | 既存ファイルに記録された親参照(例: `feature.yml` の `requirement:`)がドラフトのチェーンと矛盾 |

**終了コード**

| コード | 意味 |
|---|---|
| 0 | 成功(エラーなし) |
| 1 | バリデーションエラーあり(エラー内容はstderr、`--json`指定時はstdoutにJSONで出力) |
| 2 | 使用方法エラー(ファイル不在・YAMLパース不能) |

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

| オプション | 説明 |
|---|---|
| `<draft-file>` | (必須)ドラフトYAMLファイルのパス。形式は1.3節と共通 |
| `-d, --dir <path>` | 1.3節と同様 |
| `--json` | 1.3節と同様。成功時は書き込んだファイル一覧を出力する(下記参照) |
| `--strip-redundant-prefix` | `condition.id` が `{behavior.id}-` で始まる場合、確認なしで接頭辞を除去したidを採用する。未指定の場合は `redundant_prefix` エラーで停止する(1.3節参照)。除去後idと同名のディレクトリが既に存在する(レガシーデータ)場合は、`knowledge add` と同様に除去せず既存のものをそのまま再利用する |
| `--dry-run` | `knowledge validate` と同義(検証のみ行い書き込まない)。CI等での用途を想定した別名 |

**終了コード**

| コード | 意味 |
|---|---|
| 0 | 成功(書き込み成功。`--dry-run` 指定時はエラーなし) |
| 1 | バリデーションエラーあり(1.3節と同じ形式。ファイルは一切書き込まれない) |
| 2 | 使用方法エラー(ファイル不在・YAMLパース不能) |
| 3 | ファイルシステムエラー(書き込み失敗など) |

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

**ユースケース対応**: UC1「知識を記述する」(`docs/product-operation.md` 103行目)を、TTYに依存しない形で支援する。AIエージェント・将来のGUI実装が知識を確定登録するための共通エントリポイント。人間向けの `$EDITOR` 起動ラッパー(`knowledge add --edit`)は未実装(2章参照)。

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
- `generated_from` に `requirement` / `feature` / `behavior` / `condition` の各 id と、集約元の `expected_results`(`expected/*.yml` の `id` の一覧)を記録する。トレーサビリティ索引(`generated/traceability-index.json` 等)自体は未実装だが、この `requirement` フィールドにより生成済み TestCase 単体からでも由来する Requirement を追跡できる。
- 出力は `serde_yaml_ng` によるシリアライズで、同一入力に対して常に同一の出力になる(決定性、CIでの差分検証の前提)。
- **既知の未対応事項**: `テスト知識管理のGit-nativeモデル_統合版V2.md` §3.4「axisの継承」は `FEATURE` の `axis` を生成された `TestCase` にコピーする設計だが、現行の `TestCase` 構造体(`src/generate.rs`)には `axis` フィールドが存在せず、継承されていない。2章の未実装タスク一覧を参照。

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

**用途**: `knowledge/` から `generate` と同じロジックで TestCase を再構築し(ディスクへは書き込まない)、コミット済みの `generated/testcases/*.yml` と1ファイルずつ比較する。CI上でこのコマンドを実行し、`knowledge/` の変更を `generated/testcases/` へ反映し忘れていないかを検証する想定。

**アクター**: Reviewer / CI Bot(UC3)

**動作**

- 差分が無ければ `generated/testcases/ is up to date with knowledge/` を表示し、終了コード `0`。
- 差分があれば、追加・削除・変更されたファイルを `added:` / `removed:` / `changed:` のラベル付きでファイル名のソート順に一覧表示し、終了コード `1` で終了する(内容のunified diffまでは表示しない)。

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

## 2. 未実装(今後実装予定)のコマンド

以下は `docs/product-operation.md` のユースケース図・ユースケース記述に基づく、今後実装予定のコマンドです。コマンド名・オプションは暫定案であり、実装時に変更され得ます。

| # | ユースケース | 想定コマンド(暫定) | アクター | 概要 |
|---|---|---|---|---|
| UC1b | `forked_from` を手動記述する | (専用コマンドなし。`feature.yml` に `forked_from` フィールドを直接追記する運用を想定。将来的に `markharness knowledge fork <from-feature-id> <to-feature-id>` のような補助コマンドを検討) | Test Designer | 別Featureからの概念的派生を明示化する。Git履歴からは自動導出できないため必須の手動記述(§3.1, 153行目)。 |
| — | `generated/traceability-index.json` の生成 | 未定 | Test Designer / CI Bot | Requirement → Feature → Behavior → Condition → TestCase の対応関係を機械可読な索引として保持する(製品化提案、スコープは別タスクで検討)。なお `knowledge/<requirement>/requirement.yml` の記録自体は `knowledge add`(1.2節)で実装済み。 |
| — | `knowledge add --edit`(`$EDITOR` 起動ラッパー) | `markharness knowledge add --edit`(暫定) | Test Designer | テンプレートドラフトを一時ファイルに生成し `$EDITOR` を起動、保存後に `knowledge apply`(1.4節)相当の処理を呼ぶ。バリデーションエラー時はエディタを再度開いて修正させる(`docs/knowledge-apply-cli-spec.md` §3.3/§9.3)。参照整合性検証(`feature:`/`behavior:`/`condition:` とディレクトリ階層の一致確認)自体は `knowledge validate`/`apply`(1.3/1.4節)で実装済み。 |
| — | `markharness axes list` | `markharness axes list [--json]`(暫定) | Test Designer / AIエージェント | `axes/*.yml` に登録済みの観点(axis)一覧を出力する。`knowledge apply` の `unknown_axis` エラーを事前に回避するための参照コマンド(`docs/knowledge-apply-cli-spec.md` §8)。 |
| UC4 | マイルストーンをタグ付けする | 専用コマンドなし(`git tag <milestone>` を直接使用) | Release Manager | リリースタイミングの意思決定そのものであり、人間の判断ポイント(図3)。 |
| UC5 | ChangeEventを自動計算する | `markharness changes compute <from-milestone> <to-milestone>`(暫定) | CI Bot | 2マイルストーン間でid解決経由のblob SHAを比較し `derived_from` を算出、`changes/<milestone>.yaml` に書き込む(本研究の核心的貢献、§3.2-3.4)。 |
| UC6 | バックフィルを非同期実行する | `markharness backfill run`(暫定) | Backfill Worker | 直近マイルストーンから優先的に過去の系譜を計算し、`git notes` に進捗を記録しながら `changes/*.yaml` を段階的に埋める(§4.1-4.2)。 |
| UC7 | idキャッシュを破棄・再構築する | `markharness cache rebuild` / 各コマンドの `--no-cache` オプション(暫定) | Test Designer / CI Bot | id解決キャッシュの不整合が疑われる場合に明示的に破棄・再構築するフェイルセーフ(199行目)。 |
| UC8 | 既存ツールからインポートする | `markharness import --from <testrail\|xray\|testlink> <file>`(暫定) | Data Migration Operator | 既存TMSのエクスポートファイルを `knowledge/` 構造に変換する(§4.5)。 |
| — | `TestCase` への `axis` 継承 | `generate` の内部ロジック修正(専用コマンドなし) | CI Bot | `テスト知識管理のGit-nativeモデル_統合版V2.md` §3.4で設計されている「`FEATURE.axis` を生成された `TestCase` にコピーする」処理が未実装(1.5節「既知の未対応事項」参照)。`TestCase` 構造体への `axis` フィールド追加と、`generated/traceability-index.json`(本表2番目の項目)での観点別集計の前提になる。 |

これらは現時点で未着手であり、実装順序は別途チェックリスト(`/plan-checklist`)で管理する。

---

## 3. 動作確認・テスト

実装済みコマンドの単体テストは `cargo test` で実行できる(`src/init.rs` / `src/knowledge.rs` / `src/interactive.rs` / `src/knowledge_draft.rs` / `src/knowledge_apply.rs` / `src/generate.rs` / `src/verify.rs` の `#[cfg(test)] mod tests`、および `knowledge validate`/`apply` の終了コード・出力を検証する `tests/knowledge_cli.rs` を参照)。Pre-PR チェックリスト(`PROJECT.md`)に従い、コミット前に以下を実行すること:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo audit
```
