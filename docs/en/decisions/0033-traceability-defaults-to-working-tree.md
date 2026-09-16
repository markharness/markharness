# 0033: `traceability` defaults to reading the working tree

## Status

Accepted (decided 2026-09-16). Extends the input source for the `traceability` command that [0032](0032-cli-read-model-seam.md) introduced.

## Background

[0032](0032-cli-read-model-seam.md) aligned `traceability`, `impact`, and `coverage` on reading only committed content at a given Git ref. The implemented `traceability --at <ref>` (defaulting to `HEAD`) reads via `GitTreeKnowledgeSource`, from Git objects — so it does not work until the target ref has at least one commit (on a freshly initialized, not-yet-committed repository, `HEAD` doesn't resolve and `git ls-tree` fails).

This turned out to be impractical for a real `markharness-view` use case: a GUI or view that wants to show relations live while the user is editing Knowledge cannot demand a commit after every change — that breaks the editing loop entirely.

`impact` and `coverage`, by contrast, have no motivation to drop this constraint.

- `impact` exists to compare two points, `base..head`; both sides have to already be committed for a comparison to mean anything.
- `coverage` exists for release auditing ("what was there to verify as of this commit"); mixing in uncommitted working-tree state would break reproducibility (design principle P3).

`traceability` has neither a two-point comparison nor an auditing requirement — it is simply "show me Knowledge's current relations." `generate` and `verify` already read the working tree directly via `WorkingTreeKnowledgeSource`; there is no inherent reason `traceability` alone has to require a Git ref.

## Decision

### 1. `--at` becomes optional; omitting it reads the working tree

When `markharness traceability` is run without `--at <ref>`, it uses `WorkingTreeKnowledgeSource` (the same path `generate` and `verify` already use) instead of `GitTreeKnowledgeSource`, reading the working tree's current content as-is. When `--at <ref>` is given, it reads that Git ref exactly as before, per [0032](0032-cli-read-model-seam.md) decision 1.

The default changes from `HEAD` to "omitted" (no `default_value` on `--at`). "Omit it to read the working tree" matches the intuition `generate` and `verify` already establish.

### 2. The output's `at` field distinguishes which source was read

When the working tree was read, `at` holds the fixed string `"working-tree"`. When a Git ref was given, `at` holds that string exactly as the caller passed it, as before. This lets a reader tell which source produced a given result from the output alone.

`"working-tree"` plays the same kind of role `record_kind` does — an identifier that prevents mistaking one kind of record for another — and is treated as a reserved value that doesn't collide with a Git ref name (the same practical risk already exists for `HEAD` and is accepted the same way).

### 3. Add a working-tree path for reading Requirements and Features

The current `requirements_at`/`features_at` (`src/traceability.rs`) only read via Git objects (`git::ls_tree_recursive`/`git::show_blob_by_sha`). Supporting the working tree needs an equivalent that reads directly from the filesystem. The concrete shape of that (separate functions, or an abstraction similar to `KnowledgeSource`) is left to the implementation phase, not decided here.

### 4. `impact` and `coverage` are out of scope

Extending working-tree support to either command has no confirmed concrete need today (YAGNI). If one appears later, it gets its own ADR at that point.

### 5. The output contract (`record_kind`, `schema_version`, field layout) is unchanged

This ADR only adds an input-source option; it does not touch `TraceabilityReadModel`'s field layout.

## Consequences

- `src/traceability.rs`: `compute`'s signature changes (`git_ref: &str` becomes optional), a working-tree implementation is added for `requirements_at`/`features_at`, and the logic for the `at` field's value is added.
- `src/cli.rs`: the `Traceability` command's `at` argument becomes `Option<String>`, dropping `default_value = "HEAD"`.
- `docs/ja/design/cli-read-model-design.md`, `docs/en/design/cli-read-model-design.md`: update the `--at` description and command-mapping table for `traceability` (already updated to match this ADR).
- `docs/ja/cli-manual.md`, `docs/en/cli-manual.md` (§1.25): document the omitted-`--at` behavior (to be reflected at implementation time).
- **No change needed**: `src/impact.rs`, `src/coverage.rs`, or either command's CLI argument definitions.

## Options considered and rejected

- **Keep the status quo (always require a commit)**: does not solve the actual pain point — `markharness-view`'s live preview while editing.
- **Add an explicit value like `--at working-tree`**: defaulting to the working tree when `--at` is omitted matches the intuition `generate`/`verify` already establish. An explicit special value only adds something for users to remember about what happens when `--at` is left out, with no concrete benefit.
- **Extend working-tree support to `impact` and `coverage` too**: neither command's purpose (two-point comparison, release auditing) has a motivation for mixing in uncommitted state, and no concrete need has been confirmed (YAGNI).
