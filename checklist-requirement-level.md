# Task: knowledge/ に Requirement 階層を追加する (Requirement → Feature → Behavior → Condition → ExpectedResult)
Created: 2026-08-08

## 背景・設計合意事項(/grilling セッションより)

- コマンド配置: 既存の `markharness knowledge add` を拡張し、Feature の前に Requirement 選択/新規作成ステップを追加する(新規コマンドは作らない)。
- スコープ: 今回は Requirement の記録(CRUD)のみ。`generated/traceability-index.json` のようなトレーサビリティ索引生成は別タスクとして分離する。
- Requirement のフィールド: 既存の `Feature`/`Behavior`/`Condition` と揃え、`id` / `label` / `axis` / `description`(description は任意)とする。
- ディレクトリ構造: `knowledge/<requirement>/<feature>/feature.yml` のように Requirement をディレクトリ階層の新しいルートにする(破壊的変更。既存の `knowledge/<feature>/...` から1段深くなる)。
- 参照は必須: `Feature` は `requirement: <id>` フィールドを必ず持つ(`Behavior.feature` / `Condition.behavior` と同じパターン)。
- 既存プロジェクトの移行: 自動移行コマンドは作らない。todo-test / game-test など既存の `knowledge/` ツリーは手動で作り直す。ドキュメントに注意書きのみ残す。
- 生成物への反映: `generated/testcases/*.yml` の `generated_from` に `requirement: <id>` を追加する。

## Steps

### スキーマ(src/knowledge.rs)
- [x] Step 1 (Red): `Requirement` 構造体(`id`, `label`, `axis: Vec<String>`, `description: Option<String>`)の `parse_requirement` / `serialize_requirement` に対する失敗するテストを追加する(`Feature` のテストと対称的な内容)。
- [x] Step 2 (Green): `Requirement` 構造体・`parse_requirement`・`serialize_requirement` を実装し、Step 1 のテストを通す。
- [x] Step 3 (Red): `Feature` に `requirement: String` フィールドを追加した場合の `parse_feature` / `serialize_feature` の失敗するテストを追加する(既存テストの期待値更新を含む)。
- [x] Step 4 (Green): `Feature` 構造体に `requirement: String` を追加し、`parse_feature` / `serialize_feature` を更新して全テストを通す。

### 生成ロジック(src/generate.rs)
- [x] Step 5 (Red): `generate_testcases` が `knowledge/<requirement>/<feature>/...` の2段階ディレクトリを辿れることを検証する失敗するテスト(`write_requirement` フィクスチャ関数を追加し、既存の `write_feature` 呼び出しを requirement 配下に変更)。
- [x] Step 6 (Green): `generate_testcases` に Requirement ディレクトリを走査する外側ループを追加し、`GeneratedFrom` に `requirement: String` を追加。既存の全テストのフィクスチャ・アサーションを新ディレクトリ構造に更新して通す。

### 対話フロー(src/interactive.rs)
- [x] Step 7 (Red): `run_add` が Feature の前に Requirement の選択/新規作成ステップ(候補一覧表示・番号選択・新規id採番・再利用時のプロンプトスキップ)を行うことを検証する失敗するテスト(既存の `FULL_INPUT` などの入力フィクスチャを Requirement 入力込みに更新)。
- [x] Step 8 (Green): `run_add` に Requirement ステップを実装(`list_candidate_ids(knowledge_root, "requirement.yml")` → `prompt_id_or_label` → 新規なら axis 入力・`requirement.yml` 書き込み、既存なら再利用メッセージ)。Feature 以降の処理をすべて `knowledge/<requirement_id>/` 配下で行うようパスを更新。既存テストを含め全て通す。
- [x] Step 9 (Red→Green): Requirement 候補一覧の番号選択・既存 Requirement 再利用時の axis プロンプトスキップ・Japanese ラベルのローマ字id提案、それぞれについて個別のテストケースを追加し(既存の Feature 向けテストと対称的な内容)、通るまで実装を調整する。

### init / 周辺確認
- [x] Step 10: `src/init.rs` を確認。`SUBDIRS` はトップレベルの `knowledge/` のみを作成し、`<requirement>/<feature>/...` の下位構造は `knowledge add` 実行時に都度作成される設計のため、変更不要と確認した。

