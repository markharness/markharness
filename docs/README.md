# docs/ 資料インデックス

このディレクトリの資料は「研究設計(論文)」「製品化設計」「CLI仕様・マニュアル」「実装との相違点調査」の4層に分かれる。読む順序の目安と、資料間の依存関係を以下にまとめる。

## 読む順序の目安

1. **[テスト知識管理のGit-nativeモデル_統合版.md](./テスト知識管理のGit-nativeモデル_統合版.md)** — 本プロジェクト全体の土台となる研究設計(論文ドラフト)。他の全資料はこれを前提にする。
2. **[product-operation.md](./product-operation.md)** — 論文の設計を製品運用イメージ(UC1〜UC8、アクター、ファイル作成順序)に落とし込んだもの。
3. **[cli-manual.md](./cli-manual.md)** — 実装済み/未実装のCLIコマンド一覧。ユースケースとの対応は2.のUC番号を参照する。
4. 個別コマンドの詳細設計(cli-manualから参照される):
   - **[knowledge-apply-cli-spec.md](./knowledge-apply-cli-spec.md)** — `knowledge validate`/`apply`(非対話ナレッジ登録)の仕様。
   - **[testcase-generation-design.md](./testcase-generation-design.md)** — `generate`(TestCase決定的生成)の仕様。
   - **[change-event-verification-tracking-spec.md](./change-event-verification-tracking-spec.md)** — `verify trace`/`verify pending`(実行結果とChangeEventの自動突合)の仕様。
5. 実装と設計書の乖離を後から検証した調査資料(いずれも参考資料・監査ログの位置づけ):
   - **[gap-analysis-mm-folder.md](./gap-analysis-mm-folder.md)** — 調査対象: `c:\Users\papa\work\mm`(TODOアプリのケーススタディ運用リポジトリ、旧称)。
   - **[gap-analysis-mh-sample-test-case.md](./gap-analysis-mh-sample-test-case.md)** — 調査対象: `mh-sample-test-case`(同ケーススタディの後継リポジトリ)。
   - **[review-data-model-improvement-proposals.md](./review-data-model-improvement-proposals.md)** — 外部の「データモデル分析レポート」が提案した改善案5件を実装・論文と照合した妥当性レビュー。論文側の修正要否も併せて確認済み(結論：修正不要)。
6. **[improvement-prompts.md](./improvement-prompts.md)** — 過去のレビュー(ツール評価・論文評価・上記のデータモデルレビュー)から起こした、AIエージェント向けの実行プロンプト集。項目1〜6は旧ツール/論文レビュー由来、項目7〜9はデータモデルレビュー由来(却下した3件は末尾の却下ログに記録)。各項目冒頭に完了/未確認の状況注記がある(旧`.claude/improvement-prompts.md`から移動・統合済み)。

## 「相違点調査資料」2本の関係について(注意)

`gap-analysis-mm-folder.md`と`gap-analysis-mh-sample-test-case.md`は、**どちらかがもう片方を置き換える版違いではない**。両方とも現時点で有効な資料であり、それぞれ別の調査対象を扱う(旧ファイル名がそれぞれ「調査資料」「調査資料v2」だった名残で混同しやすい点に注意)。

| | 調査対象リポジトリ | 主な指摘 |
|---|---|---|
| `gap-analysis-mm-folder.md` | `c:\Users\papa\work\mm` | 骨格レベルでの一致を確認。`schema/`空実装・`change_type`欠如など、後日markharness側で解消された指摘を「追記」として本文中に残す。 |
| `gap-analysis-mh-sample-test-case.md` | `mh-sample-test-case` | tree SHAベース検知の実地確認、設計書に記述のない`verified_feature_tree_shas`連動が既に実運用されている点、`forked_from`等の未使用機能を報告。 |

`gap-analysis-mh-sample-test-case.md`の§6は`gap-analysis-mm-folder.md`の指摘(TestCaseファイル名と`case_id`の不一致)が後継リポジトリでは解消されていることを確認する形で言及しているのみで、`gap-analysis-mm-folder.md`自体を上書きするものではない。

## 資料の鮮度について

- **統合版**は本文中に「注(実装状況について)」「§3.6 実装状況まとめ」を持ち、CLI実装との既知の相違を追記済み。ただし本文が触れる相違点調査資料へのリンクは**`gap-analysis-mm-folder.md`のみ**であり、`gap-analysis-mh-sample-test-case.md`(`mh-sample-test-case`側の調査)は未参照。
- `gap-analysis-mm-folder.md`は本文各節に「追記(markharness側の修正について)」を継ぎ足す形で更新されており、指摘時点と現在の実装状態を区別して読む必要がある。
- `cli-manual.md`・`knowledge-apply-cli-spec.md`・`testcase-generation-design.md`は「Status: Implemented」等のステータス行と「実装時の追記/変更」節を持ち、初期案と実装の差分を本文内で自己完結して管理している。

## ファイル名の命名規則

論文(`テスト知識管理のGit-nativeモデル_統合版.md`)を除く全ドキュメントは英語kebab-case(`foo-bar.md`)で統一している。論文のみ、参照箇所が多く影響範囲が大きいため従来の日本語ファイル名を維持している。
