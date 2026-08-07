# Task: knowledge add に既存候補の番号選択とCondition id重複接頭辞の自動除去を追加
Created: 2026-08-07

## Steps
- [x] Step 1 (Red/Green/Refactor): `src/knowledge.rs` に `strip_redundant_condition_prefix` を実装(テスト3件、cargo test 31 passed)
- [x] Step 2 (Red/Green): `src/interactive.rs` に特性テスト `no_candidate_list_printed_for_fresh_knowledge_dir` を追加
- [x] Step 3 (Red/Green): `list_candidate_ids` 追加 + `prompt_slug` に `candidates` 引数を追加、Feature id 呼び出しに適用。`lists_feature_candidates_by_number_and_selects_by_index` をGreenに
- [x] Step 4 (Red/Green): Condition id 呼び出しにも候補一覧を適用。`lists_condition_candidates_by_number_and_selects_by_index` をGreenに
- [x] Step 5: `typing_literal_existing_id_with_candidates_present_still_works` を追加し無改修でGreen確認(回帰)
- [x] Step 6 (Red/Green): `run_add` に重複接頭辞除去ブロックを追加。`auto_dedup_strips_redundant_condition_prefix_and_notifies` をGreenに
- [x] Step 7: `legacy_condition_dir_with_redundant_prefix_is_reused_without_stripping` / `stripped_id_matches_a_different_preexisting_condition_reuses_it` を追加しGreen確認
- [x] Step 8: 既存4テストを含む全体 `cargo test` で回帰なし確認(38 passed)
- [x] Step 9 (Refactor): `cargo clippy --all-targets -- -D warnings` / `cargo fmt`(collapsible_if指摘を修正)
- [x] Step 10: `cargo audit` 実行(43 crate、指摘なし)
- [x] Step 11: `docs/cli-manual.md` §1.2 を更新(フロー説明・バリデーション・新規使用例2件)
- [x] Step 12: `cargo run -- init --dir tmp/manual-check` と `knowledge add` を対話実行して目視確認、`tmp/manual-check` を削除

## Notes
- Green実装を先に(候補リスト対応+重複除去)まとめて行い、7件の新規テストは一発で全通過した。設計段階で「生ID存在確認を先に行ってから除去を試みる」という順序を決めていたため、legacy保護・除去後ID再利用のテストも追加実装なしでGreenになった。
- Red確認の際、"1"を新規featureのidとして解釈してしまう旧コードに対して実行したテストがOOMクラッシュ(無限ループでwriterバッファが際限なく増大)になった。想定内のRed失敗だが、テスト実行時に大量メモリを消費する点は留意。
- clippyの `collapsible_if` 指摘により、`if let` と `&&` 条件をlet-chain形式に統合した。
- 手動確認: `knowledge add --dir tmp/manual-check` で実際に対話実行し、(1) Condition id `player-jump-ground` → 自動除去メッセージ表示 → `ground/` に保存、(2) 再実行時に `1) player-jump` / `1) ground` の番号一覧が表示され番号選択で再利用できること、(3) 生成される `expected` id が `player-jump-ground-002`(重複なし)であることを確認した。

## Summary
`markharness knowledge add` の対話フローに、既存Feature/Conditionの番号選択と、Condition id にFeature idが重複して含まれる場合の自動接頭辞除去(既存データの保護つき)を追加した。TDDで11件の新規テスト(knowledge.rs 3件、interactive.rs 8件)を追加し、既存4件を含む全38件が通過。`cargo clippy`・`cargo audit` も問題なし。`docs/cli-manual.md` に新しいフロー説明とバリデーション規則、使用例2件を追記した。
