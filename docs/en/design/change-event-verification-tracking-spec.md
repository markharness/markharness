# ChangeEvent-linked Execution Status Tracking (Verification Status Tracking) Specification

**Status**: Implemented(`markharness verify trace` / `markharness verify pending`)
**Related documents**: [git-native-model-for-test-knowledge-management.md](../git-native-model-for-test-knowledge-management.md) (hereafter "the paper"), [gap-analysis-mh-sample-test-case.md](../gap-analysis-mh-sample-test-case.md)
**Intended audience**: Implementers of `markharness` (or its successor tools)

**Positioning**: This document is an additional specification that concretizes the "execution-result-side linkage" portion of the "identify impacted TestCases starting from a ChangeEvent → re-verify" concept envisioned in Section 3.5 and Figure 4 of the paper. The paper treats automatic generation of ChangeEvents and identification of impacted_testcases as its core contribution, while a mechanism to automatically determine "whether it was actually re-executed afterward" was an undecided area corresponding to Chapter 7 (Future Work). This document specifies that area.

---

## 1. Problems to Solve

This addresses two questions that the current implementation (the MM folder) cannot answer automatically, but that are inevitably asked in operation.

- **Q1 (retrospective)**: "Which change to the Feature does the result of this TestExecution reflect?"
- **Q2 (forward-looking)**: "Among the TestCases listed in impacted_testcases by a ChangeEvent, which ones have not yet been re-executed?"

In the current implementation, `executions/<milestone>/results.yml` holds only `case_id / result / executor / executed_at`, and cross-checking against the impacted_testcases in `changes/<from>-<to>.yaml` is done visually by a human. This cross-check is to be automated.

---

## 2. Data Model Extension

### 2.1 Adding a Field to TESTEXECUTION

The following is added to `TESTEXECUTION` in the ER diagram of the paper (Section 3.1).

```yaml
# One record in executions/<milestone>/results.yml
case_id: tc-edit-existing-todo-001
result: pass
executor: soreiyu52
executed_at: 2026-08-08T16:38:52Z
verified_feature_tree_shas:        # added field
  todo-edit: 4f2c9a1e8b3d5670012ab34cd56ef7890a1b2c3
```

- `verified_feature_tree_shas`: For each Feature listed in that TestCase's `generated_from` (see Section 2.2), records the **tree SHA of the entire Feature directory at the milestone at execution time** (not just feature.yml itself, but the Git tree object SHA of the entire directory including the Behavior/Condition/ExpectedResult beneath it). Note that this is not the blob SHA of feature.yml alone (see Chapter 7: a single blob cannot detect changes to Condition/ExpectedResult).
- The recording timing is when the execution result is registered. The value is mechanically filled in by looking up the tree object SHA of the corresponding Feature directory at the target milestone from the `id_index` cache (paper Section 3.3). It is not a field a human enters by hand.
- To also support composite TestCases spanning multiple Features (in the future, where a Behavior may span multiple Features), a map form is used rather than a single value.

### 2.2 TESTCASE's `generated_from` Is Used As-Is

Since `generated/testcases/*.yml`'s `generated_from.feature` (e.g. `todo-edit`) already holds the Feature id, it can be reused as the key for `verified_feature_tree_shas`. No schema change is needed.

### 2.3 No `resolved` State Is Added to ChangeEvent

The design of giving ChangeEvent (`changes/*.yaml`) itself a "re-verified flag" is not adopted. Reasons:

- A ChangeEvent is an **immutable record of fact** representing the diff at a milestone boundary, and should not be a target for later rewriting (this is consistent with the design philosophy of paper Section 3.4).
- "Whether it has been re-verified" is derived information that can be obtained by **computing it each time** from two independent factual series — ChangeEvent and TestExecution — and does not need to be written back into either source.

### 2.4 Recomputation Modes for impacted_testcases (`--historical` default / `--current-tree`)

As of 2026-08, `impacted_testcases` written by `changes compute` has two modes.

- **Default (historical)**: Generates TestCases from the `knowledge/` tree pointed to by the `to_milestone` tag. Recomputing the same `from_milestone..to_milestone` range at a later date always yields the same `impacted_testcases`.
- **`--current-tree`** (legacy behavior, opt-in for backward compatibility): Generates TestCases from `knowledge/` in the current working tree. As long as the working tree keeps changing, recomputing the same range each time can yield different results.

