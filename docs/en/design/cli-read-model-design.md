# CLI Read-Model Design

Status: Draft

## 1. Purpose

This document defines the read-only data model (the "read model") that the `markharness` CLI exposes externally. Its primary consumer is `markharness-view` and similar display tools, to be implemented in a separate repository in the future.

This design does not bring screen structure or UI concerns into `markharness`. `markharness` produces a reproducible read model from Git, Knowledge, relations, and judgment results. `impact`, `coverage` (and the new `traceability`) each build a struct in their own dedicated module, serialized directly to JSON from `cli.rs` via `serde_json` (§8). None of these commands currently has human-readable output (a Human-Presenter equivalent); if one is added later, it is generated from the same struct.

```text
Knowledge / Git / Binding
          ↓
Domain judgment
          ↓
CLI read model (a struct owned by its command's module)
          ↓
JSON (direct serde_json serialization)
          ↓
CLI / markharness-view
```

## 2. Background and policy

`markharness` v2 provides Change Impact and Release Coverage over CLI/JSON, leaving views and dashboards to a separate tool. The CLI's JSON output is therefore not just a log; it is designed as an explicit output contract for external tools to read.

At the same time, a read model is not the same thing as a Domain model or a UI model.

- Domain model: expresses the meaning of Knowledge and judgments
- CLI read model: expresses the result an external reader needs
- UI model: expresses a screen's display state and interaction state

This document covers the middle one, the CLI read model.

## 3. Design principles

### 3.1 Make the read model the seam for JSON output

A Domain type is never serialized directly. Each command's own module (`impact`, `coverage`, `traceability`) produces its read-model struct, which is then serialized via `serde_json`. If human-readable output is ever added, it too is generated from that same read-model struct, not from the Domain type.

This localizes the impact of Domain-internal changes to the external output contract.

### 3.2 Not a UI model

The read model does not include screen tabs, selection state, expand/collapse state, sort state, paging state, or the like. Those are `markharness-view`'s responsibility.

### 3.3 Split by question

All Knowledge is not crammed into one giant `KnowledgeReadModel`. Instead, several small read models are defined, each corresponding to a question a consumer actually asks.

The initial three targets are:

1. `TraceabilityReadModel`
2. `ChangeImpactReadModel`
3. `ReleaseCoverageReadModel`

### 3.4 Generate at run time

The initial implementation does not persist read models as new source-of-truth files under `.markharness/`. They are generated from Knowledge, Git, Binding, and so on at command run time, and printed to stdout as JSON.

Caching or persistent derived artifacts, if needed, are treated as a separate design decision.

### 3.5 Never read a weak fact as a strong fact

The presence of an `ExecutionBinding` is never presented as meaning a test has been run or has passed. `ReleaseScope` is likewise never represented as an execution plan or an execution result.

## 4. Common envelope

`record_kind`/`schema_version` is not a newly invented envelope shape. `impact` (`ChangeImpact` in `src/impact.rs`) and `coverage` (`ReleaseCoverage` in `src/coverage.rs`) already emit it as-is, never going through `CommandOutcome`/`Presenter` (`src/presentation.rs`) at all — `cli.rs` serializes each struct directly via `serde_json`, producing `{"schema_version": 1, "record_kind": "<tag>", ...fields}`. `record_kind` reuses the name already established by `markharness-v2-design.md` §9.1 and ADR 0025 for persistent records (`execution_binding`, `release_scope`, `requirement`, etc.), and `impact`/`coverage`'s CLI output already follows that convention; see ADR 0032.

This document only adds `traceability`, following that same existing pattern. `canonical_imported`/`generated`/`changes_computed` (`markharness changes compute`) use a different, unrelated output shape from the `CommandOutcome`/`Presenter` path (the `outcome` field) — unrelated write commands, out of scope for this document's read models.

```json
{
  "record_kind": "change_impact",
  "schema_version": 1
}
```

### 4.1 `record_kind`

