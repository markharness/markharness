# Task: executions/ 配下のファイル作成コマンド(`milestone init` / `execution record`)を実装する
Created: 2026-08-08

## 背景・仕様(grillingセッションでの確定事項)

- `executions/<tag>/milestone.yml` … `markharness milestone init <tag>` で作成
  - 前提: 対象の `git tag` が既に存在すること。存在しなければエラー終了し、「`git tag <tag>` を先に実行してください」という旨のメッセージを出す(exit code 2、`git tag` が無いのは使用方法エラー扱い)
  - `milestone.yml` の中身は `id: <tag名>` のみ(日付はgitから都度取得する既存 `backfill.rs`/`git::commit_date` の設計を変えない)
  - タグ名はそのまま使用し、`knowledge/` のidのような正規化・バリデーションはしない
  - 冪等: 既に `executions/<tag>/milestone.yml` が存在する場合は何もせず、「既に初期化済み」である旨のメッセージを出して正常終了(exit code 0)
- `executions/<milestone>/results.yml` … `markharness execution record <case_id> --milestone <name> --result <pass|fail|skip> --executor <name> [--note <text>]` で作成・追記
  - `--milestone` は必須。`executions/<name>/milestone.yml` が存在しなければエラー(exit code 2)
  - `case_id` は現在のHEAD時点の `generated/testcases/<case_id>.yml` に存在するものだけ受け付ける。存在しなければエラー(exit code 2)
  - `result` は `pass`/`fail`/`skip` の3値のみ(clapの値挙動で制約)
  - `--executor` は自由記述の1フィールドのみ(CI/人間を区別する専用フィールドは無し)
  - 1回の呼び出しにつき1エントリのみ追記。過去のエントリは保持(上書きしない、再実行履歴も残す)
  - 書き込みは `knowledge_apply.rs::write_one` と同じ「一時ファイル+リネーム」のアトミック書き込み方式
- 両コマンドとも既存コマンドと同じCLI規約に揃える: `-d, --dir <path>` / `--json` / 終了コード体系(0=成功, 1=検証/参照エラー, 2=使用方法エラー, 3=ファイルシステムエラー)
- 今回はコマンドの実装まで(結果の集計・レポート表示、`--from-report` 一括投入、過去マイルストーン時点でのcase_id検証は将来課題としてスコープ外)

## Steps

### Phase 0: 下準備

- [x] Step 1: `src/git.rs` に `tag_exists(root: &Path, tag: &str) -> io::Result<bool>` を追加する(TDD: タグが存在する場合/しない場合それぞれのテストを書いてから実装)

### Phase 1: `milestone init`

- [x] Step 2: `src/milestone.rs` を新規作成し、`MilestoneInitError`(`TagNotFound`, `Io`)と `MilestoneInitOutcome`(`Created`, `AlreadyInitialized`)を定義する
- [x] Step 3 (TDD): `milestone_init(root, tag)` が、対象タグが存在しない場合に `MilestoneInitError::TagNotFound` を返すことをテストしてから実装する
- [x] Step 4 (TDD): `milestone_init(root, tag)` が、タグが存在し `executions/<tag>/milestone.yml` が未作成の場合に `id: <tag>\n` を書き込み `MilestoneInitOutcome::Created` を返すことをテストしてから実装する
- [x] Step 5 (TDD): `milestone_init(root, tag)` が、`executions/<tag>/milestone.yml` が既に存在する場合、中身を変更せず `MilestoneInitOutcome::AlreadyInitialized` を返すことをテストしてから実装する(冪等性)
- [x] Step 6: `src/cli.rs` に `Command::Milestone(MilestoneCommand::Init { tag, dir, json })` を追加する(`-d/--dir`・`--json` は既存コマンドと同じ流儀)。パースのユニットテストを書く
- [x] Step 7: `run()` 内に `Milestone(MilestoneCommand::Init { .. })` のハンドラを実装する
  - `Created` → `initialized executions/<tag>/milestone.yml` のようなメッセージを標準出力へ(`--json` 時は `{"ok":true,"status":"created"}` 相当)
  - `AlreadyInitialized` → 「既に初期化済み」である旨のメッセージ(`--json` 時は `{"ok":true,"status":"already_initialized"}` 相当)、exit code 0
  - `TagNotFound` → stderrにエラーメッセージ(`git tag <tag>` を先に実行するよう促す文言)、exit code 2
- [x] Step 8 (TDD): CLI統合テストで、Created/AlreadyInitialized(`src/cli.rs`内、`run()`を直接呼ぶ形)とTagNotFound(`tests/milestone_cli.rs`、実バイナリをサブプロセスとして起動しexit codeを検証)の3パターンを検証する

### Phase 2: `execution record`

