# Task: ChangeEvent連動・実行状態追跡（verified_feature_blobs / verify trace / verify pending）
Created: 2026-08-09

仕様書: docs/ChangeEvent連動_実行状態追跡仕様.md

## Steps
- [x] Step 1: `ExecutionEntry` に `verified_feature_blobs` フィールドを追加し、`execution record` 実行時に対象milestoneのFeature blob SHAを自動付与する（シーム1）
- [x] Step 2: `changes::ChangeEvent` に `Deserialize` を実装し、`changes/<milestone>.yaml` を読み込む `read_changes(root, milestone)` を追加する（シーム2）
- [x] Step 3: `verify::trace(root, case_id, milestone)` でQ1判定（reflects_change）を実装する（シーム3）
- [x] Step 4: `verify::pending(root, from, to)` でQ2判定（pending/stale区分）を実装する。stale判定の「現在」は直近マイルストーン（`order_by_recency`）（シーム4）
- [x] Step 5: CLIに `verify trace <case_id> --milestone <m>` / `verify pending [--from --to] [--fail-on-pending]` を追加し、既存の `markharness verify`（サブコマンド無し）の動作は維持する
- [x] Step 6: CLI統合テスト（`tests/verify_cli.rs`）を追加し、出力フォーマット・exit codeを検証する（シーム5）
- [x] Step 7: `cargo clippy --all-targets -- -D warnings` / `cargo fmt` / `cargo audit` を通す

## Notes
- 合意事項（ユーザーとの/grillingセッションより）:
  - スコープはフル実装（a〜d）。ただし(d) `--fail-on-pending` はCLIオプションとしての実装のみで、既存CI設定への組み込みまでは行わない。
  - stale判定の「現在」は直近にinitされたマイルストーン（`src/backfill.rs::order_by_recency`で導出）
  - `verify trace`/`verify pending`は`Command::Verify`を`VerifyArgs { command: Option<VerifySubcommand> }`に拡張する形で実装し、サブコマンド省略時は既存のdiffチェック動作(UC3)を維持する
  - `verify pending --from X --to Y`が隣接しないマイルストーンをまたぐ場合、`order_by_recency`と同じ順序でX(除く)〜Y(含む)間の各マイルストーンの`changes/<m>.yaml`を集約する
  - 遡及適用はしない（既存`results.yml`は対象外、"不明"扱い）
  - `change_type`は未実装のため`verify trace`出力では`(未記録)`固定文字列

## Summary

`verified_feature_blobs`の自動記録、`verify trace`（Q1）、`verify pending`（Q2、pending/stale区分、`--fail-on-pending`）をTDDで実装し、既存の`markharness verify`（UC3チェック）はサブコマンド省略時の動作として維持した。テストは188件のユニットテスト＋19件のCLI統合テストが全パス、clippy/fmt/audit済み。実装中に発見した既存の隣接マイルストーン推定ロジックの潜在バグ（同時刻タグでのスライス範囲外パニック）も`PendingError::InvalidRange`として修正した。

**既知の簡略化**：Q1の「§3.1 step 4: 一致するChangeEventが無い場合、derived_from鎖を遡って直近の変更マイルストーンを返す」は、`changes/`配下の全ファイルを横断検索する形で近似実装した（`changes compute`/`backfill`が該当区間で一度も実行されていない場合は「不明」として`reflects_change: None`を返す）。Featureのgit commit履歴を直接辿るフルの`derived_from`鎖トラバーサルは未実装（Future Work）。
