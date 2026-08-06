---
description: "Use before running any destructive/irreversible command — not just git (reset --hard, clean -f, push --force) but also scaffolding/CLI tools with force-overwrite flags (e.g. tauri init --force, rm -rf, DROP TABLE) — and when recovering after one has already run. Defines a pattern-based detection rule plus step-by-step recovery."
applyTo: "**/*"
---

# Destructive Command Safety — Prevention & Recovery

> Respond in the language the user is using in the chat.

This instruction defines what to do **before** a destructive command runs, and what to do **after** one has already caused damage. Other instructions in this repo (e.g. [security](./security.instructions.md)) tell you to be careful; this file is the recovery plan for when carefulness wasn't enough.

**Do not rely on a fixed list of command names.** A list like "`git reset --hard`, `rm -rf`" always has gaps — e.g. `tauri init --force` (or `-f`) silently overwrites the entire `src-tauri` folder with no fixed-list-based rule catching it, because it isn't a git or `rm` command at all. Instead, detect destructiveness by **pattern**, then apply the same prevention/recovery discipline regardless of which tool it is.

## Detecting a destructive command (pattern, not enumeration)

Treat a command as destructive if it matches any of these patterns:

1. **Force/overwrite flags**: `--force`, `-f`, `--overwrite`, `--yes`/`-y` (when it skips a confirmation prompt), `--hard`, `--clean`.
2. **Deletion/reset verbs**: `reset`, `clean`, `delete`, `drop`, `destroy`, `prune`, `purge`, `wipe`, `truncate`, `remove`.
3. **Scaffolding/init tools run against a non-empty directory**: `tauri init`, `create-react-app`, `npm create`/`npm init`, `rails new`, `ng new`, `django-admin startproject`, etc. — these normally *refuse* to run into an existing directory, and the risk is precisely the flag that bypasses that refusal (see #1).
4. **Anything the tool's own `--help` describes as overwriting, resetting, or deleting existing files/state** — when unsure, run `<command> --help` and read the flag descriptions before running it for real.

If a command matches, follow Prevention below regardless of whether it's git, a CLI scaffolder, a database migration tool, or a cloud CLI (`terraform destroy`, `kubectl delete`, `firebase deploy --force`, `prisma migrate reset`, etc.).

## Prevention (before any destructive action)

1. Run `git status` first. If there are uncommitted or untracked changes in the affected path, commit them or stash them (`git stash push -u`) before proceeding — this is what makes the action recoverable afterward.
2. Always confirm with the user before running a command matching the patterns above, and say specifically what will be overwritten/deleted (e.g. "this will overwrite `src-tauri/tauri.conf.json` and `src-tauri/icons/` without prompting").
3. Prefer a reversible step over an irreversible one when it achieves the same goal: run the scaffolder into a fresh empty temp directory and diff/copy the result in by hand, instead of `--force`-ing it over the real project; rename/move a file instead of deleting it; `git stash` instead of `git reset --hard`.

## Recovery Procedures

### Lost commits (bad `reset --hard`, rebase, amend, or a deleted branch)

```bash
git reflog                     # find the commit hash from before the destructive op
git branch recovered <hash>    # restore it onto a new branch, or:
git reset --hard <hash>        # if you're sure you want to move the current branch back
```

`git reflog` records where `HEAD` and branches pointed, even after history-rewriting commands. Entries expire (default ~90 days via `gc.reflogExpire`), so check this first — before running any more git commands that might trigger `git gc`.

### Files overwritten in place by a non-git tool (e.g. `tauri init --force`, a scaffolder re-run)

Unlike `rm -rf`, an in-place overwrite of a **tracked and committed** file is still fully recoverable, because git keeps the old blob regardless of what overwrote the working-tree copy:

```bash
git status              # see exactly which tracked files changed
git diff                # review what the tool overwrote
git restore <path>       # or: git checkout -- <path>   (restores the pre-overwrite version)
```

If the file was untracked or never committed before the tool ran, treat it like the "discarded uncommitted changes" case below — there is no git-based recovery.

### Discarded uncommitted changes (`checkout --`, `restore`, `clean -f`, or `reset --hard` with local edits)

These were never committed, so `git reflog` cannot help. Check, in order:

1. Editor local history — in VS Code: `File > Local History` (per-file, kept automatically even without an extension).
2. `git fsck --no-reflog --unreachable --dangling` — recovers dangling blobs **only** if the file was ever `git add`-ed at some point (even briefly), including via an editor auto-stage.
3. OS-level backups (Windows File History / Previous Versions, Recycle Bin for deletions done via Explorer).

If none of these apply, the content is genuinely gone — say so plainly rather than guessing.

### Force-pushed / overwritten remote branch

```bash
git fetch origin <old-sha>     # works if the old commit isn't garbage-collected yet
```

Old commits referenced in a still-open PR's timeline usually remain fetchable by SHA even after a force-push. If a teammate has an older local clone with the branch, they can push the old ref back as a last resort.

### Deleted branch

```bash
git reflog | grep <branch-name>
git branch <branch-name> <hash-from-reflog>
```

### Deleted untracked files (`rm -rf`, `git clean -f`, `git clean -fd`, tool overwrite of never-committed files)

Untracked files have no git safety net. Check the Recycle Bin first, then OS-level backup tools. There is no git-based recovery.

## After Recovery

- Verify the recovered state matches expectations (`git log`, `git diff`, `git status`) before continuing any work.
- If the destructive command ran during an agent session, tell the user exactly which files/commits were affected so they can double-check before trusting the recovery.
