# Task: verify trace / verify pending を論文本体に反映
Created: 2026-08-10

背景: `src/verify.rs` に実装済みの `markharness verify trace` / `markharness verify pending`(TestExecution と ChangeEvent の自動突合、`verified_feature_tree_shas` による未検証テストの検出)が、論文 docs/テスト知識管理のGit-nativeモデル_統合版.md に一切記載がなく、docs/change-event-verification-tracking-spec.md という別紙にのみ仕様がある(improvement-prompts.md 項目4)。

## Steps
- [x] Step 1: 論文に新しい節 §3.7「変更検知に基づく再検証トラッキング」を追加し、verify trace / verify pending の目的・仕組み・具体例を change-event-verification-tracking-spec.md の内容を要約して記載する
- [x] Step 2: §3.6 実装状況まとめ表に verify trace / verify pending の行を追加する(実装済み行・簡略化行の両方)
- [x] Step 3: PROJECT.md の主要機能一覧に verify trace / verify pending を追記する(README.md はプロダクト非依存のテンプレート説明のため対象外と判断)
- [x] Step 4: 追記内容が論文の他の主張(第6章 Threats to Validity 等)と矛盾しないか確認する(§3.7はFuture Work項目と整合し、既存の主張と矛盾なし)

## Summary
論文§3.7を新設してverify trace/verify pendingの目的・データモデル拡張(verified_feature_tree_shas)・判定アルゴリズム(pending/stale区別)・CLIインターフェースを要約記載し、§3.6実装状況表とPROJECT.mdの主要機能一覧を追従させた。

## Notes
- README.md はテンプレート汎用の使い方説明であり、markharness固有の機能一覧を持たない。機能一覧はPROJECT.mdの「主要機能」に集約されているため、そちらに追記する方針とする。
