# 0010: Automatic project-root detection when `--dir` is omitted

## Status

Accepted

## Background

Running a `markharness` command without `--dir` operated on the current working directory rather than the root of the `markharness init`-ed project (Issue #9). Running an auto-generating command such as `milestone` from a subdirectory of the project (e.g. under `knowledge/`) created files in the wrong place.

This was also the issue left out of scope in 0006: "adding automatic detection of the repository root from cwd (equivalent to `git rev-parse --show-toplevel`) is a natural next step, but remains a separate future issue." However, 0006 itself formally supports nested layouts where the markharness project root need not coincide with the git repository root, so git-root detection alone turned out to be insufficient.

## Options considered

Mechanisms for root detection:

1. Use the git repository root (`git rev-parse --show-toplevel`).
2. Infer the root by convention, walking upward for a place where the known directory set (`knowledge/` + `axes/` + `generated/` etc.) is present.
3. Create a dedicated marker file at `init` time, and have subsequent commands search upward for it the way `git` does.

## Decision

Option 3 was adopted.

- `markharness init` creates `.markharness.toml` at the project root (minimal content: just `schema_version = 1`). If it already exists, it is left untouched (for idempotency on re-init and to preserve any customization). Re-running `markharness init` on an existing project that predates this marker simply adds the marker.
- `.markharness.toml` is committed to the repository (not added to `.gitignore`), since the location of the project root is a structural fact the whole team should share.
- Every subcommand other than `init` searches upward from the current directory for `.markharness.toml` when `--dir` is omitted, and uses the nearest ancestor that has one (the innermost project, for nested layouts) as the root. If none is found anywhere upward, the command exits with an error that points the user at `markharness init`.
- An explicit `--dir` bypasses the search entirely and is used literally as the root (kept as an escape hatch for scripting and for `init` itself).
- `init` itself is excluded from this auto-detection and always operates directly on cwd (or `--dir`), so as not to conflict with 0006's nested-project support.

**Rationale**:

- Git-root detection (option 1) cannot correctly handle the nested layout (markharness root ≠ git root) that 0006 formally supports.
- Inferring from directory conventions (option 2) is ambiguous — it can misfire on directories that merely happen to share those names — and breaks the detection logic entirely if the layout ever changes.
- A marker file (option 3) is the same well-understood discovery mechanism used by `git`/`npm`/`cargo`, is unambiguous, and the `schema_version` field doubles as a place to hang future layout-compatibility checks.

The `schema_version` field is written but not yet read for compatibility checking; that consumer-side logic is deferred until actually needed (YAGNI).

## Response taken

- `src/project_root.rs`: added `find_root` (a pure upward-search function) and `resolve` (the CLI-facing function combining the `--dir` override, the search, and the error message).
- `src/init.rs`: added `ensure_project_root_marker`, which creates `.markharness.toml` (and leaves it alone if it already exists).
- `src/cli.rs`: replaced `None => env::current_dir()?` with `project_root::resolve(dir, &env::current_dir()?)?` in every subcommand except `init`.
- Updated the existing integration tests (`tests/import_cli.rs`, `tests/plan_cli.rs`) that had implicitly relied on cwd without `--dir`, adding an explicit `--dir` so they are unaffected by the new search behavior.
- Added `tests/project_root_cli.rs`, a new integration test verifying automatic resolution from a nested subdirectory and the error message shown when no project is found.
