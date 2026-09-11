# markharness v2 Design

Written: 2026-09-11 (original). Corrected the same day against the existing implementation under `src/` (§5.2.1, §6.1, §9.1). Rewritten in full the same day after a redesign session (grilling) deliberately not anchored to the existing design or vocabulary.
Status: design proposal. The types, CLI, and MVP scope below are a v2 proposal, not an implemented spec.

## 1. Conclusion and Product Thesis

When a developer changes a feature, a Requirement, or a test case, the team gets four answers:

1. Which test cases relate to a given feature, and which Requirements relate to it.
2. Detection of a missed test-case update or a missed spec update.
3. For the previous release, which tests were in the verification scope — as a decision aid.
4. For the current release, which features are affected — as a decision aid.

This is v2's North Star, and every design decision below is evaluated against these four points (glossary: [markharness-v2-glossary.md](markharness-v2-glossary.md); the canonical source of confirmed terms is [CONTEXT.md](../../../CONTEXT.md)).

Success is judged by three signals:

- Faster to notice "we're missing a test" during review.
- Fewer production incidents caused by a stale test that went unnoticed.
- Less manual traceability work in spreadsheets/wikis.

### 1.1 Main changes from the previous draft

This document was rewritten in full on 2026-09-11, deliberately not anchored to the existing design or concepts (the previous draft's heavyweight `Artifact`/`Evidence`/etc. model, or the existing implementation's retire/restore identity machinery). The main differences from the previous draft:

| Area | Previous draft | This draft |
|---|---|---|
| Execution result / evidence | Heavyweight Evidence/EvidenceSelection/Execution Manifest/Implementation revision/Environment matrix model | A single `automated`/`manual` axis plus an optional reference string only ([0020](../decisions/0020-execution-status-lightweight-model.md)) |
| Alignment check | A dedicated `AlignmentObligation`/`AlignmentDecision` domain type | A lightweight record via commit trailer ([0019](../decisions/0019-alignment-check-commit-trailer.md)) |
| Identity (retire/restore) | Kept a strict retire/restore/release guarantee | Simplified to "retire means deleted" ([0021](../decisions/0021-identity-retire-simplification.md)) |
| External integration (StrictDoc/Playwright) | Required for the MVP (baked into M0–M4) | Out of MVP scope; only the data shapes anticipate a future connection |
| Structure profile | A profile-switching mechanism (minimal-trace-v1/hierarchical-test-v1) | A single fixed structure (Feature → Behavior → Scenario → TestCase); no profile switching |
| GUI/dashboard | Excluded from MVP (deferred to Stage 3) | CLI/JSON output only, views delegated to a separate tool. The `server.rs` / `ui/` shipped as ADR 0008 Stage 3 are deleted ([0022](../decisions/0022-remove-stage3-dashboard.md)) |
| Owner of Requirement content | A native markharness entity only (`label`/`description`/`axis`) | Two modes, native and external; in external mode StrictDoc owns the content and markharness keeps only a fixed reference ([0023](../decisions/0023-requirement-native-and-external-source.md), §5.2.1) |

## 2. Basis

The primary source is the 2026-09-11 redesign grilling session (this conversation). The settled answers:

