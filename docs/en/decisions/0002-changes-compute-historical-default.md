# 0002: Make historical mode the default for `impacted_testcases` in `changes compute`

## Status

Accepted

## Background

`markharness changes compute` compares two past tags, `from_milestone`/`to_milestone`, but `impacted_testcases` (the set of impacted TestCases) was always generated from the **current working tree** (`impacted_testcases_by_feature`, `src/changes.rs`). As a result, recomputing the same past interval at a later date could produce different results depending on changes to the current test structure (P1 finding in `docs/テスト知識管理のGit-nativeモデル_評価レビュー.md`).

This was caused by the question "what was actually impacted during a given past interval" (historical reproduction) and the question "what tests should be re-checked right now" (current candidate extraction) being mixed together in a single implementation without being explicitly separated.

## Decision

Two modes were introduced for the `impacted_testcases` computation in `markharness changes compute`, with **historical mode** (generated from the Git tree that the `to_milestone` tag points to) as the default.

- **historical mode (default)**: `knowledge/` is expanded from the Git tree of the `to_milestone` tag into a temporary `git worktree`, and `TestCase`s are generated from that (`historical_testcases_by_feature`). Recomputing the same interval at a later date always produces the same result.
- **`--current-tree` (opt-in)**: The legacy behavior of generating from `knowledge/` in the current working tree (`impacted_testcases_by_feature`). As long as the working tree keeps changing, recomputation results for the same interval can also change.

`markharness backfill run` (the backfill worker from Chapter 4), which also reconstructs past milestone intervals, was likewise changed to use the same default (historical).

## Rationale (why historical was made the default)

- Safety (the same query always returns the same result) was prioritized over backward compatibility (preserving the existing working-tree-referencing behavior). `changes/*.yaml` is designed as an **immutable record of facts** at milestone boundaries ([change-event-verification-tracking-spec.md](../design/change-event-verification-tracking-spec.md) §2.3), so an implementation in which `impacted_testcases`, a part of it, could change on every recomputation is inconsistent with this design philosophy.
- Since the use case of current candidate extraction — "what tests should be re-checked right now" — is still needed, it was not removed but changed into an explicit opt-in as `--current-tree`.

## Impact / Conditions for future reconsideration

- The default behavior of `markharness backfill run` changes (the existing behavior was equivalent to `--current-tree`, but was changed to historical). Since the purpose of backfill targets is to reconstruct past milestone intervals, this change is more consistent with the use case.
- Explanations of both modes have been added to §3.5 of `docs/git-native-model-for-test-knowledge-management.md` and §2.4 of `docs/design/change-event-verification-tracking-spec.md`.
- If it turns out in the future that `--current-tree` mode sees little use, removing the option itself may be reconsidered. Conversely, if `--current-tree` turns out to be more useful as the standard operating mode in CI etc., re-reversing the default should be recorded as overriding this decision.
