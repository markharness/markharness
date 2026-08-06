---
name: setup
description: "Check and install prerequisites for this project, guide external API setup, and initialize project tooling. Use when setting up the development environment, when a user first opens the workspace, when troubleshooting missing tools, or when the user mentions setup, install, prerequisites, or environment."
---

# Setup (Claude Code)

実体は Copilot と共通の手順書です。**[.github/skills/setup/SKILL.md](../../../.github/skills/setup/SKILL.md) を読み、その手順に従ってください**。ただし以下を読み替えること:

- **Phase 0(エディタ設定)はスキップ** — VS Code Copilot 固有(モデル選択・権限レベル UI)の内容のため。
- スクリプトのパスはリポジトリルートからの相対パスでそのまま実行可能:
  - 前提チェック: `.github/skills/setup/scripts/check-prerequisites.sh`(Windows は `.ps1`)
  - プロジェクト初期化: `node .github/skills/setup/scripts/initialize-project.mjs`
  - Google OAuth トークン取得: `node .github/skills/setup/scripts/google-get-token.mjs`
- 外部 API の要否・認証情報ディレクトリは [PROJECT.md](../../../PROJECT.md) を参照。
- Google OAuth の詳細手順: [google-oauth-example.md](../../../.github/skills/setup/references/google-oauth-example.md)
