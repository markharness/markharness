# 0032: Introduce a CLI read-model seam

## Status

Accepted (decided 2026-09-16).

## Background

`markharness` v2 leaves Change Impact and Release Coverage views/dashboards to a separate repository, `markharness-view` (working name). To support that split, `docs/en/design/cli-read-model-design.md` (Draft) proposed an external read contract (a "read model") for the CLI, assuming a new `record_kind`/`schema_version` envelope and new `CommandOutcome` variants.

A grilling session that inspected the actual implementation (`src/`) found this premise wrong. Two independent output paths already exist.

1. **The `CommandOutcome`/`Presenter` path** (`src/presentation.rs`): represents the result of three **write** commands — `canonical_imported`, `generated`, and `changes_computed` (the result of `markharness changes compute`, which writes to `.markharness/changes/`). `JsonPresenter` has emitted the flat shape `{"schema_version": 1, "outcome": "<tag>", ...fields}` since v0.6.1.
2. **The per-command-module path** (`src/impact.rs`, `src/coverage.rs`): the **read-only** commands `impact` and `coverage` never go through `CommandOutcome`/`Presenter` at all. Each module owns its own struct (`ChangeImpact`, `ReleaseCoverage`) that already carries `schema_version` and `record_kind` (values `"change_impact"` and `"release_coverage"` respectively), serialized directly from `cli.rs` via `serde_json::to_string_pretty`. `--format` currently has only one value, `Json`; human-readable output is not implemented.

In other words, what the draft set out to design — a read model carrying `record_kind`/`schema_version`, produced in the Application layer and handed to the JSON Presenter — is **already implemented** for `impact`/`coverage`. And `record_kind` itself is not a new name the draft invented; it's the existing convention `markharness-v2-design.md` §9.1 and [0025](0025-v2-forward-compatible-evolution.md) already established for persistent records under `.markharness/`, and `impact`/`coverage`'s CLI output already follows it.

The only genuinely new thing needed is a `traceability` command (not yet implemented) that follows this same existing pattern.

There is a further wrinkle: a file named `src/traceability.rs` already exists, implementing `TraceabilityIndex`/`build_index`/`serialize_index`. This is a **generation artifact**: `generate` builds it and writes it to `.markharness/generated/traceability-index.json` (`application.rs`), and `verify` uses it to check regeneration is deterministic (`verify.rs`). It's a flat, per-TestCase list (`case_id`, `requirement_ids`, `feature`, `behavior`, `scenario`, `axis`), carries no `record_kind`/`schema_version`, and is not something `markharness-view` reads. The new `TraceabilityReadModel` (Node arrays per entity type plus Relations, computed on demand for a given `--at` and returned over stdout as a read-only query result) differs from it in every respect that matters, including whether it's a persisted artifact or a query result. Reusing the same file name for two different responsibilities would leave readers unsure which module a reference to "traceability" means, so this ADR resolves the naming collision.

## Decision

### 1. `traceability` follows the same output pattern as `impact`/`coverage`

The new `traceability` command does not go through `CommandOutcome`/`Presenter`. Like `impact.rs`/`coverage.rs`, it gets its own module defining a `TraceabilityReadModel` struct with `schema_version: u32` and `record_kind: &'static str` (value `"traceability"`), serialized directly from `cli.rs` via `serde_json`.

No new `CommandOutcome` variants (`ImpactComputed`, `CoverageComputed`, `TraceabilityComputed`) are added. `impact` and `coverage` never used `CommandOutcome` to begin with, and `traceability` matches that.

### 2. Rename the existing `src/traceability.rs` to `src/traceability_index.rs`; reuse `src/traceability.rs` for the new CLI command

`TraceabilityIndex`/`build_index`/`serialize_index` (the `generate`-owned artifact) move to `src/traceability_index.rs`. Callers (`application.rs`, `verify.rs`) update their `use` paths accordingly. The type names, function names, and the artifact's on-disk name (`.markharness/generated/traceability-index.json`) are unchanged — the artifact's file name is an existing external detail independent of the Rust module name, and there's no reason to touch it.

The now-free `src/traceability.rs` becomes the module for the new `traceability` CLI command (`TraceabilityReadModel`). This keeps the "module name matches its command" pattern `impact.rs`/`impact` and `coverage.rs`/`coverage` already establish, extended to `traceability`.

This rename is a breaking change to the public Rust API path `markharness::traceability::TraceabilityIndex` (`src/lib.rs` makes every module `pub`, including `pub mod traceability;`), which becomes `markharness::traceability_index::TraceabilityIndex`. This crate's `pub mod` set exists so the integration tests under `tests/` can reach crate internals; it is not offered or guaranteed as a stable API for external library consumers (`Cargo.toml`'s `description` reads "Git-native test knowledge management CLI" — use as a library is not a design goal). No `pub use` re-export is added to keep the old path working. CLAUDE.md's "no backward-compatibility designs" policy applies here too, to the Rust public API.

### 3. `impact` and `coverage`'s output contracts are unchanged

