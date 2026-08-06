# Task: init コマンドにディレクトリ指定オプションを追加
Created: 2026-08-07

## Steps
- [x] Step 1 (Red): `markharness init --dir <path>` の CLI パースをテストする失敗テストを cli.rs に書く(`Command::Init { dir: Some(PathBuf) }` になることを検証)
- [x] Step 2 (Green): `clap` の `Init` variant に `dir: Option<PathBuf>` フィールドを追加し、テストを通す
- [x] Step 3 (Red): `run()` が `--dir` 指定時、存在しないディレクトリでも作成し、その配下に UC1-UC8 サブディレクトリを作ることを検証するテストを書く(既存の `init::run_init` の `create_dir_all` 実装により追加実装なしで green だった)
- [x] Step 4 (Green): `run()` 内でルートを決定し `init::run_init` を呼ぶよう実装(Step 2 で実装済み、テストは green)
- [x] Step 5 (Refactor): `cargo clippy --all-targets -- -D warnings` と `cargo fmt` を実行し、警告を解消する(テストモジュールを `run()` の後ろに移動して clippy の `items_after_test_module` を解消)
- [x] Step 6: `cargo test` で全体のテストが通ることを確認する(26 passed)
- [x] Step 7: `cargo audit` を実行し脆弱性がないことを確認する(Vulnerability Check)(0 vulnerabilities)

## Notes
- `init::run_init` は既に `fs::create_dir_all` でルート自体も作成される(サブディレクトリ作成時に親ディレクトリも作られるため)。
- オプション名は `--dir`(短縮 `-d`)とする。

## Summary
`markharness init --dir <path>` で任意のディレクトリを指定でき、未存在時は自動作成されるようになった(`src/cli.rs` の `Command::Init` に `dir: Option<PathBuf>` を追加)。
