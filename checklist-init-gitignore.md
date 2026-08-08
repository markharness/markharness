# Task: `markharness init` で `.gitignore` を自動生成する
Created: 2026-08-09

## Steps
- [x] Step 1: `src/init.rs` に `.gitignore` 追記/新規作成の失敗するテストを追加(Red)
- [x] Step 2: `run_init` に `.gitignore` 生成ロジックを実装してテストを通す(Green)
- [x] Step 3: 必要ならリファクタリング(Refactor) — clippy 指摘なし、rustfmt 適用のみ
- [x] Step 4: `PROJECT.md` の `markharness init` の説明を更新
- [x] Step 5: `cargo test` / `cargo clippy` / `cargo fmt` を実行して確認

## Notes
- 対象: Rust CLI サブコマンド `markharness init`(src/init.rs)。テンプレート側の `/setup` やこのリポジトリ自身の `.gitignore` とは無関係。
- 作成場所: 対象プロジェクトのルート直下。
- 既存 `.gitignore` がある場合: 不足分のみ追記(マージ)、既存内容は保持。
- 追記行にはコメントヘッダを付ける(例: `# markharness init` の後に `.markharness-cache/`)。
- 内容は `.markharness-cache/` のみ(7つの管理対象ディレクトリは generated/ を含めすべてコミット対象と確認済み)。

## Summary
`markharness init` に `.gitignore` の自動生成/マージ機能(`.markharness-cache/` を非破壊的に追記)を実装し、テスト・clippy・fmt を確認、PROJECT.md も更新済み。