Identifies the kind of record. The three read models' values are (`change_impact` and `release_coverage` are already emitted by `impact`/`coverage`):

```text
traceability
change_impact
release_coverage
```

### 4.2 `schema_version`

Starts at `1`. This is not a compatibility mechanism for distinguishing past formats; it is an identifier that prevents confusing one record kind, or a future different model, with another.

If reading past formats is ever needed, that is handled by adding a separate record kind or a separate Reader, not a branch inside the existing model.

## 5. TraceabilityReadModel

### 5.1 Purpose

Provides the relations among Requirement, Feature, Behavior, Scenario, and TestCase in a form external tools can browse.

Omitting `--at <ref>` reads the working tree; giving it reads that Git ref (ADR 0033). Unlike `impact`/`coverage`, `traceability` has no two-point-comparison or release-auditing requirement, so nothing stops it from reading the working tree directly, the same way `generate`/`verify` already do.

### 5.2 Structure (implemented: `src/traceability.rs`)

```rust
struct TraceabilityReadModel {
    schema_version: u32,       // already emitted alongside "record_kind": "traceability"
    record_kind: &'static str,
    at: String, // fixed "working-tree" when --at is omitted; otherwise the given string as-is (ADR 0033)
    requirements: Vec<RequirementNode>,
    features: Vec<FeatureNode>,
    behaviors: Vec<BehaviorNode>,
    scenarios: Vec<ScenarioNode>,
    test_cases: Vec<TestCaseNode>,
    relations: Vec<TraceabilityRelation>,
}
```

