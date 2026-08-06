# CLAUDE.md

このリポジトリは VS Code Copilot / Claude Code 両対応の開発テンプレートです。
**プロダクト固有の情報(名前・技術スタック・外部 API・認証情報パス・Pre-PR チェックリスト)はすべて [PROJECT.md](./PROJECT.md) に集約**されています。作業前に必ず読んでください。PROJECT.md が未設定のテンプレート状態なら、`/customize` の実行を提案してください。

## 常時適用されるルール

以下のワークフロー規約に従ってください(実体は `.github/instructions/` にあり、Copilot と共有):

- **チェックリスト運用** — 複数ステップの作業は `checklist-<task>.md` で進捗管理する。詳細: [checklist-workflow](./.github/instructions/checklist-workflow.instructions.md)
- **TDD** — `src/` 配下のコードは Red-Green-Refactor で開発する。テストなしのプロダクションコードは書かない。詳細: [tdd-workflow](./.github/instructions/tdd-workflow.instructions.md)
- **シークレット保護** — 認証情報はワークスペース外(PROJECT.md 定義のディレクトリ)に保存。値の表示・読み込み・ハードコード禁止。詳細: [security](./.github/instructions/security.instructions.md)
- **破壊的コマンドの事前確認・事後復旧** — `git reset --hard` / `rm -rf` だけでなく `tauri init --force` のような他ツールの force 上書き系コマンドも含め、実行前に必ずユーザーに確認し、実行後に問題が起きた場合は reflog 等で復旧を試みる。詳細: [destructive-command-safety](./.github/instructions/destructive-command-safety.instructions.md)

## 標準コマンド

PROJECT.md の「標準コマンド」表を参照(デフォルト: `npm run build` / `npm test` / `npm run lint` / `npm audit`)。

## スラッシュコマンド

`.claude/commands/` に定義済み。実体は `.github/prompts/` の共通ファイルを参照します。

| コマンド | 用途 |
|---|---|
| `/customize` | テンプレートをプロダクト用に構成(PROJECT.md を書き換え) |
| `/setup` | 環境構築(ツールチェック → 外部 API 設定 → 初期化) |
| `/plan-checklist` | タスクをチェックリスト化して着手 |
| `/dev-tdd` | TDD で機能を実装 |
| `/cleanup` | 完了済みチェックリストの整理 |
| `/help` | トラブルシューティング |

## 注意

- `.github/skills/skill-creator/` は **VS Code Copilot 専用**の移植版です。Claude Code / Cowork では組み込みの skill-creator(eval・ベンチマーク機能付き)を使ってください。
- `git push` はユーザーの明示的な指示なしに実行しないでください。
- チャットはユーザーが使っている言語で応答してください。
