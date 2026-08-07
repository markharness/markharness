# Task: knowledge add コマンドに --dir オプションを追加する
Created: 2026-08-07

## 背景
`markharness init` は `-d, --dir` オプションで対象ディレクトリを指定できる(例: `markharness init --dir tmp/todo-sample`)。
`markharness knowledge add` には同様のオプションがなく、常にカレントディレクトリを対象にする。
`interactive::run_add` は既に `root: &Path` を引数に取る設計になっているため、CLI 層(`cli.rs`)に `--dir` を追加するだけで対応できる。

## Steps
- [x] Step 1 (Red): `cli.rs` に `parses_knowledge_add_dir_option` / `parses_knowledge_add_without_dir_option` のテストを書き、失敗させる
- [x] Step 2 (Green): `KnowledgeCommand::Add { dir: Option<PathBuf> }` を追加し、`run()` 内で `dir.unwrap_or(current_dir)` を使うよう実装してテストを通す
- [x] Step 3 (Refactor): `cargo clippy --all-targets -- -D warnings` と `cargo fmt` を実行し整形
- [x] Step 4: `cargo test` で全テスト通過を確認(28 passed)
- [x] Step 5: `docs/cli-manual.md` の `1.2 markharness knowledge add` セクションに `--dir` オプションの説明・使用例を追記
- [x] Step 6: `cargo audit` を実行し脆弱性がないことを確認(43 crate、指摘なし)

## Notes
- `interactive::run_add` は元々 `root: &Path` を受け取る設計だったため、変更は `cli.rs` の CLI 引数定義と `run()` のディスパッチのみで完結した。
- `init --dir` と同じ `-d, --dir <path>` の形にオプション名を揃え、一貫性を持たせた。
- `cargo run -- knowledge add --help` で `-d, --dir <DIR>` が表示されることを目視確認済み。

## Summary
`markharness knowledge add` に `-d, --dir <path>` オプションを追加し、`markharness init --dir` と同様にカレントディレクトリ以外(例: `tmp/todo-sample`)を対象に知識を記述できるようにした。TDD(Red-Green-Refactor)で実装し、`cargo test`(28件)・`cargo clippy`・`cargo audit` はすべて問題なし。`docs/cli-manual.md` にも使用例を追記済み。
