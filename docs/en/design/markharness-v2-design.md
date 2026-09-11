# markharness v2 Design

Written: 2026-09-11 (original). Corrected the same day against the existing implementation under `src/` (§5.2.1, §6.1, §9.1). Rewritten in full the same day after a redesign session (grilling) deliberately not anchored to the existing design or vocabulary. Contracts that preserve an evolution path after real StrictDoc → markharness → Playwright operation were then added in §9.2 and [ADR 0025](../decisions/0025-v2-forward-compatible-evolution.md).
Status: design proposal. The types, CLI, and MVP scope below are a v2 proposal, not an implemented spec.

## 1. Conclusion and Product Thesis

When a developer changes a feature, a Requirement, or a test case, the team gets four answers:

1. Which test cases relate to a given feature, and which Requirements relate to it.
2. Detection of a missed test-case update or a missed spec update.
3. For the previous release, which tests were in the verification scope — as a decision aid.
4. For the current release, which features are affected — as a decision aid.

For question 3, markharness answers "what was chosen for verification" when that release has a recorded `ReleaseScope` (a selection list), and otherwise "which TestCases and verification methods were registered at that point". Neither answers whether anything actually ran (§6.2). This is v2's North Star, and every design decision below is evaluated against these four points (glossary: [markharness-v2-glossary.md](markharness-v2-glossary.md); the canonical source of confirmed terms is [CONTEXT.md](../../../CONTEXT.md)).

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

ExecutionBinding {
  case_uid,              // referenced by Case UID, not by display id (ADR 0013; survives renames)
  mode: automated | manual,
  reference: string,     // optional — a path to the test code, or a URL
}

