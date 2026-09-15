# 0031: Add `contributes_to` from Scenario to Requirement

## Status

Accepted (decided 2026-09-15). Extends the Feature-level relation to Requirement (`feature.requirement_uids`) that [0017](0017-scenario-case-revision-and-execution-evidence.md) defined, so a `Scenario` can carry the same relation too.

## Background

A real-world usage report with StrictDoc (from a sample project the user actually ran commands against; this ADR neither links to nor names that project) surfaced the following harm.

When one Feature `contributes_to` more than one Requirement (e.g. a "Task management" Feature relating to both a "Task management" Requirement and a "Task persistence" Requirement), **every** TestCase generated under that Feature mechanically inherits both Requirements' UIDs in its `generated_from.requirement_uids`, regardless of what it actually verifies. Concretely, a TestCase verifying "blank input is rejected" ended up associated with the persistence Requirement too, and a TestCase verifying "recovery from corrupted storage" ended up associated with the task-management Requirement too — both wrong.

This answers North Star question 1 ("which test cases relate to a given feature, and which Requirements relate to it") incorrectly. StrictDoc's Low-Level Requirements (LLRs) are typically written at a finer grain than High-Level Requirements (HLRs) — one concrete behavior per LLR, the same granularity as one Scenario (the same granularity ADR 0017 §3 fixes for "1 Scenario = 1 TestCase"). A Feature-level relation alone cannot express that granularity.

## Decision

### 1. Add `requirement_uids: Vec<String>` to `Scenario`

Add a field to `Scenario` (`src/knowledge.rs`) of the same type and meaning as `Feature.requirement_uids`. Mark it `#[serde(default)]`, defaulting to an empty vector when absent (following this codebase's existing pattern for additive fields: required-ness, if any, is enforced by a specific validation rule, not by the type itself).

### 2. markharness does not know Requirement hierarchy (HLR/LLR)

Even when StrictDoc's own Requirements carry Parent/Child relations (an HLR/LLR hierarchy), markharness keeps and interprets none of it. Whatever a StrictDoc `[REQUIREMENT]` is — HLR or LLR — it becomes exactly one `Requirement` of the same shape on the markharness side (holding that UID in `source_key` when `source: external`) (P5, "Core doesn't know external formats"). Relations between Requirements, including parent/child, remain the separate undecided matter [0017](0017-scenario-case-revision-and-execution-evidence.md) already left open; this ADR keeps that position. Revisit only if a concrete need for it appears, in a separate ADR.

### 3. Combination rule when both Feature and Scenario carry a relation

The generated TestCase's `generated_from.requirement_uids` is computed as follows.

- If the Scenario's `requirement_uids` has one or more entries, use **only** the Scenario's value (the Feature's is ignored).
- If the Scenario's `requirement_uids` is empty, fall back to the Feature's `requirement_uids` as-is.

A union (always combining Feature and Scenario) is not adopted. The goal is to answer precisely, per TestCase, which Requirement it verifies; there is no reason to mix in coarser information (Feature) once more precise information (Scenario) exists. The Feature-level relation continues to serve as a backward-compatible fallback for Features that haven't been broken down to Scenario granularity.

### 4. No consistency check between Feature and Scenario content

`validate` does not detect or reject a Scenario referencing a Requirement UID absent from its Feature's `requirement_uids`. Feature and Scenario relations are treated as independent sources of information; consistency between them is left to the author. This constraint is not added until a concrete need for it is confirmed (YAGNI).

### 5. The Knowledge Intent field name is unified as `contributes_to`

Like `RequirementIntent`, `ScenarioIntent` gets a `contributes_to: [<requirement key or uid>]` field (stored as `requirement_uids`). Reusing the same name as the existing Feature-side field avoids adding a rule for users to remember.

### 6. Change `coverage`'s TestCase matching from Feature membership to a direct lookup

`coverage.rs` (`features_for_requirement`) currently computes the set of Features that `contributes_to` a Requirement, then treats a TestCase as "covering" that Requirement merely because it **belongs to** one of those Features — it never consulted `case.generated_from.requirement_uids` at all.

This changes: the Feature-level gap detection (`RequirementHasNoFeature`, no Feature `contributes_to`s this Requirement at all) stays as-is, but the per-TestCase-to-Requirement matching drops the Feature-set gate and instead **scans every TestCase directly against `case.generated_from.requirement_uids`**. This correctly detects the case where no Feature `contributes_to` a Requirement at all, yet some Scenario under some Feature does so on its own.

For any project that never uses a Scenario-level override (every usage before this ADR), this matching is mathematically equivalent to the current behavior: `generated_from.requirement_uids` still inherits the Feature's `requirement_uids` verbatim via the fallback, so "the TestCase belongs to that Feature" and "the TestCase's `generated_from.requirement_uids` contains that Requirement" coincide. This change therefore does not break `coverage`'s existing behavior; it is a strictly safer implementation that removes an inaccurate association. `impact.rs` needs no further change, since it already reads `case.generated_from.requirement_uids` directly.

### 7. `schema_version` is unchanged

During this prototype period, per the existing policy in §9.2.2 of [markharness-v2-design.md](../design/markharness-v2-design.md) (the same reasoning as [0018](0018-identity-schema-version-freeze.md) and [0030](0030-external-requirement-source-key.md)), a field addition alone does not advance `schema_version`. It stays at `1` until an actual comparison/compatibility gate needs it.

## Consequences

- Changes: `src/knowledge.rs` (`Scenario` struct, YAML serialization), `src/generate.rs` (the composition rule when generating a TestCase), `src/knowledge_reconcile/` (Intent schema, plan construction), `src/coverage.rs` (TestCase matching logic), `schema/scenario.schema.json`.
- No change needed: `src/impact.rs` (already reads `case.generated_from.requirement_uids` directly) and `src/alignment.rs` (the `Spec-Reviewed` trailer names a Requirement/Case id directly and is independent of this relation).
- **Applying this to the real-world sample project that motivated this ADR is out of scope for this ADR and its implementation.** The markharness-side change lands first; verifying it against that sample is a separate, later task.

## Options considered and rejected

- **Carry the relation at `Behavior` granularity instead**: the real-world sample showed multiple cases of one Behavior (e.g. "add a task") owning several Scenarios (valid input / blank input) that map to different Requirements. Behavior granularity cannot express that and would not fix the harm.
- **Pull StrictDoc's Requirement hierarchy (HLR/LLR and Parent relations) into markharness's domain model**: already out of scope per [0017](0017-scenario-case-revision-and-execution-evidence.md), and contrary to P5 (Core doesn't know external formats). The harm this ADR fixes does not require knowing the hierarchy — a Scenario-level relation alone is sufficient.
- **Always union Feature and Scenario relations**: simpler to implement, but if a Feature keeps a broad relation while only some of its Scenarios get refined, the union lets the inaccurate association survive right alongside the precise one — exactly the pattern that caused the original harm.
- **Add a Feature/Scenario consistency check**: the actual problem was never "both are written and disagree" but "only Feature can be written, with no way to refine it" — there is no concrete motivation for a consistency check.
