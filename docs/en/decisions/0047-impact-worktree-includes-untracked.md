# 0047: `impact --worktree` includes untracked files and sees the same changes a commit would

## Status

Accepted (decided 2026-10-12). Replaces decision 2 of [0046](0046-impact-worktree.md) (untracked files are out of scope) and the way decision 3 builds the temporary index (`git add -u`). The other decisions in 0046 are unchanged.

## Background

`impact --worktree` in [0046](0046-impact-worktree.md) targeted tracked files only. Right after creating a new requirement or Feature under `knowledge/`, its impact could not be checked without `git add` (Issue #131). What the author wants to know is "how would this look if I committed now"; a new file being visible or invisible depending on `git add` does not match that expectation. 0046 left "include untracked files" out of the initial scope because it required deciding how to stay consistent with `.gitignore`, but `git add -A` handles that consistency automatically, so there is nothing left to decide.

## Decision

### 1. Untracked files are included

The target is the tree the author would get by running `git add -A` and committing. Staged, unstaged and untracked files are not distinguished: the target is the current working-tree content of every file not ignored by `.gitignore`, deletions included. Deciding which files to ignore is left to `git add -A`; `markharness` carries no such logic. Files that should be ignored belong in `.gitignore`.

### 2. The scope is the whole repository

`git add -A` is applied to the whole repository, not only to `.markharness/knowledge`. This makes files outside `knowledge/`, such as the `.sdoc` an external requirement's `source_locator` points to, look the same as they would after a commit. Limiting the scope to `knowledge/` would make only those uncommitted changes look different.

### 3. The temporary index is built from a copy of the real index

A copy of the real index is pointed to by `GIT_INDEX_FILE`, then `git add -A` and `git write-tree` run. The index is not built from empty so that a file that is tracked yet matches `.gitignore` stays in the tree, as it does in a real commit. Of decision 3 in 0046 only `git add -u` becomes `git add -A`; everything else (passing the tree OID unchanged to the existing ref-based code, leaving the real index and HEAD unmodified) stays.

### 4. No guard against object-database writes

Blobs for untracked files are also written to the object database. A large file not covered by `.gitignore` (a build artifact, a video) is hashed and written on every run. No size limit, warning, or opt-in such as `--include-untracked` is added. `git add -A` behaves the same way, and no case of such files causing trouble has been reported (YAGNI). Instead, the cli-manual states the estimate and that files which should be ignored belong in `.gitignore`. Written blobs that end up unreferenced are reclaimed by `git gc`.

## Scope of change

- `src/git.rs`: rename `write_tree_of_tracked_worktree` to `write_tree_of_worktree` and change `git add -u` to `git add -A`
- `src/impact.rs`: update the call and the doc comment of `compute_worktree`
- `src/cli.rs`: remove "untracked files are out of scope" from the `--worktree` help
- `tests/impact_cli.rs`: invert the existing untracked test and add tests for `.gitignore`, a new Feature, an `.sdoc` outside `knowledge/`, and an unchanged real index
- `docs/ja/cli-manual.md` and `docs/en/cli-manual.md` (§1.13), `docs/ja/design/cli-read-model-design.md` and `docs/en/design/cli-read-model-design.md`, `docs/ja/design/markharness-v2-design.md` and `docs/en/design/markharness-v2-design.md` (§9.2.2)

## Alternatives considered and rejected

- **Keep the status quo (run `git add` first)**: every new file needs a `git add`, and the view does not match what a commit would show.
- **Apply `-A` to `.markharness/knowledge` only**: a smaller scope, but changes to files outside `knowledge/`, such as an external requirement's referenced document, would look different from how they would after a commit.
- **Read directly from the file system**: the `id_cache` tree-SHA comparison, `changed_between` and `blob_sha_at` would all need a working-tree version, which is a large change.
- **Build from an empty index with `git add -A`**: a file that is tracked yet matches `.gitignore` would drop out of the tree, diverging from a real commit.
- **Add an option to toggle whether untracked files are included**: no motivation to exclude them has been confirmed, and the meaning of the result would change from run to run (YAGNI).
