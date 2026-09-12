# 0026: Module inventory, removal of `plan`, and the no-backward-compatibility rule

## Status

Accepted (decided 2026-09-11, implemented 2026-09-12; see `checklist-v2-core.md`). Settles the modules that [markharness-v2-design.md](../design/markharness-v2-design.md) §9 had deferred as "to be inventoried" or "decision needed", and states explicitly that the implementation of [0019](0019-alignment-check-commit-trailer.md)–[0025](0025-v2-forward-compatible-evolution.md) considers no backward compatibility at all.

## Context

[markharness-v2-design.md](../design/markharness-v2-design.md) §9 left `src/verify.rs`, `src/audit_scope.rs`, `src/derived_index.rs`, and `src/lineage.rs` "to be inventoried (unclassified in this document)", `src/milestone.rs` and `src/backfill.rs` as "decision needed", and `src/canonical.rs` as "revisit", to be settled at the start of M0.

In the pre-implementation grilling session on 2026-09-11, the actual call relationships of each module were measured. The measurement showed that some premises in the design document and in [0021](0021-identity-retire-simplification.md) do not match the code, and that removability differs sharply from module to module.

## Decision

### 1. Remove `plan`

Delete `src/plan.rs`, the `markharness plan` subcommand, the plan paths in `src/application.rs` and `src/presentation.rs` (`CommandOutcome::PlanBuilt`, `plan_exit_code`, `build_verification_plan_value`, etc.), and `tests/plan_domain.rs` / `tests/plan_cli.rs`.

Once [0020](0020-execution-status-lightweight-model.md) removed the strict evidence-applicability matching, the only function left in `plan` is reporting whether a TestCase has an `ExecutionBinding`. That is a subset of what `coverage` returns ([markharness-v2-design.md](../design/markharness-v2-design.md) §6.2), leaving two subcommands answering the same question. Removing it is smaller than reducing and keeping it.

### 2. Narrow `src/canonical.rs` to serving `import`

`markharness import` (native/JUnit) is kept. What [0020](0020-execution-status-lightweight-model.md) rejected was the storage and management of evidence, not the ingestion path for external test results. The plan-only types among `CanonicalEvidence`, `EvidenceResult`, and `RelationOriginKind` are deleted along with §1.

If StrictDoc ingestion is added later, it is designed as a separate Adapter, as [markharness-v2-design.md](../design/markharness-v2-design.md) §9 states. This ADR does not anticipate that design.

### 3. Remove `src/derived_index.rs` and `markharness cache index`

`derived_index.rs` takes `plan::BoundVersions` and `execution::read_all_results` as input. §1 deletes `plan.rs`, and `execution.rs` is replaced by [0025](0025-v2-forward-compatible-evolution.md)'s `ExecutionBinding`, so both inputs disappear. The derived indexes under `.markharness-cache/index/` appear nowhere in the Change Impact or Release Coverage computation, and no CLI integration test covers them.

### 4. Keep `src/lineage.rs` and `src/milestone.rs`

Both are used internally by `src/changes.rs`, which classifies merge-commit parentage through `lineage::classify` and calls `milestone::verify_audit_matches_tag` as a fail-closed gate. `changes.rs` is kept as the basis of Change Impact ([markharness-v2-design.md](../design/markharness-v2-design.md) §6.1), so deleting either module makes `changes compute` and `backfill run` fail to compile.

The `changes lineage` and `milestone init` subcommands are kept as well. The `.markharness/executions/<tag>/milestone.yml` that `milestone init` writes is not the execution record [0020](0020-execution-status-lightweight-model.md) removes — it is an audit copy of the schema version, and it is the input to the `changes compute` gate. Removing the CLI would leave no way for a human to produce that input.

### 5. Keep `src/verify.rs`, `src/backfill.rs`, and `src/audit_scope.rs`

`verify.rs` (diffing regenerated output against committed output) and `backfill.rs` (bulk ChangeEvent computation across past milestones) have no reverse dependencies and could be deleted on their own. But neither conflicts with [0019](0019-alignment-check-commit-trailer.md)–[0025](0025-v2-forward-compatible-evolution.md), and there is no positive reason to remove them. The YAGNI rule in [CLAUDE.md](../../../CLAUDE.md) asks that unrequested implementation not be added; it is not a basis for deleting working, non-conflicting code.

`audit_scope.rs` (62 lines) is the type behind the `audit_scope` field in the output of `identity migrate --json`, `identity audit --json`, and `changes compute --json` — part of the output contract set by [0013](0013-immutable-identity-model.md)'s verification rules.

### 6. Move `execution::iso8601_utc_now` to `src/time.rs`

This function is used by `src/identity/feature_ops.rs` and `src/identity/migration_manifest.rs`. Because [0025](0025-v2-forward-compatible-evolution.md) §1 gives `ExecutionBinding` no execution timestamp, leaving a timestamp generator in the `execution` module produces a misleading structure: `ExecutionBinding` holds no time, yet `execution` exports a time function. Identity events keep their timestamps, so the function itself is still needed; it moves to a location that matches its responsibility.

