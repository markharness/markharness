# 0022: Remove the Stage 3 dashboard (`server.rs`, `ui/`)

## Status

Accepted (decided 2026-09-11, implemented 2026-09-12; see `checklist-v2-core.md`). This removes the Release Verification Dashboard built in [0008](0008-verification-plan-product-roadmap.md) Stage 3. The rest of [0008](0008-verification-plan-product-roadmap.md) — the record of Stages 0–2 and the modular-monolith stance — remains in effect.

## Background

[0008](0008-verification-plan-product-roadmap.md) Stage 3 shipped a localhost-only, read-only Release Verification Dashboard, Feature History, and frontend assets embedded in the Rust binary (`src/server.rs`, `ui/`, `markharness serve`, `tests/server.rs`).

The current UI (`ui/app.js`) is built around `plan` output — the evidence-applicability judgment. Because [0020](0020-execution-status-lightweight-model.md) reduces `plan.rs`'s evidence-matching logic to a simple "does an `ExecutionStatus` exist" lookup, the very content the dashboard displays disappears. Keeping the dashboard would mean rebuilding it around Change Impact / Release Coverage.

Meanwhile, [markharness-v2-design.md](../design/markharness-v2-design.md) §8 limits MVP output to CLI/JSON and delegates views to a separate tool. Rebuilding the dashboard would mean carrying permanent UI maintenance cost that does not advance the MVP thesis (the four North Star questions).

## Decision

Delete `src/server.rs`, `ui/`, the embedded frontend assets, the `markharness serve` command, and the related tests (`tests/server.rs` and friends).

The removal happens at the same implementation step as the `plan.rs` reduction driven by [0020](0020-execution-status-lightweight-model.md). Where a view is wanted, a separate tool reads the CLI/JSON output (`impact`, `coverage`).

## Consequences

- `markharness serve` is gone; the corresponding sections of `docs/ja/cli-manual.md` and `docs/en/cli-manual.md` are removed at the same time.
- A viewer outside this repository that reads `plan` output has to switch to Change Impact / Release Coverage output.
- A pointer to this ADR is added to [0008](0008-verification-plan-product-roadmap.md)'s status line; that ADR's body is left as the historical record.

## Options considered and rejected

- **Rebuild it for Change Impact / Release Coverage**: a view is nice to have, but the v2 MVP satisfies its thesis with CLI/JSON alone. Designing the UI once a concrete demand exists — against the output contract of that time — is the better trade ([CLAUDE.md](../../../CLAUDE.md), YAGNI).
- **Leave it in place**: it breaks (build or display) the moment `plan` is reduced, leaving broken code and tests behind. Deferring the decision buys nothing.
