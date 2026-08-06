---
description: "Customize this template for a specific product. Use when starting a new project from this template, e.g. 'カスタマイズして: 家計簿アプリ' or 'customize this for a recipe manager'."
agent: "agent"
---

# Customize — テンプレートをプロダクト用に構成する

> Respond in the language the user is using in the chat.

このテンプレートを、ユーザーが指定したプロダクト用にカスタマイズします。
**原則: 書き換えるのは [PROJECT.md](../../PROJECT.md) が中心。** instructions / prompts / skills は PROJECT.md を参照する設計なので、通常は変更不要です。

## 手順

### 1. プロダクトを理解する

ユーザーの指定（例: 「家計簿アプリ用に」）から以下を確定させる。不明瞭な点だけ最小限質問する:

- プロダクト名と概要
- 主要機能（3〜5 個）
- 外部 API 連携の有無（Google, Slack, OpenAI など）
- 技術スタックの変更有無（デフォルト: TypeScript + Vitest + ESLint）

### 2. PROJECT.md を書き換える

`<!-- CUSTOMIZE -->` マークの付いた各セクションを更新する:

- **プロダクト概要**: 名前・概要・主要機能
- **技術スタック**: 変更がある場合のみ。コマンド表も追従させる
- **認証情報・シークレット**: 認証情報ディレクトリをプロダクト名に合わせる（例: `~/.kakeibo-app/`）
- **外部 API 連携**: 使用する API を表に記載。Google OAuth を使う場合は [google-oauth-example.md](../skills/setup/references/google-oauth-example.md) を実装の参考として案内する

### 3. スタック変更時のみ: 追従修正

技術スタックをデフォルトから変更した場合のみ、以下も修正する:

- `.github/instructions/tdd-workflow.instructions.md` — テスト・lint コマンド
- `.github/skills/setup/SKILL.md` — 前提ツールの表とスクリプト
- `.github/skills/setup/scripts/initialize-project.mjs` — 生成する設定ファイル

### 4. 検証

1. PROJECT.md に「未設定」「（〜）」のプレースホルダが残っていないか確認
2. 各ファイル間の相対リンクが切れていないか確認
3. カスタマイズ結果のサマリをユーザーに提示:

   ```text
   ✅ PROJECT.md — プロダクト概要・認証情報パスを更新
   ✅ 外部 API: Gmail API を追加（google-oauth-example.md 参照）
   ➡️ 次のステップ: /setup で環境構築、/plan-checklist で計画開始
   ```

### 5. カスタマイズ内容をコミットする

このステップで変更したファイル（PROJECT.md、および手順 3 で追従修正したファイル)のみを対象にコミットする。理由: 未コミットのままだと、後で `tauri init --force` のような破壊的コマンドを実行した際にカスタマイズ内容が復旧不能になる（詳細: [destructive-command-safety](../instructions/destructive-command-safety.instructions.md)）。手順 2〜3 で触っていない他のファイルは対象に含めない。

1. `git status` でこのステップの対象ファイルのみが変更されていることを確認する。対象外の未コミット変更（ユーザーの作業中のもの）が混ざっている場合は、それらを含めずに対象ファイルだけをコミットする。
2. コミットメッセージ例: `customize: <プロダクト名> 用にテンプレートを構成`
3. コミットした旨とコミットハッシュをサマリーに追記して提示する。

## 注意

- ユーザーが明示的に頼まない限り、instructions / prompts の本文は書き換えない（汎用のまま保つ）
- シークレットの実値を PROJECT.md やチャットに書かない
- 手順 5 のコミットは PROJECT.md（および手順 3 の追従修正ファイル）に限定する。無関係なファイルを一緒にコミットしない
