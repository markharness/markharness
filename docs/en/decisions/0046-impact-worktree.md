# 0046: `impact --worktree` compares HEAD with uncommitted working-tree changes

## Status

Accepted (decided 2026-10-12). Replaces the part of decision 4 in [0033](0033-traceability-defaults-to-working-tree.md) that excludes `impact` from working-tree support (`coverage` stays excluded).

## Background

`impact` resolves `--base` and `--head` as Git refs and reads Requirements, Features and TestCases from those refs' trees. Checking "which requirements and TestCases does this change affect" while still editing a requirement or Feature therefore required committing first. Decision 4 of [0033](0033-traceability-defaults-to-working-tree.md) excluded `impact` because "no motivation to mix uncommitted state into a two-point comparison has been confirmed (YAGNI)". The need to check impact before committing (Issue #129) is that motivation.

## Decision

### 1. Add `impact --worktree`

`--worktree` analyses impact from the uncommitted changes between HEAD and the working tree, with HEAD as the base. It cannot be combined with `--base`, `--head` or `--fail-on-findings` (clap `conflicts_with`). The base is fixed to HEAD because comparing against any other ref is already possible by committing and using `--base`/`--head`; "any ref to the working tree" has no confirmed motivation and is not handled (YAGNI).

### 2. The target is the current content of tracked files

Staged and unstaged changes are not distinguished: the target is the current working-tree content of every tracked file, deletions included. A new file that was `git add`ed but not committed counts as tracked. Untracked files are out of scope and appear in the result once `git add`ed. This limit is stated in the cli-manual and the command help.

### 3. The working tree is read as a tree object through a temporary index

A copy of the real index is pointed to by `GIT_INDEX_FILE`, then `git add -u` and `git write-tree` run to obtain the tree OID that corresponds to the working tree. That OID is passed unchanged to the existing ref-based code (`ls-tree`, blob-SHA comparison, the `id_cache` tree-SHA comparison, `GitTreeKnowledgeSource`), so differences and identifiers mean exactly what they mean between commits. The real index and HEAD are not modified.

The side effect is that the blobs `git add -u` writes accumulate in the object database. Files identical to HEAD hash to existing blobs and add nothing; only files being edited add blobs. The estimate is tens to hundreds of KB per run, and unreferenced blobs are reclaimed by `git gc`. This was judged negligible.

### 4. The result is provisional and is distinguished by `record_kind`

`record_kind` is `change_impact_worktree`. `schema_version` stays fixed at `1` for every kind and does not branch by version ([markharness-v2-design.md](../design/markharness-v2-design.md) §9.2.2). The fields are identical to `change_impact`, so consumers can read it with the same parser. Only these values differ:

- `base_commit`: the HEAD commit OID.
- `head_commit`: the fixed string `"worktree"`. It is not a commit OID; no commit corresponds to the working tree, so it does not pretend to be one.
- `rejected_trailers`: always an empty array.

Because `record_kind` differs, a consumer written for `change_impact` cannot silently ingest a provisional result as an official one. The working tree keeps changing, so the commit-to-commit property that the same input yields the same result (AC37) does not hold for this result.

### 5. `Spec-Reviewed` is not counted

No commits lie in the range `HEAD..working tree`, so no `Spec-Reviewed` trailer can be counted. The verdict is `followed_up` or `unconfirmed`; `confirmed` never appears. `--fail-on-findings` would therefore always exit 2 whenever a requirement is touched, so the combination is rejected (decision 1).

### 6. No history is walked, so a shallow clone works

Only the HEAD commit is used and no commit range is walked, so the shallow-clone check is not applied. A repository with no HEAD (before the first commit) exits with an error, since there is no base to compare against.

## Impact

- `src/impact.rs`: add `compute_worktree`; extract the analysis shared with `compute` as `analyze`
- `src/git.rs`: add `write_tree_of_tracked_worktree`, a temporary-index tree builder limited to tracked files
- `src/cli.rs`: add `--worktree` to `impact`; `--base` and `--head` are not required with `--worktree`
- `tests/impact_cli.rs`: tests for staged, unstaged, untracked, real index unchanged, rejected combinations, shallow clone and no HEAD
- `docs/ja/cli-manual.md`/`docs/en/cli-manual.md` (§1.13), `docs/ja/design/cli-read-model-design.md`/`docs/en/design/cli-read-model-design.md`, `docs/ja/design/markharness-v2-design.md`/`docs/en/design/markharness-v2-design.md` (the `record_kind` list in §9.2.2)

## Alternatives considered

- **Keep the status quo (commit, then run `impact`)**: means making a temporary commit and undoing it, which either pollutes history or needs cleanup.
- **Read the file system directly, like `WorkingTreeKnowledgeSource`**: the `id_cache` tree-SHA comparison, `changed_between` and `blob_sha_at` that `impact` uses would all need working-tree rewrites, and what a difference means could drift from the commit-to-commit meaning.
- **Include untracked files**: needs a decision on which files to ignore and consistency with `.gitignore`, which exceeds the initial scope. Handle it separately when needed.
- **Staged changes only**: the result would change depending on whether `git add` ran.
- **Keep `record_kind` and add a `provisional` field**: a consumer that ignores the field would treat a provisional result as an official one. It also conflicts with the "do not add possibly useful future fields as optional" rule in [markharness-v2-design.md](../design/markharness-v2-design.md) §9.2.2.
- **A separate struct without `head_commit`**: splitting the type for the same content gives consumers two parsers.
- **Redefine `--fail-on-findings` and allow it**: no motivation to gate CI on a provisional result has been confirmed.
- **Substitute `changes compute --current-tree`**: its unit is the Feature and it has no requirement mapping or verdict.
