# 0012: Consolidate the project-root marker into `.markharness/config.toml` and validate it even when `--dir` is explicit

## Status

Accepted

## Background

ADR [0011](./0011-markharness-dot-directory-namespace.md) gave "`.markharness/` is the single name `markharness` claims at the project root" as one of its stated reasons. In practice, though, the project-root marker file, `.markharness.toml`, was left out of that decision and stayed as an independent file directly at the project root. As a result, an initialized project's root ends up with two dot-prefixed entries that share almost the same name, `.markharness.toml` and `.markharness/`, contradicting ADR 0011's own "single name" claim. Because the names are nearly identical, this reads at a glance like a stray duplicate file, and it doesn't match the common "marker + unrelated-named tool directory" separation pattern seen in `Cargo.toml`+`target/` or `package.json`+`node_modules/`.

Separately, `project_root::resolve()` never performed upward search (`find_root`) when `--dir` was given explicitly — it simply trusted the given path as the project root. It never verified the marker's presence, so passing a non-existent or uninitialized directory to `--dir` failed later with a confusing, generic filesystem I/O error buried in downstream code. This diverged from a common CLI convention such as `cargo --manifest-path`, which validates that the named manifest exists before trusting it.

## Options considered

For marker placement:

1. Keep the status quo: leave `.markharness.toml` as an independent file at the project root, justified as a `Cargo.toml`+`target/`-style separation.
2. Consolidate into `.markharness/config.toml`, and change root-marker detection to check for a file's presence inside the `.markharness/` namespace.

For `--dir` validation:

1. Keep the status quo: trust an explicit path without validation.
2. Follow the `cargo --manifest-path` convention: validate the marker's presence for an explicit `--dir` too, and exit with a `markharness init`-guidance error when it's missing.

## Decision

Option 2 was adopted for both marker placement and `--dir` validation.

**Rationale (marker placement)**:

- The `Cargo.toml`/`target/` and `package.json`/`node_modules/` separation patterns assume the marker and the tool-owned directory have unrelated names. `.markharness.toml` and `.markharness/` share almost the same name and don't fit that assumption. There's little reason to keep two entries with matching names at two separate locations, and consolidating them is more consistent with ADR 0011's own stated goal of a single occupied name.
- `.markharness/config.toml` lives inside the `.markharness/` namespace, so it carries none of the top-level generic-name collision risk that an independent, generically-named `config.toml` would. This is the same tradeoff ADR 0011 already accepted for the six directories (`knowledge/`, `schema/`, etc.).

**Rationale (`--dir` validation)**:

- `--dir`'s "no upward search, trust the given path as the exact target" behavior already matches the "explicit path = exact target" style of `cargo --manifest-path` and `npm --prefix`, but `cargo --manifest-path` validates the named file exists before using it. markharness's `--dir` skipping that validation was the actual point of divergence from convention, and it produced a confusing error when given a non-existent directory.
- Doing this alongside the marker relocation lets `resolve()`'s validation logic stay unified: both the upward search (`find_root`) and the explicit-`--dir` check now consult the same `MARKER_FILE` constant.

This is a breaking change with no migration path or backward-compatibility shim: a project that used `.markharness.toml` must move its content to `.markharness/config.toml` by hand. No automated migration command exists as of this decision.

Command paths that don't actually need a project root at all — e.g. `import --source junit`, which accepts `--dir` but never used the resolved root — were excluded from this validation (`src/cli.rs`). The point of validating `--dir` is to confirm a given path is a valid project root; applying that check to code paths that don't need a project root at all would go beyond this decision's actual intent (aligning with other CLIs' conventions).

## Response taken

- `src/project_root.rs`: changed `MARKER_FILE` from `.markharness.toml` to `.markharness/config.toml` (a string literal built from `MARKHARNESS_DIR`, composed manually since `Path::join` isn't usable in a `const` context; a test asserts the two stay in sync).
- `src/project_root.rs`: `resolve()` now validates `MARKER_FILE`'s presence for an explicit `--dir` too, returning the same `markharness init`-guidance `NotFound` error used when `find_root` fails.
- `src/init.rs`: `ensure_project_root_marker` now writes to `.markharness/config.toml` (by the time it runs, `run_init` has already created the `.markharness/` subdirectories, so the parent directory already exists).
- `src/cli.rs`: since `import --source junit` never uses the project root, the `project_root::resolve()` call was scoped to the `--source native` branch only (previously it ran unconditionally for the whole `Import` command, which — once marker validation was added — would have broken junit imports that don't need a project root at all).
- Test fixtures (`tests/knowledge_cli.rs`, `tests/plan_cli.rs`) that hand-assembled `.markharness/knowledge`, `axes`, etc. without going through `markharness init` were updated to also write the marker file — correctly simulating the already-initialized state a real project would have, rather than loosening the validation itself.
- Documentation (`docs/ja/cli-manual.md`, `docs/en/cli-manual.md`, `docs/ja/knowledge-from-code.standalone.md`) references to `.markharness.toml` were updated to `.markharness/config.toml`.
