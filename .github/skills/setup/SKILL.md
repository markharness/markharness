---
name: setup
description: "Check and install prerequisites for this project. Use when setting up the development environment, when a user first opens the workspace, when troubleshooting missing tools, or when the user mentions setup, install, prerequisites, or environment."
argument-hint: "Run environment setup check"
---

# Setup — Development Environment

> **Default language: Japanese (日本語)**
> At the very beginning of the setup, ask the user to confirm their preferred language.
> Example: greet in Japanese and offer English as an alternative.
> Use the confirmed language for all subsequent communication.

This skill checks whether all required tools are installed, guides the user through external API setup (if the project needs it), and initializes the project.

**Project-specific values** (product name, external APIs, credential directory, stack) are defined in [PROJECT.md](../../../PROJECT.md). Read it before starting. If PROJECT.md is still in its unconfigured template state, suggest running `/customize` first.

## Prerequisites

This project's stack requirements, per [PROJECT.md](../../../PROJECT.md):

| Tool | Minimum Version | Purpose | Required |
|------|----------------|---------|----------|
| Rust (`rustc`/`cargo`) | stable, recent | Language toolchain and build/test/lint runner | Yes |
| Git | v2+ | Version control | Yes |
| GitHub CLI (`gh`) | v2+ | PR creation and repo management | Yes |

On Windows without MSVC Build Tools, also set the GNU toolchain (see the note in PROJECT.md's 技術スタック section): `rustup override set stable-x86_64-pc-windows-gnu` and add WinLibs (mingw64) `bin` to `PATH`.

## Procedure

### Phase 0 — Editor Configuration

Before anything else, help the user configure their AI coding environment for a smooth experience.

#### Step 0a — Confirm AI model

Check with the user that they have selected a high-capability model (e.g. **Claude Opus 4.6** or later) in the chat model selector. If they're unsure, guide them to the model dropdown at the bottom of the chat input.

#### Step 0b — Configure Permission Level

By default, the agent asks for confirmation before every action (running commands, editing files, etc.). For a smoother experience, guide the user to change the permission level.

Explain the three levels:

| Level | Description |
| --- | --- |
| **Default Approvals** | Only safe operations are auto-approved (default) |
| **Bypass Approvals** | All tool operations are auto-approved. No confirmation dialogs |
| **Autopilot** (Preview) | All auto-approved + auto-responds to questions. Fully autonomous |

Recommend **"Bypass Approvals"** for a trusted workspace, but explain the safety trade-off before the user changes it:

- With Bypass Approvals, the agent can run terminal commands and edit files without asking for confirmation each time.
- Use it only in a trusted workspace, and only while the user is comfortable accepting responsibility for those actions.
- The user should keep an eye on the chat and terminal output, and should never paste secrets or credentials into the chat.
- When the session is finished, the user can switch back to **Default Approvals**.

Do **not** recommend Autopilot when the user wants to participate in decisions interactively.

Tell the user to look for the permission picker near the chat input area, close to the "Agent" dropdown.

Wait for the user to confirm before proceeding.

### Phase 1 — Tool Check

#### Step 1 — Detect the OS

Determine whether the user is on Windows or macOS/Linux. Use the terminal environment to detect this:
- Windows: PowerShell is available, `$env:OS` is `Windows_NT`
- macOS/Linux: Bash/Zsh is available

#### Step 2 — Run the prerequisite check

Run the appropriate script for the user's OS:

- **Windows**: [check-prerequisites.ps1](./scripts/check-prerequisites.ps1)
- **macOS/Linux**: [check-prerequisites.sh](./scripts/check-prerequisites.sh)

The script will output a table showing each tool, its status (installed/missing), and its version.

#### Step 3 — Report results

Present the results clearly to the user in their language. Example:

```text
✅ Node.js v22.1.0
✅ npm v10.8.0
✅ Git v2.44.0
❌ GitHub CLI (gh) — not found
```

#### Step 4 — Offer to install missing tools

If any tools are missing, explain what each one does and ask the user for permission before installing.

Provide the install commands grouped by OS:

**Windows (winget)**:

```powershell
winget install Rustlang.Rustup
winget install Git.Git
winget install GitHub.cli
```

**macOS (Homebrew)**:

```bash
brew install rustup-init git gh
rustup-init
```

**Linux (apt)**:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
sudo apt update && sudo apt install -y git
# gh: https://github.com/cli/cli/blob/trunk/docs/install_linux.md
```

Only install after the user confirms.

### Phase 2 — External API Setup

Per PROJECT.md's「外部 API 連携」section, markharness has no external API and no credentials to configure — skip directly to Phase 3. (This phase is only relevant again if a future feature, e.g. a TMS import connector, introduces one; re-check PROJECT.md at that point.)

### Phase 3 — Project Initialization

#### Step 5 — Verify the Rust project builds

`Cargo.toml` and `src/main.rs` already exist in this repository, so there is no scaffolding step equivalent to `npm init`. Confirm the toolchain works end-to-end:

```bash
cargo build
```

On Windows, if this fails with a linker error and there is no MSVC Build Tools installation, apply the GNU toolchain workaround from PROJECT.md (`rustup override set stable-x86_64-pc-windows-gnu` + WinLibs `bin` on `PATH`) and retry.

If `cargo audit` is not available yet, install it once:

```bash
cargo install cargo-audit
```

#### Step 6 — Final verification

Run the prerequisite check script one more time to confirm all tools are present, and verify the project builds, tests, and lints cleanly:

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

Present a final summary:

```text
✅ Rust (rustc/cargo) 1.8x.x
✅ Git v2.44.0
✅ GitHub CLI v2.50.0
✅ cargo build / cargo test / cargo clippy verified

You're all set! Try /plan-checklist to start planning, or /dev-tdd to start building.
```