For the cross-check with `verified_feature_tree_shas` in Section 2.1 (Q2 in Section 3.2), the set of `impacted_testcases` written to `changes/*.yaml` is used as-is as `Impacted`, so note that if `changes/*.yaml` computed with `--current-tree` is later recomputed with `--historical` (default), the `Impacted` set itself may change. For operations that require reproducibility (the pending/stale determination premised on this section), use the default historical mode.

---

## 3. Determination Algorithm

### 3.1 Q1: Which Change Does This Result Reflect Execution After?

Input: `case_id`, `milestone` (target TestExecution record)

1. Retrieve `verified_feature_tree_shas` of the target record from `results.yml`.
2. For each Feature id, search all ChangeEvent records under `changes/` for `to_tree_sha == verified_feature_tree_shas[feature_id]`.
3. Return the matching ChangeEvent's `event_id`, `from_milestone`, and `to_milestone` as "the change this result reflects."
4. If no matching ChangeEvent is found (i.e., there was no change to the target Feature at that milestone), trace back along the most recent `derived_from` chain and return "the last milestone at which a change occurred."

Example output:

```
$ markharness verify trace tc-edit-existing-todo-001 --milestone test2
case_id: tc-edit-existing-todo-001
feature: todo-edit
executed_at: 2026-08-08T16:38:52Z
reflects_change: todo-edit--test1--test2
  from_milestone: test1
  to_milestone: test2
  change_type: (not yet recorded; will display if entered afterward via markharness changes annotate)
```

### 3.2 Q2: Which TestCases Were Changed but Not Yet Executed?

Input: `from_milestone`, `to_milestone` (the milestone range to compare; the most recent adjacent pair if omitted)

1. Read all ChangeEvents for the target range from `changes/<from>-<to>.yaml` and build a unified set `Impacted` from their `impacted_testcases`.
2. Scan `results.yml` for `to_milestone` onward (including `to_milestone` itself and all subsequent milestones), and for each `case_id`, consider it "re-verified" if there is even one record satisfying `verified_feature_tree_shas[feature_id] == changes[event].to_tree_sha`.
3. Output `Impacted - re-verified set` as "not re-executed."
4. If the target Feature has been further changed after `to_milestone` (i.e., `to_tree_sha` is already stale), list it separately under the category "the target itself has become stale" rather than "not re-executed" (Section 3.3).

Example output:

```
$ markharness verify pending --from test1 --to test2
pending (not re-executed):
  - tc-edit-existing-todo-001  (impact of todo-edit change test1->test2, not executed)

stale (impacted scope has changed further):
  (none)
```

### 3.3 Distinguishing "pending" from "stale"

When Q2 is operated across milestones, cases will inevitably arise where "the target Feature changed further before it could be re-executed." If these are all uniformly treated as "not executed," testers lose track of "which version they should be verifying against." Hence the two categories.

- **pending**: There is still no execution record at all against the `to_tree_sha` at the time the ChangeEvent occurred.
- **stale**: With no execution record against the `to_tree_sha` at the time the ChangeEvent occurred, the tree SHA of that Feature has since changed to something even newer (i.e., verification against the old version is no longer meaningful). In this case, the latest ChangeEvent is re-presented as the "effective verification target."

Determination: For the target Feature id, check via the `id_index` cache whether the `to_tree_sha` of the ChangeEvent that generated the `Impacted` set matches the **current** tree SHA (at query time). If it matches, pending; if not, stale.

---

## 4. Tool Interface Specification

The following two commands are implemented as CLI subcommands of the markharness main body.

| Command | Purpose | Corresponding question |
|---|---|---|
| `markharness verify trace <case_id> --milestone <m>` | Shows which ChangeEvent the specified execution result reflects | Q1 |
| `markharness verify pending [--from <m1> --to <m2>]` | Shows a list of not-yet-re-executed / stale TestCases | Q2 |

- Both commands are read-only (they do not write to files). They take only the existing `verified_feature_tree_shas`, `changes/*.yaml`, and `.markharness-cache/` (id_index) as input.
- CI integration: `verify pending` is run at the milestone release gate, with an option (`--fail-on-pending`) that returns a non-zero exit code if there is even one `pending` item. This allows "omissions in re-verifying change-impacted tests" to be mechanically blocked in CI.