### 7. Consider no backward compatibility at all

In implementing [0019](0019-alignment-check-commit-trailer.md)–[0025](0025-v2-forward-compatible-evolution.md), **earlier schemas and data are treated as never having existed**. No compatibility code, no migration code, no diagnostic that names old data, and no schema-version bump is written. This applies the design rule in [CLAUDE.md](../../../CLAUDE.md) (assume no backward compatibility; eliminate design that exists for compatibility; always aim at the best product), and it replaces the former [markharness-v2-design.md](../design/markharness-v2-design.md) §9.1 rule that a log containing removed event kinds be "rejected with a diagnostic naming the offending events".

The consequences:

- `ExecutionBinding` reads only the new location `.markharness/bindings/`; the execution records under the old `.markharness/executions/` are never consulted.
- The removed event kinds are deleted from `IdentityMutation`, so a log containing them has no read path at all. No rejection diagnostic is written either.
- `source` in `requirement.yml` becomes **required**. [0023](0023-requirement-native-and-external-source.md) §1's "omitted means native" existed to let existing files pass unchanged, so it does not apply; mode is never decided by an implicit default.
- `knowledge_schema_version` ([0014](0014-knowledge-schema-version-persistence.md)) is not bumped.
- [0025](0025-v2-forward-compatible-evolution.md) §2's `schema_version` is kept but fixed at `1` for every kind and never bumped. It is not a compatibility mechanism for reading the past but a forward contract that keeps a future record kind from being mistaken for this one.

### 8. The impact section of [0021](0021-identity-retire-simplification.md) does not match the code

The "Impact" section of [0021](0021-identity-retire-simplification.md) names `src/identity/recovery.rs` (742 lines) and `src/identity/audit.rs` (816 lines) as the reduction targets for retire/restore/release/reissue. Measurement on 2026-09-11 shows this is wrong. The actual targets are:

- `ReleaseError`/`release_id`/`RetireError`/`retire_entity`/`RestoreError`/`restore_entity`/`ReissuedEntity`/`ReissueError`/`reissue_entity` in `src/identity/feature_ops.rs` (~524 lines)
- `IdentityMutation::{Retired, Restored, Released, Reissued}` in `src/identity/event.rs`
- The replay-time status transitions and `Status::Retired` in `src/identity/engine.rs`
- The reissue-dependent part of `src/identity/migration_manifest.rs`
- `IdentityCommand::{Release, Retire, Restore, Reissue}` in `src/cli.rs` and the corresponding tests in `tests/identity_cli.rs`

The `release` occurrences in `recovery.rs` and `lock.rs` are `IdentityLock` file-lock releases, unrelated to id reservation. `audit.rs` contains none of the four terms.

The **decision** in [0021](0021-identity-retire-simplification.md) (what to remove) is correct; only its estimate of the impact is wrong, so the decision's force is unchanged. Following the ADR policy in the [release-and-license instructions](../../../.github/instructions/release-and-license.instructions.md), [0021](0021-identity-retire-simplification.md) itself is not rewritten — it stands as a historical record — and the correction is recorded here. [markharness-v2-design.md](../design/markharness-v2-design.md) §9 is a living document describing the current plan, so it is corrected there.

## Impact

- [markharness-v2-design.md](../design/markharness-v2-design.md) §9's table, §9.1, §9.2.2, and acceptance criteria AC09b and AC22 are updated to match this ADR. AC22 (rejecting a log with removed events, with a diagnostic) is deleted per §7.
- `plan` and `cache index` join the removed commands, alongside the existing `identity retire`/`restore`/`release`/`reissue`, `serve`, and `execution record`.
- The corresponding sections of the CLI manuals (`docs/ja/cli-manual.md`, `docs/en/cli-manual.md`) are deleted.

## Options considered and not taken

- **Keep `plan`, rewritten to consult `ExecutionBinding`**: it would preserve the path existing users know, but leaves two subcommands answering the same question as `coverage`. Under §7 there is no reason to keep it.
- **Delete `verify` and `backfill` too, to minimize surface area**: easy to do, but neither conflicts with any v2 decision, and it would remove working features without a basis for the judgment. Retire them individually once their necessity is actually disproved.
- **Keep the internal functions of `lineage`/`milestone` but drop their CLIs**: the `milestone.yml` that `milestone init` writes is the input to the `changes compute` fail-closed gate, so dropping the CLI leaves no way to produce it. `changes lineage` is the path for inspecting `changes compute`'s classification, and its upkeep is small.
- **Implement diagnostics for old data only**: kinder to users, but a diagnostic is a form of compatibility code, which the [CLAUDE.md](../../../CLAUDE.md) design rule excludes. Since no code path reads old data, there is no operational hazard either.