ReleaseScope {
  release_id,            // the release's display name (a Git tag name is recommended); must be a safe single path component (below)
  case_uids: [case_uid], // the TestCases chosen for verification in that release
}
```

A `source: external` Requirement never duplicates StrictDoc's own content: markharness keeps only a fixed reference, and the body/acceptance criteria are always looked up from StrictDoc itself (P1). In `source: native` mode markharness continues to own `label`/`description`. A `requirement.yml` carrying both sets of fields, or neither, is rejected by `validate` ([0023](../decisions/0023-requirement-native-and-external-source.md)).

The many-to-many relation from Feature to Requirement reuses the existing `feature.requirement_uids` field as-is; no new `ContributesTo` type or store is introduced (the Feature side owns it, the reverse listing is derived — [0017](../decisions/0017-scenario-case-revision-and-execution-evidence.md) §1/§3). It means "contributes to realization" only — not proof of verification. Not introducing a new type when an existing field suffices follows P6 (YAGNI).

`ReleaseScope` records only "what was chosen for verification in this release" ([0024](../decisions/0024-release-scope-selection-list.md)). It carries no selection timestamp, owner, approval state, or result, and a human records it through the CLI. It lives in Git at `.markharness/releases/<release_id>.yml`, so `--at <ref>` reproduces a past selection as well. Because `release_id` becomes a **single component** of that path, its character set is constrained for the same reason `generate.rs`'s `require_valid_slug` constrains `id:` today: ASCII lowercase alphanumerics, hyphen, and dot only, rejecting — before any write — the empty string, `.` and `..` themselves, values starting with a dot, and anything containing a path separator (`/`, `\`) or a drive specifier (so `v1.2.0` passes and `../../etc/passwd` does not). The write itself goes through `fs_safety`'s atomic replacement path. For a release with no selection list, Release Coverage returns the registered-state listing exactly as before (§6.2).

`ExecutionBinding` is the minimal per-TestCase record described in §1.1 / [0020](../decisions/0020-execution-status-lightweight-model.md) / [0025](../decisions/0025-v2-forward-compatible-evolution.md). It declares **the relationship to a verification method (automated or manual) and where to look**, not the fact that an execution happened. Because it carries no result, timestamp, or run count, the presence of a value must not be read as "executed against the latest revision." It is never converted into or reinterpreted as a future Execution Fact; that is a separate type.

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

When a Requirement's meaning changes, or a TestCase's effective content changes, markharness reports the state of the other side as one of **three values** ([0019](../decisions/0019-alignment-check-commit-trailer.md)). No dedicated approval workflow is introduced.

| State | Condition |
|---|---|
| Followed | The other side's effective content also changed within the same base/head range. This is not evidence that a human checked semantic agreement |
| Confirmed | A valid `Spec-Reviewed` trailer names the target |
| Unconfirmed | Neither of the above |

"Followed" and "Confirmed" are never collapsed into one state: two files happening to change in the same PR does not establish semantic agreement — exactly the premise [0019](../decisions/0019-alignment-check-commit-trailer.md) starts from, now carried into the output.

Trailers are governed by the following rules.

1. **A trailer whose target cannot be resolved is not accepted.** Write the target, as in `Spec-Reviewed: no-change-required (req-login-01)`. A targetless trailer never clears several Requirements/TestCases at once; they stay unconfirmed. When a one-sided target does not resolve its counterpart uniquely, name both endpoints, as in `Spec-Reviewed: no-change-required (req-login-01, <case-uid>)` (rule 2).
2. **Validity is scoped to the requirement/case pair at the commit.** Bind each confirmation to a Requirement UID / Case UID pair resolved at the commit carrying the trailer. Resolve display IDs at that commit. If a one-sided target plus the changes and relations cannot identify the other endpoint uniquely, do not accept it; require both endpoints to be specified. Resolve revisions from Git without manually entered revision strings or a separate persistent type. If either endpoint changes effectively in a subsequent commit in the range (Case revision for the TestCase; `requirement.yml` or the `.sdoc` blob for the Requirement), invalidate confirmation for that pair. Do not reuse it for another pair or extend it to cases added later. An invalid record cannot establish confirmed status; absent another valid record, report followed or unconfirmed under the rules in design §5.3.
3. **The syntax is narrow.** Only a line of the form `Spec-Reviewed: <value>` starting at the beginning of a line in a commit body is read as a trailer. Quoted lines, indented lines, and the same text inside a code block are ignored, so a mention in prose is never mistaken for a declaration.
4. **Squash merges are handled.** The check scans every commit body in `git log base..head` rather than only the last line, and a record whose validity scope (rule 2) cannot be resolved is not accepted.
5. **History is a declared input.** Change Impact's inputs are the Knowledge/`.sdoc` trees *and* the `base..head` commit history (this belongs to P3's reproducibility contract). If the history is unavailable (shallow clone, filtered clone), the run fails with a diagnostic; missing history is never reported as "confirmed".

## 6. Change Impact and Release Coverage

### 6.1 Change Impact (per PR)

On top of the Feature-revision comparison between base and head (reusing the current `changes.rs` `ChangeEvent` computation), markharness:

1. **Builds the changed set in both directions.** It walks from changed Features to the Requirements they `contributes_to`, *and* from changed Requirements to their related Features and TestCases. The search never depends on a Feature having changed, so a PR that touches only a Requirement is not missed.
2. **Detects spec-side change from the base/head diff.** In both modes the comparison is "content at base" versus "content at head".
   - `source: native`: the base/head diff of `requirement.yml` itself. Granularity is per Requirement and no external tool is involved.
   - `source: external`: the base/head diff of the `.sdoc` blob named by `source_locator`. This assumes the `.sdoc` file is **managed in the same Git repository as markharness**, and requires no `.sdoc` parsing. Granularity is per file: a change to another Requirement in the same file also reads as "changed" (false positives are accepted; per-Requirement granularity waits for M3's `.sdoc` parsing).
3. **Reports a stale pin as its own item.** In external mode, when `source_revision` does not match the blob OID at head, that is output as "pinned reference is stale." It is independent of step 2: advancing the pin with `requirement repin` must never cancel out detection of a spec change (changing the `.sdoc` and repinning inside the same PR still leaves the step-2 diff intact).
4. Computes the Alignment-check state (the three values of §5.3) for each changed TestCase and Requirement.
5. Outputs the affected TestCase list, the related Requirement list, the alignment states, and the stale-pin list.

With this mechanism, Change Impact (M1) depends neither on the `.sdoc` parser (M3) nor on whether StrictDoc is adopted at all. `repin` merely advances a pinned reference to its current value; it is not a substitute for an alignment check (only the §5.3 trailer records that). In the next, unchanged PR there is no base/head diff, so nothing is reported as a new spec change.

### 6.2 Release Coverage (per release)

For a given set of Requirements/Features, markharness lists:

- Whether each TestCase has an `ExecutionBinding`, and its `mode`.
- Whether each Requirement has any Feature `contributes_to` it (a coverage gap).
- Whether a Feature in scope has no Scenario/TestCase at all (a coverage gap). A Requirement with a related Feature but zero verification examples produces no TestCase rows to show as missing, so the gap is stated per Feature.

Where Change Impact shows "what changed in this diff," Release Coverage is supporting information showing "can we see the whole release target without missing anything" — used alongside Change Impact when making a release decision.

Release Coverage is evaluated at a given Git ref (HEAD by default). Without `--release`, the output means **"the TestCases and verification methods registered in Knowledge at that point"** — not evidence that anything ran. The presence of a `mode` is never displayed as "executed" (§5.2).

With `--release <release-id>`, markharness reads that `ReleaseScope` (§5.2, [0024](../decisions/0024-release-scope-selection-list.md)) and additionally shows:

- Whether each selected TestCase has an `ExecutionBinding`, and its `mode`.
- TestCases under the target Requirements/Features that are *not* in the selection list (candidate omissions).
- Case UIDs in the selection list that do not exist in the Knowledge at that ref (deleted or not generated).

Question 3 in §1 (which tests were in the verification scope of the previous release) is answered by passing the release tag to `--at` and that release's `release_id` to `--release`. For a release with no recorded `ReleaseScope`, the answer stops at reproducing registered state. Neither `ExecutionBinding` nor `ReleaseScope` carries a timestamp, so the point in time is delegated to the Git ref (P3). A selection list is a human's declaration that something was *chosen* — never evidence that it ran.

## 7. CLI Proposal

```text
markharness requirement link --feature <feature-id> --requirement <requirement-id>
markharness requirement unlink --feature <feature-id> --requirement <requirement-id>
markharness requirement repin --requirement <requirement-id>   # external mode only; advance source_revision to the blob OID at head
markharness binding set --case-uid <case-uid> --mode automated --reference src/tests/login.spec.ts
markharness binding set --case-uid <case-uid> --mode manual
markharness release scope set --release <release-id> --case-uid <case-uid> [--case-uid ...]   # replaces the selection list
markharness release scope show --release <release-id> [--at <ref>] --format json
markharness impact --base <ref> --head <ref> --format json
markharness coverage --requirements <requirement-ids-or-all> [--release <release-id>] --at <ref> --format json
```

`requirement link`/`unlink` edit `feature.yml`'s `requirement_uids`; they introduce no new store (§5.2). Output is CLI/JSON only; no local server or dashboard is in the MVP (§8). Exit codes and JSON schema versioning policy are settled at implementation time. Commands that are removed are covered in §9.1.

## 8. Non-Goals

- A StrictDoc requirement-editing UI, or a bespoke requirement-approval workflow.
- Any new test-case CRUD UI (the existing `knowledge/` editing flow is kept, unchanged).
- Detailed execution-result management (pass/fail, evidence artifacts, an environment matrix) — a separate tool's job. `ReleaseScope` records only the declaration that something was chosen, never a result ([0024](../decisions/0024-release-scope-selection-list.md)).
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
| `execution.rs` (target_revision/environment, etc.) | Reduced | Replaced by [0020](../decisions/0020-execution-status-lightweight-model.md) / [0025](../decisions/0025-v2-forward-compatible-evolution.md)'s `ExecutionBinding` |
| `plan.rs` (evidence applicability judgment) | Reduced/replaced | The strict matching logic is no longer needed; simplified to checking whether an `ExecutionBinding` exists |
| `src/identity/` (retire/restore/release/reissue portions) | Reduced | [0021](../decisions/0021-identity-retire-simplification.md) |
| `src/identity/` (UID issuance/rename portions) | Kept | Out of scope for [0021](../decisions/0021-identity-retire-simplification.md) |
| `src/git.rs`, `fs_safety.rs` | Kept | Immutable-ref reads and atomic file operations are independent of this redesign |
| `src/canonical.rs` (ImportSourceArg, etc.) | Revisit | StrictDoc ingestion is redesigned as a future, separate Adapter; its relationship with the current Native/JUnit importers is sorted out when work begins |
| `knowledge/requirements/` (the native Requirement entity) | Kept and extended | Native Requirements keep `label`/`description` unchanged; only a Requirement set to `source: external` drops body-equivalent fields in favor of a fixed reference ([0023](../decisions/0023-requirement-native-and-external-source.md), §5.2.1) |
| `src/traceability.rs` (Requirement index) | Kept | The existing Requirement↔TestCase reverse lookup is used as-is |
| `src/server.rs`, `ui/`, `markharness serve` (the ADR 0008 Stage 3 dashboard) | Removed | The current UI depends on `plan`/evidence output and breaks when `plan` is reduced ([0022](../decisions/0022-remove-stage3-dashboard.md)). §9.1 |
| `src/milestone.rs`, `src/backfill.rs` | Decision needed | Whether they fold into base/head Change Impact is decided when work begins. §9.1 |
| `src/verify.rs`, `audit_scope.rs`, `derived_index.rs`, `lineage.rs` | To be inventoried | Unclassified in this document; keep/reduce/remove is settled at the start of M0 |
| The `identity` CLI's `retire`/`restore`/`release`/`reissue` | Removed | [0021](../decisions/0021-identity-retire-simplification.md). Existing event logs are covered in §9.1 |

### 9.1 Existing CLI, data, and UI

- **Commands removed**: `identity retire`/`restore`/`release`/`reissue` ([0021](../decisions/0021-identity-retire-simplification.md)), and the evidence-related options of `plan` and `execution record` ([0020](../decisions/0020-execution-status-lightweight-model.md)). The exact removal scope is settled in the implementation checklist.
- **Existing data**: the execution records under `.markharness/executions/` and any `.markharness/identity-events/` log containing `retire`/`restore`/`release`/`reissue` events are not converted automatically (§2). A log containing a removed event kind is **rejected, with a diagnostic naming the offending events**. Ignoring them with a warning is not an option: `IdentityEvent` ordering is decided by a single causal chain of `previous_identity_event_uid` (`src/identity/event.rs`), and `Released` changes the id↔UID allocation itself, so skipping events mid-chain leaves dangling predecessors or evaluates from a state the log never recorded. A rejected run modifies nothing — not the event log, not Knowledge, not generated output. No compatibility replay or automatic conversion is built; the diagnostic only points at the cause so a human can prepare input in the new form.
- **The existing dashboard**: `src/server.rs`, `ui/`, `markharness serve`, and the embedded frontend assets are deleted ([0022](../decisions/0022-remove-stage3-dashboard.md)). The removal happens at the same time as the `plan` reduction, together with the related tests (`tests/server.rs` and friends). A viewer outside this repository that reads `plan` output has to switch to Change Impact / Release Coverage output.

### 9.2 Contracts V2 preserves for future evolution

V2 is not a partial implementation of the future full model. It is completed as a small product that answers its four North Star questions on its own. The StrictDoc → markharness → Playwright flow is then used in real operation, and only concepts whose need is demonstrated are added. [0025](../decisions/0025-v2-forward-compatible-evolution.md) is authoritative for the rationale.

#### 9.2.1 Shared foundations that are expensive to change later

V2 stabilizes the following contracts:

- Requirement, Feature, Behavior, and Scenario use kind-distinguished UIDs, separating identity from display id, label, and path.
- One Scenario equals one TestCase, with Case UID derived deterministically from Scenario UID.
- Case revision is calculated from effective verification content and excludes Requirement relations, execution results, Release Scope, and display information.
- An external Requirement distinguishes its external key, same-repository locator, and fixed revision. Even when V2 detects changes at file granularity, it retains the identity needed for a later StrictDoc Adapter to resolve Requirement-level content.
- A Playwright relation uses Case UID, not test title or filename. `reference` is a movable navigation hint, not the identity used for matching.
- Public Change Impact and Release Coverage JSON has a top-level `schema_version` and includes resolved full Git commit ids, input schema versions, and rule versions that affect the result. Another AI, CLI, or CI process can therefore reproduce the basis of the judgment.

These are not premature implementations of future features. They are the smallest persistent contracts whose later alteration would require migration of existing Knowledge, relations, and history.

#### 9.2.2 Never reinterpret a minimal record as a stronger fact

The relationship between V2 and future models is fixed as follows:

| V2 record | Fact V2 guarantees | Separate future record | Forbidden reinterpretation |
|---|---|---|---|
| `ExecutionBinding` | A Case UID has an automated or manual verification method and reference | `ExecutionFact` | Treating a binding as evidence that the case ran or passed |
| `ReleaseScope` | The Case UID was selected for that release | `ReleasePlan` | Treating the list as a plan that fixed revisions, reasons, build, and environment |
| `Spec-Reviewed` trailer | An alignment check was recorded at that commit | `ImpactDecision`, `HumanAttestation` | Filling in absent link/policy digests or approval |
| Git deletion or reappearance | A file is absent or present again at that point | `retire`, `restore` event | Inferring deletion intent or restoration as the same Identity |

Persistent records carry a `schema_version`. When multiple record types share a storage area or output, a `record_kind` or equivalent identifies the type. V2 does not add every possibly useful future field as optional. A full model is introduced with a separate type and storage contract; information absent from V2 remains `unknown` or `legacy`.

#### 9.2.3 How Adapters and readers evolve

StrictDoc parsing and Playwright reporter formats do not enter the Domain. V2 also does not create a generic plugin interface while only one real format exists. The first implementation normalizes at the Application edge; a common seam is extracted when a second real Adapter or a replacement requirement appears.

When future formats are added, old records are not destructively converted. Where needed, multiple readers normalize into the same read model:

```text
ExecutionBinding reader ─┐
ExecutionFact reader ────┴→ release verification read model

