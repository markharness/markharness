# 0043: Make `traceability` working-tree only and drop the `--format` option

## Status

Accepted (decided 2026-10-06). Replaces [0033](./0033-traceability-defaults-to-working-tree.md)'s "`--at` is optional, and omitting it reads the working tree" (the decision to read the working tree itself still stands).

## Background

[0033](./0033-traceability-defaults-to-working-tree.md) kept `--at <ref>` on `traceability` and made omitting it read the working tree. No one has used `--at <ref>` since. The GUI reads only the working tree, and reads a specific committed point through `coverage --at`. The only use of `--at` left in the design doc is one line, "to check committed state".

Keeping `--at` costs:

- The Requirement-to-case link has to be kept on two read paths, one from a Git tree and one from the working tree.
- The Git tree path starts git once per blob, which is slow on Windows (Issue #112).

Also, `coverage`, `impact`, `traceability`, `import` and `release scope show` each have `--format`, but its only value is `json`, so there is nothing to choose. It states only that the output is always JSON, and no human-readable output is planned (`--json` on `generate`/`verify` switches to human text, which is a different meaning).

## Decision

### 1. Remove `--at` from `traceability` and `traceability show`

Both commands read the working tree only. To read a specific committed point, use `coverage --at`.

The output's `at` is removed too. With the working tree as the only source its value is always `"working-tree"` and carries no information (the purpose in [0033](./0033-traceability-defaults-to-working-tree.md) decision 2, telling the sources apart, is no longer needed). When `traceability show --uid` finds nothing, the error names only the `uid`.

`impact` and `coverage` keep `--base`/`--head` and `--at`, since comparing two points and auditing a release are their purpose (as in [0033](./0033-traceability-defaults-to-working-tree.md) decision 4).

### 2. Remove `--format` from `coverage`, `impact`, `traceability`, `import` and `release scope show`

The output is always JSON. When human-readable output is needed, an option of the needed shape is added at that point.

## Consequences

- `traceability --at`, `traceability show --at`, and `--format` on the five commands above are now unknown-argument errors from clap. If the GUI passes `--format json`, drop it.
- The `traceability` read path no longer reads Git trees.
- `at` disappears from the output of `traceability` and `traceability show`, and from their JSON Schemas. If the GUI reads `at`, drop it.

## Alternatives considered and rejected

- **Keep `--at`**: it keeps two read paths and a slow Git tree path with no one using them. Add it back, to fit the requirement then, if it is needed.
- **Keep `at` as the fixed value `"working-tree"`**: the value never changes and carries no information; it would stay only for backward compatibility.
- **Keep `--format`**: it would stay only for a future extension with a single value (YAGNI).