- [x] Step 9: `src/execution.rs` を新規作成し、`ExecutionResult`(`Pass`, `Fail`, `Skip` の enum)、`ExecutionEntry`(`case_id`, `result`, `executor`, `note: Option<String>`, `executed_at: String`)、`RecordError`(`MilestoneNotFound`, `CaseNotFound`, `Io`)を定義する
- [x] Step 10 (TDD): `record_execution(root, args)` が、指定した `--milestone` の `executions/<name>/milestone.yml` が存在しない場合に `RecordError::MilestoneNotFound` を返すことをテストしてから実装する
- [x] Step 11 (TDD): `record_execution(root, args)` が `case_id` の存在確認を行うことをテストしてから実装する。**設計変更**: `generated/testcases/` のファイル名は `condition.id` であり `case_id` と一致しないため([src/generate.rs:46-48](src/generate.rs#L46-L48))、ファイル名照合ではなく各YAMLの `case_id` フィールドを読んで照合する(`case_id_exists`)
- [x] Step 12 (TDD): `record_execution(root, args)` が、`executions/<milestone>/results.yml` が存在しない場合に新規作成し、1件のエントリ(`case_id`/`result`/`executor`/`note`/`executed_at`)を書き込むことをテストしてから実装する。`executed_at` はISO8601 UTC(日時クレートを追加せず `SystemTime` + Hinnantのcivil_from_daysアルゴリズムで自前実装)
- [x] Step 13 (TDD): `record_execution(root, args)` を既存の `results.yml` に対して呼んだ場合、既存エントリを保持したまま新しいエントリを末尾に追記することをテストで確認(Step 12の実装が「既存を読む→pushする→全体を書く」という形のため、追加実装なしでこの要件を満たした)
- [x] Step 14: 書き込みは `knowledge_apply.rs::write_one` と同じ「一時ファイル+リネーム」方式で実装済み(`results_path.with_extension("yml.tmp")` → `fs::rename`)
- [x] Step 15: `src/cli.rs` に `Command::Execution(ExecutionCommand::Record { case_id, milestone, result, executor, note, dir, json })` を追加する。`result` は `clap::ValueEnum`(`ResultArg`)で `pass`/`fail`/`skip` に制約する。パースのユニットテストを書く
- [x] Step 16: `run()` 内に `Execution(ExecutionCommand::Record { .. })` のハンドラを実装する
  - 成功 → `recorded <result> for <case_id> into executions/<milestone>/results.yml` のようなメッセージ(`--json` 時は `{"ok":true}` 相当)、exit code 0
  - `MilestoneNotFound`/`CaseNotFound` → stderrにエラーメッセージ、exit code 2
- [x] Step 17: CLI統合テストで、成功(`src/cli.rs`内、`run()`を直接呼ぶ形。新規作成/追記は`execution.rs`のユニットテストで既にカバー済み)とMilestoneNotFound/CaseNotFound(`tests/execution_cli.rs`、実バイナリのexit codeを検証)を確認

### Phase 3: 仕上げ

- [x] Step 18: `docs/cli-manual.md` に新セクション(1.13 `markharness milestone init`, 1.14 `markharness execution record`)を追記し、「1. 実装済みコマンド」に昇格させた
- [x] Step 19: `docs/product-operation.md` にUC4の実装補足として「5. 補足:UC4「実行結果の記録先」の実装」節を追記した(UC4本体の主フロー・人間の判断ポイントは変更なし)
- [x] Step 20: 品質ゲートを実行した: `cargo test`(188件全パス)/ `cargo clippy --all-targets -- -D warnings`(1件のcollapsible_if警告を修正して再実行しクリーン)/ `cargo fmt --check`(未整形箇所を`cargo fmt`で修正し再確認)/ `cargo audit`(脆弱性報告なし)

## Notes

- 設計の合意形成は `/mattpocock-skills:grilling` セッションで実施済み(Q1〜Q15)。本チェックリストはその出力を実装タスクに分解したもの。
- `git::tag_exists` は `git rev-parse --verify --quiet refs/tags/<tag>` 相当を想定(存在しない場合は非ゼロ終了するが、これはエラーではなく「存在しない」という結果として扱う点に注意 — 既存の `run_git` はステータス非ゼロを `io::Error` にしてしまうため、専用の実装が必要かもしれない)。
- スコープ外(将来課題): `--from-report` による一括記録、過去マイルストーン時点の `generated/testcases/` に対する `case_id` 検証、`results.yml` の集計・レポート表示コマンド。

## Summary

`markharness milestone init <tag>` と `markharness execution record <case_id> --milestone <name> --result <pass|fail|skip> --executor <name>` をTDD(Red→Green)で実装し、`docs/cli-manual.md`/`docs/product-operation.md` を更新、品質ゲート(test/clippy/fmt/audit)を全てクリアした。実装中に、`generated/testcases/` のファイル名が `case_id` ではなく `condition.id` であるという既存設計上の事実が判明し、`case_id` 検証をファイル名照合からYAML内容照合に変更した(Step 11)。
