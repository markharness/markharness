# markharness CLI マニュアル

本資料は `markharness` CLI の使用方法を、**実装済みコマンド**と**未実装(今後実装予定)のコマンド**に分けてまとめたものです。ユースケース(UC1〜UC8)の対応は `docs/product-operation.md` の「3. ユースケース記述」表に基づきます。実装済みコマンドの具体的な生成規則は `docs/testcase-generation-design.md` を参照してください。

---

## 1. 実装済みコマンド

### 1.1 `markharness init` — プロジェクトの初期化(UC1 の前提)

```text
markharness init [--force]
```

**用途**: `knowledge/`・`generated/`・`changes/` の3ディレクトリを作成し、以降のコマンドが動作できる状態にする。

**オプション**

| オプション | 説明 |
|---|---|
| `--force` | 既に初期化済みでもエラーにせず再実行する。既存の `knowledge/` 配下のファイルは削除・上書きされない(存在しないディレクトリのみ作成)。 |

**動作**

- 3ディレクトリのいずれかが既に存在し、`--force` を指定しなかった場合はエラーで終了する(誤って既存プロジェクトを壊さないためのフェイルセーフ)。
- 成功すると作成先のパスを標準出力に表示する。

**使用例**

```console
$ markharness init
initialized knowledge/, generated/, changes/ under /path/to/project

$ markharness init
error: /path/to/project/knowledge already exists; pass --force to re-initialize

$ markharness init --force
initialized knowledge/, generated/, changes/ under /path/to/project
```

**ユースケース対応**: どのUCにも明示的には現れないが、UC1(知識を記述する)を開始する前提条件を満たすための補助コマンド。

---

### 1.2 `markharness knowledge add` — 知識の対話的記述(UC1: 知識を記述する)

```text
markharness knowledge add
```

**用途**: Test Designer が `Feature` → `Condition` → `ExpectedResult` を対話形式(標準入力への逐次プロンプト)で記述し、`knowledge/` 配下にYAMLファイルを作成する。

**アクター**: Test Designer(`docs/product-operation.md` UC1)

**フロー**

1. `Feature id:` — Feature の slug(小文字英数字とハイフンのみ)を入力
   - 既存の `knowledge/<feature_id>/feature.yaml` があれば再利用し、次のプロンプトへスキップする
   - 新規の場合のみ `Axis (comma separated):` で観点をカンマ区切りで入力し、`feature.yaml` を新規作成する
2. `Condition id:` — Condition の slug を入力
   - 既存の `knowledge/<feature_id>/<condition_id>/condition.yaml` があれば再利用し、次のプロンプトへスキップする
   - 新規の場合のみ `Summary:` で条件の要約を入力し、`condition.yaml` を新規作成する
3. `Expected result:` — 期待結果のテキストを入力し、`expected/NNN.yaml`(3桁連番、既存ファイル数+1)を作成する

**入力バリデーション**

- id(Feature id / Condition id)は小文字英数字とハイフンのみ許可。不正な場合は再入力を促す。
- すべてのプロンプトで空入力(trim後に空)は再入力を促す。

**生成されるファイル**(例: `player-jump` / `jump-ground` / 1件目)

```
knowledge/player-jump/feature.yaml
knowledge/player-jump/jump-ground/condition.yaml
knowledge/player-jump/jump-ground/expected/001.yaml
```

`feature.yaml`:
```yaml
id: player-jump
kind: feature
axis:
  - gameplay
  - animation
```

`condition.yaml`:
```yaml
id: jump-ground
kind: condition
summary: Jump from the ground and land
```

`expected/001.yaml`(id は `{feature_id}-{condition_id}-{連番3桁}`):
```yaml
id: player-jump-jump-ground-001
kind: expected-result
result: lands safely
```

**使用例(初回セッション)**

```console
$ markharness knowledge add
Feature id: player-jump
Axis (comma separated): gameplay, animation
Condition id: jump-ground
Summary: Jump from the ground and land
Expected result: lands safely
```

**使用例(既存Featureへの2件目のExpectedResult追加)**

```console
$ markharness knowledge add
Feature id: player-jump
既存のFeature 'player-jump' を再利用します。
Condition id: jump-ground
既存のCondition 'jump-ground' を再利用します。
Expected result: falls over
```
→ `knowledge/player-jump/jump-ground/expected/002.yaml` が作成される。

**ユースケース対応**: UC1「知識を記述する」(手動記述、`docs/product-operation.md` 103行目)を対話形式で支援する。

---

### 1.3 `markharness generate` — TestCase の決定的生成(UC2: TestCaseを決定的生成する)

```text
markharness generate
```

**用途**: `knowledge/` 配下を決定的に走査し、`Feature × Condition × ExpectedResult` から `TestCase` を機械的に組み立てて `generated/testcases.yaml` を再生成する。

