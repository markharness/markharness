# 0011: Consolidate managed directories under a single `.markharness/` namespace

## Status

Accepted

## Background

`markharness init` created six top-level directories directly at the project root: `knowledge/`, `axes/`, `generated/`, `executions/`, `changes/`, `schema/`. These names are generic enough that an existing software project could already be using them for something unrelated (a domain `knowledge/` base, a `schema/` for a database or API, a `generated/` for compiled assets, etc.), and even without an outright collision, a bystander reading the repository's top level has no way to tell that these six directories are owned and managed by `markharness` rather than being ordinary project content.

This was raised as a concrete adoption concern for introducing `markharness` into a pre-existing repository: occupying six generic top-level names at once measurably raises the odds of a naming collision compared to occupying one clearly-scoped name.

## Options considered

1. Keep the current flat top-level layout (do nothing).
2. Consolidate the six directories under a single `.markharness/` namespace (`.markharness/knowledge/`, `.markharness/axes/`, ...).
3. Default to `tests/markharness/` (treating these as test-related assets, following the `tests/`/`test/` convention some ecosystems use).
4. Make every directory's location independently configurable via a `paths:` section in a config file, keeping the current flat layout as the default.

## Decision

Option 2 was adopted: `markharness init` now creates all six directories under `.markharness/`, and `.markharness/` is the single name `markharness` claims at the project root (alongside the pre-existing `.markharness.toml` project-root marker and `.markharness-cache/`, both already dot-prefixed).

**Rationale**:

- A dot-directory is a well-established convention for "this directory belongs to a specific tool," and is not, on its own, read as "this is local/uncommitted-only" — `.github/`, `.changeset/`, `.devcontainer/`, and `.husky/` are all common examples of dot-directories whose contents are committed and expected to be. `.changeset/` in particular is a close structural precedent: a tool-owned namespace holding human-authored, git-native, structured records that a later step processes into generated output — the same shape as `.markharness/knowledge/` feeding `.markharness/generated/`.
- Reducing six generic top-level names to one dot-prefixed name minimizes collision risk with an existing project's own `knowledge/`, `schema/`, etc., while keeping `init`'s output self-describing.
- Option 3 (`tests/markharness/`) was rejected: `knowledge/`, `axes/`, and `schema/` are not test code, and many test runners (pytest, Jest, ...) glob broadly under a `tests/`/`test/` directory by convention, risking accidental collection of markharness-managed files by unrelated tooling in a consumer project. It would also force a `tests/` convention on projects that don't already use one, contradicting `markharness`'s intent not to prescribe a directory convention outside its own namespace.
- Option 4 (fully configurable per-directory paths) was rejected as premature: there is currently no path-resolution abstraction to build on (each of the ~150 call sites joined a literal path segment directly), and none of the six directories are treated differently from one another today (all six, including `generated/` and `executions/`, are committed to git in this git-native model — only `.markharness-cache/` is gitignored). Introducing six independently configurable roots would be designing for a distinction (per-directory placement, per-directory git treatment) that doesn't exist in the tool's actual behavior yet. If a concrete need for configurable placement emerges later, it can be revisited then, informed by the `MARKHARNESS_DIR`/`KNOWLEDGE_PATH_IN_REPO` constants this change introduced as the single place such a knob would hook into.
- A "does `.markharness/` read as local-only" concern was raised and addressed by documentation rather than a new safety-check feature (a `markharness doctor`-style command that would, for example, warn if `.markharness/` is accidentally gitignored, was considered but deliberately deferred — YAGNI until there's a concrete report of the mistake happening in practice).

This is a breaking change with no migration path or backward-compatibility shim: a project previously initialized with the flat layout must move its six directories under `.markharness/` by hand (or re-run `markharness init` into a fresh location and re-apply knowledge). No automated migration command exists as of this decision.

## Response taken

- `src/project_root.rs`: added `MARKHARNESS_DIR` (`".markharness"`) and `KNOWLEDGE_PATH_IN_REPO` (`".markharness/knowledge"`, used wherever a git pathspec string rather than a `Path::join` is needed, e.g. `git ls-tree`/`git rev-parse <rev>:<path>` arguments) as the single constants the six directories' location is now built from.
- `src/init.rs`: `SUBDIRS` are now created under `root.join(MARKHARNESS_DIR)`; the `.gitignore` entries comment now explicitly notes that `.markharness/` itself (everything except `.markharness-cache/`) must not be added to `.gitignore`.
- Every other module that previously built a path like `root.join("knowledge")` now goes through `root.join(MARKHARNESS_DIR).join("knowledge")` (or the `KNOWLEDGE_PATH_IN_REPO` constant for git pathspec arguments).
- `src/fs_safety.rs`: `replace_dir_from_staging` now creates the target's parent directory before renaming into it, since a managed directory's parent (`.markharness/`) can no longer be assumed to already exist the way the project root itself was.
- User-facing CLI output (`src/cli.rs`, `src/presentation.rs`, `src/validate.rs`) was updated to reference `.markharness/...` paths.
- README.md / README.ja.md were updated, including a short note that everything under `.markharness/` except `.markharness-cache/` is meant to be committed.
