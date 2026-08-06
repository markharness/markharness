# Task: UC1(知識を記述する)関連コマンドの実装 — init / knowledge add(対話) / generate
Created: 2026-08-07

## 決定事項(ユーザー確認済み)

- 対話入力: 標準入力での逐次プロンプト方式(`markharness knowledge add` の1コマンドで feature→condition→expected を順に聞く)
- id命名規則: `testcase-generation-design.md` 9章の(b)案を採用。`condition_id` は feature 名を含まない短い slug とし、生成id/expected-resultのidは `{feature_id}-{condition_id}-{連番3桁}` をそのまま採用する(現行サンプルとの一致は求めない、本リポジトリに `samples/` は存在しないためゼロから定義)。
- 技術選定: `clap`(derive) でCLIパーサー、`serde` + `serde_yaml_ng` でYAML読み込み、書き込みは決定的な出力順を厳密に制御するため手書きフォーマッタを使う。テスト用に `tempfile` を dev-dependency に追加。
- title/expected_result のテンプレート文言はプロダクト側の裁量(design doc 3.3節)。以下を採用:
  - `title = "{condition.summary} (#{seq})"`
  - `expected_result = "{expected.result} (condition: {condition.summary})"`
- テスト容易性のため `src/lib.rs` を新設し、`main.rs` は薄いエントリポイントにする。

## Steps

### Phase 0: セットアップ
- [x] Step 0-1: `cargo add clap --features derive`, `cargo add serde --features derive`, `cargo add serde_yaml_ng`, `cargo add tempfile --dev` を実行し `cargo build` が通ることを確認
- [x] Step 0-2: `src/lib.rs` を新設し `src/main.rs` から呼び出す最小の骨組みを作る(既存の "Hello, world! M" 出力はCLI未実装時のフォールバックとして一旦削除)

### Phase 1: `init` コマンド(ディレクトリ構成の初期設定)
- [x] Step 1-1 (Red): 空の一時ディレクトリで `init` 相当の関数を呼ぶと `knowledge/`, `generated/`, `changes/` が作成されるテストを書き、失敗させる
- [x] Step 1-2 (Green): `src/init.rs` に最小実装を書きテストを通す
- [x] Step 1-3 (Red): 既に初期化済みのディレクトリに対し `--force` なしで再実行するとエラーになるテストを追加
- [x] Step 1-4 (Green): 実装を追加してテストを通す(初期実装が既にforce分岐を含んでいたため追加実装は不要、テストのみでGreen確認)
- [x] Step 1-5 (Red): `--force` 付きなら既存の `knowledge/` 配下のファイルを消さずに再実行できるテストを追加
- [x] Step 1-6 (Green): 実装・テストを通す(同上、既存実装で合格)
- [x] Step 1-7 (Refactor): `cargo clippy --all-targets -- -D warnings` / `cargo fmt` を実行し警告ゼロを確認
- [x] Step 1-8: `main.rs` に `init` サブコマンドを clap で配線し、手動で `cargo run -- init` を実行して動作確認(一時ディレクトリで検証、knowledge/generated/changesが作成されることを確認)

### Phase 2: knowledge データモデルとYAML入出力
- [x] Step 2-1 (Red): `Feature`/`Condition`/`ExpectedResult` 構造体を `serde_yaml_ng` でデシリアライズするテスト(feature.yaml/condition.yaml/expected/NNN.yaml のサンプル文字列から)を書き失敗させる
- [x] Step 2-2 (Green): `src/knowledge.rs` に構造体とパース関数を実装
- [x] Step 2-3 (Red): 手書きYAMLシリアライザ(キー順固定)で `Feature`/`Condition`/`ExpectedResult` を書き出すテスト(期待するバイト列と完全一致)を書き失敗させる
- [x] Step 2-4 (Green): シリアライズ関数を実装
- [x] Step 2-5 (Red): slugバリデーション(空文字・不正文字を拒否)の単体テストを書き失敗させる
- [x] Step 2-6 (Green): バリデーション実装(小文字英数字とハイフンのみ許可)
- [x] Step 2-7 (Refactor): lint/format 警告ゼロ確認

