/**
 * Google OAuth トークン取得スクリプト（実装例）
 *
 * カスタマイズ: 下の APP_DIR_NAME と SCOPES をプロダクトに合わせて編集すること。
 * （PROJECT.md の「認証情報・シークレット」「外部 API 連携」と一致させる）
 *
 * 使い方:
 *   1. node .github/skills/setup/scripts/google-get-token.mjs        → URL を表示
 *   2. node .github/skills/setup/scripts/google-get-token.mjs <code> → トークンを保存
 */
import { google } from "googleapis";
import { readFile, writeFile, mkdir } from "fs/promises";
import { homedir } from "os";
import { join } from "path";

// ===== CUSTOMIZE =====
const APP_DIR_NAME = ".my-app"; // PROJECT.md の認証情報ディレクトリと一致させる
const SCOPES = [
  // 必要なスコープに差し替える。例:
  // "https://www.googleapis.com/auth/gmail.readonly",
  // "https://www.googleapis.com/auth/calendar.events",
];
// =====================

const APP_DIR = join(homedir(), APP_DIR_NAME);
const CREDENTIALS_PATH = join(APP_DIR, "credentials.json");
const TOKEN_PATH = join(APP_DIR, "token.json");

if (SCOPES.length === 0) {
  console.error("SCOPES が未設定です。スクリプト冒頭の CUSTOMIZE セクションを編集してください。");
  process.exit(1);
}

const credentialsRaw = await readFile(CREDENTIALS_PATH, "utf-8");
const credentials = JSON.parse(credentialsRaw);
const { client_id, client_secret, redirect_uris } = credentials.installed;

const auth = new google.auth.OAuth2(client_id, client_secret, redirect_uris[0]);

const code = process.argv[2];

if (!code) {
  // Step 1: URL を表示する
  const authUrl = auth.generateAuthUrl({
    access_type: "offline",
    scope: SCOPES,
  });
  console.log(
    "\n以下の URL をブラウザで開いて、Google アカウントでログインしてください:\n",
  );
  console.log(authUrl);
  console.log(
    "\n承認後にリダイレクトされた URL の中の code= の値をコピーして:",
  );
  console.log("  node .github/skills/setup/scripts/google-get-token.mjs <コード>\n");
} else {
  // Step 2: コードをトークンに交換する
  const { tokens } = await auth.getToken(code.trim());
  auth.setCredentials(tokens);

  await mkdir(APP_DIR, { recursive: true });
  await writeFile(TOKEN_PATH, JSON.stringify(tokens, null, 2));

  console.log("\n✅ トークンを保存しました:", TOKEN_PATH);
}
