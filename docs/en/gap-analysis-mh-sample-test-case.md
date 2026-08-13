# Discrepancies between mh-sample-test-case execution results and the design document (integrated edition)

**Status**: Survey (a snapshot at the time of investigation)
**Related documents**: [A Git-Native Model for Test Knowledge Management: Integrated Edition](./git-native-model-for-test-knowledge-management.md) (hereafter "the design document")
**Investigation scope**: the tracked files, working tree, and `git log` of this repository (`mh-sample-test-case`)

**Positioning**: This document cross-checks the design document against `mh-sample-test-case` (this folder), an actual case-study operational repository built around a TODO app in which the `markharness` CLI has been operated for 3 milestones, and organizes the discrepancies that could be confirmed. The design document itself already organizes the discrepancies between the code implementation and design of `markharness` (the CLI itself, `C:\Users\papa\work\markharness`) in its own §3.6 "Implementation status summary", but separately from that, this document confirms **how the results of actually operating this CLI (the commit history and generated artifacts of this repository) differ from what the design document describes**.

---

## 1. Overall assessment

The operation of this repository has confirmed, in the field, all three points that the design document positions as its core contributions.

- Using the tree SHA as the Feature lineage key, changes under Condition or ExpectedResult are detected even when `feature.yml` itself is unchanged (confirmed by the concrete example in Chapter 2 below).
- TestCase is separated from `knowledge/` and managed as a derived artifact under `generated/`.
- `changes/test2.yaml`/`changes/test3.yaml` are automatically generated at milestone boundaries (`git tag test1/test2/test3`).

At the same time, **the "automatic reconciliation of TestExecution and ChangeEvent" feature (`verified_feature_tree_shas` / `markharness verify trace` / `markharness verify pending`), which is not described anywhere in the body of the design document, is already incorporated into and used by every execution result in this repository** (Chapter 3). This is not so much an omission in the design document as a case where a feature added to the CLI after the design document was written has come to be used ahead of time on this case-study side — something worth recording as a divergence between the design document and actual operation.

There are also other features that are implemented on the CLI side but have never been used in the operation of this case study, such as `forked_from`, `change_type`, `schema/` validation, and `markharness changes lineage` (Chapter 4).

---

## 2. Tree-SHA-based lineage detection (design document §3.1): confirmed with a concrete example

Design document §3.1 records the correction that "rather than the blob SHA of `feature.yml` alone, the tree SHA of the entire Feature directory is compared." The ChangeEvent between `test2` and `test3` in this repository is a concrete example that bears this out.

```
$ git diff test2 test3 -- knowledge/todo-simple/todo-add
diff --git a/knowledge/todo-simple/todo-add/todo-add-from-form/todo-add-valid-title/expected/004.yml
+++ (only a new file was added; feature.yml itself is unchanged)
```

`todo-add/feature.yml` itself was not changed at all between `test2` and `test3`, but because `expected/004.yml` underneath it was newly added, a ChangeEvent for `todo-add` (`from_tree_sha: 79324b98…` → `to_tree_sha: ef424d86…`) was correctly recorded in `changes/test3.yaml`. Had the initial design, as originally proposed, judged this by the blob SHA of `feature.yml` alone, this change would have gone undetected — so the defect that the design document points out, and its fix, are reproduced and confirmed here with real data.

---

## 3. TestExecution ↔ ChangeEvent linkage (not described in the design document)

`executions/*/results.yml` in this repository has, in every record, the fields of `TESTEXECUTION` described by the design document's ER diagram, §3.1, and §3.5 (`case_id`/`result`/the equivalent of executor and timestamp), plus a **`verified_feature_tree_shas` field that never appears in the design document**.

```yaml
- case_id: tc-edit-existing-todo-001
  result: pass
  executor: soreiyu52
  executed_at: 2026-08-09T17:08:29Z
  verified_feature_tree_shas:
    todo-edit: 0b769f0d5ed46a92798107bcd4256c1513a21e8e
```

This records, for each `generated_from.feature` (the Feature that a TestCase was generated from), the tree SHA of the Feature directory at the time of execution, enabling the Q1/Q2 determinations made by `markharness verify trace <case_id> --milestone <m>` (which ChangeEvent a given execution reflects) and `markharness verify pending` (mechanically detecting TestCases that have not yet been re-verified). What the design document's §3.5 and Figure 4 describe as "propagation of change impact" → "the set of TestCases requiring re-confirmation" stops, within the design document, at the level of a static generation graph — but the actual CLI goes one step further: **it also has the capability to mechanically track how far a change has been reflected from the execution-result side, and this repository has used that consistently from its very first execution (`test1`)**.