### Phase 3: `knowledge add`(対話形式で知識を記述するコマンド)
- [x] Step 3-1 (Red): `BufRead`/`Write` を注入できる対話ロジック関数に対し、新規Feature+新規Condition+新規Expectedを一気に作るシナリオ(スクリプト化した入力)で3ファイルが正しい内容で作成されるテストを書き失敗させる
- [x] Step 3-2 (Green): `src/interactive.rs` に最小実装(feature/condition/expectedの一連のプロンプトを一括実装)
- [x] Step 3-3 (Red): 既存Featureのidを入力した場合、axisの質問をスキップして再利用するテストを追加
- [x] Step 3-4 (Green): 実装(既存実装で合格、追加変更なし)
- [x] Step 3-5 (Red): 既存Conditionのidを入力した場合、summaryの質問をスキップして再利用するテストを追加
- [x] Step 3-6 (Green): 実装(既存実装で合格、追加変更なし)
- [x] Step 3-7 (Red): 同一Condition配下に2件目のexpectedを追加すると連番が002になるテストを追加
- [x] Step 3-8 (Green): 実装(既存実装で合格、追加変更なし)
- [x] Step 3-9 (Red): 空入力(trim後空文字)を入力した場合に再プロンプトするテストを追加
- [x] Step 3-10 (Green): 実装(既存実装で合格、追加変更なし)
- [x] Step 3-11 (Refactor): lint/format 警告ゼロ確認
- [x] Step 3-12: `main.rs`/`cli.rs` に `knowledge add` サブコマンドを配線し、`markharness init` → `markharness knowledge add` を実際に対話実行して3ファイルが生成されることを確認

### Phase 4: `generate`(テストケース作成コマンド、UC2アルゴリズムの実装)
- [x] Step 4-1 (Red): 空の `knowledge/` に対し `generate` すると空のtestcaseリストが出力されるテストを書き失敗させる
- [x] Step 4-2 (Green): `src/generate.rs` に最小実装(ディレクトリ階層の決定的走査、`sorted(glob(...))`相当をsortedなVec<PathBuf>で実現)
- [x] Step 4-3 (Red): 単一 feature/condition/expected から正しいフィールド(id/feature_id/condition_id/axis/title/expected_result)を持つ1件のtestcaseが生成されるテストを追加
- [x] Step 4-4 (Green): 実装(既存実装で合格)
- [x] Step 4-5 (Red): 同一condition配下に複数expectedがある場合、連番と昇順ソートで複数testcaseが生成されるテストを追加
- [x] Step 4-6 (Green): 実装(既存実装で合格)
- [x] Step 4-7 (Red): 複数feature/conditionが存在する場合、id昇順でソートされて出力されるテストを追加
- [x] Step 4-8 (Green): 実装(既存実装で合格)
- [x] Step 4-9 (Red): expectedを持たないconditionからはtestcaseが生成されない(エッジケース表どおり)テストを追加
- [x] Step 4-10 (Green): 実装(既存実装で合格)
- [x] Step 4-11 (Red): 同じ入力に対して2回 `generate` を実行すると出力がバイト単位で完全一致する(決定性)テストを追加
- [x] Step 4-12 (Green): 実装(既存実装で合格)
- [x] Step 4-13 (Refactor): lint/format 警告ゼロ確認
- [x] Step 4-14: `main.rs`/`cli.rs` に `generate` サブコマンドを配線し、`markharness generate` を実際に実行して `generated/testcases.yaml` の中身を目視確認

### Phase 5: 仕上げ
- [x] Step 5-1: `cargo test` 全件パス確認(23件 pass)
- [x] Step 5-2: `cargo clippy --all-targets -- -D warnings` 警告ゼロ確認
- [x] Step 5-3: `cargo fmt --check` 確認
- [x] Step 5-4: `cargo audit` で脆弱性なし確認(43クレート中0件)
- [x] Step 5-5 (Skipped): README.md はテンプレート自体の使い方を説明する汎用ドキュメントであり、markharness CLI(プロダクト固有機能)の使い方を書く場所ではないため追記を見送り。プロダクト固有の使い方ドキュメントが必要になった場合は `docs/` 配下に追加するのが適切
- [x] Step 5-6: Summary セクションを本ファイルに追記

## Summary

`markharness init` / `markharness knowledge add` / `markharness generate` の3コマンドをTDDで実装した。`init`はknowledge/generated/changesディレクトリを作成し(`--force`で既存knowledgeを保持したまま再実行可)、`knowledge add`は標準入力での逐次プロンプトでFeature/Condition/ExpectedResultをYAMLとして記述し(既存id入力時は該当プロンプトをスキップして再利用)、`generate`はknowledge/を決定的に走査してgenerated/testcases.yamlをid昇順で再生成する。テスト23件・clippy警告ゼロ・fmt済み・audit脆弱性なしを確認済み。

## Notes
- `docs/testcase-generation-design.md` 9章で指摘されていた id 命名の未解決差異は (b) 案採用で解消。旧サンプル(`samples/repo/**`)は本リポジトリに存在しないため、互換性は考慮不要。
- title/expected_result のテンプレート文言はプロダクト独自定義(design doc は最小限のテンプレート規則の存在のみを要求し、具体文言はプロダクト裁量としている)。
