# docs/ja/ 資料インデックス

English version: [docs/en/README.md](../en/README.md)

このディレクトリの資料は「研究設計(論文)」「製品化設計」「CLI仕様・マニュアル」「実装との相違点調査」「設計判断の記録」の4層に分かれる。読む順序の目安と、資料間の依存関係を以下にまとめる。外部評価レビューへの対応は都度`decisions/`に判断記録として残し、対応完了後はレビュー本体を削除する運用としている(下記「整理の記録」参照)。

## 読む順序の目安

1. **[テスト知識管理のGit-nativeモデル_統合版.md](./テスト知識管理のGit-nativeモデル_統合版.md)** — 本プロジェクト全体の土台となる研究設計(論文ドラフト)。他の全資料はこれを前提にする。末尾の変更履歴(Changelog)に、外部評価レビュー対応の要約と`decisions/`への参照がまとまっている。
2. **[product-operation.md](./product-operation.md)** — 論文の設計を製品運用イメージ(UC1〜UC8、アクター、ファイル作成順序)に落とし込んだもの。
3. **[cli-manual.md](./cli-manual.md)** — 実装済み/未実装のCLIコマンド一覧。ユースケースとの対応は2.のUC番号を参照する。
4. 個別コマンドの詳細設計(cli-manualから参照される):
   - **[knowledge-apply-cli-spec.md](./design/knowledge-apply-cli-spec.md)** — `knowledge validate`/`apply`(非対話ナレッジ登録)の仕様。
   - **[testcase-generation-design.md](./design/testcase-generation-design.md)** — `generate`(TestCase決定的生成)の仕様。
   - **[change-event-verification-tracking-spec.md](./design/change-event-verification-tracking-spec.md)** — `verify trace`/`verify pending`(実行結果とChangeEventの自動突合)の仕様。
   - **[verification-plan-canonical-model-design.md](./design/verification-plan-canonical-model-design.md)** — (Status: Proposed、未実装)PR Verification Plan生成のためのcanonical model・パイプライン設計。[decisions/0008](./decisions/0008-verification-plan-product-roadmap.md)で決定したロードマップのStage 1〜2に対応。
   - **[domain-application-infrastructure-layering-design.md](./design/domain-application-infrastructure-layering-design.md)** — (Status: Proposed、未実装)CLIをDomain/Application/Infrastructureへレイヤー分離するアーキテクチャ設計。[decisions/0009](./decisions/0009-domain-application-infrastructure-layering.md)で決定した方向性の詳細。
5. **[gap-analysis-mh-sample-test-case.md](./gap-analysis-mh-sample-test-case.md)** — 設計と実装の乖離を、ケーススタディ運用リポジトリ`mh-sample-test-case`の実データ(tree SHAベース検知の実地確認、分岐・マージシナリオの検証を含む)で検証した調査資料。参考資料・監査ログの位置づけ。
6. **[decisions/](./decisions/)** — 外部評価レビュー対応・設計上のトレードオフなど、「なぜそう決めたか」という判断理由の記録。番号順に読むと経緯を追える。単一ディレクトリ・単一の番号連番で管理し、各ファイル冒頭の`## ステータス`セクションでライフサイクル(Proposed/Accepted/Rejected/Deprecated/Superseded、実行途中を表す`Accepted(一部実行済み)`等)を表現する。未確定/一部未実行の文書も別ディレクトリへ移さずここに置く(Michael Nygard「Documenting Architecture Decisions」・MADRの運用に準拠。以前は`docs/internal-notes/`という別ディレクトリに分けていたが、番号空間の分断とパス陳腐化の原因になっていたため廃止し、`decisions/0007`に統合した)。[decisions/0008](./decisions/0008-verification-plan-product-roadmap.md)(Status: Proposed)は、`Markharness_改善・実装検討_統合設計文書.md`のレビューを踏まえた、PR Verification Planを中心とする製品ロードマップの決定。[decisions/0009](./decisions/0009-domain-application-infrastructure-layering.md)(Status: Proposed)は、ユーザー提供のアーキテクチャ設計提案のレビューを踏まえた、CLIのDomain/Application/Infrastructureへのレイヤー分離の決定。

## 資料の鮮度について

