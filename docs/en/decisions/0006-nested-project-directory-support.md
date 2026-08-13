# 0006: Placement of the project directory within a repository (formal support for subdirectory placement)

## Status

Accepted

## Background

A defect was found where, in an external project (`todo2`, in which the app itself sits at the repository root and markharness's `knowledge/` etc. is initialized under `docs/` via `markharness init --dir docs`), running `markharness execution record` failed in a layout where markharness's project directory (`--dir`) does not coincide with the root of the git repository (i.e., is placed in a subdirectory within the repository). This is a primary-source case demonstrating that real users do attempt this layout in practice, and before fixing it, a design decision was needed on whether this layout should be formally supported at all.

## Options considered

1. Require that the project directory always be placed at the repository root (document it as a constraint only; no code change required).
2. Formally support placement in any subdirectory within the repository (requires a bug fix).

## Decision

Option 2 was adopted. The project directory (the parent of `knowledge/` etc., the `root` specified via `--dir`/cwd) need not coincide with the root of the git repository.

**Rationale**:

- The `todo2` primary-source case already demands this layout in actual operation.
- `changes compute` is designed to compare only the tree SHA of the subtree under `knowledge/` between milestones, while the milestone itself (a `git tag`) is a global concept for the entire repository. It is natural, and consistent with subdirectory placement, for the product's own release tags to be reused directly as milestones while markharness looks only at the subtree it cares about.
- Forcing "always at the repository root" would either (a) impose the operational burden of preparing a separate repository dedicated to markharness and keeping it synced with the product repository's tags, or (b) require placing `knowledge/`/`generated/` etc. directly under the product's source tree — both unnatural as repository structures. No structural downside on the subdirectory-placement side was found that offsets this disadvantage.

## Response taken

- Cause: The Git `<rev>:<path>` syntax used by `tree_sha`/`show_blob` in `src/git.rs` is always interpreted as a path relative to the repository root unless it begins with `./` or `../` (changing the current directory via `git -C <root>` does not affect this interpretation rule). Meanwhile, the caller `id_cache::resolve_feature_versions` passed paths obtained via `git ls-tree`'s pathspec syntax (correctly resolved relative to `-C root`) directly into `<rev>:<path>`, causing paths to become misaligned and fail when `root` was a subdirectory of the repository.
- Fix: `tree_sha` was rewritten from `git rev-parse --verify <rev>:<path>` to a `git ls-tree`-based approach (a pathspec relative to `-C root`). The path-based `show_blob` was removed and replaced with `show_blob_by_sha` (`git cat-file -p`), which directly takes the content-addressed SHA already returned by `ls_tree_recursive`. Since a SHA does not go through path interpretation, it is unaffected by where `root` is located within the repository.
- The description of `--dir` in `docs/cli-manual.md` was corrected from "the target project directory (the root of the git repository)" to the effect of "any directory within a git repository (need not be the repository's own root)" (§1.11/1.12/1.13/1.14).
- Verification: It was confirmed that `execution record` and `changes compute` actually succeed for a subdirectory layout equivalent to `todo2`.
- Detailed reproduction steps, root-cause analysis, TDD implementation steps, and integration-verification records had been kept in `docs/nested-project-dir-git-path-fix-spec.md`, but with the response complete, its key points were transcribed into this decision record and the file was deleted.

## Issues left out of scope

- Adding the ability to automatically detect the git repository root from the current directory without specifying `--dir` at all (equivalent to `git rev-parse --show-toplevel`) is a natural next step to operationally complete this decision, but remains a separate future issue.