Each Node carries at least a display ID and a UID (the UID may be `None` for an element `identity migrate` hasn't run on yet). TestCase additionally carries the Case revision and its generated path.

`traceability`'s purpose is to provide relations "in a form external tools can **browse**" (§5.1), and identifiers (`*_id`/`*_uid`) alone are not browsable. Each Node therefore also carries a human-readable `label`. Feature/Behavior/Scenario's `label` is required in Knowledge, so it is never `None` (`String`). Requirement's `label` exists only for `source: native` — unlike `source_locator`/`source_key`, it is never added for `external`, since markharness never owns external content (ADR 0023).

```rust
struct RequirementNode {
    requirement_id: String,
    requirement_uid: Option<String>,
    source: &'static str, // "native" | "external"
    // Present only for source: "native". Never given a representative text
    // for "external", since markharness doesn't own that content (ADR 0023).
    label: Option<String>,
    // Present only when source is "external" (ADR 0023): a way to reach the
    // actual StrictDoc (or similar) content. Always None for "native".
    source_locator: Option<String>, // repo-relative path of the referenced .sdoc file
    source_key: Option<String>,     // StrictDoc's MID (ADR 0030)
}

struct FeatureNode {
    feature_id: String,
    feature_uid: Option<String>,
    label: String,
}

struct BehaviorNode {
    behavior_id: String,
    // None until `identity migrate` runs (same as the other uid fields).
    behavior_uid: Option<String>,
    feature_id: String,
    label: String,
}

struct ScenarioNode {
    scenario_id: String,
    scenario_uid: Option<String>,
    behavior_id: String,
    label: String,
}

struct TestCaseNode {
    case_id: String,             // display id; matches the existing term (TestCase.case_id) used elsewhere (generate.rs etc.)
    case_uid: Option<CaseUid>,
    case_revision: CaseRevision, // a hash-string type, not u64
    relative_path: String,
    scenario_id: String,
}
```

Relations are expressed as explicit Relation entries rather than each Node duplicating an array of the other side.

```rust
enum RelationKind {
    ContributesTo, // Feature or Scenario to Requirement
    GeneratedFrom, // TestCase to Scenario
}

struct TraceabilityRelation {
    from_uid: String,
    to_uid: String,
    kind: RelationKind,
}
```

An element with no UID (not yet migrated) never appears on either side of a relation: relating two UID-less values would give an external reader nothing to match against a later re-run.

Example:

```json
{
  "from_uid": "feature-uid-1",
  "to_uid": "requirement-uid-1",
  "kind": "contributes_to"
}
```

`TraceabilityReadModel` does not include Binding presence or Coverage judgment results; those are `ReleaseCoverageReadModel`'s responsibility.

`requirements` and `features` cover everything in Knowledge, regardless of whether a generated TestCase exists (the same reasoning as coverage's AC21: a Feature with nothing underneath it yet is still made visible). `behaviors`, `scenarios`, and `test_cases` are derived from every generated TestCase; since `generate` rejects a Scenario with empty phases at generation time, an existing Scenario always corresponds to exactly one TestCase, so this derivation cannot miss one.

### 5.4 Relationship to the editing Intent

`TraceabilityReadModel` is read-only; it is not shaped so it can be written straight back to Knowledge files. To modify an existing element, a minimal Knowledge Intent is generated from the read result and passed to `knowledge reconcile`.

Existing elements are selected by `uid`.

```yaml
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: 01J8Z...
    id: todo-create
    label: Create a TODO
    contributes_to: [01J8A..., 01J8B...]
```

This Intent's semantics are:

- An element with a `uid` is treated as an update to the existing element.
- A new element without a `uid` cross-references others via a local `key` inside the Intent.
- A single-value field omitted from the Intent keeps its current value.
- Collections such as `axis`, `contributes_to`, and `procedures` are fully replaced, but only when specified.
- Specifying a collection as an empty array clears the current value.
- Changing `id` is a rename that preserves the UID.
- Under `mode: merge`, existing elements not present in the Intent are not deleted.

So when a GUI generates an editing Intent, it includes at least:

1. The UID of the element being edited
2. Single-value fields the user actually changed
3. The full contents of any collection targeted for change

There's no need to unconditionally expand all of Knowledge into the Intent. Omitting unchanged fields keeps the Intent small and avoids unintended updates.

Applying from a GUI follows this path:

```text
TraceabilityReadModel
          ↓
Minimal Intent with the edited UID
          ↓
knowledge reconcile --check
          ↓
User confirmation
          ↓
knowledge reconcile
```

If Knowledge or identity state has changed since `--check`, the real run stops as a stale plan. The GUI re-fetches the latest read model, re-confirms the edit, and regenerates the Intent. A GUI or view must never write to Knowledge files directly.

## 6. ChangeImpactReadModel (`impact` already implements this)

### 6.1 Purpose

Provides the Requirement changes, related Features/TestCases, and acknowledgment state computed by the `impact` command.

### 6.2 Structure (the existing `impact::ChangeImpact`)

`ChangeImpactReadModel` is not a new design; it refers to `ChangeImpact` in `src/impact.rs`, already implemented and emitted with `record_kind`/`schema_version`.

```rust
struct ChangeImpact {
    schema_version: u32,       // already emitted alongside "record_kind": "change_impact"
    record_kind: &'static str,
    rule_version: u32,
    base_commit: String,
    head_commit: String,
    requirements: Vec<RequirementImpact>, // each carries its Feature/TestCase relations
    stale_pins: Vec<StalePin>,
    rejected_trailers: Vec<RejectedTrailerReport>,
}

struct RequirementImpact {
    requirement_id: String,
    requirement_uid: Option<String>,
    source: &'static str,   // "native" | "external"
    spec_changed: bool,
    feature_ids: Vec<String>,
    cases: Vec<CaseAlignment>,
}

struct CaseAlignment {
    case_id: String,
    case_uid: Option<String>,
    case_changed: bool,
    status: AlignmentStatus, // Confirmed | FollowedUp | Unconfirmed
}
```

The acknowledgment state derived from the `Spec-Reviewed` trailer (`AlignmentStatus`) keeps the following distinction (ADR 0019):

```text
confirmed      a valid Spec-Reviewed exists for this pair
followed_up    both sides changed in the range, but no acknowledgment was recorded
unconfirmed    the spec side changed with neither follow-up nor acknowledgment
```

### 6.3 `impact` as seen from markharness-view

`impact --base <ref> --head <ref> --format json` already returns the full `ChangeImpact` — each Requirement's Feature/TestCase relations and acknowledgment state — not just a count. `markharness-view` can consume this output as-is. No further implementation is needed.

## 7. ReleaseCoverageReadModel (`coverage` already implements this)

### 7.1 Purpose

Provides Feature/TestCase/verification means/coverage gaps/ReleaseScope selection state for a given Requirement set and Git ref.

### 7.2 Structure (the existing `coverage::ReleaseCoverage`)

`ReleaseCoverageReadModel` is not a new design; it refers to `ReleaseCoverage` in `src/coverage.rs`, already implemented and emitted with `record_kind`/`schema_version`.

```rust
struct ReleaseCoverage {
    schema_version: u32,       // already emitted alongside "record_kind": "release_coverage"
    record_kind: &'static str,
    rule_version: u32,
    at_commit: String,
    requirements: Vec<RequirementCoverage>,
    gaps: Vec<CoverageGap>,
    release: Option<ReleaseView>, // omitted entirely when no release was requested
}

struct RequirementCoverage {
    requirement_id: String,
    requirement_uid: Option<String>,
    source: &'static str,   // "native" | "external"
    feature_ids: Vec<String>,
    cases: Vec<CaseCoverage>,
}

struct CaseCoverage {
    case_id: String,
    case_uid: Option<String>,
    feature_id: String,
    binding_mode: Option<String>,       // omitted when undeclared
    binding_reference: Option<String>,  // omitted when undeclared
    selected: Option<bool>,             // only meaningful when a release was requested
}

struct CoverageGap {
    kind: GapKind, // "requirement_has_no_feature" | "feature_has_no_case"
    requirement_id: String,
    feature_id: Option<String>,
}

struct ReleaseView {
    release_id: String,
    selected_case_uids: Vec<String>,
    unselected_case_uids: Vec<String>,
    absent_case_uids: Vec<String>,
}
```

`binding_mode`/`binding_reference` express `ExecutionBinding` (a declared verification means) as-is. These values must never be presented as "executed" or "passed" — pass/fail, execution time, executor, and execution environment are a separate tool's responsibility (ADR 0025 §1). "Selected" is likewise never conflated with "executed" (ADR 0024 §5).

### 7.3 `coverage` as seen from markharness-view

`coverage --requirements <ids-or-all> [--release <id>] --at <ref> --format json` already returns the full `ReleaseCoverage` above. `markharness-view` can consume this output as-is. No further implementation is needed.

## 8. Relationship to the CLI's internal result

The existing `CommandOutcome` (`src/presentation.rs`) is a type with three variants — `CanonicalImported`, `Generated`, `ChangesComputed` — representing the result of the **write** commands `canonical_import`, `generate`, and `changes compute`. `Presenter` (`HumanPresenter`/`JsonPresenter`) serializes it.

The read models do not extend `CommandOutcome`. `impact` (`ChangeImpact`) and `coverage` (`ReleaseCoverage`) never went through `CommandOutcome`/`Presenter` to begin with — each command's own module serializes its own struct directly from `cli.rs` via `serde_json`. The new `traceability` (`TraceabilityReadModel`) follows that same path (ADR 0032).

Note that the name `src/traceability.rs` is already taken by the `TraceabilityIndex` generation artifact `generate` builds (`.markharness/generated/traceability-index.json`, used by `verify` to check regeneration is deterministic). Since the two differ in responsibility — a persisted artifact versus a query result — the existing file is renamed to `src/traceability_index.rs`, and the freed-up `src/traceability.rs` is used for the new `traceability` command (ADR 0032 decision 2).

```text
CommandOutcome (write commands: CanonicalImported / Generated / ChangesComputed)
  → Presenter (Human / Json)

Per-command modules for the read-only commands (unrelated to CommandOutcome)
  impact::ChangeImpact                     → already implemented, serialized directly from cli.rs
  coverage::ReleaseCoverage                → already implemented, serialized directly from cli.rs
  traceability::TraceabilityReadModel      → new, follows the same path
  traceability_index::TraceabilityIndex    → existing (renamed from the old traceability.rs); generate's artifact, not a read-only command
```

## 9. Mapping to commands

The initial mapping is:

```text
markharness traceability [--at <ref>] --format json
  → TraceabilityReadModel (omit --at for the working tree, give it for a Git ref; ADR 0033)

markharness impact --base <ref> --head <ref> --format json
  → ChangeImpactReadModel

markharness coverage --requirements <ids-or-all> [--release <id>] --at <ref> --format json
  → ReleaseCoverageReadModel
```

`traceability` is new. The existing `generate` is a write command that produces artifacts; a successful generation result and a Knowledge-browsing result are not mixed into the same command. `traceability` is read-only: giving `--at` reads that Git ref; omitting it reads the working tree's Knowledge and already-generated TestCases. `impact` and `coverage` keep `--at` (or `--base`/`--head`) required, since comparing two points and auditing a release are their whole point (ADR 0033).

`impact` and `coverage` already return the full `ChangeImpactReadModel`/`ReleaseCoverageReadModel` via `--format json`. Both currently support only `--format json`; human-readable output is not implemented. If it is added later, it is generated from the same struct.

The command name and the JSON `record_kind` are kept aligned, but the JSON contract's identity is determined by `record_kind` and `schema_version`, not the command name.

## 10. What is not persisted

The initial read models do not save the following as new source-of-truth files.

- A copy of all of Knowledge under `.markharness/`
- Per-screen display state
- A search index
- A UI cache
- Execution results or evidence

They are generated deterministically, when needed, from existing Knowledge, Binding, ReleaseScope, and Git history as input.

## 11. Verification policy

Read models are tested from these angles.

1. The same input produces the same JSON.
2. `record_kind` and `schema_version` are always present in the output.
3. If human-readable output is added, it shows the same judgment results as the JSON output.
4. UID, Case revision, and Git ref are never lost from the result.
5. Nothing presents `ExecutionBinding` in a way that could be mistaken for an execution result.
6. Whether a Requirement's source is native or external is never lost. When external, `source_locator`/`source_key` let a reader actually reach the underlying StrictDoc (or similar) content — distinguishing the two is not enough on its own.
7. Adding an unknown, optional field never breaks an existing reader's handling of required fields.

JSON fixtures are prepared per read model, and also serve as output examples the view repository can reference.

Fixtures live at `tests/fixtures/read-models/<record_kind>/v1/`. The markharness-side fixtures are the canonical example for each read model, and CLI integration tests verify that actual output matches them.

## 12. Added later

The following models are added only once a concrete need is confirmed on the view side.

- A model dedicated to detailed TestCase display (`phases`, `axis`, and similar TestCase body content; reading `.markharness/generated/testcases/<relative_path>` directly, via `test_cases[].relative_path`, covers this for now)
- A search-result model over all of Knowledge
- A Git history comparison model
- An aggregation model spanning multiple refs
- A persisted search/display cache

These are not included in the initial models merely because they might be useful someday. A new read model or Reader is designed once a second real read shape, or a concrete usage gap, actually appears.

Each Node's `label` (§5.2) is an exception: it's identifier metadata directly required by `traceability`'s own purpose — being browsable — not independent content the way a TestCase's body is, so it belongs in the initial model.

## 13. Settled in the initial design

### 13.1 `traceability` is its own command

Folding it into `generate` is not adopted. `generate` is a write command that produces TestCases and artifacts from Knowledge, while `traceability` is a read command that reads Knowledge and already-generated TestCases. Keeping them separate lets a view that only wants to read avoid triggering generation or file writes.

### 13.2 Nodes are arrays per type

`RequirementNode`, `FeatureNode`, `BehaviorNode`, `ScenarioNode`, and `TestCaseNode` are kept separate. Normalizing every type into a common `Node { kind, uid, fields }` is not adopted.

The reason is to avoid losing, from the JSON contract, the meaning of per-type required fields, external-Requirement constraints, and TestCase's Case revision. Only relations are normalized into the common `TraceabilityRelation`.

### 13.3 JSON Schema is managed in the repository

The external contract is not managed by implementation and fixtures alone. The following JSON Schemas are added to the existing `schema/` directory.

```text
schema/traceability-read-model.schema.json
schema/change-impact-read-model.schema.json
schema/release-coverage-read-model.schema.json
```

JSON Schema validates structure and types. Semantic invariants — that a UID exists, what a Git ref means, that a Binding must never be interpreted as an execution result — are verified by Rust Domain/Application tests.

### 13.4 markharness is the fixture source of truth

Since markharness and view are separate repositories, no shared directory, submodule, or runtime cross-reference is set up at this initial stage.

markharness manages the read models' JSON Schema and representative fixtures as a published contract. view pulls the fixtures for the matching `record_kind` and `schema_version` into its own repository and uses them for contract tests. Fixtures are updated explicitly, as a Schema change or a deliberate change to the output contract.

This separation keeps view's build from depending on markharness's working tree or a local path. On the markharness side, using the fixtures in CLI integration tests lets divergence between actual CLI output and the external contract be detected.

## 14. Final CLI command list

In connection with the CLI read-model design, the commands ultimately provided are:

| Command | Category | Writes | Primary use | Output read model |
|---|---|---:|---|---|
| `markharness traceability [--at <ref>] [--format json]` | New | No | Read Requirement/Feature/Behavior/Scenario/TestCase relations (omit `--at` for the working tree) | `TraceabilityReadModel` |
| `markharness impact --base <ref> --head <ref> [--format json]` | Existing (implemented) | No | Read change impact, affected TestCases, and acknowledgment state | `ChangeImpactReadModel` |
| `markharness coverage --requirements <ids-or-all> [--release <id>] --at <ref> [--format json]` | Existing (implemented) | No | Read Release Coverage, verification means, and coverage gaps | `ReleaseCoverageReadModel` |
| `markharness knowledge reconcile <intent-file> [--check]` | Existing command | Yes (no with `--check`) | Validate a UID-bearing Intent and create/update/rename Knowledge | The applied result; updates the read models' input |

### 14.1 Read commands for view

`markharness-view` uses these three read commands.

```text
markharness traceability --format json                # live preview while editing: reads the working tree (ADR 0033)
markharness traceability --at HEAD --format json       # to check committed state instead
markharness impact --base main --head HEAD --format json
markharness coverage --requirements all --at HEAD --format json
```

Each prints a single JSON read model to stdout. view never reads Knowledge or `.markharness/` directly; it takes these outputs as input. `traceability` differs from `impact`/`coverage` in that it can reflect what the user just edited, before it's committed (ADR 0033).

### 14.2 Relationship to write commands

When a GUI or view modifies test knowledge, it goes only through `knowledge reconcile`.

```text
Fetch a read model
  ↓
Generate/edit a minimal Intent with the target UID
  ↓
markharness knowledge reconcile <intent-file> --check
  ↓
markharness knowledge reconcile <intent-file>
```

`traceability`, `impact`, and `coverage` never modify Knowledge files directly. Nor does `view`.

### 14.3 Commands not added at this stage

The following commands are not added as part of the initial read-model design.

- `markharness view`: view ships as a separate tool in a separate repository
- `markharness serve`: no local web server is built into the main tool
- `markharness search`: not built until a concrete search requirement is confirmed
- `markharness edit`: no additional editing path; everything goes through Intent and `knowledge reconcile`