`ChangeImpact` and `ReleaseCoverage`, `record_kind`/`schema_version` included, are already in reasonable shape as an external contract. The draft's proposed `ChangeImpactReadModel`/`ReleaseCoverageReadModel` are reinterpreted as referring to the existing `ChangeImpact`/`ReleaseCoverage`. No breaking change to their existing fields is made within this ADR's scope.

### 4. The `CommandOutcome`/`Presenter` path (the `outcome` field) is out of scope for the read models

`canonical_imported`, `generated`, and `changes_computed` are results of write commands, unrelated to what `markharness-view` reads (`traceability`/`impact`/`coverage`). Unifying their `outcome` field to `record_kind` has no concrete motivation and is out of this ADR's scope (YAGNI). If a concrete need arises later to tidy up write commands' output as an external contract, that gets its own ADR.

### 5. This ADR's scope is limited to "add `traceability` following the existing read-command pattern"

`TraceabilityReadModel`'s detailed field layout (per-type Node arrays, how Relations are represented, etc.) is not decided by this ADR; it remains the responsibility of `docs/en/design/cli-read-model-design.md` (the design doc).

### 6. This ADR records a design decision; implementation is separate

The `src/traceability.rs` rename, its new implementation, and wiring `traceability` into `cli.rs` are out of scope for this ADR. They are a separate implementation task, done with TDD (Red-Green-Refactor).

## Consequences

- The existing `src/traceability.rs` (`TraceabilityIndex`/`build_index`/`serialize_index`) is renamed to `src/traceability_index.rs`; type names, function names, and the `.markharness/generated/traceability-index.json` output file name are unchanged.
- `src/application.rs`, `src/verify.rs` — update `use crate::traceability::...` paths to match the rename.
- `src/lib.rs` — change `pub mod traceability;` to `pub mod traceability_index;`, and add a new `pub mod traceability;` for the new command's module. A breaking change to the public Rust module path; no re-export is added (see decision 2).
- New, at the `src/traceability.rs` name freed up by the rename: the `TraceabilityReadModel` struct and its generation logic, for the new CLI command.
- Changes: `src/cli.rs` — add the `traceability` command, wired the same way as `impact`/`coverage` (direct `serde_json` serialization).
- Changes: `docs/ja/design/cli-read-model-design.md`, `docs/en/design/cli-read-model-design.md` — document `impact`/`coverage`'s existing implementation, that it is unrelated to `CommandOutcome`, and the `src/traceability.rs` rename. **Only these two files are already updated as part of this ADR** (both are new, not-yet-tracked files).

**Not yet done (deferred to the implementation phase)**: the following existing, git-tracked documents still reference `src/traceability.rs` as of this ADR, deliberately left unchanged. The rename in decision 2 has not landed in code yet — `src/traceability.rs` still holds `TraceabilityIndex` today — so rewriting them to say `src/traceability_index.rs` now would describe a file that does not yet exist. They are updated in the same commit that lands the actual Rust rename (decision 6).

- `docs/ja/design/markharness-v2-design.md`, `docs/en/design/markharness-v2-design.md` (§5.1, the implementation-status table)
- `docs/ja/design/testcase-generation-design.md`, `docs/en/design/testcase-generation-design.md` (the opening Status line, §3.4)
- `docs/ja/cli-manual.md`, `docs/en/cli-manual.md` (§3, the list of modules `cargo test` covers — also document the new `traceability` command here at the same time)
- **No change needed**: `src/presentation.rs` (`CommandOutcome`, `Presenter`), the output structs in `src/impact.rs`/`src/coverage.rs`, or the format/content of `.markharness/generated/traceability-index.json`.
- Future: `schema/` — when `schema/traceability-read-model.schema.json` etc. are added, they follow the `record_kind` naming already used by `impact`/`coverage`.

## Options considered and rejected

- **Add `ImpactComputed`/`CoverageComputed`/`TraceabilityComputed` to `CommandOutcome`**: the draft's original premise. Checking the implementation showed `impact`/`coverage` never used `CommandOutcome` at all, so the premise itself was wrong. There is no concrete reason to fold an already-working, independent output path into `CommandOutcome`.
- **Also rename `canonical_imported`/`generated`/`changes_computed`'s `outcome` field to `record_kind`**: considered once, as a way to unify naming across every command. But none of these are read by `markharness-view`, so the rename has no concrete motivation (YAGNI). If write commands' output contract ever needs tidying, that's a separate ADR.
- **Nest `record_kind`/`schema_version` in a new object shape**: no reason to invent a different shape from the flat one `impact`/`coverage` already use.
- **Put `TraceabilityReadModel` in the same file as the existing `TraceabilityIndex` (`src/traceability.rs`)**: keeps the file count down, but mixes two different responsibilities and lifecycles — a `generate`-owned artifact and an on-demand read-only query result for `markharness-view` — in one file, leaving readers unsure which one a given reference means.
- **Give the new CLI read model a different name (e.g. `traceability_read_model.rs`) and leave the existing `TraceabilityIndex` at `src/traceability.rs`**: breaks the naming symmetry `impact.rs`/`impact` and `coverage.rs`/`coverage` already establish, for `traceability` alone. The existing `TraceabilityIndex` is an internal detail of `generate`, not something that represents the `traceability` command name — so it makes more sense for the new command to keep that name.