---

## 5. Trace Example with Existing MM Implementation Data

Reconstructing the current `changes/test2.yaml` and `executions/test2/results.yml` per this specification gives the following (`verified_feature_tree_shas` is assumed to be attached starting from newly executed records after this spec is introduced, and is not applied retroactively to existing records; see Chapter 6. `to_tree_sha` is the tree SHA of the entire Feature directory, and its value changes upon re-running `changes compute`. The values below are illustrative).

```yaml
# changes/test2.yaml (illustrative; actual values will differ once tree-SHA-based)
- event_id: todo-edit--test1--test2
  feature_id: todo-edit
  from_milestone: test1
  to_milestone: test2
  from_tree_sha: null
  to_tree_sha: 4f2c9a1e8b3d5670012ab34cd56ef7890a1b2c3
  impacted_testcases:
  - tc-edit-existing-todo-001
```

```yaml
# executions/test2/results.yml (form after this spec is introduced)
- case_id: tc-edit-existing-todo-001
  result: pass
  executor: soreiyu52
  executed_at: 2026-08-08T16:38:52Z
  verified_feature_tree_shas:
    todo-edit: 4f2c9a1e8b3d5670012ab34cd56ef7890a1b2c3
```

Cross-checking these two, since `to_tree_sha` matches `verified_feature_tree_shas.todo-edit`, `markharness verify pending --from test1 --to test2` does not treat `tc-edit-existing-todo-001` as pending, and correctly determines it as "re-verified."

---

## 6. Introduction / Migration Policy

- **No retroactive application**: Since existing `results.yml` (test1 through test3) does not have `verified_feature_tree_shas`, execution records prior to the introduction of this specification are out of scope for Q1/Q2 determination (treated as "unknown"). As with the backfill policy in Chapter 4 of the paper, it is theoretically possible to mechanically backfill the tree SHA at the time from the id_index cache, but this is out of scope for now and left as Future Work.
- **Relationship with `change_type`**: `ChangeEvent.change_type` (paper Section 3.5) can now be entered after the fact via `markharness changes annotate`. The Q1/Q2 determination in this specification itself is completed solely via tree SHA comparison and does not depend on the presence of `change_type`. Adding filtering/grouping by change type (spec change / bug fix, etc.) to the output of `verify pending` is not yet implemented, and remains a future task.
- **Schema**: `schema/execution_result.schema.json` (placed by `markharness init` as part of the default set, and included by `markharness validate` as a validation target for `executions/*/results.yml`) is implemented. Since it defines `case_id`/`result`/`executor`/`executed_at` as required and `note`/`verified_feature_tree_shas` as optional fields, existing records from before this specification was introduced (which lack `verified_feature_tree_shas`) also pass structural validation, which does not contradict the "no retroactive application" policy above (cli-manual.md Section 1.17).

---

## 7. Threats / Points of Note

- Since `verified_feature_tree_shas` is recorded per the Feature that is the TestCase's generation source, changes at the Condition/ExpectedResult level are automatically captured: a Condition is part of the tree beneath the Feature, so if a Condition changes, the tree SHA of the entire Feature directory also changes. **This capture is realized by `id_cache::resolve_feature_versions` comparing the tree SHA of the entire Feature directory (obtaining the directory's tree object SHA via `git ls-tree -r -t`); an implementation that compares only the blob SHA of feature.yml alone does not achieve this** (in early implementation, only the blob of feature.yml alone was compared, which had a known bug of missing additions/changes to Condition/ExpectedResult; the description in this section presumes that fix has been applied). However, the case where **the Feature itself is unchanged but only the Axis registry (axes/*.yml) side changes** is out of scope for tracking under this specification and requires separate consideration.
- For TestCases spanning multiple Features (not present in the current MM implementation, but possible in the future if Behaviors are designed to span multiple Features), since `verified_feature_tree_shas` would have multiple keys, a state of "partial re-verification," where only some Features are re-verified and others are not, can occur. In this case, the algorithm in Section 3.2 treats it as pending if even one mismatch exists (a conservative determination).
