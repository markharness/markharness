# 0020: Lighten execution status and drop evidence management from markharness's scope

## Status

Accepted (design agreed, implementation pending). Based on the redesign grilling session (2026-09-11) for [markharness-v2-design.md](../design/markharness-v2-design.md). Supersedes, within its own scope (execution records attached to a TestCase), the Evidence Applicability model in [0017](0017-scenario-case-revision-and-execution-evidence.md) §5 ("Execution evidence and applicability"). §1–4 (Scenarios, shared procedures, Case revision, split/merge) are out of scope for this ADR and remain in effect.

## Context

[0017](0017-scenario-case-revision-and-execution-evidence.md) envisioned separating execution result, applicability, and presence of evidence, adopting evidence toward a pass only when Case UID, Case revision, target build, and required environment all match exactly. The later, now-superseded draft of [markharness-v2-design.md](../design/markharness-v2-design.md) built on this and expanded it further into a heavyweight set of types: Evidence, EvidenceSelection, an Execution Manifest, Implementation revision, and an Environment matrix.

Re-deriving the North Star (what answer someone wants in hand once they're done using this tool) during the grilling session showed the actual required level is much lower:

- "What ran in the previous release" is mainly about scope — was this test in the verification set at all — with the pass/fail result being secondary, supporting information.
- Managing detailed evidence (screenshots, logs, execution timestamp, executor) is not markharness's job; that belongs to a separate tool (currently a spreadsheet with pass/fail checkmarks).
- The one distinction that *is* needed is automated vs. manual, because it directly determines whether a reviewer can just read the test code or has to consult separate material.
- Which environment (browser/OS) a test ran in is not a property of the test case itself — it belongs to the spec document or the execution tool (Playwright, etc.), not to markharness's domain.

This is clearly smaller than the build/environment-level matching [0017](0017-scenario-case-revision-and-execution-evidence.md) §5 called for, and does not justify the implementation cost of the `Evidence`/`EvidenceSelection`/`Manifest`/`Implementation revision`/`Environment` type family (YAGNI).

## Decision

### 1. Execution status is one mode field plus an optional reference string

The execution record attached to a TestCase carries exactly two fields:

```text
ExecutionStatus {
  mode: automated | manual,
  reference: string (optional, free text — e.g. a path to the test code or a URL)
}
```

No detailed result (pass/fail/skip), execution timestamp, executor, target build, environment, or evidence artifact is stored. Where any of that is needed, `reference` points to where it lives (a separate tool).

The TestCase is referenced by its **Case UID**, not by display id ([0013](0013-immutable-identity-model.md)), so that renaming a display id does not orphan the record.

A note on naming: this record states **the verification method (automated or manual) and where to look**, not the fact that an execution happened. Because it carries no timestamp, run count, or result, the presence of a value must not be read as "executed against the latest Case revision." The name "Execution status" does not imply that limit, so even if the field is renamed at implementation time, the meaning defined in this section governs.

### 2. "Was it run" does not require build/environment matching

The strict applicability check [0017](0017-scenario-case-revision-and-execution-evidence.md) §5 required — Case UID, Case revision, target build, and required environment all matching — is not introduced. Change Impact / Release Coverage computations consult only whether a TestCase has an `ExecutionStatus` and its `mode`. If build- or environment-precise pass/fail tracking is genuinely needed later, that concrete requirement drives a separate ADR at that time.

### 3. Storing and managing evidence is out of markharness's scope

The concepts and types for immutable evidence storage, EvidenceSelection, and an Execution Manifest are not introduced into markharness. CI-driven automatic evidence ingestion is likewise deferred until an actual request arises; the MVP records `ExecutionStatus` through the CLI, by a human.

### 4. Shape the data with a future Playwright connection in mind

Direct integration with an automated test tool (CI wiring, reporter ingestion) is out of scope for the MVP, but the shape — `mode: automated` with `reference` holding a test-file path — is chosen so it can later map cleanly onto Playwright's inventory/annotations. No tool-specific fields (project, locator, etc.) are added at this time.

## Impact

- The Evidence Applicability model envisioned in [0017](0017-scenario-case-revision-and-execution-evidence.md) §5 will not be implemented; that ADR's Status section is updated to point here.
- The `target_revision` and `environment` fields currently on `src/execution.rs` become candidates for reduction under this decision (tracked separately as implementation work).
- [markharness-v2-design.md](../design/markharness-v2-design.md) is rewritten in full to match this ADR.

## Alternatives considered and rejected

- **Keep the environment matrix** — would let per-browser/OS verification status be tracked precisely, but makes the test case itself depend on environment, contradicting this redesign's agreed position that "a test case verifies a feature and does not depend on environment." Environment-specific status belongs to the spec document or the execution tool.
- **Store the pass/fail result** — useful as a decision aid, but a bare pass/fail with no accompanying evidence management (when, by whom, on which build) tends to create false confidence, and conflicts with delegating evidence management to a separate tool. Kept to the single "was it run" axis instead.
