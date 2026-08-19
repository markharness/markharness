# CLAUDE.md

このリポジトリは VS Code Copilot / Claude Code 両対応の開発テンプレートです。
**プロダクト固有の情報(名前・技術スタック・外部 API・認証情報パス・Pre-PR チェックリスト)はすべて [PROJECT.md](./PROJECT.md) に集約**されています。作業前に必ず読んでください。PROJECT.md が未設定のテンプレート状態なら、`/customize` の実行を提案してください。

## 常時適用されるルール

以下のワークフロー規約に従ってください(実体は `.github/instructions/` にあり、Copilot と共有):

- **設計に関してのルール**
  - 後方互換性を想定せず互換のための設計は排除し、常に最善のプロダクトを目指す
  - 常に最善のプロダクトを構築するために以下を実施する
    - 設計の有用性、無用性を検討
    - 設計のデメリットに工数は含めない
- **チェックリスト運用** — 複数ステップの作業は `checklist-<task>.md` で進捗管理する。詳細: [checklist-workflow](./.github/instructions/checklist-workflow.instructions.md)
- **TDD** — `src/` 配下のコードは Red-Green-Refactor で開発する。テストなしのプロダクションコードは書かない。詳細: [tdd-workflow](./.github/instructions/tdd-workflow.instructions.md)
- **シークレット保護** — 認証情報はワークスペース外(PROJECT.md 定義のディレクトリ)に保存。値の表示・読み込み・ハードコード禁止。詳細: [security](./.github/instructions/security.instructions.md)
- **破壊的コマンドの事前確認・事後復旧** — `git reset --hard` / `rm -rf` だけでなく `tauri init --force` のような他ツールの force 上書き系コマンドも含め、実行前に必ずユーザーに確認し、実行後に問題が起きた場合は reflog 等で復旧を試みる。詳細: [destructive-command-safety](./.github/instructions/destructive-command-safety.instructions.md)
- **論文(`docs/ja/テスト知識管理のGit-nativeモデル_統合版.md`、English: `docs/en/git-native-model-for-test-knowledge-management.md`)の変更履歴運用** — 記述内容に実質的な変更(追加・修正・削除)を加えたら、末尾の「変更履歴(Changelog)」セクションに追記する。参照リンクの張り替え・表記統一など内容に実質的な変更を伴わない編集では追記しない。同様の理由で、内容に実質的な変更がない場合はファイル名・見出しへのバージョン番号(V2等)の付与も行わない(過去に`統合版V2.md`というファイル名運用があったが、実質的な差分がないまま番号だけが積み上がる問題があったため廃止した)。
- **依存ライセンス・バージョニング・docs配置** — 依存追加前のライセンス確認、`Cargo.toml`のversionを唯一の情報源とするバージョニング、`docs/ja/decisions`・`docs/en/decisions`(ADR)/`docs/ja/design`・`docs/en/design`(実装設計)の使い分けに従う。詳細: [release-and-license](./.github/instructions/release-and-license.instructions.md)
- **ADRの管理(番号付き決定記録)** — `docs/ja/decisions/`・`docs/en/decisions/`をそれぞれ単一ディレクトリ・単一の番号連番(`NNNN-slug.md`、両言語で同一番号)で運用し、確定度やライフサイクル(Proposed/Accepted/Rejected/Deprecated/Superseded)によって別ディレクトリへ移さない。各ファイル冒頭に`## ステータス`/`## Status`セクションを置き、状態が変わったらその行を書き換える(ファイルの移動はしない)。これはMichael Nygard「Documenting Architecture Decisions」(https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions)の提案に基づく運用で、`docs/internal-notes/`という別ディレクトリに未確定文書を退避する旧運用は、番号空間の分断とファイル移動に伴うリンク陳腐化(本リポジトリで実際に発生した不具合)の原因になっていたため廃止した。詳細: [release-and-license](./.github/instructions/release-and-license.instructions.md)
- **日本語版/英語版ドキュメントの同時更新** — `docs/ja/`と`docs/en/`は内容を鏡合わせで維持する運用。`docs/`配下(論文・ADR・design仕様書を含む)を編集する際は、同じ変更を両言語のファイルに同一PR内で反映する。どちらかのみを更新して他方を放置しない。
- **GitHub Flow** — ファイル変更・commit・push・pull requestを伴いうる全作業では、最初に[github-flow](./.github/instructions/github-flow.instructions.md)を読み、`main`ではなく目的別branchで作業する。push・pull request作成・mergeはユーザーが明示した範囲だけ実行する。

## 標準コマンド

PROJECT.md の「標準コマンド」表を参照(デフォルト: `npm run build` / `npm test` / `npm run lint` / `npm audit`)。

## スラッシュコマンド

`.claude/commands/` に定義済み。実体は `.github/prompts/` の共通ファイルを参照します。

| コマンド          | 用途                                                    |
| ----------------- | ------------------------------------------------------- |
| `/customize`      | テンプレートをプロダクト用に構成(PROJECT.md を書き換え) |
| `/setup`          | 環境構築(ツールチェック → 外部 API 設定 → 初期化)       |
| `/plan-checklist` | タスクをチェックリスト化して着手                        |
| `/dev-tdd`        | TDD で機能を実装                                        |
| `/cleanup`        | 完了済みチェックリストの整理                            |
| `/help`           | トラブルシューティング                                  |

## 注意

- `.github/skills/skill-creator/` は **VS Code Copilot 専用**の移植版です。Claude Code / Cowork では組み込みの skill-creator(eval・ベンチマーク機能付き)を使ってください。
- Git操作の権限境界はGitHub Flow instructionを正とします。
- チャットはユーザーが使っている言語で応答してください。
