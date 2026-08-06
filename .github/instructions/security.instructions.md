---
description: "Use when handling API keys, OAuth tokens, credentials, or any sensitive data. Enforces secret protection rules to prevent accidental exposure."
applyTo: credentials.json, token.json
---

# Security — Secret Protection

> Respond in the language the user is using in the chat.

## Design Principle

Credentials are stored **outside the workspace** in a hidden directory under the user's home (see the "認証情報・シークレット" section of [PROJECT.md](../../PROJECT.md) for the exact path) so they are never included in editor context sent to the LLM. This is the primary defense — even if a user opens every file in the workspace, no secrets are exposed.

## Credential Location

The credentials directory is defined in [PROJECT.md](../../PROJECT.md). Example layout:

```text
~/.<app-name>/
├── credentials.json   ← API client config (e.g. OAuth client downloaded from the provider console)
└── token.json         ← Generated after first login (auto-created by the app)
```

In code, read credentials from that path:

```typescript
import path from "node:path";
import os from "node:os";

// Directory name must match PROJECT.md
const CREDENTIALS_DIR = path.join(os.homedir(), ".<app-name>");
const CREDENTIALS_PATH = path.join(CREDENTIALS_DIR, "credentials.json");
const TOKEN_PATH = path.join(CREDENTIALS_DIR, "token.json");
```

## Absolute Rules

1. **NEVER display secret values** in chat responses, code output, terminal output, or logs.
   This includes: API keys, OAuth client secrets, access tokens, refresh tokens.

2. **NEVER read credential file contents** into the conversation. To verify credentials exist, check for the file's presence — not its contents.

3. **NEVER hardcode secrets** in source files. Always read from the credentials directory defined in PROJECT.md.

4. **NEVER store secrets inside the workspace**. No `.env` with real values, no `credentials.json` in the project root.

## When Users Need to Configure Credentials

- Guide them to use the `/setup` skill, which walks them through external API setup step by step.
- The setup skill will place `credentials.json` in the credentials directory for them.
- Do NOT generate placeholder or example secret values.

## If a Secret Is Accidentally Exposed

1. Immediately warn the user.
2. Advise them to rotate the credential at its source (e.g., regenerate the client secret in the provider's console).
3. If the secret was committed to git, help them remove it from history using `git filter-branch` or `git rebase`.

## Logging

- Never log request headers containing `Authorization`.
- Use structured logging that excludes sensitive fields by default.
