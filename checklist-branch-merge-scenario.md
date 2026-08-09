# Task: mh-sample-test-caseに分岐・マージを含む検証シナリオを追加する

Created: 2026-08-10

背景: `mh-sample-test-case`は現状(調査時点)、単一ブランチ・単一担当者の逐次運用(test1→test2→test3)のみで、論文が強調する「複数ブランチの分岐」「2親分岐判定」が一度も検証されていなかった(improvement-prompts.md項目3)。項目2(lineage統合)の実装後に検証すると効率的、との推奨順序に従い項目2完了後に着手した。

ユーザー確認済み: `mh-sample-test-case`リポジトリ(`c:\Users\papa\work\mh-sample-test-case`)に新しいfeatureブランチ・マージコミット・マイルストーンタグ(`test4`)をローカルで追加する(pushしない)。既存の`test1`〜`test3`は変更しない。

## Steps
- [x] Step 1: `mh-sample-test-case`でfeatureブランチ(`markharness-lineage-scenario-feature`)を切り、`todo-simple/todo-add`Featureに変更を加える(`expected/005.yml`追加)
- [x] Step 2: main側でも`todo-add`Featureの別ファイル(`expected/004.yml`)に変更を加え、両方をマージする(`--no-ff`マージコミット、コンフリクトなし)
- [x] Step 3: マイルストーンタグ`test4`を打ち、`markharness changes compute test3 test4`と`markharness changes lineage --commit <merge-commit>`を実行する
- [x] Step 4: 2親分岐が`derived_from`相当のフィールド(`from_tree_shas`)に正しく記録されることを確認する
- [x] Step 5: 実行結果(コマンド出力、生成された`changes/test4.yaml`、`.markharness-cache/test4.json`)をdocs/gap-analysis-mh-sample-test-case.md §8に記録する
- [x] Step 6: 想定通りに動かなかった場合の乖離を記録する(§8.4に記載: `from_tree_sha`と`from_tree_shas`の併存は想定内だが、下流ツールでの使い分けは未検証)

## Notes
- `mh-sample-test-case`側の変更: 新規ブランチ`markharness-lineage-scenario-feature`、マージコミット`b467ce1`、タグ`test4`、コミット`ac0ce77`(`changes/test4.yaml`追加)。いずれもローカルのみでpushしていない。既存の`test1`〜`test3`のコミット・タグ・`changes/test2.yaml`・`changes/test3.yaml`・`executions/`は無変更。
- 結果は期待通り: `todo-add`のみ`true_divergence`と判定され、`changes/test4.yaml`の`from_tree_shas`に両親のtree SHAが記録された。これは項目2で実装した`to_milestone`直接マージコミット統合が実リポジトリでも機能することを確認した初めての実例。
- 統合範囲の限界(マイルストーン区間内の任意の位置でのマージには非対応)は本シナリオでは検証していない(`to_milestone`が直接マージコミットである最も単純なケースのみ)。

## Summary
`mh-sample-test-case`に分岐・マージを含む`test4`マイルストーンを追加し、`markharness changes compute`/`changes lineage`が実際の複数コミットリポジトリで設計通り(`true_divergence`の検出・`from_tree_shas`への反映)に動作することを確認した。既存の`test1`〜`test3`は無変更。詳細はdocs/gap-analysis-mh-sample-test-case.md §8を参照。