- **統合版**は本文中に「注(実装状況について)」「§3.6 実装状況まとめ」を持ち、CLI実装との既知の相違を追記済み。詳細な突き合わせへのリンクは`gap-analysis-mh-sample-test-case.md`を参照する。
- `cli-manual.md`・`knowledge-apply-cli-spec.md`・`testcase-generation-design.md`・`change-event-verification-tracking-spec.md`は「Status: Implemented」等のステータス行と「実装時の追記/変更」節を持ち、初期案と実装の差分を本文内で自己完結して管理している。
- `gap-analysis-mh-sample-test-case.md`は「調査時点のスナップショット」であり、指摘時点と現在の実装状態を区別して読む必要がある。

## ファイル名の命名規則

論文(`テスト知識管理のGit-nativeモデル_統合版.md`)を除く全ドキュメントは英語kebab-case(`foo-bar.md`)で統一している。この1件のみ、参照箇所が多く影響範囲が大きいため従来の日本語ファイル名を維持している。

## 整理の記録

外部評価レビューへの対応が完了した資料は、判断理由を`decisions/`または論文の変更履歴(Changelog)に転記した上で削除する運用としている(`git log -- docs/`で復元可能)。同じ運用は、単発の不具合修正指示書のように役目が完結した資料にも適用する。

**2026-08-18**：

- `markharness_評価・Related_Work・新規性評価_研究ノート.md` — Related Work・新規性評価に関する外部レビュー(2026-08-17付)。指摘への対応(EBT・Requirements RTS・RBTG/MBT・Trace Link Evolutionの追加、Doorstop/StrictDocの比較追加、novelty claimの慎重化)は統合版.mdの§2.4〜2.9・§1.3・変更履歴に反映済みのため削除。
- `Markharness_改善・実装検討_統合設計文書.md` — canonical import・PR Verification Plan・GUIの3テーマを統合した実装検討レビュー。採否判断とロードマップは[decisions/0008](./decisions/0008-verification-plan-product-roadmap.md)に、詳細設計は[verification-plan-canonical-model-design.md](./design/verification-plan-canonical-model-design.md)に転記済みのため削除。

**2026-08-13(2)**：

- `nested-project-dir-git-path-fix-spec.md` — プロジェクトディレクトリがgitリポジトリのサブディレクトリの場合に`execution record`等が失敗する不具合の修正指示書。対応完了(修正・テスト・統合検証済み)に伴い、設計判断(サブディレクトリ配置の正式サポート)と対応内容の要点を[decisions/0006](./decisions/0006-nested-project-directory-support.md)に転記した上で削除。`cli-manual.md`内の参照リンクも同decisionに張り替え済み。

**2026-08-13**：

- `テスト知識管理のGit-nativeモデル_評価レビュー.md` — 2026-08-13版の外部評価レビュー本体。指摘への対応方針は`テスト知識管理のGit-nativeモデル_評価レビュー_有用性判定と修正指示.md`で判定済み、対応結果は統合版.mdの変更履歴・[decisions/0005](./decisions/0005-review-2026-08-13-triage.md)に反映済みのため削除。
- `テスト知識管理のGit-nativeモデル_評価レビュー_有用性判定と修正指示.md` — 上記レビューの有用性判定文書。判定基準・却下理由は[decisions/0005](./decisions/0005-review-2026-08-13-triage.md)に転記済みのため削除。
- `improvement-prompts.md` — 2026-08-12レビュー対応の実行プロンプト集。項目1〜6・11は[decisions/0001](./decisions/0001-version-dag-to-changeevent-model.md)・[decisions/0002](./decisions/0002-changes-compute-historical-default.md)・統合版.md変更履歴に反映済み、項目8は[decisions/0003](./decisions/0003-related-work-gtm-tmt.md)で対応済み、項目9・10は[decisions/0005](./decisions/0005-review-2026-08-13-triage.md)で却下、項目7(インポータ・大規模ケーススタディ)は統合版.md第7章 Future Workに引き継いだため削除。

**2026-08-12**：

- `review-data-model-improvement-proposals.md` — 外部データモデル分析レポートへのレビュー。採用した改善案は`improvement-prompts.md`経由で実装・論文反映が完了しており、論文修正も不要と結論済みだったため削除。
- `gap-analysis-mm-folder.md` — 最も古い相違点調査資料。指摘の大半が本文内の「追記」で解消済みとなっており、後継の`gap-analysis-mh-sample-test-case.md`と内容が重複していたため削除。
