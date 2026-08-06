# 実装例 — Google OAuth (Gmail / Calendar API)

> 外部 API 連携に Google API を使う場合の具体的なセットアップ手順です。
> `/setup` の Phase 2 から参照されます。他の API（Slack, OpenAI 等）の場合はこのファイルをテンプレートとして同様の手順書を作成してください。
> 以下の `~/.<app-name>/` は [PROJECT.md](../../../../PROJECT.md) で定義した認証情報ディレクトリに読み替えてください。

## 前提

- OAuth クライアント（Desktop app 型）を使用
- 認証情報は `~/.<app-name>/credentials.json` に配置（ワークスペース外）

## Step 1 — 既存の認証情報を確認

**Windows**:

```powershell
Test-Path "$env:USERPROFILE\.<app-name>\credentials.json"
```

**macOS/Linux**:

```bash
test -f ~/.<app-name>/credentials.json && echo "exists" || echo "not found"
```

存在すればこの手順はスキップ。

## Step 2 — ガイド: Google Cloud プロジェクト作成

ユーザーに伝える:

> ブラウザで https://console.cloud.google.com/ を開いてください。
>
> 1. Google アカウントでサインイン
> 2. 上部の「プロジェクトを選択」→「新しいプロジェクト」
> 3. プロジェクト名を入力（任意。例: アプリ名）
> 4. 「作成」をクリック
>
> 完了したら教えてください!

確認を待ってから次へ進む。

## Step 3 — ガイド: 必要な API を有効化

ユーザーに伝える（有効化する API は PROJECT.md の「外部 API 連携」表に従う）:

> 1. 左メニュー「API とサービス」→「ライブラリ」
> 2. 使用する API（例: **Gmail API**, **Google Calendar API**）を検索して「有効にする」
>
> すべて有効化できたら教えてください!

確認を待つ。

## Step 4 — ガイド: OAuth 同意画面の設定

ユーザーに伝える:

> 1. 「API とサービス」→「OAuth 同意画面」
> 2. 「External」を選択して「作成」
> 3. 入力:
>    - アプリ名: 任意
>    - ユーザーサポートメール: 自分のメール
>    - デベロッパー連絡先: 自分のメール
> 4. 「保存して次へ」
> 5. 「スコープ」ページはそのまま「保存して次へ」（スコープはコード側で指定）
> 6. 「テストユーザー」で「Add users」から自分の Gmail アドレスを追加
> 7. 「保存して次へ」→「ダッシュボードに戻る」
>
> 完了したら教えてください!

確認を待つ。

## Step 5 — ガイド: OAuth 認証情報の作成

ユーザーに伝える:

> 1. 「API とサービス」→「認証情報」
> 2. 「認証情報を作成」→「OAuth クライアント ID」
> 3. アプリケーションの種類: **デスクトップアプリ**（リダイレクト URI 不要でシンプル）
> 4. 名前: 任意
> 5. 「作成」
> 6. ダイアログで **「JSON をダウンロード」** をクリック
> 7. わかる場所（例: ダウンロードフォルダ）に保存
>
> 保存した場所かファイル名を教えてください!

ユーザーからパスの回答を待つ。

## Step 6 — 認証情報を安全な場所へ移動

ユーザーの回答を絶対パスに正規化する:

- ファイル名のみの場合: ダウンロードフォルダを探す
- 相対パスの場合: カレントディレクトリから解決
- ワイルドカードは使わない。ファイル内容は読まない・表示しない

**Windows**:

```powershell
$sourcePath = (Resolve-Path -LiteralPath "<absolute-path-to-downloaded-json>").Path
New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.<app-name>"
Move-Item -LiteralPath $sourcePath -Destination "$env:USERPROFILE\.<app-name>\credentials.json" -Force
```

**macOS/Linux**:

```bash
source_path="<absolute-path-to-downloaded-json>"
mkdir -p ~/.<app-name>
mv "$source_path" ~/.<app-name>/credentials.json
```

**IMPORTANT**: 認証情報ファイルの中身は読まない・表示しない。ワークスペース外へ移動し、誤コミットを防ぐ。

移動後にユーザーへ確認:

> ✅ 認証情報を `~/.<app-name>/credentials.json` に保存しました。
> プロジェクトフォルダの外なので、AI に送信されたり Git にコミットされることはありません。

## Step 7 — トークン取得

初回のアクセストークン取得には [google-get-token.mjs](../scripts/google-get-token.mjs) を使用する。
スクリプト冒頭の `APP_DIR_NAME` と `SCOPES` をプロダクトに合わせて編集してから実行:

```bash
node .github/skills/setup/scripts/google-get-token.mjs          # 認可 URL を表示
node .github/skills/setup/scripts/google-get-token.mjs <code>   # トークンを保存
```
