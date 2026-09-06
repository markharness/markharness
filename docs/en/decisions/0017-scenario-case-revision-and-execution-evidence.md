# 0017: Separate Scenarios, case revisions, and execution evidence

## Status

Accepted (design agreed, implementation pending). Based on [Issue #40](https://github.com/markharness/markharness/issues/40) and the [decision record](https://github.com/markharness/markharness/issues/40#issuecomment-5554681808).

This ADR records the intended contract. It does not claim that current code or schemas implement it, or that every acceptance criterion of Issue #40 is complete.

## Context

Previously, Features belonged under Requirements, and cases were generated from a Condition and independent ExpectedResults. Deriving case_uid from the contributing UID set changes identity when results are added or ownership changes. The Feature tree SHA used for evidence cannot represent effective inputs outside that Feature, or the tested build and environment.

The purpose is test-case management and change detection across revisions, not code generation, an execution engine, or retry adjudication. Semantic clarity, determinism, accurate evidence, editable documents, and simple implementation take priority. Compatibility with old formats is not a constraint.

## Decision

### 1. Ownership and requirement relationships

Store Features independently of Requirements. A Feature's `requirement_uids` associates it equally with multiple Requirements. The Feature owns the relationship; reverse lists are derived. This means "contributes to realization," not proof that the Feature alone satisfies the entire Requirement.

Do not automatically inherit Requirement axes into Features. Define Feature classification on the Feature and traverse relationships for requirement-based selection. Do not introduce a generic graph or multiple ownership. Requirement-to-Requirement relationships remain a separate open decision.

```text
Requirement ← referenced by Feature, potentially multiple Requirements
                            └ Behavior: common procedures
                                └ Scenario: explicit procedure references
                                    └ ordered Phases: operations and observations
```

### 2. Scenarios and common procedures

Combine Condition and ExpectedResult into Scenario. A Scenario owns an ordered Phase array; each Phase contains operations and expected observations. Array order is the sole authority for execution order, replacing filename order. A Phase is part of its Scenario and has no independent UID or lifecycle.

A Behavior groups related Scenarios and defines common procedures. Scenarios explicitly reference them at the required execution positions; nothing is automatically prepended. References are restricted to the owning Behavior. Common procedures cannot call other common procedures. Generation expands references into operations.

This example illustrates semantics, not the final schema:

```yaml
# Common procedures in a Behavior
procedures:
  login:
    steps:
      - Enter credentials
      - Press the login button

# Phases in a Scenario
phases:
  - steps:
      - use: login
    results:
      - The account page is displayed
  - steps:
      - action: Log out
      - use: login
    results:
      - The account page is displayed again
```

Future external procedure management is possible, but does not justify adding a generic reference or external retrieval mechanism now.

### 3. Identity and verification revision

**1 Scenario = 1 TestCase** is the definitive contract. A TestCase is the generated effective definition after expanding common procedures and other inputs. Derive Case UID deterministically from Scenario UID with distinct typing; generation needs neither random issuance nor a separate case registry.

FeatureUid, ScenarioUid, CaseUid, ExecutionUid, display IDs, and revision references have distinct domain types. Never mix display IDs and UIDs as matching keys. Validate format, required fields, and references at input boundaries.

| Change | Case UID | Case revision |
|---|---|---|
| Display name, description, implementation note, provenance | Preserved | Preserved |
| Operations, preconditions, observations, test data | Preserved | Changes when effective content changes |
| Adding, removing, reordering Phases | Preserved | Changes when effective content or order changes |
| Common procedure change | Preserved | Changes only for referencing cases whose effective content changes |
| Move to another Feature / Behavior | Preserved | Changes when effective content changes |
| Classification or requirement relationships | Preserved | Preserved; handled as plan or relationship changes |
| Copy as a distinct test | New | Computed from the copy's effective content |

Compute Case revision deterministically from effective inputs: setup operations, preconditions, expanded common procedures, operations, expected observations, order, and test data. Do not place execution requirements only in descriptions. Audit stored-content changes through Git OIDs. Keep tested builds and environments outside Case revision, as evidence applicability conditions.

Implementation design must specify canonicalization and how its rules are identified. Hash equality is not proof of natural-language semantic equivalence; unsupported rule comparisons must not silently compare equal. Effective-input changes must reach case change detection, not merely evidence revision comparison.

### 4. Splitting and merging

| Operation | Identity handling |
|---|---|
| Revise an existing Scenario | Preserve |
| Extract part into another Scenario | Preserve original; issue new identity for extracted Scenario |
| Retire original and split into several | Issue new identities for all replacements |
| Retire several and merge into one | Issue new identity for replacement |

The editor explicitly chooses revision versus creation; do not infer identity from textual similarity. Record derivation, but do not transfer passing evidence to new cases. The derivation record format remains open.

### 5. Execution evidence and applicability

Separate execution result (pass/fail/skip), applicability (matching, older case revision, different target/environment, etc.), and evidence presence. Do not replace a historical fail with a single stale value.

A plan explicitly references the execution result it selects. Accept it only when Case UID, Case revision, target build, and required environment match. Records with unknown target/environment may be stored but cannot establish a pass. Missing, corrupted, or unresolved evidence cannot establish a pass. Do not overwrite or prioritize independent results solely by timestamp.

External tools own execution, retries, aggregation, and final results. markharness owns storage and applicability verification. Do not introduce a Run/Attempt model, flaky-test adjudication, or a rule independently rejecting a success after retry. Manual records use the same reference and applicability contracts.

Records are logically independent per case. Decide physical records-per-file based on expected scale; this ADR does not commit to one file per record.

Persist the effective definition used by execution in Git as an immutable record per Case UID + Case revision, referenced by evidence and shared by executions using that definition. Display names are outside revision inputs: changing them must not overwrite a definition under the same key. Keep execution-time display information and source-snapshot audit metadata separate; specify the concrete format during implementation design.

### 6. Boundary with external integration

This ADR covers the internal data model: execution record identity and references to cases, revisions, targets, and environments. The tool responsible for importing and the integration approach belong in a separate ADR based on concrete integration requirements. Gherkin and Playwright are anticipated use cases, not established integration contracts.

### 7. Prototype policy and existing ADRs

The project is currently a prototype without users. Replace the model directly without incrementing format versions or adding compatibility layers, old-format readers, migration, or dedicated old-format detection. Validate inputs against the new schema. Establish post-release format versioning when the schema stabilizes. This is distinct from tracking verification changes through Case revision.

- [0013](0013-immutable-identity-model.md): partially replace the five independently identified hierarchy levels, provenance-set Case UID derivation, and Feature-version-centered execution matching. This does not abandon identity declaration, determinism, or recovery safety. Adapt those guarantees to the new entity model during implementation.
- [0014](0014-knowledge-schema-version-persistence.md): retain the prototype format-version policy.
- [0015](0015-behavior-step-model.md) / [0016](0016-behavior-condition-precondition-step-result-model.md): replace separate Condition / ExpectedResult entities, filename order, and automatically combined setup operations. Do not adopt an arbitrary shared Step registry.

## Alternatives and consequences

See [Section 3.8 of the paper](../git-native-model-for-test-knowledge-management.md#38-settled-design-and-open-contracts) for the relationship to the previous model. Previous implementation checks do not validate this ADR's implementation. Case revision changes and the scope of semantic reconsideration after requirement/code changes are not synonymous; candidate selection rules remain open. Inputs needed for internal historical comparison must be fixed in Git snapshots.

| Alternative | Disposition and reason |
|---|---|
| Add extra references while keeping Features under Requirements | Rejected: avoid separate rules for a primary parent and equal references |
| Keep ExpectedResults as separate files and UIDs | Rejected: no concrete independent-tracking need; simplify order and ownership |
| Automatically prepend common procedures to every Scenario | Rejected: cannot serve intermediate positions and obscures execution placement |
| Identify cases by their contributing entity set | Rejected: fragments continuity across revisions of the same test |
| Use only Feature tree SHA as verification revision | Rejected: misses complete effective inputs and case-level changes |
| Manage execution and retries in markharness | Rejected: exceeds test-management and change-detection responsibilities |

Reference resolution, case revision computation, and immutable definition storage add work. In return, Phase identity management and filename-based order disappear. Independent Phase history is not guaranteed. Performance superiority has not been measured.

## Behaviors this decision must satisfy

Required examples cover UID continuity across renames; revision changes for operations, order, and common procedures; unchanged revisions for descriptions alone; new identities after split/merge; rejecting old revisions, different builds, unknown environments, and skip as proof of passing; consistency during concurrent recording; and never replacing the executed revision with the current one.

Decide atomic record storage and corruption preservation, corrections, timestamp comparison, target/environment types, canonicalization, missing-reference and empty-Phase validation, and exact derived-model formats before implementation. This ADR does not establish every implementable schema detail. Implementation ordering and task breakdown are tracked separately as a `checklist-<task>.md` once work begins ([checklist-workflow](../../../.github/instructions/checklist-workflow.instructions.md)).
