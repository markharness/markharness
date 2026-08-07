# markharness CLI マニュアル

本資料は `markharness` CLI の使用方法を、**実装済みコマンド**と**未実装(今後実装予定)のコマンド**に分けてまとめたものです。ユースケース(UC1〜UC8)の対応は `docs/product-operation.md` の「3. ユースケース記述」表に基づきます。実装済みコマンドの具体的な生成規則は `docs/testcase-generation-design.md` を参照してください(ただし `generate`/`verify` の現行実装は、同ドキュメント作成後に `feature → behavior → condition → expected` の4階層モデルへ刷新されており、詳細は本マニュアル 1.3/1.4 節を正としてください)。

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

### 1.2 `markharness knowledge add` — 知識の対話的記述(UC1: 知識を記述する)

```text
markharness knowledge add [--dir <path>]
```

**用途**: Test Designer が `Feature` → `Behavior` → `Condition` → `ExpectedResult` の4階層を対話形式(標準入力への逐次プロンプト)で記述し、`knowledge/` 配下に `.yml` ファイルを作成する。`Behavior` は「機能がどう振る舞うか」を表す必須の中間階層で、`generate` が組み立てる TestCase の `steps`(手順)の元になる。

**オプション**

| オプション | 説明 |
|---|---|
| `-d, --dir <path>` | 対象プロジェクトディレクトリ(`knowledge/` の親)を指定する。省略時はカレントディレクトリを対象にする。 |

**使用例(カレントディレクトリ以外を対象にする)**

```console
$ markharness knowledge add --dir tmp/todo-sample
Feature name (e.g. add-todo): player-jump
Axis (comma separated, e.g. ui, validation): gameplay, animation
Behavior name (e.g. add-task): jump
Behavior axis (comma separated, e.g. ui, validation): gameplay
Behavior description (e.g. User adds a new task to the list.): Player presses jump.
Condition name (e.g. empty-title): ground
Scenario (e.g. Submit the todo form with an empty title): Jump from the ground and land
Expected result (e.g. shows a validation error): lands safely
```
→ `tmp/todo-sample/knowledge/player-jump/...` にファイルが作成される。

**アクター**: Test Designer(`docs/product-operation.md` UC1)

**フロー**

1. `Feature name (e.g. add-todo):` — Feature の slug(小文字英数字とハイフンのみ)、または日本語ラベルを入力
   - `knowledge/` 配下に既存の Feature が1件以上あれば、プロンプトの前に `N) id` 形式で番号付き一覧を表示する。番号を入力すると対応する Feature を選択でき、既存の id をそのまま直接入力しても再利用できる(挙動は変わらない)。候補が0件の場合は一覧を表示しない。
   - 既存の `knowledge/<feature_id>/feature.yml` があれば再利用し、次のプロンプトへスキップする
   - 新規の場合のみ `Axis (comma separated, e.g. ui, validation):` で観点をカンマ区切りで入力し、`feature.yml` を新規作成する
2. `Behavior name (e.g. add-task):` — Behavior の slug、または日本語ラベルを入力
   - 選択した Feature 配下に既存の Behavior が1件以上あれば、同様に番号付き一覧を表示し、番号選択・直接入力のいずれも可能。
   - 既存の `knowledge/<feature_id>/<behavior_id>/behavior.yml` があれば再利用し、次のプロンプトへスキップする
   - 新規の場合のみ `Behavior axis (...)` と `Behavior description (...)` を入力し、`behavior.yml` を新規作成する