### ドキュメント
- [x] Step 11: `PROJECT.md` のディレクトリ構成説明を `knowledge/<requirement>/<feature>/<behavior>/<condition>/expected/*.yml` に更新した。
- [x] Step 12: `docs/cli-manual.md` の `knowledge add` の仕様(プロンプト順序、入出力例)・`generate` の説明・「未実装コマンド」表の Requirement 行を更新し、実装済みの範囲(`requirement.yml` の記録・`generated_from.requirement`)とトレーサビリティ索引生成が引き続き未実装であることを明記した。
- [x] Step 13: `PROJECT.md` のディレクトリ構成説明直下に、既存プロジェクト(todo-test, game-test)向けの手動移行の注意書きを追加した。

### 仕上げ
- [x] Step 14: `cargo test` を実行し全69テストが通ることを確認した。
- [x] Step 15: `cargo clippy --all-targets -- -D warnings` と `cargo fmt --check` を実行し、いずれも警告・差分なしを確認した(`write_expected` テストヘルパーの引数8個について `#[allow(clippy::too_many_arguments)]` を付与済み)。

### 追加対応: Behavior/Condition の label 復活(ユーザー指摘によるフォローアップ)

- [x] Step 16 (Red): 過去の3階層モデル(コミット `c8f9051`)では `Condition`/`ExpectedResult` に `label: Option<String>` があり、日本語名入力時の元テキストを保持していたが、`e2b3b4c` の4階層スキーマ刷新で `Behavior`/`Condition` から `label` が失われ、`interactive.rs` は `_behavior_label` / `_condition_label` として値を捨てていた。`Behavior`/`Condition` の `parse_*`/`serialize_*` に `label` を要求する失敗するテストを追加。
- [x] Step 17 (Green): `Behavior`/`Condition` 構造体に `label: String`(`Feature`/`Requirement` と同じ非Option方式で統一)を追加し、`interactive.rs` で握りつぶしていた `behavior_label`/`condition_label` を実際に書き込むよう修正。`generate.rs`/`verify.rs` のテストフィクスチャ、既存アサーションを更新。
- [x] Step 18: 日本語ラベル(例:「プレイヤーがジャンプする」)入力時に Behavior/Condition の `label` フィールドへ元の日本語文字列がそのまま保存されることを検証するテストを新規追加(`creates_new_behavior_with_japanese_label_and_saves_it_to_yaml` / `creates_new_condition_with_japanese_label_and_saves_it_to_yaml`)。
- [x] Step 19: `docs/cli-manual.md` の日本語ラベル入力の説明・`behavior.yml`/`condition.yml` のサンプルを `label` 追加後の内容に更新。
- [x] Step 20: `cargo test`(全71テスト)/ `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check` が通ることを確認。

## Notes

- Requirement 導入は破壊的変更(既存の `knowledge/<feature>/...` ツリーはそのままでは動かなくなる)。デモ版であることを理由にユーザーが明示的に許容している。
- `verify` (`src/verify.rs`) は `generate_testcases` の出力を比較するだけの実装なので、Requirement 対応は Step 6 の `generate.rs` 変更のみで自動的に波及する。専用の改修は不要。
- 既存の `Feature`/`Behavior`/`Condition` も「ディレクトリ位置」と「親参照フィールド」の間に明示的な整合性チェックは現状存在しない(`generate_testcases` はフィールド値をそのまま信頼する)。Requirement もこの既存方針に倣い、今回は厳密な不一致検出ロジックは追加しない。

## Summary

`knowledge/` に Requirement 階層を追加し、`Requirement → Feature → Behavior → Condition → ExpectedResult` の順で `markharness knowledge add` から記録できるようにした。ディレクトリ構造を `knowledge/<requirement>/<feature>/...` に変更(破壊的変更、自動移行なし)、`generate`/`verify` は Requirement を含めて `generated_from.requirement` に反映する。さらにフォローアップとして、4階層スキーマ刷新時に失われていた `Behavior`/`Condition` の `label` フィールド(日本語名入力時の元テキスト保持)を復活させた。全71テストが通過し、`cargo clippy` / `cargo fmt --check` もクリーン。
