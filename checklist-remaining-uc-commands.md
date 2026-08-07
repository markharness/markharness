# Task: UC8以外の未実装コマンドを実装する
Created: 2026-08-08

## Steps

### Phase 1: 小粒・低リスク
- [x] Step 1: Feature に `forked_from: Option<String>` フィールドを追加(parse/serialize/テスト)
- [x] Step 2: `knowledge validate`/`apply` に `forked_from` の参照整合性検証(`unknown_forked_from`)を追加
- [x] Step 3: `markharness axes list [--json]` コマンドを実装

### Phase 2: 知識まわりの小機能
- [x] Step 4: `generate` が `TestCase.axis` に Requirement/Feature/Behavior の axis を union して継承する
- [x] Step 5: `serde_json` を追加し、`generate` が `generated/traceability-index.json` を自動生成する
- [x] Step 6: `verify` が `traceability-index.json` も差分検証対象に含める

### Phase 3: knowledge add --edit
- [x] Step 7: `markharness knowledge add --edit` ($EDITOR起動ラッパー)を実装(空ドラフトテンプレート→apply、エラー時再編集)

### Phase 4: Git連携基盤 + UC7
- [x] Step 8: `git` CLIシェルアウトのヘルパーモジュールを実装(ls-tree, log, notes)
- [x] Step 9: `.markharness-cache/` を使った簡易id解決(ls-tree走査結果のキャッシュ)を実装
- [x] Step 10: `markharness cache rebuild` コマンドを実装(`.markharness-cache/` 全削除)

### Phase 5: UC5
- [x] Step 11: `markharness changes compute <from-tag> <to-tag>` を実装し `changes/<to-tag>.yaml` を生成

### Phase 6: UC6
- [x] Step 12: `markharness backfill run` を実装(executions/*/milestone.yml 由来のマイルストーンを直近優先で順次処理、git notesで進捗管理)

## Notes
- 設計方針は /mattpocock-skills:grilling セッションで確定済み(ユーザー合意済み)。詳細は会話ログ参照。
- UC8(既存ツールインポート)はスコープ外。
- Git操作は `std::process::Command` で `git` バイナリをシェルアウト(git2/gix は導入しない)。
- id解決キャッシュは簡易版(tree_sha非コミットキャッシュのフル実装は見送り)。

## Summary
UC8(既存ツールインポート)を除く全ユースケース(UC1b/UC2〜UC7)のコマンドを6フェーズで実装し、`docs/cli-manual.md` を更新。`cargo test`(158件)/`cargo clippy -D warnings`/`cargo fmt`/`cargo audit` すべて通過。