3. `Condition name (e.g. empty-title):` — Condition の slug、または日本語ラベルを入力
   - 選択した Behavior 配下に既存の Condition が1件以上あれば、同様に番号付き一覧を表示し、番号選択・直接入力のいずれも可能。
   - 新規に作成する Condition id が `{behavior_id}-` で始まる場合(Behavior id を重複して含めてしまった場合)、その接頭辞を自動的に除去してから作成し、その旨を通知する(例: Behavior `jump` に Condition id `jump-ground` と入力すると `ground` として作成される)。ただし、入力された id そのままのディレクトリが既に存在する場合は除去せずそのまま再利用する(過去に手動で重複した名前のまま作成されたデータを壊さないため)。
   - 既存の `knowledge/<feature_id>/<behavior_id>/<condition_id>/condition.yml` があれば(除去後の id で判定し)再利用し、次のプロンプトへスキップする
   - 新規の場合のみ `Scenario (e.g. Submit the todo form with an empty title):` で条件の説明を入力し、`condition.yml` を新規作成する
4. `Expected result (e.g. shows a validation error):` — 期待結果のテキストを入力し、`expected/NNN.yml`(3桁連番、既存ファイル数+1)を作成する

**プロンプト文言について**: 各プロンプトは内部的には Feature/Behavior/Condition の `id`(ディレクトリ名・YAMLの `id` フィールド)を決めるものだが、人間が入力する際に「id」という抽象的な概念に迷わないよう、`Feature name` / `Behavior name` / `Condition name` のように分かりやすい英語表現と入力例を添えている。内部データモデル(`id`フィールド、通知メッセージ、コード上の変数名)は変更していない。

**日本語ラベル入力(Feature name / Behavior name / Condition name)**

各 name プロンプトでは、ASCII以外の文字を含む入力(日本語ラベルなど)を渡すと、id直接入力の代わりに以下のローマ字変換フローに切り替わる。ExpectedResult の id は自動連番のため対象外。