| Question | Settled answer |
|---|---|
| Scope of the redesign | Everything, including the existing `knowledge/` core (Feature/Behavior/Scenario/Axis/deterministic generation/ChangeEvent), was open to reconsideration — and the conclusion was to keep that core |
| What "feature" refers to | A spec-level concept (today's Feature). Diffing application source code is out of markharness's scope |
| Where specs live | StrictDoc (`.sdoc`, Git-managed). JSON-export ingestion / a custom parser are roadmap items |
| Where test cases live | The existing `knowledge/` assets, kept |
| Missed-update detection | Bidirectional (spec ⇄ test case). Automatic detection plus a lightweight confirmation record via commit trailer |
| Release granularity | Both PR base/head diff (day to day) and a full release listing (decision aid) |
| Execution result | Only a lightweight status (automated/manual) is kept. Evidence management is a separate tool's job (currently a spreadsheet) |
| Environment (browser/OS, etc.) | Outside the test case. Belongs to the spec document / execution tool |
| Identity | Simplified. No strict guarantee of restore or id-reservation after retirement |
| Backward compatibility | Not required (still a prototype with very few real users, same premise as [0017](../decisions/0017-scenario-case-revision-and-execution-evidence.md)'s Context) |

## 3. Design Principles

| ID | Principle | Consequence for the design |
|---|---|---|
| P1 | Respect the external source of truth | Don't duplicate or edit Requirement text from StrictDoc; keep only a fixed reference (id, revision) |
| P2 | Separate identity from revision | Never derive a UID unconditionally from a name, path, or content hash (keeps the existing `knowledge/` policy) |
| P3 | Judgments must be reproducible | The same Git snapshot always produces the same Change Impact / Release Coverage |
| P4 | Separate pass/fail from the verification method | Detailed pass/fail judgment is delegated to another tool; markharness only tracks "automated or manual, and where to look" (§5.2) |
| P5 | Core doesn't know external formats | StrictDoc-specific fields and conversion rules live in a future Adapter, never in Core |
| P6 | Extensions are judged against the thesis | Don't build a generic plugin foundation or bespoke business workflow ahead of need (the YAGNI principle in [CLAUDE.md](../../../CLAUDE.md)) |
| P7 | Concentrate judgment in one place | CLI and CI consume the same Application result (continues [0008](../decisions/0008-verification-plan-product-roadmap.md)'s modular-monolith policy) |

## 4. Responsibility Boundaries

| Area | Source of truth / owner | What markharness keeps |
|---|---|---|
| Requirement text and structure | external: StrictDoc / native: markharness | External keeps only a fixed reference (id, revision); native holds `label`/`description` ([0023](../decisions/0023-requirement-native-and-external-source.md)) |
| Test-case intent, steps, expected results | markharness native (`knowledge/`) | Feature/Behavior/Scenario/TestCase themselves (existing assets, kept) |
| Execution, evidence artifacts, timestamp, executor | A separate tool (currently a spreadsheet; Playwright etc. in the future) | Only a single `automated`/`manual` axis plus an optional reference string |
| Execution environment (browser/OS, etc.) | The spec document / execution tool | Not kept |
| Alignment-check records | Git commit history (trailer) | Only the automatic detection of whether a check is needed; the record itself lives in Git history |
| Computing Change Impact / Release Coverage | markharness Core | Deterministic diff / listing computation |

## 5. Domain Model

### 5.1 Core concepts (kept as-is)

Feature, Behavior, Scenario, TestCase, Axis, Case revision, and ChangeEvent are kept exactly as the current `knowledge/` implementation has them. Deterministic generation, Git-tree-SHA-based revision comparison, and Axis-based cross-cutting search are out of scope for this redesign and are unchanged.

Note that the current implementation also holds Requirement as a *native* entity at `knowledge/requirements/<id>/requirement.yml`, with its own UID, `label`, `description`, and `axis`, related many-to-many through `feature.requirement_uids` ([0017](../decisions/0017-scenario-case-revision-and-execution-evidence.md) §1/§3; `src/knowledge.rs`, `src/identity/entity_kind.rs`, `src/traceability.rs`). Requirement is not part of case identity (`case_id = tc-{feature}-{behavior}-{scenario}`, `src/generate.rs`). §5.2's `Requirement` is therefore not a newly added concept but **the addition of a mode in which the content is owned externally** ([0023](../decisions/0023-requirement-native-and-external-source.md), §5.2.1). markharness must remain usable on its own for teams that have not adopted StrictDoc, so ownership is never forced outward.

### 5.2 New concepts

```text
Requirement {
  id,                    // display id (the current `requirement.yml` `id`); in external mode it equals StrictDoc's UID
  uid,                   // the current Requirement UID (ADR 0013) is preserved
  source: native | external,   // defaults to native
  axis,                  // kept in both modes: markharness's own classification, not a copy of external content

  // required when source = native; not allowed in external mode
  label,
  description,           // optional

  // required when source = external; not allowed in native mode
  source_locator,        // the .sdoc path, inside the same Git repository
  source_revision,       // the Git blob OID pinned at link time
}

ExecutionStatus {
  case_uid,              // referenced by Case UID, not by display id (ADR 0013; survives renames)
  mode: automated | manual,
  reference: string,     // optional — a path to the test code, or a URL
}
```

A `source: external` Requirement never duplicates StrictDoc's own content: markharness keeps only a fixed reference, and the body/acceptance criteria are always looked up from StrictDoc itself (P1). In `source: native` mode markharness continues to own `label`/`description`. A `requirement.yml` carrying both sets of fields, or neither, is rejected by `validate` ([0023](../decisions/0023-requirement-native-and-external-source.md)).

The many-to-many relation from Feature to Requirement reuses the existing `feature.requirement_uids` field as-is; no new `ContributesTo` type or store is introduced (the Feature side owns it, the reverse listing is derived — [0017](../decisions/0017-scenario-case-revision-and-execution-evidence.md) §1/§3). It means "contributes to realization" only — not proof of verification. Not introducing a new type when an existing field suffices follows P6 (YAGNI).

`ExecutionStatus` is the minimal per-TestCase record described in §1.1 / [0020](../decisions/0020-execution-status-lightweight-model.md). It records **the verification method (automated or manual) and where to look**, not the fact that an execution happened. Because it carries no result, timestamp, or run count, the presence of a value must not be read as "executed against the latest revision."

### 5.2.1 Relationship to the current native Requirement

| Item | Current | This edition |
|---|---|---|
| `source` | Absent | Added; absent means `native` |
| `requirement.yml`'s `label`/`description` | markharness holds body-equivalent content | Kept in native mode. Not allowed in external mode (P1 — when a display name is needed, the M3 StrictDoc Adapter fetches it on demand) |
| `source_locator`/`source_revision` | Absent | Required in external mode, not allowed in native mode. Missing or mixed fields are rejected by `validate` |
| `axis` | Held | Kept in both modes (markharness's own classification, not a copy of external content) |
| `uid` / `feature.requirement_uids` | ADR 0013 UID and the many-to-many relation | Kept as-is |
| The interactive authoring flow's Requirement prompts (`src/interactive.rs`, `knowledge_draft.rs`) | Prompt for `label`/`axis` | Unchanged for native; only when external is chosen does it switch to a `source_locator` prompt (to stay consistent with AC02) |
| `traceability.rs`'s Requirement index and `GeneratedFrom.requirement_ids`/`requirement_uids` | Implemented | Kept |

Existing `requirement.yml` files stay valid as native (`source` omitted) and need no conversion. Moving one to external mode is a human rewrite; no automatic conversion is built (§2, no backward compatibility).

### 5.3 Alignment check

When a Requirement's meaning changes, or a TestCase's effective content changes, markharness checks whether the other side (TestCase or Requirement) was updated within the same Git diff range, or was explicitly confirmed as not needing a change via a commit trailer like `Spec-Reviewed: no-change-required` ([0019](../decisions/0019-alignment-check-commit-trailer.md)). If neither holds, it is listed as "unconfirmed." No dedicated approval workflow is introduced.

The trailer must identify its target (e.g. `Spec-Reviewed: no-change-required (req-login-01)`). When one commit touches several Requirements or TestCases, a trailer without a target cannot say which alignment check was actually done (the exact format is settled in implementation design, per [0019](../decisions/0019-alignment-check-commit-trailer.md)). Also, in a squash-merged PR the trailer line can end up in the middle of the merge commit body, so the check must scan every commit body in `git log base..head` rather than only the last line.

## 6. Change Impact and Release Coverage

### 6.1 Change Impact (per PR)

On top of the Feature-revision comparison between base and head (reusing the current `changes.rs` `ChangeEvent` computation), markharness:

1. Identifies the Requirements that changed Features `contributes_to`.
2. Decides whether the spec side changed, according to the Requirement's mode.
   - `source: native`: from the base/head diff of `requirement.yml` itself. Granularity is per Requirement and no external tool is involved.
   - `source: external`: by comparing the pinned `source_revision` against the blob OID of `source_locator` at head. This assumes the `.sdoc` file is **managed in the same Git repository as markharness**, and requires no `.sdoc` parsing. Granularity is per file: a change to another Requirement in the same file also reads as "changed" (false positives are accepted; per-Requirement granularity waits for M3's `.sdoc` parsing).
3. Computes the Alignment-check state (confirmed/unconfirmed) for each changed TestCase and Requirement.
4. Outputs the affected TestCase list, the related Requirement list, and any unconfirmed alignment checks.

With this mechanism, Change Impact (M1) depends neither on the `.sdoc` parser (M3) nor on whether StrictDoc is adopted at all. In external mode, the pinned reference must be advanced to the new blob OID once the change is reviewed (`requirement repin`, §7); until then the same Requirement keeps being reported as changed in later diffs.

### 6.2 Release Coverage (per release)

For a given set of Requirements/Features, markharness lists:

- Whether each TestCase has an `ExecutionStatus`, and its `mode`.
- Whether each Requirement has any Feature `contributes_to` it (a coverage gap).

Where Change Impact shows "what changed in this diff," Release Coverage is supporting information showing "can we see the whole release target without missing anything" — used alongside Change Impact when making a release decision.

Release Coverage is evaluated at a given Git ref (HEAD by default). Question 3 in §1 (which tests were in the verification scope of the previous release) is answered by passing that release tag to `--at` and evaluating the Knowledge and `ExecutionStatus` as they stood then. `ExecutionStatus` itself carries no timestamp or release number, so the point in time is delegated to the Git ref (P3).

## 7. CLI Proposal

```text
markharness requirement link --feature <feature-id> --requirement <requirement-id>
markharness requirement unlink --feature <feature-id> --requirement <requirement-id>
markharness requirement repin --requirement <requirement-id>   # external mode only; advance source_revision to the blob OID at head
markharness execution set --case-uid <case-uid> --mode automated --reference src/tests/login.spec.ts
markharness execution set --case-uid <case-uid> --mode manual
markharness impact --base <ref> --head <ref> --format json
markharness coverage --requirements <requirement-ids-or-all> --at <ref> --format json
```

`requirement link`/`unlink` edit `feature.yml`'s `requirement_uids`; they introduce no new store (§5.2). Output is CLI/JSON only; no local server or dashboard is in the MVP (§8). Exit codes and JSON schema versioning policy are settled at implementation time. Commands that are removed are covered in §9.1.

## 8. Non-Goals

- A StrictDoc requirement-editing UI, or a bespoke requirement-approval workflow.
- Any new test-case CRUD UI (the existing `knowledge/` editing flow is kept, unchanged).
- Detailed execution-result management (pass/fail, evidence artifacts, an environment matrix) — a separate tool's job.
- Playwright code generation, an execution engine, or automated CI wiring. Designed later, once a concrete request exists.
- A custom `.sdoc` parser or automated JSON-export ingestion for StrictDoc (a roadmap item, §2).
- A strict post-retirement identity guarantee (restore, id reservation) ([0021](../decisions/0021-identity-retire-simplification.md)).
- A dashboard, SaaS, RBAC/SSO, or a shared database (the existing Stage 3 dashboard is deleted too — [0022](../decisions/0022-remove-stage3-dashboard.md)).

## 9. Reuse, Replacement, and Removal of Existing Implementation

| Existing asset | Decision | Reason |
|---|---|---|
| `knowledge/` (Feature/Behavior/Scenario/Axis, deterministic generation) | Kept | The North Star's foundational asset. §5.1 |
| `changes.rs` (ChangeEvent computation) | Kept and extended | Used as-is as the basis for Change Impact. §6.1 |
| `case_definition.rs` (frozen Case-revision storage) | Kept | Already implements [0017](../decisions/0017-scenario-case-revision-and-execution-evidence.md) §1–4 |
| `execution.rs` (target_revision/environment, etc.) | Reduced | Replaced by [0020](../decisions/0020-execution-status-lightweight-model.md)'s `ExecutionStatus` |
| `plan.rs` (evidence applicability judgment) | Reduced/replaced | The strict matching logic is no longer needed; simplified to checking whether an `ExecutionStatus` exists |
| `src/identity/` (retire/restore/release/reissue portions) | Reduced | [0021](../decisions/0021-identity-retire-simplification.md) |
| `src/identity/` (UID issuance/rename portions) | Kept | Out of scope for [0021](../decisions/0021-identity-retire-simplification.md) |
| `src/git.rs`, `fs_safety.rs` | Kept | Immutable-ref reads and atomic file operations are independent of this redesign |
| `src/canonical.rs` (ImportSourceArg, etc.) | Revisit | StrictDoc ingestion is redesigned as a future, separate Adapter; its relationship with the current Native/JUnit importers is sorted out when work begins |
| `knowledge/requirements/` (the native Requirement entity) | Reduced / meaning changed | Body-equivalent fields (`label`/`description`) are dropped in favor of a fixed reference. §5.2.1 |
| `src/traceability.rs` (Requirement index) | Kept | The existing Requirement↔TestCase reverse lookup is used as-is |
| `src/server.rs`, `ui/`, `markharness serve` (the ADR 0008 Stage 3 dashboard) | Removed | The current UI depends on `plan`/evidence output and breaks when `plan` is reduced ([0022](../decisions/0022-remove-stage3-dashboard.md)). §9.1 |
| `src/milestone.rs`, `src/backfill.rs` | Decision needed | Whether they fold into base/head Change Impact is decided when work begins. §9.1 |
| `src/verify.rs`, `audit_scope.rs`, `derived_index.rs`, `lineage.rs` | To be inventoried | Unclassified in this document; keep/reduce/remove is settled at the start of M0 |
| The `identity` CLI's `retire`/`restore`/`release`/`reissue` | Removed | [0021](../decisions/0021-identity-retire-simplification.md). Existing event logs are covered in §9.1 |

### 9.1 Existing CLI, data, and UI

- **Commands removed**: `identity retire`/`restore`/`release`/`reissue` ([0021](../decisions/0021-identity-retire-simplification.md)), and the evidence-related options of `plan` and `execution record` ([0020](../decisions/0020-execution-status-lightweight-model.md)). The exact removal scope is settled in the implementation checklist.
- **Existing data**: the execution records under `.markharness/executions/` and any `.markharness/identity-events/` log containing `retire`/`release` events are not converted automatically (§2). On replay, removed event kinds are ignored with a warning; their mere presence must not fail the run.
- **The existing dashboard**: `src/server.rs`, `ui/`, `markharness serve`, and the embedded frontend assets are deleted ([0022](../decisions/0022-remove-stage3-dashboard.md)). The removal happens at the same time as the `plan` reduction, together with the related tests (`tests/server.rs` and friends). A viewer outside this repository that reads `plan` output has to switch to Change Impact / Release Coverage output.

## 10. Roadmap

| Stage | Builds | Exit criteria |
|---|---|---|
| M0 | The new `Requirement` schema (native/external modes) and `ExecutionStatus` schema, linking via `feature.requirement_uids`, the CLI (§7), automatic Alignment-check detection (§5.3), and the updated interactive authoring flow (§5.2.1) | Both native operation (no StrictDoc) and external operation complete Feature↔Requirement linking and TestCase ExecutionStatus recording end-to-end via Git/CLI, and a `requirement.yml` mixing the two modes is rejected |
| M1 | Change Impact (§6.1) | Between a PR's base and head, affected Features, Requirements, and unconfirmed alignment checks can be listed (no dependency on `.sdoc` parsing = M3) |
| M2 | Release Coverage (§6.2) | Coverage gaps across a given set of Requirements can be listed |
| M3 (future) | StrictDoc `.sdoc` ingestion (reflecting the actual Git-managed requirement content) | Started once demand is confirmed; whether a custom parser is needed is designed separately at that time |
| M4 (future) | Playwright integration (ingesting automated execution results) | Started once a concrete request exists; designed against the `ExecutionStatus` shape from §9 |

The MVP is M0–M2. M3 and M4 are not committed to as of this document.

## 11. Acceptance Criteria

| ID | Scenario | Expected result |
|---|---|---|
| AC01 | Link a Feature to a Requirement via `contributes_to` | The Feature side owns the relation; the reverse listing is derived |
| AC02 | Attempt to edit a Requirement's content from markharness | Rejected. markharness keeps only a fixed reference to a Requirement |
| AC03 | A Requirement changes but its related TestCase is not updated | Change Impact's output marks it "unconfirmed" |
| AC04 | A TestCase-change commit carries `Spec-Reviewed: no-change-required` | The Alignment check is judged "confirmed" |
| AC05 | Record `ExecutionStatus(mode=manual)` on a TestCase without a timestamp or executor | The record succeeds; there are no timestamp/executor fields to omit |
| AC06 | Compute Change Impact / Release Coverage multiple times from the same input | The same output is reproduced (P3) |
| AC07 | Re-add a TestCase with the same content as one that was retired (deleted) | Treated as a new, distinct TestCase; the old UID is not carried over ([0021](../decisions/0021-identity-retire-simplification.md)) |
| AC08 | A Requirement has no Feature `contributes_to` it | Listed as a coverage gap in Release Coverage |
| AC09 | A `source: external` `requirement.yml` without `source_locator`/`source_revision` | `validate` rejects it (§5.2.1) |
| AC09b | An existing `requirement.yml` with `label` and no `source` field | Valid as native; Change Impact and Release Coverage work with no StrictDoc present ([0023](../decisions/0023-requirement-native-and-external-source.md)) |
| AC09c | A `requirement.yml` carrying both `label` and `source_locator` | `validate` rejects it (mixed modes) |
| AC10 | For a `source: external` Requirement, the `.sdoc` blob at head differs from the pinned reference | Change Impact reports a spec-side change, without parsing the `.sdoc` (§6.1) |
| AC10b | A `source: native` Requirement's `label`/`description` changes between base and head | Change Impact reports a spec-side change (§6.1) |
| AC11 | Compute Release Coverage with a past release tag passed to `--at` | The listing reproduces the Knowledge and ExecutionStatus as of that ref (§6.2) |
| AC12 | One commit touches several Requirements and carries a trailer without a target | It stays "unconfirmed", because which check was done cannot be determined (§5.3) |
| AC13 | Rename a Scenario's display id | `ExecutionStatus` survives, because it references the Case UID (§5.2) |
