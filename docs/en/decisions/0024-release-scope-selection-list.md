# 0024: Record a release's verification scope as a lightweight selection list

## Status

Accepted (decided 2026-09-11, implemented 2026-09-12; see `checklist-v2-core.md`). It keeps [0020](0020-execution-status-lightweight-model.md)'s stance — execution results and evidence stay outside markharness — and adds the smallest type that records only *what was chosen for verification* in a release. [0025](0025-v2-forward-compatible-evolution.md) keeps it as a record kind distinct from any future `ReleasePlan`; it is never reinterpreted as a complete plan.

## Background

Question 3 in [markharness-v2-design.md](../design/markharness-v2-design.md) §1 is "check which tests were in the verification scope of the previous release."

[0020](0020-execution-status-lightweight-model.md)'s `ExecutionStatus` carries only the verification method (`automated` / `manual`) and a reference — no timestamp, no release number. Reproducing a past point by Git ref therefore answers "which TestCases and verification methods were registered then," not "which set was actually chosen for that release." The 2026-09-11 design review (SP-05) raised exactly this, asking that registered state and selection not be conflated.

At the same time, heavyweight contract objects such as the old v2 design's `Verification Plan`, `EvidenceSelection`, and `Execution Manifest` were explicitly rejected by [0020](0020-execution-status-lightweight-model.md). What is needed is a list of what was chosen — not approval of the choice, its history, or its results.

## Decision

### 1. Add `ReleaseScope`

```text
ReleaseScope {
  release_id,            // the release's display name (a Git tag name is recommended)
  case_uids: [case_uid], // the TestCases chosen for verification in that release
}
```

TestCases are referenced by Case UID rather than display id ([0013](0013-immutable-identity-model.md); survives renames).

### 2. What it does not hold

No selection timestamp, no selector, no approval state or status transitions, no pass/fail, no execution result, no target build, no environment, and no structured rationale field. Where those are needed, they are a separate tool's job — the same line [0020](0020-execution-status-lightweight-model.md) draws. How a selection came about is recorded by Git history.

### 3. Where it lives

In Git, at `.markharness/releases/<release_id>.yml`. That makes `--at <ref>` reproduce a past selection list directly, preserving the reproducibility contract (design P3).

Because `release_id` becomes a single component of that path, its value is constrained. Only ASCII lowercase alphanumerics, hyphen, and dot are allowed; the empty string, `.` and `..` themselves, dot-leading values, and anything containing a path separator (`/`, `\`) or a drive specifier are rejected before any file is created. This mirrors how `src/generate.rs`'s `require_valid_slug` already validates `id:` because it becomes a directory component under `generated/testcases/`, and it structurally rules out writing outside `.markharness/` or traversing directories. Ordinary tag names such as `v1.2.0` remain allowed. Writes go through the atomic replacement path in `src/fs_safety.rs`.

### 4. A human records it

`markharness release scope set` lets a human record or replace the list. markharness does not judge whether a selection is appropriate, and never generates one. For a release with no list, Release Coverage returns the registered-state listing exactly as before.

### 5. A selection list is not evidence of execution

Being in a `ReleaseScope` is a declaration that something was *chosen* — not that it ran, and not that it passed. The output never equates "selected" with "executed."

## Consequences

- `markharness coverage --release <release-id>` lists, for the selected TestCases, whether each has a verification method; TestCases under the target Requirements/Features that are absent from the list (candidate omissions); and Case UIDs in the list that do not exist in the Knowledge at that ref (design §6.2).
- Question 3 in design §1 is answerable down to "what was chosen" only for releases with a recorded `ReleaseScope`; otherwise the answer stops at reproducing registered state.
- On the roadmap it belongs to M2 (Release Coverage).

## Options considered and rejected

- **Rely on Git tags alone**: a tag marks a point in time but cannot distinguish the full Knowledge at that point from the subset actually selected, so "was everything in scope, or only part?" is unrecoverable afterwards.
- **Put a release identifier on `ExecutionStatus`**: per-TestCase records would multiply per release, drifting back toward the per-run records [0020](0020-execution-status-lightweight-model.md) deliberately avoided. A selection is a per-release set and is smaller when kept on the release side.
- **Add rationale, approver, and approval state**: that reintroduces an approval workflow, contradicting [0019](0019-alignment-check-commit-trailer.md)'s decision not to build one. The narrative that matters stays in commit history.
