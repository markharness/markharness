---
description: "Troubleshoot problems during development. Use when the user is stuck, encountering errors, confused by UI changes, or the agent is not responding as expected."
agent: "agent"
tools: [read, search, web]
---

# Help — Troubleshooting Guide

> Respond in the language the user is using in the chat.

The user is stuck and needs help. Your job is to diagnose the problem and guide them to a solution.

## Procedure

### 1. Understand the Problem

Ask the user to describe what's happening. Encourage them to share:
- Error messages (paste as-is)
- Screenshots of what they see
- What they were trying to do when the problem occurred

If the user provides a screenshot, analyze it to understand the current UI state.

### 2. Categorize and Diagnose

Determine which category the problem falls into:

| Category | Examples | Approach |
|----------|----------|----------|
| **Environment** | Missing tools, version mismatch, npm errors | Run `/setup` check, verify versions |
| **Code error** | Test failures, TypeScript errors, lint errors | Read the error, find the source, fix |
| **UI / settings** | VS Code UI changed, can't find a button | Ask for screenshot, check latest docs with web search |
| **Agent behavior** | Agent not responding, stuck in a loop, wrong output | Suggest stopping, clearing context, or switching models |
| **External API** | OAuth errors, credential issues | Check `credentials.json` exists in the credential directory defined in [PROJECT.md](../../PROJECT.md), verify the API is enabled |
| **Destructive command / lost work** | Accidental `git reset --hard`, force-push, `rm -rf`, `tauri init --force`, deleted branch | Follow the recovery procedures in [destructive-command-safety](../instructions/destructive-command-safety.instructions.md) |

### 3. If the UI or Instructions Don't Match Reality

External provider consoles (e.g. Google Cloud Console), VS Code, and GitHub Copilot update frequently. If the user reports that a screen looks different from what's described in README.md or the setup guide:

1. Don't insist on the documented steps — the docs may be outdated.
2. If the user provides a screenshot, use it to determine the current state.
3. Use the `web` tool to search for the latest documentation or instructions.
4. Guide the user based on what **actually** appears on their screen.

### 4. If the Agent Was Stuck or Unresponsive

If the user reports the agent was working for a long time without results:

1. Ask what the agent was trying to do before it got stuck.
2. Check the current state of files and tests to understand where things stand.
3. Suggest a simpler approach or break the problem into smaller steps.
4. If the same problem keeps occurring, suggest switching to a different model temporarily — different models have different strengths.

### 5. Resolve

- Provide a clear, actionable solution.
- If the fix requires multiple steps, create a mini checklist.
- After the fix, verify that the problem is actually resolved.
- If you cannot resolve the issue, be honest and suggest alternative approaches.
