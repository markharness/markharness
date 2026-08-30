# 0015: Introducing a step concept for Behavior (a phased approach)

## Status

Proposed (2026-08-30)

## Context

The `TestCase.steps` produced by `generate.rs::generate_testcases` is array-typed, but in practice it is always a single-element array, `[behavior.description]` ([testcase-generation-design.md §3.3](../design/testcase-generation-design.md#33-title--steps--expected-のテキスト組み立て), Japanese-only design doc). `behavior.yml` (`behavior.schema.json`) itself has only one free-text `description` field, so Test Designers are forced to cram what should be an ordered sequence of operations into a single string. [testcase-generation-design.md §7](../design/testcase-generation-design.md#7-将来課題との切り分け) explicitly called out "more advanced grouping via the Behavior layer and multi-tier axis management" as future work, but did not mention splitting steps into multiple elements.

At the same time, a future reuse need may arise where multiple Behaviors repeat the same procedure (e.g., a login sequence) verbatim. If `description` is copy-pasted each time, drift becomes undetectable — if one Behavior's copy is updated and another's is not, nothing surfaces the discrepancy.

The initial draft of this ADR considered introducing multi-element steps, a shared Step registry, UIDs, hash-based integrity checking, and an accept command all at once. A review concluded that this all-at-once approach was excessive. The sharing need has not yet been confirmed against real data (as of this ADR, no `behavior.yml` instance data exists anywhere in the repository), and neither the blast radius of a shared-Step change (whether to bulk-update every referencing Behavior or approve them individually, atomicity, recovery on failure) nor the necessity of integrating Step into the [identity lifecycle infrastructure](./0013-immutable-identity-model.md) as a sixth `EntityKind` has been validated. Locking in fail-closed hash-mismatch behavior before those are settled carries significant risk.

This ADR therefore decides only on the multi-element split itself (Phase 1); the shared registry and everything downstream of it (Phases 2–4) is left as future direction, to be designed once real data confirms the need.

## Premises

- As of this ADR, markharness has no external users.
- No migration of existing data is performed for this `behavior.yml` format change. No `behavior.yml` instance data exists anywhere in the repository (including `samples/`) as of this ADR, so no bulk rewrite or migration command is required.
- The Knowledge schema version is left unchanged at v1. Adding the required `steps` field is not treated as a `schema_version` bump under [ADR 0014](./0014-knowledge-schema-version-persistence.md).
- If v1 later gains external compatibility guarantees, this policy (no migration, no schema_version bump) must be reevaluated.
- If a need arises to make refs on either side of Phase 1 subject to `changes compute` or other history comparisons, the schema-version treatment must be reevaluated.

## Decision

### Phase 1 (decided by this ADR): introduce inline `Behavior.steps`

1. Add `steps: Vec<String>` (required, ordered array) to `behavior.yml`. Each element is a plain inline string; there is no reference to a shared registry.
2. Step granularity is fixed at "one `steps` array element equals one operation." Bundling multiple operations into a single element is not allowed.
3. `description` remains a one-sentence, human-facing summary but is no longer consumed by test-case generation at all.
4. Replace `generate.rs::generate_testcases`'s `steps = [behavior.description]` with `steps = behavior.steps`. `behavior.description` is entirely excluded from generation logic and retains only its role as human-facing documentation within `knowledge/`.
5. Knowledge validation must reject an empty `steps` array, and must reject any element that is an empty string.
6. A shared Step registry, UIDs, hash-based integrity checking, and a `steps accept`-style recovery command are not introduced in this Phase.
7. `knowledge add --edit` (`KnowledgeDraft` / `BehaviorDraft`, `src/knowledge_draft.rs`) — the only supported path for creating a Behavior — must be updated so it can accept and validate the newly required `steps`. Concretely: add a `steps` field to `BehaviorDraft`; include a `steps:` entry in the blank draft template that `knowledge add --edit` opens and in the non-interactive template output (the `markharness knowledge add --edit --print-template`-equivalent in `cli.rs`); and, mirroring `push_missing_description`, reject an empty or all-blank `steps` on the draft side too. Without this update, the only creation path could not satisfy the new required field, and Behaviors could not be created at all.

```yaml
# behavior.yml
id: todo-add-task
feature: todo
label: Add Task
axis: [ui]
description: "User adds a task."
steps:
  - "Click the title field"
  - "Leave it empty"
  - "Press submit"
```

#### Implementation notes (not decided by this ADR)

- Changing `generate.rs::generate_testcases` requires auditing existing test helpers and assertions that assume `steps = [behavior.description]` (e.g., test-case construction sites that build `steps: vec![case.behavior_description.clone()]`, and `assert_eq!` groups that assume `tc.steps` is a single-element array), and rewriting them to handle a multi-element `steps` array. A full inventory of affected call sites should be done as part of checklist creation (`checklist-<task>.md`) when implementation begins.
- Adding the required `steps` field to `behavior.schema.json`, and updating `serialize_behavior`, fixtures, and schema tests, is required. The schema version stays at v1 (see "Premises" above).
- `knowledge_draft.rs`'s `KnowledgeDraft` / `BehaviorDraft`, the associated validation (a `steps` counterpart to `push_missing_description`), the draft template string, `knowledge_apply::apply_draft`'s Behavior write-out path, and the existing draft parse/validate tests (the ones exercising missing-field patterns like `description: null`) all need updating to match the new required `steps`.
- This ADR does not decide which layer implements decision 5 (rejecting an empty array or an empty-string element). `schema/behavior.schema.json` is actually used as runtime validation via `validate.rs`, and `description` already pushes its non-empty check to the schema layer with `minLength: 1`. However, this repository has no precedent for constraining array length with `minItems` — the only existing array field, `axis`, has always allowed an empty array (`axis: []`). Whether `steps` gets a declarative `minItems`/`items.minLength` in the schema, or a procedural Rust-side check mirroring `push_missing_description`, should be pinned down explicitly when implementation begins.
- The "one `steps` element equals one operation" granularity rule from decision 2 cannot be mechanically enforced, since `steps` elements are free-form strings (e.g., `knowledge validate` would still pass an element written as "Do A. Then do B."). This rule is left to Test Designer review discipline and is out of scope for Knowledge validation — worth stating explicitly during implementation.

### Phase 2 (future direction, undecided): confirm sharing demand against real data

After Phase 1 ships and real data has accumulated for a while, check:

- Whether the same procedure actually gets repeated across multiple Behaviors
- Whether copy-update drift actually occurs in practice
- Whether the unit that should be shared is a single operation or a bundled procedure block
- Whether users can understand the blast radius of a shared-Step change

No quantitative threshold is set; the criterion is qualitative — move to the next Phase once duplication or update drift is actually observed. If it is not observed, inline `Behavior.steps` (Phase 1) alone is considered complete.

### Phase 3 (future direction, undecided): design a shared Step registry as a separate ADR

Only if Phase 2 confirms sharing demand, design the following as a separate ADR (or a revision of this one). None of this is decided by this ADR.

- The data model for `.markharness/steps/`
- Whether a UID is actually needed, and why
- Whether Step should become a sixth `EntityKind` under [ADR 0013](./0013-immutable-identity-model.md), or whether a simpler shared registry suffices
- How rename, retire, and restore are handled
- How the blast radius of a shared-Step change is surfaced to every referencing Behavior

### Phase 4 (future direction, undecided): add hash-based integrity checking and accept operations

Only if Phase 3 introduces a shared registry, design the following. None of this is decided by this ADR.

- Detecting a hash mismatch, and the fail-closed behavior of `knowledge validate` / `generate`
- The behavior of `steps add` / `steps accept` (whether to bulk-update every referencing Behavior or approve them individually, atomicity between the Step change and reference updates, recovery on partial failure, how the list of affected Behaviors is surfaced)
- Documenting the hash normalization rules (handling of UTF-8, line endings, trailing newlines, YAML block scalars)

## Conditions for moving to Accepted

- Phase 1's implementation (adding `Behavior.steps: Vec<String>`, and updating the schema test, fixtures, `generate_testcases`, and related tests) is complete.
- The updates to `knowledge add --edit`'s `BehaviorDraft`, its template, `steps` validation, the apply path, and related tests are complete.
- Phase 2 onward (shared registry, UID, hash, accept) is out of scope for this ADR's Accepted determination — once demand is confirmed, it will be proposed and decided as a separate ADR.

## Out of scope

- The detailed design of Phases 2–4 (the shared Step registry's data model, whether a UID is needed, whether to become an `EntityKind`, rename/retire/restore, hash normalization rules, the behavior of `steps add` / `steps accept`). A separate ADR will be raised once sharing demand is confirmed.
- The UX for selecting from, or creating entries in, the shared Step registry (the `knowledge add --edit` extension needed if Phase 3 onward is introduced). Note that adding a plain `steps:` entry to the `knowledge add --edit` draft template in Phase 1 is itself part of the Phase 1 decision above, and is not out of scope.
- Recording per-step execution results in ExecutionResult (not part of this ADR's motivation).