1. 入力文字列に非ASCII文字が含まれるかを判定する。
2. 含まれる場合、[`kakasi`](https://crates.io/crates/kakasi) crate で入力をローマ字に変換し、続けて正規化(小文字化・空白のハイフン化・連続ハイフンの圧縮・先頭/末尾ハイフン除去・許可されない記号の除去)を行った id 候補を1件提示する。
3. `id候補: <候補> (Enterで採用、編集する場合は入力):`というプロンプトに対して空入力(Enter)のみを送るとその候補をそのまま id として採用する。何か文字列を入力すると、その入力を同じ正規化ルールに通した上で id として採用する(自由編集)。
4. 正規化後の id が既存候補一覧(番号付き一覧に表示されているもの)と衝突する場合は警告を表示し、その階層の id 入力からやり直しになる(自動的な既存id流用はしない。既存idの意図的な再利用は番号選択で行う)。
5. 日本語ラベル入力を経由して新規作成された Feature の `label` フィールドには、入力した日本語文字列がそのまま保存される。ASCII直接入力の場合や番号選択で既存を再利用した場合は入力値そのもの(= id と同じ文字列)が `label` として保存される。Behavior/Condition/ExpectedResult のスキーマには `label` フィールドは存在しない。

**入力バリデーション**

- id(Feature id / Behavior id / Condition id)は小文字英数字とハイフンのみ許可。不正な場合は再入力を促す。
- 候補一覧が表示されている場合、1以上・候補件数以下の整数を入力すると対応する候補が選択される。範囲外の整数や非数値は通常のid入力(またはASCII以外を含む場合は日本語ラベル)として扱われる。
- すべてのプロンプトで空入力(trim後に空)は再入力を促す。ただし日本語ラベル変換後の id候補提示に対する空入力は「候補をそのまま採用」を意味し、再入力を促す対象ではない。

**生成されるファイル**(例: `player-jump` / `jump` / `ground` / 1件目)

```
knowledge/player-jump/feature.yml
knowledge/player-jump/jump/behavior.yml
knowledge/player-jump/jump/ground/condition.yml
knowledge/player-jump/jump/ground/expected/001.yml
```

`feature.yml`:
```yaml
id: player-jump
label: player-jump
axis: [gameplay, animation]
```

`behavior.yml`:
```yaml
id: jump
feature: player-jump
axis: [gameplay]
description: |
  Player presses jump.
```

`condition.yml`:
```yaml
id: ground
behavior: jump
description: |
  Jump from the ground and land
```

`expected/001.yml`(id は `{condition_id}-{連番3桁}`):
```yaml
id: ground-001
condition: ground
description: |
  lands safely
```

**使用例(初回セッション)**

```console
$ markharness knowledge add
Feature name (e.g. add-todo): player-jump
Axis (comma separated, e.g. ui, validation): gameplay, animation
Behavior name (e.g. add-task): jump
Behavior axis (comma separated, e.g. ui, validation): gameplay
Behavior description (e.g. User adds a new task to the list.): Player presses jump.
Condition name (e.g. empty-title): ground
Scenario (e.g. Submit the todo form with an empty title): Jump from the ground and land
Expected result (e.g. shows a validation error): lands safely
```

**使用例(既存Feature/Behavior/Conditionへの2件目のExpectedResult追加、番号選択)**

```console
$ markharness knowledge add
Feature name (e.g. add-todo):
  1) player-jump
1
既存のFeature 'player-jump' を再利用します。
Behavior name (e.g. add-task):
  1) jump
1
既存のBehavior 'jump' を再利用します。
Condition name (e.g. empty-title):
  1) ground
1
既存のCondition 'ground' を再利用します。
Expected result (e.g. shows a validation error): falls over
```
→ `knowledge/player-jump/jump/ground/expected/002.yml` が作成される。番号の代わりに `player-jump` / `jump` / `ground` を直接入力しても同じ結果になる。

**使用例(Condition id の重複接頭辞を自動除去)**

```console
$ markharness knowledge add
Feature name (e.g. add-todo): player-jump
既存のFeature 'player-jump' を再利用します。
Behavior name (e.g. add-task): jump
既存のBehavior 'jump' を再利用します。
Condition name (e.g. empty-title):
  1) ground
jump-ground
Condition id 'jump-ground' から Behavior id 'jump' と重複する接頭辞を除去し、'ground' として作成します。
Scenario (e.g. Submit the todo form with an empty title): Jump and land, then stand still
Expected result (e.g. shows a validation error): stands still
```
→ `knowledge/player-jump/jump/ground/condition.yml` と `knowledge/player-jump/jump/ground/expected/001.yml` が作成される(`jump-ground/` ディレクトリは作成されない)。

**ユースケース対応**: UC1「知識を記述する」(手動記述、`docs/product-operation.md` 103行目)を対話形式で支援する。

---

### 1.3 `markharness generate` — TestCase の決定的生成(UC2: TestCaseを決定的生成する)

```text
markharness generate
```

**用途**: `knowledge/` 配下を決定的に走査し、`Feature × Behavior × Condition × ExpectedResult` から `TestCase` を機械的に組み立てて、`generated/testcases/` 配下に **1 Condition = 1 ファイル** の `.yml` として再生成する。実行のたびに `generated/testcases/` を空にしてから書き直すため、削除された Condition に対応する古いファイルも自動的に消える。

**アクター**: 本来は CI Bot(UC2)だが、ローカルでの事前確認用に手動実行も可能。

**アルゴリズム概要**

- `knowledge/` 配下を `feature.yml` → `behavior.yml` → `condition.yml` → `expected/*.yml` の順に、パスのソート順で走査する(実行環境・タイムスタンプに依存しない)。`Behavior` を持たない `Feature` や `expected/` が空(または存在しない)の `Condition` からは `TestCase` は生成されない。
- **集約モデル**: 1つの `Condition` の `expected/` 配下にある全ファイルを、1つの `TestCase` の `expected` 配列に集約する(1 Condition = 1 TestCase。1 expected ファイルごとに別 TestCase を作る旧モデルからの変更)。
- `case_id = "tc-{condition.id}-001"`。連番は将来 1 Condition から複数 TestCase を生成する拡張のために予約されており、現状は常に `001`。
- 出力ファイル名は `generated/testcases/{condition.id}.yml`(`case_id` の `tc-` 接頭辞は付けない)。
- `title` = `condition.description`、`steps` = `[behavior.description]`、`expected` = 各 `expected/*.yml` の `description` をファイル名のソート順で列挙。
- `generated_from` に `feature` / `behavior` / `condition` の各 id と、集約元の `expected_results`(`expected/*.yml` の `id` の一覧)を記録する。
- 出力は `serde_yaml_ng` によるシリアライズで、同一入力に対して常に同一の出力になる(決定性、CIでの差分検証の前提)。

**使用例**

```console
$ markharness generate
generated 1 testcase(s) into generated/testcases/
```

`generated/testcases/ground.yml`:
```yaml
case_id: tc-ground-001
generated_from:
  feature: player-jump
  behavior: jump
  condition: ground
  expected_results:
  - ground-001
title: |
  Jump from the ground and land
steps:
- |
  Player presses jump.
expected:
- |
  lands safely
```

`knowledge/` に何も無い場合は `generated/testcases/` が空(0ファイル)になる。

**ユースケース対応**: UC2「TestCaseを決定的生成する」(`docs/product-operation.md` 105行目)。CI上での差分検証(UC3)は 1.4 節の `markharness verify` で行う。

---

### 1.4 `markharness verify` — 生成物の差分検証(UC3: 生成物をレビュー・マージする)

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
added: generated/testcases/ground.yml
changed: generated/testcases/air.yml
removed: generated/testcases/water.yml
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
| — | `knowledge/requirement.yml` の管理・`generated/traceability-index.json` の生成 | 未定 | Test Designer / CI Bot | Requirement → Feature → Behavior → Condition → TestCase の対応関係を機械可読な索引として保持する(製品化提案、スコープは別タスクで検討)。 |
| — | 参照整合性検証(`behavior.feature` 等とディレクトリ階層の一致確認) | `markharness knowledge validate`(暫定) | Test Designer / CI Bot | `feature:` / `behavior:` / `condition:` フィールドが実際の親ディレクトリと一致しているかを検証する(スコープは別タスクで検討)。 |
| UC4 | マイルストーンをタグ付けする | 専用コマンドなし(`git tag <milestone>` を直接使用) | Release Manager | リリースタイミングの意思決定そのものであり、人間の判断ポイント(図3)。 |
| UC5 | ChangeEventを自動計算する | `markharness changes compute <from-milestone> <to-milestone>`(暫定) | CI Bot | 2マイルストーン間でid解決経由のblob SHAを比較し `derived_from` を算出、`changes/<milestone>.yaml` に書き込む(本研究の核心的貢献、§3.2-3.4)。 |
| UC6 | バックフィルを非同期実行する | `markharness backfill run`(暫定) | Backfill Worker | 直近マイルストーンから優先的に過去の系譜を計算し、`git notes` に進捗を記録しながら `changes/*.yaml` を段階的に埋める(§4.1-4.2)。 |
| UC7 | idキャッシュを破棄・再構築する | `markharness cache rebuild` / 各コマンドの `--no-cache` オプション(暫定) | Test Designer / CI Bot | id解決キャッシュの不整合が疑われる場合に明示的に破棄・再構築するフェイルセーフ(199行目)。 |
| UC8 | 既存ツールからインポートする | `markharness import --from <testrail\|xray\|testlink> <file>`(暫定) | Data Migration Operator | 既存TMSのエクスポートファイルを `knowledge/` 構造に変換する(§4.5)。 |

これらは現時点で未着手であり、実装順序は別途チェックリスト(`/plan-checklist`)で管理する。

---

## 3. 動作確認・テスト

実装済みコマンドの単体テストは `cargo test` で実行できる(`src/init.rs` / `src/knowledge.rs` / `src/interactive.rs` / `src/generate.rs` / `src/verify.rs` の `#[cfg(test)] mod tests` を参照)。Pre-PR チェックリスト(`PROJECT.md`)に従い、コミット前に以下を実行すること:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo audit
```
