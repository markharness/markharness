---
description: "Use for every repository task that may edit files, create commits, push a branch, or open a pull request. Defines the required GitHub Flow from branch creation through PR handoff."
applyTo: "**"
---

# GitHub Flow

Repository changes use GitHub Flow. Complete the applicable steps in order.

## 1. Establish a safe branch

Before the first file mutation, inspect `git status --short` and `git branch --show-current`.

- On `main`, create and switch to a short-lived branch before editing: `feature/<topic>`, `fix/<topic>`, `docs/<topic>`, `refactor/<topic>`, or `chore/<topic>`.
- If `main` already has uncommitted changes, preserve them and switch them onto the new branch with `git switch -c <branch>` before further edits.
- On an existing non-main branch, continue there when its purpose matches the task. Create a new branch when it does not.
- In detached HEAD state, create a named branch before editing.

This step is complete when `git branch --show-current` reports a task-appropriate branch other than `main`.

## 2. Implement and verify

Keep the branch scoped to one reviewable purpose. Follow the repository's TDD and Pre-PR checks. Preserve unrelated user changes and include only task-related files in commits.

This step is complete when the requested outcome is implemented and every required local check passes.

## 3. Commit locally

Create commits only when the user requests a commit or the requested workflow explicitly includes commits. Use Conventional Commits with a Japanese subject. Never commit directly to `main`; the tracked pre-commit hook enforces this locally.

This step is complete when the requested commits exist on the working branch and the worktree has no unintended changes.

## 4. Publish and review

Treat push, pull-request creation, and merge as separate externally visible actions:

- Push the working branch only when the user explicitly requests it.
- Open or update a pull request only when the user explicitly requests it. Target `main` and summarize the outcome and verification evidence.
- Merge only when the user explicitly requests it and required checks/review are satisfied.
- Delete the branch only after merge and only when requested or clearly included in the merge workflow.

This step is complete at the boundary the user authorized. Report the branch name, local commits, checks, and any remaining push/PR/merge action.
