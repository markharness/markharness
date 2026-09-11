# 0025: Evolve V2's minimal model without reinterpreting it as the future full model

## Status

Accepted (2026-09-11, design agreed, implementation pending). V2 will first exercise the StrictDoc → markharness → Playwright vertical flow in real use, and only the full-model capabilities justified by those observations will then be added. This decision renames [0020](0020-execution-status-lightweight-model.md)'s record to `ExecutionBinding` without changing the information it carries or the MVP responsibility boundary.

## Context

V2 uses a small model for Requirement/TestCase relations, missed updates, Release Scope, and verification methods. A future design may also cover selection reasons, execution against a particular Case revision/build/environment, decision invalidation, and a complete Identity lifecycle. Implementing all of that before V2 has been used would freeze speculative complexity; reinterpreting V2's sparse records as those richer facts later would fabricate history or evidence that was never recorded.

In particular, registering a verification method, selecting a TestCase for a release, executing it, and passing are different facts. Likewise, a simple deletion during the V2 period cannot deterministically reveal a later intent to retire, restore, or release an id reservation. These distinctions must be fixed before persistent formats are implemented.

## Decision

### 1. Rename `ExecutionStatus` to `ExecutionBinding`

V2's `case_uid`, `mode: automated | manual`, and optional `reference` declare the relationship between a TestCase and a verification method; they do not state execution status. Current design, terminology, and new CLI work use `ExecutionBinding` as the canonical name.

An `ExecutionBinding` has no execution time, result, Case revision, build, environment, attempt, or evidence. Its presence is never interpreted as "executed" or "passed." [0020](0020-execution-status-lightweight-model.md)'s lightweight scope remains in force.

### 2. Keep minimal and future full records as different types and record kinds

The following distinctions are invariants:

```text
ExecutionBinding ≠ ExecutionFact
ReleaseScope ≠ ReleasePlan
Spec-Reviewed trailer ≠ structured ImpactDecision / HumanAttestation
Git deletion or reappearance ≠ retire / restore event
```

Persistent records and public JSON carry a `schema_version`. When multiple record types share a storage area or output, a `record_kind` or equivalent explicitly identifies the type. A future type is added as a new type, not as empty placeholder fields on the old one. Readers represent information absent from an old record as `unknown` or `legacy`; they never infer it.

### 3. Stabilize only the shared foundation in V2

V2 preserves the contracts that a later model can safely build on: typed UIDs; one Scenario equals one TestCase; separation of Case UID and Case revision; Requirement relations excluded from Case revision; a fixed StrictDoc reference; Playwright binding by Case UID; and the Git refs and rule versions required to reproduce an output.

V2 does not pre-build a generic plugin mechanism, empty Domain types, or state machines for future runners, evidence, approvals, or Identity lifecycle. A seam is introduced when a second real Adapter or a concrete operational requirement exists.

### 4. Use real StrictDoc and Playwright operation to decide the next model

After V2 is introduced, the team will actually use the flow from a StrictDoc change, through affected-TestCase selection, to a Playwright test bound by Case UID. It will observe whether Requirement-level parsing is needed, zero or multiple bindings, parameterized tests, Playwright projects, retries, target commits, and differences between Release Scope and the executed set.

Observation does not by itself mean persisting a complete Execution Fact in Git. The corresponding Domain types and Adapters are decided in a later ADR after real data establishes the necessary matching conditions and storage unit.

### 5. Put a cutover on guarantees added later

If execution-fact applicability or a complete Identity lifecycle is introduced later, it declares the commit from which that guarantee begins. V2 records before the cutover remain `legacy` or `unknown` for Case revision, build, environment, and deletion intent that they never stored; migration never guesses those values.

Identity remains simplified as decided by [0021](0021-identity-retire-simplification.md). If retire, restore, and id reservation are later reintroduced, a migration manifest fixes the active identities and any required retired identities at the cutover. Only events after that point receive the complete lifecycle guarantee.

## Consequences

- V2 is a self-contained MVP, not an incomplete instance of the future model.
- Real StrictDoc and Playwright behavior can determine which Release Plan, Execution Fact, and decision records are worth adding.
- Sparse historical records cannot be silently promoted into stronger evidence.
- The `ExecutionBinding` name reduces the chance that people or AI confuse registered verification capability with execution status.
- A future complete Identity guarantee starts at an explicit cutover instead of guessing intent during the V2 period.

## Considered options

- **Add every future field to V2 as optional**: rejected because one type would conflate declaration, plan, and fact, leaving missing values ambiguous.
- **Automatically convert V2 records into future records**: rejected because execution conditions and deletion intent that were never stored cannot be recovered without fabricating evidence.
- **Implement V2 without preserving any evolution path**: rejected because small contracts such as UID, Case revision, and record kind become expensive persistent-data migrations if deferred.
- **Implement the complete future design first**: rejected because the necessary granularity and value of its Domain types have not been established through real StrictDoc and Playwright operation.