This feature, and the design intent behind it, is documented only in a separate paper within the same `markharness` repository as the design document — "[`change-event-verification-tracking-spec.md`](./design/change-event-verification-tracking-spec.md)" — and appears nowhere, neither as a section nor even as a mention, in the body of the integrated edition (the file in this folder's `docs/`). Reading the design document alone, one cannot know this feature exists, and it is worth specifically noting that **there is a divergence between "the completeness of the paper" and "the actual functionality of the CLI"**.

Likewise, `generated/traceability-index.json` (an index of Requirement→Feature→Behavior→Condition→TestCase, generated at the same time by `generate`, where `axis` is the union of the three tiers Requirement/Feature/Behavior) is also an artifact that does not appear in the directory-structure diagram of the design document's §3.5, yet it actually exists in this repository and is also a target of `markharness verify`'s diff verification.

---

## 4. Features described in the design document but unused in this case study

Features that are implemented on the CLI side (`markharness`) but have never been used in the actual operation of this repository (the sequence of operations recorded in `memo.md`, and `git log`).

| Feature | Corresponding section in the design document | Usage status in this repository |
|---|---|---|
| `forked_from` (manual notation of a conceptual derivation) | §3.1 | None of the 4 Features (`todo-add`/`todo-complete`/`todo-delete`/`todo-edit`) has the `forked_from` key itself present in `feature.yml`. Given the subject matter — a TODO app with no branching — there has been no occasion to use it |
| `change_type` (post-hoc entry via `markharness changes annotate`) | §3.5 | `change_type` in both `changes/test2.yaml` and `changes/test3.yaml` remains `null`. The `annotate` command never appears in the sequence of operations in `memo.md` |
| `markharness validate` (JSON Schema validation + axis/forked_from cross-reference checking) | §3.5, §3.6 | The full set of `schema/*.schema.json` was placed by `markharness init` and is under Git management (the only directory related to this document that is actually tracked by Git), but a call to `markharness validate` never appears in the sequence of operations in `memo.md`. Whether the schema has actually validated this repository's `knowledge/`/`axes/` cannot be confirmed |
| `markharness changes lineage --commit <sha>` (`git merge-base` ancestor search / two-parent divergence auditing) | §3.2 | As `git log --all --oneline --graph` shows, this repository's history is completely linear from `first commit` through to `Automatically compute the test2-test3 ChangeEvent` (no branch divergence or merge), so the "true divergence" case that `lineage` handles has not itself occurred |

None of these is "unimplemented" — rather, they are features that "do not occur, or there was no motivation to use, in a small single-branch, single-owner case study," which suggests that a simple sequential-operation repository like this one is not, on its own, sufficient to verify the "change-impact identification task spanning multiple generations and multiple branches" that §5.2 of the design document targets for evaluation.

---

## 5. Differences in directory structure and accompanying files (design document §3.5)

Design document §3.5 "Implementation status" already notes the explicit materialization of `REQUIREMENT` as a file and the per-milestone file format of `changes/`, and the actual data in this repository is consistent with this (`knowledge/todo-simple/requirement.yml`, and `changes/test2.yaml` being one file per interval with an array of multiple events).

Beyond that, there are two operational differences specific to this repository that cannot be read from the description in the design document.

- **`docs/`, `memo.md`, `.markharness-cache/`, and `tmp/` are excluded via `.gitignore`**: the design document only specifies that `.markharness-cache/` is not committed (§3.3, §3.5 — this matches the design), but in this repository, `docs/` (which includes the design document itself) and `memo.md` (the operation log) are also excluded from Git management. As a result, looking only at this repository's `git log`, one cannot trace from Git history alone "which version of which design document the operation was based on" or "which commands were run in what order" — one must rely on the actual working-tree artifacts (`memo.md`, and the `docs/` this document itself references).
- **`tmp/` is used as a staging area for drafts of Features not yet imported**: three unused Feature drafts exist — `tmp/todo-reopen`, `tmp/todo-search`, `tmp/todo-show` — and the only ones actually imported into `knowledge/` are `tmp/todo-edit` (imported at `test2`) and `tmp/004.yml` (imported at `test3`). The design document contains no explanation of a working area like `tmp/`, nor does the CLI implementation have any mechanism that treats `tmp/` specially (it is simply a developer's own working-directory convention).

---

## 6. TestCase file naming (a discrepancy from a prior MM document's observation)

For reference, an earlier survey document from the `markharness` repository (whose subject was a different folder, `c:\Users\papa\work\mm`, unrelated to this repository; that document has since been removed as part of a cleanup) pointed out the problem that "the filename does not correspond to `case_id`." In this repository's `generated/testcases/*.yml`, however, the filename (e.g. `todo-add-valid-title.yml`) and the `case_id` (`tc-todo-add-valid-title-001`) correspond, aside from the `tc-` prefix and the `-001` sequence number, and correspondence is systematically maintained. This can be confirmed as an improvement over the earlier document.

---

## 7. Summary

| Category | Content |
|---|---|
| Matches the design document (confirmed and backed by real data) | Tree-SHA-based Feature lineage detection (detecting an added Expected even when feature.yml is unchanged, Chapter 2), TestCase derivation management, automatic generation of ChangeEvent at milestone boundaries, non-commitment of `.markharness-cache/` |
| Not described in the design document but used in actual operation | TestExecution ↔ ChangeEvent linkage via `verified_feature_tree_shas` (data for `verify trace`/`verify pending`, Chapter 3), `generated/traceability-index.json` (the 3-tier axis union index, Chapter 3) |
| Implemented on the CLI side but unused in this case study | `forked_from`, `change_type` annotation, `markharness validate`, `markharness changes lineage` (Chapter 4) |
| Operational elements not present in the design document | `.gitignore` exclusion of `docs/`/`memo.md`/`tmp/`, using `tmp/` as a staging area for drafts (Chapter 5) |

This repository has been able to empirically verify, with small-scale data on a single branch, the design document's core claims (tree-SHA-based version history and automated ChangeEvent generation). At the same time, it has confirmed both (a) that it uses, ahead of time, implemented features that the design document has not yet documented, and (b) that features the design document does address — branch divergence, `forked_from`, `change_type`, schema validation — remain unverified by this case study alone. Verifying the "change-impact identification task spanning multiple generations and multiple branches" (Layer β) targeted for evaluation in Chapter 5 will require, in addition to a linear, simple-operation case study like this repository, separate, more complex operational data that includes branching and merging.

**Note (added 2026-08-10)**: The row for `markharness changes lineage` under "Implemented on the CLI side but unused in this case study" in the table above refers to the state as of the time of this document's investigation (only the linear history of `test1` through `test3`). As noted in Chapter 8, branching and merge scenarios were subsequently added and verified as `test4`.

---

## 8. Verification scenario including branching and merging (test4, conducted 2026-08-11)

As a response to item 3 of improvement-prompts.md, a new case-study scenario involving branching and merging was added to and verified in this repository. This was done to resolve the constraint noted in Chapter 4 that "the history is linear only, so the true-divergence case for `lineage` has not occurred." The existing `test1` through `test3` data, commits, and tags have not been changed in any way.

> **Note (2026-08-11)**: This section was originally recorded as having been "conducted on 2026-08-10," but review revealed that the actual clone of `mh-sample-test-case` had no such branch, merge, or `test4` tag at all (only the tags `test1` through `test3` exist, and there is no trace in `git reflog` either), and that an empty `.git/index.lock` dated 2026-08-10 had been left behind. In other words, the earlier record was not actually obtained by running the commands. This section discards that erroneous record and has been replaced in full with the results actually obtained by re-executing the procedure on 2026-08-11.

### 8.1 Procedure

1. From `main` (commit `3e0d3f5`, the state including the computation of `test3`'s `changes/test3.yaml`), a working branch `markharness-lineage-scenario-feature` was created.
2. On the working branch, a description was appended to the existing `expected/005.yml` (addition via the Enter-key shortcut) of the `todo-add-valid-title` Condition of the `todo-add` Feature, and this was committed (`7a0b09f`).
3. On the `main` side, a different description was appended to the existing `expected/004.yml` (the description of the success popup) of the same `todo-add-valid-title` Condition, and this was committed (`9a51136`). Because the changes touch different files, no conflict occurs at merge time.
4. The working branch was merged into `main` with `--no-ff` (`d23fb31`), and the merge commit was tagged `test4`.
5. `markharness changes lineage --commit d23fb31e649619848a991af30e16d97f2ab39443 --dir <repo>` and `markharness changes compute test3 test4 --dir <repo>` were each run.
6. The generated `changes/test4.yaml` was committed (`7666a2c`), aligning it with the same "commit the artifact" operational convention as the existing `changes/test2.yaml`/`changes/test3.yaml`.

### 8.2 Execution results

Actual output of `markharness changes lineage --commit d23fb31e649619848a991af30e16d97f2ab39443`:

```
todo-add: true_divergence
todo-complete: single_parent
todo-delete: single_parent
todo-edit: single_parent
```

The `changes/test4.yaml` actually generated by `markharness changes compute test3 test4`:

```yaml
- event_id: todo-add--test3--test4
  feature_id: todo-add
  from_milestone: test3
  to_milestone: test4
  from_tree_sha: f0f91f81d3f584ff269703b17a9277f114eb282f
  to_tree_sha: 7215feb4ab30541d9c252c4a30c3d2bd109b8c93
  impacted_testcases:
  - tc-todo-add-valid-title-001
  change_type: null
  true_divergences:
  - merge_commit: d23fb31e649619848a991af30e16d97f2ab39443
    parent_tree_shas:
    - b9952cccc56380eda14a926241399c96edcff9d9
    - 378c1834cc23deab8130ea13ca267993a69c41f6
```

The generated `.markharness-cache/test4.json` (excerpt; target of `.gitignore`, not committed):

```json
{"key":{"tree_sha":"514029850e150758eeecbe8369e1e847c7a92f08","canonicalization_rule_version":"1","id_index_schema_version":"1","tool_version":"0.1.0"},"entries":[{"id":"todo-add","path":"knowledge/todo-simple/todo-add","tree_sha":"7215feb4ab30541d9c252c4a30c3d2bd109b8c93"}, ...]}
```

### 8.3 Points that matched expectations

- Only `todo-add` was judged `true_divergence`, and the other 3 Features (`todo-complete`/`todo-delete`/`todo-edit`), which are not involved in the branch/merge, were each judged `single_parent`. This matches the case breakdown in design document §3.2.
- The `true_divergences` in `changes/test4.yaml` recorded the same two parent tree SHAs (`parent_tree_shas`) and the same merge-commit SHA (`merge_commit`) as the `true_divergence` case individually reported by the `lineage` command. This is the first concrete example confirming that "integrating `lineage` into every merge within an interval" (`checklist-changes-lineage-generalization.md`), the generalization made in improvement-prompt item 2, works as designed not only in unit tests (on a tempdir on the `markharness` repository side) but also in an actual case-study repository with multiple commits and multiple Features.
- The merge commit is located at the `to_milestone` (= `test4`) tag itself rather than partway through the `from_milestone..to_milestone` interval — a simple case that was already handled before the generalization — and it was confirmed that the post-generalization implementation continues to detect it correctly as before.

### 8.4 Points that differed from expectations, and caveats

- `from_tree_sha` (a single value) was recorded as-is as the tree SHA at the `test3` point in time, coexisting with `true_divergences` (two-parent information). The design document only states, regarding the coexistence of these two fields, that "`from_tree_sha` is retained as a representation of the linear history"; this is the first time a record with both actually filled in at once has been observed. There is no contradiction in the values (`from_tree_sha` is the result of a simple two-point comparison in the main lineage, while `true_divergences` is two-parent information specific to the merge commit), but how future tools that consume this record — such as `verify trace`/`verify pending` (§3.7) — will differentiate the use of the two fields was not verified in this scenario and remains a topic for the future.
- This scenario is a case with exactly one merge within the `from_milestone..to_milestone` interval. The case of "multiple merges within an interval" (the case that the generalization of improvement-prompt item 2 originally targets — where a merge lies partway through the interval rather than at the `to_milestone` tag's position, or where the same Feature undergoes a true divergence more than once within the interval) was not verified in this scenario and remains a topic for the future.

### 8.5 Impact on the repository

- The new branch `markharness-lineage-scenario-feature`, the merge commit (`d23fb31`), the `test4` tag, and `changes/test4.yaml` were committed (`7666a2c`). None of these has been pushed to a remote (this repository has no `origin` configured).
- The existing `test1` through `test3` commits, tags, `changes/test2.yaml`, `changes/test3.yaml`, and the contents of `executions/` have not been changed.
