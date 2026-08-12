# AI 開発テンプレート

AI コーディングエージェント（**VS Code Copilot / Claude Code 両対応**）で新しいプロダクトを開発するための汎用テンプレートです。
チェックリスト駆動・TDD・シークレット保護のワークフローが組み込まれています。

**位置づけ**：このファイルは本リポジトリ(markharness)が由来した汎用AI開発テンプレートの説明です。markharness 自体の使い方は [README.md](../README.md) を参照してください。

## 使い方

1. このテンプレートを新しいプロジェクトのルートにコピー
2. チャットで **`/customize`** を実行し、作りたいプロダクトを伝える
   例: `/customize 家計簿アプリ用にカスタマイズしてください`
   → [PROJECT.md](../PROJECT.md) がそのプロダクト用に書き換えられます
3. **`/setup`** で開発環境を構築（ツールチェック → 外部 API 設定 → プロジェクト初期化）
4. **`/plan-checklist`** で計画、**`/dev-tdd`** で TDD 実装

## 設計

**プロダクト固有の情報はすべて [PROJECT.md](../PROJECT.md) に集約**されています。
`.github/` 配下のワークフロー定義は PROJECT.md を参照する汎用的な内容なので、プロダクトを変えるときに書き換えるのは原則 PROJECT.md だけです。

```text
PROJECT.md                     ← 唯一のカスタマイズポイント（プロダクト名・API・スタック・認証情報パス）
CLAUDE.md                      ← Claude Code 用エントリポイント（PROJECT.md と instructions を参照）
.github/                       # 実体はすべてここ（Copilot 用 + Claude から参照される共通ファイル）
├── prompts/                   # スラッシュコマンド
│   ├── customize.prompt.md    #   /customize      — テンプレートをプロダクト用に構成
│   ├── plan-checklist.prompt.md #  /plan-checklist — タスクをチェックリスト化して着手
│   ├── dev-tdd.prompt.md      #   /dev-tdd        — TDD で機能を実装
│   ├── cleanup.prompt.md      #   /cleanup        — 完了済みチェックリストの整理
│   └── help.prompt.md         #   /help           — トラブルシューティング
├── instructions/              # 常時適用されるワークフロー規約
│   ├── checklist-workflow.instructions.md  # チェックリスト運用ルール
│   ├── tdd-workflow.instructions.md        # Red-Green-Refactor の規律
│   └── security.instructions.md            # シークレット保護ルール
└── skills/
    ├── setup/                 # /setup — 環境構築（OS 判定 → ツールチェック → API 設定 → 初期化）
    │   ├── scripts/           #   前提チェック・プロジェクト初期化・Google トークン取得
    │   └── references/        #   Google OAuth の具体的手順（他 API の雛形にも使える）
    └── skill-creator/         # 新しいスキルの作成・改善（Copilot 専用）
.claude/                       # Claude Code 用の薄いラッパー（実体は .github を参照、二重管理なし）
├── skills/setup/              #   /setup（Phase 0 スキップ等の読み替え付き）
└── commands/                  #   /customize /plan-checklist /dev-tdd /cleanup /help
```

## エディタ別の使い方

- **VS Code Copilot**: そのまま使用。prompts / instructions / skills が自動で認識されます。
- **Claude Code / Cowork**: [CLAUDE.md](../CLAUDE.md) がエントリポイント。スラッシュコマンドは `.claude/commands/` 経由で同名で使えます。skill-creator のみ移植版ではなく**組み込みの公式版**を使ってください（eval・ベンチマーク機能付き）。

## デフォルトの技術スタック

TypeScript (Node.js v20+) / Vitest / ESLint + eslint-plugin-security / Prettier。
`/customize` で変更可能です（詳細は PROJECT.md）。

## 適用範囲とスケールの目安

このテンプレートは**単一スタック・少人数(個人〜小規模チーム)での MVP 開発**に最適化されています。プロダクトの成長に応じた対応の目安:

| イベント | 対応 | 内容 |
|---|---|---|
| チームが増える | 追記で対応可 | CI 定義(テスト・lint・audit の強制)、レビュー規約、ADR を追加。既存の instructions / PROJECT.md 構造はそのまま使える |
| デプロイを AI に任せる | 追記で対応可 | `security.instructions.md` に本番シークレットの規則(値を扱わずシークレットマネージャ経由で参照)を追記し、デプロイ手順をスキルとして追加 |
| モノレポ化・複数スタック化 | **再設計が必要** | 「PROJECT.md 1 ファイル = 1 スタック」という中核前提が崩れるため、パッケージ単位での分割等の構造変更が必要 |

境界線はチーム規模やプロダクトの野心ではなく、**単一スタック前提が守れるかどうか**です。

## セキュリティ方針

認証情報（`credentials.json`, `token.json`）は**ワークスペースの外**（`~/.<app-name>/`）に保存します。
エディタのコンテキストとして LLM に送信されたり、Git にコミットされることを防ぐためです。
詳細: [security.instructions.md](../.github/instructions/security.instructions.md)