**アクター**: 本来は CI Bot(UC2)だが、ローカルでの事前確認用に手動実行も可能。

**アルゴリズム概要**(詳細は `docs/testcase-generation-design.md` 3章)

- `knowledge/` 配下を feature → condition → expected の順にパスのソート順で走査する(実行環境・タイムスタンプに依存しない)。
- `id = "{feature_id}-{condition_id}-{連番3桁}"`(連番は `expected/` 配下のファイル名のソート順)。
- `title = "{condition.summary} (#{連番})"`
- `expected_result = "{expected.result} (condition: {condition.summary})"`
- `axis` は Feature の `axis` をそのまま継承する。
- `Condition` を持たない `Feature`、`expected/` が空の `Condition` からは `TestCase` は生成されない。
- 出力は `TestCase.id` の昇順にソートされ、同一入力に対して常にバイト単位で同一の出力になる(決定性、CIでの差分検証の前提)。

**使用例**

```console
$ markharness generate
generated 1 testcase(s) into generated/testcases.yaml
```

`generated/testcases.yaml`:
```yaml
- id: player-jump-jump-ground-001
  feature_id: player-jump
  condition_id: jump-ground
  axis:
    - gameplay
    - animation
  title: Jump from the ground and land (#1)
  expected_result: lands safely (condition: Jump from the ground and land)
```

`knowledge/` に何もない場合は `[]` (空のYAML配列)を出力する。

**ユースケース対応**: UC2「TestCaseを決定的生成する」(`docs/product-operation.md` 105行目)。CI上での差分検証(UC3)は本コマンドの出力と `git diff` を組み合わせて実現する想定(未実装、§2.2 参照)。

---

## 2. 未実装(今後実装予定)のコマンド

以下は `docs/product-operation.md` のユースケース図・ユースケース記述に基づく、今後実装予定のコマンドです。コマンド名・オプションは暫定案であり、実装時に変更され得ます。

| # | ユースケース | 想定コマンド(暫定) | アクター | 概要 |
|---|---|---|---|---|
| UC1b | `forked_from` を手動記述する | (専用コマンドなし。`feature.yaml` に `forked_from` フィールドを直接追記する運用を想定。将来的に `markharness knowledge fork <from-feature-id> <to-feature-id>` のような補助コマンドを検討) | Test Designer | 別Featureからの概念的派生を明示化する。Git履歴からは自動導出できないため必須の手動記述(§3.1, 153行目)。 |
| UC3 | 生成物をレビュー・マージする | `markharness verify`(暫定): `generate` を一時領域で再実行し、コミット済み `generated/testcases.yaml` との差分有無を報告する。CIでの利用を想定 | Reviewer / CI Bot | 再生成結果と現在のファイルの一致を検証し、差分があればレビューを要求する(§4.5)。 |
| UC4 | マイルストーンをタグ付けする | 専用コマンドなし(`git tag <milestone>` を直接使用) | Release Manager | リリースタイミングの意思決定そのものであり、人間の判断ポイント(図3)。 |
| UC5 | ChangeEventを自動計算する | `markharness changes compute <from-milestone> <to-milestone>`(暫定) | CI Bot | 2マイルストーン間でid解決経由のblob SHAを比較し `derived_from` を算出、`changes/<milestone>.yaml` に書き込む(本研究の核心的貢献、§3.2-3.4)。 |
| UC6 | バックフィルを非同期実行する | `markharness backfill run`(暫定) | Backfill Worker | 直近マイルストーンから優先的に過去の系譜を計算し、`git notes` に進捗を記録しながら `changes/*.yaml` を段階的に埋める(§4.1-4.2)。 |
| UC7 | idキャッシュを破棄・再構築する | `markharness cache rebuild` / 各コマンドの `--no-cache` オプション(暫定) | Test Designer / CI Bot | id解決キャッシュの不整合が疑われる場合に明示的に破棄・再構築するフェイルセーフ(199行目)。 |
| UC8 | 既存ツールからインポートする | `markharness import --from <testrail\|xray\|testlink> <file>`(暫定) | Data Migration Operator | 既存TMSのエクスポートファイルを `knowledge/` 構造に変換する(§4.5)。 |

これらは現時点で未着手であり、実装順序は別途チェックリスト(`/plan-checklist`)で管理する。

---

## 3. 動作確認・テスト

実装済みコマンドの単体テストは `cargo test` で実行できる(`src/init.rs` / `src/knowledge.rs` / `src/interactive.rs` / `src/generate.rs` の `#[cfg(test)] mod tests` を参照)。Pre-PR チェックリスト(`PROJECT.md`)に従い、コミット前に以下を実行すること:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo audit
```
