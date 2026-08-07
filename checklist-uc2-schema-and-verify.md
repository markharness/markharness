# Task: knowledge/ スキーマを tmp/todo-simple/ 例に刷新し、generate を再実装、verify(UC3差分検証)を新規実装する
Created: 2026-08-08

## 背景・合意事項(grilling セッションのサマリー)

- `markharness generate`(UC2)は実装済みだが、`.gitignore` 対象の `tmp/todo-simple/` に置かれた「正解例」とスキーマ・出力形式が一致していない。
- `tmp/todo-simple/` を正としてスキーマ・生成物形式を刷新する。
- CI差分検証(UC3、`markharness verify`)は未実装のため新規実装する。
- スコープ外(別タスク): `knowledge/requirement.yml` 対応、`generated/traceability-index.json` 生成、参照整合性検証(`behavior.feature` とディレクトリ階層の一致チェックなど)。

### 確定仕様

- 拡張子: `.yaml` → `.yml` に統一。
- `kind` フィールドは廃止(ファイル種別はファイル名で判別)。
- 階層: `feature.yml`(id, label, axis, 他任意) → `behavior.yml`(必須中間層。id, feature, axis, description) → `condition.yml`(id, behavior, description) → `expected/*.yml`(id, condition, description、複数可)。
- 集約モデル: **1 condition = 1 testcase**。`expected/` 配下の全ファイルを 1 つの testcase の `expected` 配列に集約する(現行の「1 expected ファイル = 1 testcase」から変更)。
- `case_id: tc-{condition.id}-001`(seq は将来拡張用、今回は常に `001`)。
- 出力ファイル: `generated/testcases/{condition.id}-001.yml`(ファイル名に `tc-` は付けない)。
- 生成物の内容: `generated_from: {feature, behavior, condition, expected_results: [...]}`、`title` = condition.description、`steps` = `[behavior.description]`、`expected` = 各 expected ファイルの description をファイル名ソート順に列挙。
- 生成ファイル先頭に固定コメント行 `# 生成物(コミット対象)。CIが knowledge/ から再生成し、内容が一致することを検証する(第3.2節(A)、第4.5節)。` を出力する。
- `markharness verify`: `generated/testcases/` を一時領域で再生成し、コミット済みファイルと比較。差分あり(追加/削除/変更)は変更ファイル一覧を表示して exit code 1、一致なら exit code 0。
- `markharness knowledge add` も新スキーマ(behavior プロンプト追加)に対応させる。

参照元: `tmp/todo-simple/`(schema/*.json, knowledge/**, generated/testcases/*.yml)

## Steps

- [x] Step 1: `src/knowledge.rs` のスキーマを刷新する(Red-Green-Refactor)。`Feature`(id, label, axis, requirement/forked_from/description は任意)・新設 `Behavior`(id, feature, axis, description)・`Condition`(id, behavior, description)・`ExpectedResult`(id, condition, description, note/added_in_milestone は任意)の構造体・parse/serialize 関数を `.yml` 形式・`kind` なしで書き直す。既存テストを新スキーマに合わせて更新する。
- [x] Step 2: `src/interactive.rs`(`knowledge add`)に behavior 入力ステップを追加する(feature → behavior → condition → expected の順)。既存の id 重複除去・ローマ字化・候補一覧ロジックを behavior 層にも適用し、`.yml` 拡張子・新スキーマで書き出す。既存テストを更新する。
- [x] Step 3: `src/generate.rs` の走査アルゴリズムを feature → behavior(必須) → condition → expected(全件集約) に書き直す。`TestCase` 構造体を `case_id` / `generated_from{feature, behavior, condition, expected_results}` / `title` / `steps` / `expected` に変更する。
- [x] Step 4: `generate.rs` の出力を serde ベースの YAML シリアライズ(`serde_yaml_ng`)に切り替え、固定コメント行を先頭に付与する。1 testcase = 1 ファイルとして返す関数(またはファイル名とYAML文字列のペアを返す関数)に変更する。
- [x] Step 5: `src/cli.rs` の `Generate` コマンドを、`generated/testcases/` ディレクトリをクリアしてから複数ファイルを書き出す実装に更新する。
- [x] Step 6: `src/verify.rs` を新規作成し、一時ディレクトリで再生成した結果と `generated/testcases/` を比較して差分(追加/削除/変更ファイル名)を返すロジックを実装する(TDD)。
- [x] Step 7: `src/cli.rs` に `Verify` サブコマンドを追加し、差分ありなら一覧表示して exit code 1、一致なら exit code 0 で終了するよう配線する。
- [x] Step 8: `docs/cli-manual.md` の `generate` セクションと `PROJECT.md` のディレクトリ構成を新形式(`.yml`、`generated/testcases/`)に合わせて更新し、`verify` コマンドのセクションを追加する。
- [x] Step 9: Pre-PR チェックリストを実行する: `cargo test` / `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check` / `cargo audit`。

## Notes

- Condition の id 重複除去は「文字列正規化」ではなく、そもそも `case_id` 生成時に `feature_id` を連結しない(`condition.id` のみを使う)方式にした。`todo-simple` 例の `condition.id` が命名規約により既にフルパス相当(例: `todo-add-task-empty-input`)を含んでいるため、この方式で重複が起きない。
- `knowledge add` の Condition id 重複除去は、階層が1段増えたため比較対象を `feature_id` から `behavior_id` に変更した(直近の親と比較する方が自然なため)。
- `Feature.label` は新スキーマで必須(JSON Schema上 required)なため、ASCII直接入力時は id と同じ文字列を label として保存するようにした(旧実装は `label: None` としていた)。
- `requirement.yml` / `generated/traceability-index.json` / 参照整合性検証(`behavior.feature` 等とディレクトリ階層の一致確認)は合意通りスコープ外とし、`docs/cli-manual.md` の「未実装」表に追記するに留めた。
- `verify` は「一時ディレクトリに再生成して比較」ではなく、生成結果をメモリ上の文字列のまま `generated/testcases/` の既存ファイルと比較する実装にした(純粋関数の組み合わせで十分再現でき、実ファイルI/Oを増やす必要がなかったため)。
- 生成ファイル先頭の固定コメント行は完了後にユーザーから不要と判断され削除した(`generate.rs` の `serialize_testcase` から除去、`docs/cli-manual.md` の該当記述も削除)。

## Summary

`knowledge/` スキーマを `tmp/todo-simple/` の正解例に合わせて刷新し(`.yml`・`kind`廃止・`behavior.yml`必須化)、`markharness generate` を「1 Condition = 1 TestCase」集約モデルの複数ファイル出力に書き直し、新規に `markharness verify`(UC3 CI差分検証、差分ありでexit code 1)を実装した。`knowledge add` も新階層に対応させ、関連ドキュメント(`docs/cli-manual.md` / `PROJECT.md`)を更新。`cargo test`(63件)・`clippy`・`fmt`・`audit` すべて通過。
