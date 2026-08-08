# Task: knowledge add --edit で未登録axisを自動作成する
Created: 2026-08-08

## Steps
- [x] Step 1: `knowledge_draft.rs` の `nearest_axis_suggestion` を crate内から再利用できるよう可視性を調整する
- [x] Step 2: 新規axis候補を判定する純粋関数を追加する(登録済み・近似候補あり・不正slug形式を除外)。TDDでテストを先に書く
- [x] Step 3: `axes.rs` に `axes/<id>.yml` を新規作成するヘルパー(`id`+`label=id`)を追加する。TDDでテストを先に書く
- [x] Step 4: `knowledge_edit::run_edit_loop` にStep2/3を組み込み、パース成功後・apply_draft呼び出し前に未登録axisを自動作成しメッセージ表示する。TDDでテストを先に書く(新規axisのみ作成/近似候補は作成しない/不正slugは作成しない/混在時の部分作成)
- [x] Step 5: `knowledge_edit.rs` のエラー表示に `suggestion`(近似候補)を追加する(`cli.rs` の `validate`/`apply` と同形式)。TDDでテストを先に書く
- [x] Step 6: `docs/cli-manual.md` §1.9 にaxis自動作成の挙動を追記する
- [x] Step 7: `cargo test` / `cargo clippy --all-targets -- -D warnings` / `cargo fmt` / `cargo audit` を実行し全て通過させる

## Notes
- 設計方針は /mattpocock-skills:grilling セッションで確定済み・ユーザー合意済み(前回チャットログ参照)。
- 対象は `knowledge add --edit` のみ。対話式 `knowledge add`・`knowledge validate`/`apply` は対象外。
- 自動作成の判定: 登録済みでない かつ 近似候補(levenshtein距離<=2)なし かつ `is_valid_slug` を満たす、の3条件をすべて満たすaxis値のみ自動作成する。
- テスト中、ビルドをブロックしていた残存 `markharness.exe` プロセス(過去の動作確認セッションの残骸)をユーザー許可のうえ終了した。
- Windowsのファイルシステムは大文字小文字を区別しないため、`UI.yml`/`ui.yml` の存在チェックによるテストが誤検知した。ファイル内容ベースのアサーションに修正した。

## Summary
`knowledge add --edit` に、未登録axisの安全な自動登録(タイポの可能性がある近似候補・不正slug形式は除外)を実装。`cargo test`(171件)/`cargo clippy -D warnings`/`cargo fmt`/`cargo audit` すべて通過。`docs/cli-manual.md` §1.9 を更新。