Trailer decision reader ─┐
Structured decision reader┴→ alignment resolution read model
```

This diagram is a direction for future evolution, not a requirement to implement empty readers or seams in V2. Until the second input exists, V2 preserves only distinct type meanings and non-conflicting storage.

#### 9.2.4 What real StrictDoc and Playwright operation measures

M3 and M4 capture design evidence as well as delivering integration behavior:

- The false-positive rate of file-level StrictDoc change detection and cases that actually require Requirement-level parsing.
- Time from a Requirement change to a confirmed TestCase set, plus the reasons people accept, add, or remove candidates proposed by AI or rules.
- How often a Case UID has zero or multiple Playwright tests, and which multiplicities are legitimate.
- Whether parameterized tests, Playwright projects, and retries need execution identities separate from the Logical TestCase.
- Differences between Release Scope and the actually executed set, and real cases where a result could not be accepted because of Case revision, target commit, build, or environment.
- Cases where a commit trailer needed later addition or correction, and judgments that CLI/JSON alone could not explain effectively.

Only when these observations demonstrate a concrete gap are Release Plan, Execution Fact, structured decisions, dashboard, or similar concepts promoted in a separate ADR.

#### 9.2.5 Cutover for guarantees added later

Information V2 never stored is not reconstructed from Git history or natural language as if it were a complete fact. A future Execution Fact that matches Case revision/build/environment, or a complete Identity lifecycle with retire/restore/id reservation, declares the commit at which its guarantee begins.

Pre-cutover records are handled as follows:

- An `ExecutionBinding` remains a valid binding but is never converted into a historical execution fact.
- A `ReleaseScope` remains a valid selection fact, while reasons, target revision, and execution conditions stay `unknown`.
- V2 deletion or reappearance is not converted into retire/restore except where an explicit migration manifest chooses it.
- When a complete Identity lifecycle begins, a migration manifest fixes the active identities and only those retired identities whose continuity must be preserved. Only subsequent events receive the complete lifecycle guarantee.

The cutover does not discard historical data. It prevents V2's weaker recorded facts from being confused with stronger facts recorded by the future model.

#### 9.2.6 What V2 does not pre-build

V2 does not add the following merely to preserve future flexibility:

- A generic runner plugin system or unused Adapter interfaces.
- build, environment, attempt, or evidence fields with empty values on `ExecutionBinding`.
- Empty `ImpactDecision` or `HumanAttestation` records with no approval semantics.
- Unused retire, restore, and id-reservation state transitions.
- A `ReleaseScope` filled with optional fields intended for a future Release Plan.

V2 remains extensible by keeping each current type's meaning narrow and allowing different facts to be added later as different types, not by reserving speculative fields.

## 10. Roadmap

| Stage | Builds | Exit criteria |
|---|---|---|
| M0 | The new `Requirement` schema (native/external modes) and `ExecutionBinding` schema, linking via `feature.requirement_uids`, the CLI (§7), automatic Alignment-check detection (§5.3), and the updated interactive authoring flow (§5.2.1) | Both native operation (no StrictDoc) and external operation complete Feature↔Requirement linking and TestCase `ExecutionBinding` recording end-to-end via Git/CLI, and a `requirement.yml` mixing the two modes is rejected |
| M1 | Change Impact (§6.1) | Between a PR's base and head, affected Features, Requirements, and unconfirmed alignment checks can be listed (no dependency on `.sdoc` parsing = M3) |
| M2 | Release Coverage (§6.2) and `ReleaseScope` (§5.2) | Coverage gaps across a given set of Requirements can be listed, and for a release with a selection list, the selected set, candidate omissions, and missing Case UIDs are listed alongside |
| M3 (future) | StrictDoc `.sdoc` ingestion (reflecting the actual Git-managed requirement content) | Started once demand is confirmed; whether a custom parser is needed is designed separately at that time |
| M4 (future) | Real-use validation of Playwright integration | Started once a concrete request exists. First connect by Case UID and `ExecutionBinding` and observe external reports; whether results become persistent Execution Facts is decided in a separate ADR after the observations in §9.2 |

The MVP is M0–M2. M3 and M4 are not committed to as of this document.

## 11. Acceptance Criteria

| ID | Scenario | Expected result |
|---|---|---|
| AC01 | Link a Feature to a Requirement via `contributes_to` | The Feature side owns the relation; the reverse listing is derived |
| AC02 | Attempt to edit the body (`label`/`description`) of a `source: external` Requirement from markharness | Rejected. In external mode markharness keeps only a fixed reference ([0023](../decisions/0023-requirement-native-and-external-source.md)) |
| AC02b | Edit the `label`/`description` of a `source: native` Requirement | Succeeds. In native mode markharness owns the body |
| AC03 | A Requirement changes but its related TestCase is not updated | Change Impact's output marks it "unconfirmed" |
| AC04 | A TestCase-change commit carries a targeted `Spec-Reviewed: no-change-required (req-xxx)` whose counterpart Case UID resolves uniquely | The Alignment check is judged "confirmed" for that pair (§5.3) |
| AC05 | Record `ExecutionBinding(mode=manual)` on a TestCase without a timestamp or executor | The record succeeds; there are no timestamp/executor fields to omit |
| AC06 | Compute Change Impact / Release Coverage multiple times from the same input | The same output is reproduced (P3) |
| AC07 | Create, via the CLI, a Scenario with the same content as a deleted one | A new Scenario UID is issued, so the Case UID derived from it differs too. Matching content never implies the old UID ([0021](../decisions/0021-identity-retire-simplification.md)) |
| AC07b | Restore a deleted Scenario's file from Git history (`git checkout <ref> -- <path>`) | The file's `uid:` comes back, so the original Scenario UID and Case UID return. That is a Git history operation, not markharness's `restore`; markharness neither prevents nor detects it ([0021](../decisions/0021-identity-retire-simplification.md) §2) |
| AC08 | A Requirement has no Feature `contributes_to` it | Listed as a coverage gap in Release Coverage |
| AC09 | A `source: external` `requirement.yml` without `source_locator`/`source_revision` | `validate` rejects it (§5.2.1) |
| AC09b | An existing `requirement.yml` with `label` and no `source` field | Valid as native; Change Impact and Release Coverage work with no StrictDoc present ([0023](../decisions/0023-requirement-native-and-external-source.md)) |
| AC09c | A `requirement.yml` carrying both `label` and `source_locator` | `validate` rejects it (mixed modes) |
| AC10 | For a `source: external` Requirement, the `.sdoc` blob named by `source_locator` differs between base and head | Change Impact reports a spec-side change, without parsing the `.sdoc` (§6.1 step 2) |
| AC10b | A `source: native` Requirement's `label`/`description` changes between base and head | Change Impact reports a spec-side change (§6.1) |
| AC10c | The `.sdoc` is unchanged between base and head, but `source_revision` does not match the blob OID at head | Reported as a stale pin only, never as a spec-side change (§6.1 step 3) |
| AC11 | Compute Release Coverage with a past release tag passed to `--at` | The listing reproduces the Knowledge and `ExecutionBinding` as of that ref (§6.2) |
| AC12 | One commit touches several Requirements and carries a trailer without a target | It stays "unconfirmed", because which check was done cannot be determined (§5.3) |
| AC13 | Rename a Scenario's display id | `ExecutionBinding` survives, because it references the Case UID (§5.2) |
| AC14 | C1 changes Requirement R and carries `Spec-Reviewed`; C2 in the same PR changes R again | C1's confirmation is void and R is reported as "unconfirmed" (§5.3 rule 2) |
| AC15 | A Requirement and a TestCase both change in the same PR, with no `Spec-Reviewed` | Reported as "followed", never as "confirmed" (§5.3) |
| AC16 | A targetless `Spec-Reviewed` trailer on a commit touching several Requirements | None of those Requirements becomes confirmed (§5.3 rule 1) |
| AC17 | The `base..head` commit history is unavailable (shallow clone) | The run fails with a diagnostic; missing history is never reported as "confirmed" (§5.3 rule 5) |
| AC18 | A PR changes a `.sdoc` and also runs `requirement repin` in the same PR | The spec change is still detected; repin does not cancel detection (§6.1 step 3) |
| AC19 | Evaluate the next PR, which changes nothing, after that repin | Nothing is reported as a new spec change; only a stale pin, if the reference is behind (§6.1 step 3) |
| AC20 | Only a Requirement changed; no related Feature changed | Related Features and TestCases are found by reverse lookup, and impact plus alignment state is reported (§6.1 step 1) |
| AC21 | A Requirement has a related Feature, but that Feature has no Scenario at all | Release Coverage names that Feature as a coverage gap (§6.2) |
| AC22 | Read an existing `identity-events` log containing `retire`/`release` events | Rejected with a diagnostic naming those events; the log, Knowledge, and generated output are left untouched (§9.1) |
| AC23 | Read an `identity-events` log made up of `issued` and rename events only | Replays deterministically and reproduces the id↔UID mapping (§9.1) |
| AC24 | Record a `ReleaseScope`, then compute Release Coverage with the past release tag in `--at` and its `release_id` in `--release` | Reproduces the TestCases selected then, and whether each had a verification method (§6.2) |
| AC25 | A TestCase under a target Requirement is absent from the selection list | It is listed as a candidate omission (§6.2) |
| AC26 | The selection list contains a Case UID that does not exist in the Knowledge at that ref | It is reported as a missing Case UID; the list is never rewritten automatically (§6.2) |
| AC27 | Attempt to record a selection timestamp, owner, or result on a `ReleaseScope` | No such fields exist, so it cannot be recorded ([0024](../decisions/0024-release-scope-selection-list.md)) |
| AC28 | Pass `../../etc/passwd`, `..`, `/abs/path`, or a dot-leading value as `release_id` | Rejected before any write; no file is created inside or outside `.markharness/releases/` (§5.2) |
| AC29 | C1 changes Case A and confirms no change needed to R; C2 changes only A again | Confirmation for (R,A) is invalid even though R is unchanged. It is not confirmed; this example is unconfirmed |
| AC30 | A one-sided trailer resolves to multiple candidate cases | Do not confirm all candidates. Accept only records explicitly identifying both endpoints and resolving the pair uniquely |
| AC31 | Case B is added after confirmation of (R,A) | Do not transfer confirmation to B. Keep the original confirmation if that pair remains unchanged |
| AC32 | Pass result, executed_at, build, or environment to an `ExecutionBinding` | Reject them as fields absent from the V2 binding schema; do not store the record as an execution fact (§9.2.2) |
| AC33 | A future reader reads a V2 `ReleaseScope` | Only the selected Case UIDs are known; reason, Case revision, build, and environment remain `unknown` (§9.2.2) |
| AC34 | Rename a Playwright test title or move its file while preserving its Case UID annotation | Resolve it as the same TestCase binding; title and path are not identity (§9.2.1) |
| AC35 | One Case UID resolves to zero or multiple entries in a Playwright report | Record it explicitly in the operational observations; never choose an arbitrary entry automatically (§9.2.4) |
| AC36 | An element was deleted and reappeared during V2 before a future complete Identity lifecycle begins | Unless an explicit migration manifest says otherwise, do not infer retire/restore; treat the pre-cutover lifecycle as `legacy` or `unknown` (§9.2.5) |
| AC37 | Recompute Change Impact with the same base/head and rule versions | The JSON includes `schema_version`, resolved commit ids, and rule versions and reproduces the same judgment (§9.2.1) |
