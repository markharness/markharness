# 0016: Introducing a precondition/step/result model for Behavior, Condition, and ExpectedResult (supersedes [0015](./0015-behavior-step-model.md))

## Status

Accepted (2026-09-01). [0015](./0015-behavior-step-model.md) has been changed to `Superseded`.

## Context

[0015](./0015-behavior-step-model.md)'s Phase 1 added a required, ordered `steps: Vec<String>` to `behavior.yml`, shared by every Condition under that Behavior (commit `5480b85`).

After shipping Phase 1 and actually building test cases against it, the following problems surfaced:

1. **Operations differ per condition.** For example, an "add TODO" Behavior has one condition that enters blank text and another that enters valid text — the text being entered is itself different. A single `behavior.steps` cannot express this; Test Designers were forced to either bias `steps` toward one condition or push the per-condition difference into `description`'s free text.
2. **Some conditions have preconditions that no sequence of shared steps can reach.** For example, a condition premised on "the target TODO has already been deleted" cannot be reproduced by the Behavior's shared `steps` alone.
3. **`expected_result.description` (predating [0015](./0015-behavior-step-model.md)) only supports a single sentence**, so it cannot hold multiple distinct observable outcomes (e.g., "the item is added to the list," "the input field resets," "focus returns to the input field") in one ExpectedResult, and it ends up mixing an implementation-detail rationale with the user-facing outcome in the same sentence.
4. **There is no way to express a verification that requires an extra action first** (e.g., reloading the page before checking persistence). `generate.rs::load_knowledge_snapshot` currently flattens every `expected/*.yml` under one Condition into a single `Vec<ExpectedSnapshot>`, and `TestCase` holds only one flat `steps: Vec<String>` and one flat `expected: Vec<String>` (`src/generate.rs`). This makes it impossible to distinguish "a result only visible after a reload" from other results that need no reload.

These are distinct from the "duplicated procedure / copy-update drift" problem that [0015](./0015-behavior-step-model.md) reserved for Phase 2. Phase 2 was about the need for a shared Step registry; what actually surfaced here is that **the sharing granularity at the Behavior level is too coarse for the real variation across conditions** — the very premise Phase 1 locked in ("`Behavior.steps` is shared by every Condition") broke against real data.

To work through this, a scratch sample (`examples/bdd-sample/`, deleted once this ADR is finalized) was built that deliberately ignored `.markharness`'s actual schema and mapped the Gherkin (Given/When/Then/Background) way of thinking directly onto the directory structure, to figure out what was actually needed, no more and no less. This ADR reflects the conclusions of that exploration.

## Decision

### 1. Schema changes

Rename `behavior.schema.json`'s `steps` to `preconditions`, changing its meaning to "preconditions common to every Condition." The actual operation sequence moves entirely to Condition.

```yaml
# behavior.yml
id: add-todo
feature: todo-management
label: Add TODO
axis: [ui]
description: |
  Adds a TODO from the entered text on form submit
preconditions:
  - Open the TODO app
  - Confirm the input field is empty
```

```yaml
# condition.yml (valid-text)
id: valid-text
behavior: add-todo
label: Valid text
description: |
  When non-empty, valid text is entered into the input field and submitted
additional_preconditions: []
steps:
  - Enter "Buy milk" into the input field
  - Click the "Add" button
```

```yaml
# expected/001.yml (under the same valid-text directory)
id: valid-text-001
condition: valid-text
generated_by: manual
description: The valid text is added as a TODO and the input field resets
results:
  - The "Buy milk" TODO appears, unchecked, at the end of the TODO list
  - The input field resets to empty
  - Focus returns to the input field
implementation_note: |
  addTodo() pushes {id, text, completed:false} onto todos using the trimmed text,
  then calls render(). The submit handler sets input.value = "" and calls input.focus()
```

```yaml
# expected/002.yml (example with an extra action in between)
id: valid-text-002
condition: valid-text
generated_by: manual
description: The TODO persists across a reload
additional_steps:
  - Reload the page
results:
  - The "Buy milk" TODO is still shown in the list
implementation_note: |
  addTodo() calls saveTodos(), which persists to localStorage,
  so loadTodos() restores it on reload
```

Field list:

| entity | field | type | required | meaning |
|---|---|---|---|---|
| behavior | `preconditions` (renamed from `steps`) | `Vec<String>` | empty array allowed (no `minItems`) | preconditions common to every Condition |
| condition | `steps` (new) | `Vec<String>` | `minItems: 1` required | condition-specific operation sequence (inherits the granularity rule of the old `behavior.steps`) |
| condition | `additional_preconditions` (new) | `Vec<String>` | empty array allowed | condition-specific extra preconditions (ones no sequence of steps alone can reach) |
| expected_result | `description` (existing, unchanged) | `String` | required | human-facing one-sentence summary; not consumed by generation |
| expected_result | `results` (new) | `Vec<String>` | `minItems: 1` required | multiple observable outcomes; consumed by test-case generation |
| expected_result | `additional_steps` (new) | `Vec<String>` | optional only for the `expected_result` that is first in filename order within a Condition; non-empty (at least one action) required for every subsequent one | extra action(s) needed before this result can be checked |
| expected_result | `implementation_note` (new) | `String` | optional | implementation-detail rationale; not consumed by generation |

**File-splitting convention for `expected/*.yml`**: multiple independent observations checked after the same action are written as multiple lines within one `expected_result.results` array, not split across files. A new file (e.g. `002.yml`) is created only to express a new phase — one that requires an intervening action (`additional_steps`) before its results are checked. Rather than leaving this convention to review discipline alone, it is enforced mechanically: within a Condition, every `expected_result` after the first (in filename order) must have non-empty `additional_steps` (only the first `expected_result` may omit it, or leave it empty).

This constraint cannot be expressed by `expected_result.schema.json`'s JSON Schema alone — `validate.rs`'s `validate_file` validates each `expected/*.yml` file independently, one at a time, so JSON Schema has no way to know a given file's ordinal position among its siblings within the same Condition directory. This constraint is therefore implemented as a `validate.rs`-side cross-reference check (the same category as the existing `axis`-tag and `forked_from`-target reference-integrity checks): it lists a Condition's `expected/*.yml` files in filename order and errors if any file beyond the first has empty `additional_steps`. This turns "creating a new file with no intervening action" into a Knowledge-validation error, structurally eliminating the ambiguity a 2026-09-01 Standards/Spec review raised (`docs/ja/decisions/0016-review-2026-09-01.md`, Japanese only) — whether `002` represents an independent observation, a re-run of the action, or an additional action layered on top of the retained state.

### 2. `TestCase` structure changes (`generate.rs`)

`TestCase` granularity stays "1 Condition = 1 TestCase," as before. Its internal structure changes as follows:

- `TestCase.preconditions: Vec<String>` — the concatenation of `behavior.preconditions` and `condition.additional_preconditions`. Kept as a field independent of `phases`. Per §4, `preconditions` and `steps` are the same kind of executable operation, so this separation is not an execution-level necessity; it is a structuring choice so a human reading the generated output can visually distinguish "the common setup" from "the operation this Condition actually verifies," reflecting §5's framing of this as a human-facing procedure document.
- `TestCase.phases: Vec<Phase>` — an ordered sequence produced by walking `expected/*.yml` in filename order and building one `Phase { steps: Vec<String>, results: Vec<String> }` per file. Each phase's `steps` is that file's own `additional_steps` (possibly empty for the first file; required non-empty for every subsequent one, per the rule above), with `condition.steps` prepended only for the phase built from the first `expected/*.yml` in filename order; every other phase's `steps` consists solely of that (non-empty) `additional_steps`. `results` is that file's `results`. In other words, the first phase's `steps` is `condition.steps` followed by that first file's `additional_steps` if it declares any — the design does not assume the first `expected/*.yml` omits `additional_steps`.

The existing flat `title` / `steps` / `expected` fields are removed and replaced by `preconditions` / `phases`.

### 3. Naming policy

Gherkin terminology (Given/When/Then/Background) is not adopted; naming follows the existing schema's convention (non-BDD terms like `id`/`label`/`description`/`axis`). `preconditions` / `steps` / `additional_preconditions` / `additional_steps` / `results` / `implementation_note` are all non-Gherkin-specific vocabulary.

### 4. Granularity convention

The "one element = one fact" rule [0015](./0015-behavior-step-model.md) set for `behavior.steps` carries over to every new array field, retargeted per field. Each field's unit of "one fact" differs by the field's nature:

- `behavior.preconditions` / `condition.additional_preconditions` / `condition.steps` / `expected_result.additional_steps`: one element = one operation (same rule as [0015](./0015-behavior-step-model.md)). `preconditions`/`additional_preconditions` are "operations that establish the running precondition state," written as imperative commands just like `condition.steps` (e.g., "Open the TODO app," "Delete the target TODO"). There is no "state description, not an operation" distinction — just as Gherkin's Background/Given is actually executed as step definitions, preconditions exist to be run and actually produce that state, not merely to state it as documentation. The difference between the two is not a difference in kind, but in scope of execution (run first, common to the Behavior, vs. the Condition's own main procedure) and sharing scope (common to the Behavior vs. specific to the Condition).
- `expected_result.results`: one element = one observable outcome

As in [0015](./0015-behavior-step-model.md), Knowledge validation does not mechanically enforce this rule; it is left to Test Designer review discipline.

### 5. Relationship to the execution model (not an automated execution engine)

As `execution_result.schema.json` defines, markharness has no automated execution engine: a human Test Executor reads the procedure and performs it manually, recording exactly one `pass`/`fail`/`skip` for the whole TestCase (unchanged from before 0016). Accordingly, the `phases` this ADR introduces is a single, sequential procedure a human reads top to bottom, not a state machine that assumes automated execution (per-phase pass/fail recording, branching or retrying on failure). State between phases (e.g., checking persistence after a reload) carries over naturally because the same human performs the whole sequence in the same environment continuously; no explicit state-management mechanism is needed. This ADR does not introduce a teardown/cleanup concept — if a concrete need is confirmed against real data, it will be considered separately under the same criterion as [0015](./0015-behavior-step-model.md)'s Phase 2 onward (no quantitative threshold, revisit once actual friction is observed).

"Not an automated state machine" does not mean phase boundaries carry no meaning. §1's requirement that every `expected_result` after the first carry non-empty `additional_steps` is not there to make an automated state transition precise — it exists so a human reading the procedure can read it **unambiguously**. Precisely because execution is not automated, an ambiguous procedure document translates directly into a human misreading or misperforming it (an automated system would surface an ambiguous spec as a program branch; a manual procedure lets different readers silently settle on different interpretations, unnoticed). So the `additional_steps` requirement exists to guarantee unambiguity at authoring time (clarity as a procedure document), not to pin down an execution-time state machine — it does not contradict this section's premise that there is no automated execution engine.

## Conditions for moving to Accepted

- The schema changes (renaming `steps` to `preconditions` in `behavior.schema.json`; adding `steps`/`additional_preconditions` to `condition.schema.json`; adding `results`/`additional_steps`/`implementation_note` to `expected_result.schema.json`) are complete.
- The `TestCase` structural change in `generate.rs` (replacing the flat fields with `preconditions`/`phases`) and the updates to affected existing tests and fixtures are complete.
- `knowledge add --edit` (`KnowledgeDraft`, `BehaviorDraft`, and the new `ConditionDraft`/`ExpectedResultDraft`-equivalents) is updated to accept and validate the new fields, and related tests are updated.
- `examples/bdd-sample/` is deleted, leaving no duplication with this ADR's own examples.
- The `validate.rs` cross-reference check (§1) has tests covering at least: the first `expected_result` in a Condition (e.g. `001.yml`) passes even when `additional_steps` is omitted; a subsequent `expected_result` (e.g. `002.yml`) fails validation when `additional_steps` is omitted (or empty); and a subsequent `expected_result` passes once `additional_steps` has at least one action.

## Out of scope

- The `examples:` field seen in the sample's `condition.yml` (Scenario Outline-style data parameterization). Including how it would tie into generation logic — left for separate design once demand is confirmed (same criterion as [0015](./0015-behavior-step-model.md)'s Phase 2 onward: no quantitative threshold, revisit once actual duplication or friction is observed).
- Changes to [0013](./0013-immutable-identity-model.md)'s identity model. This ADR has no effect on `uid` handling for Behavior/Condition/ExpectedResult. All new fields are additions to existing entities; no new `EntityKind` is introduced.
- [0015](./0015-behavior-step-model.md)'s Phase 3 and Phase 4 (shared Step registry, UID, hash-based integrity checking). This ADR does not make them unnecessary; that judgment remains deferred.
- **Implementing Gherkin (`.feature`) integration.** The fields this ADR introduces make most of a plain Feature + Background + Scenario + Given/When/Then representable, but that is not the design or implementation of the integration feature itself. It is carved out as a feature that will definitely be built, tracked in [product-operation.md UC8](../product-operation.md) and [the paper's Chapter 7 Future Work](../git-native-model-for-test-knowledge-management.md#7-future-work). The integration is designed as **two independent one-way features, not a bidirectional round trip**:
  - **Import (Gherkin → markharness)**: converts a `.feature` file into markharness YAML through human review (a draft flow equivalent to `knowledge add --edit`). Syntax with no structural home in markharness's semantic model — `Scenario Outline` + `Examples`, Data Tables/Doc Strings, the `Rule:` keyword, the tag-to-`axis` mapping, non-canonical Given ordering, and Scenarios with no `When` step — is not auto-converted; the conversion tool warns the human and asks for manual handling instead (since this is always a human-supervised conversion, an unconditional lossless auto-conversion is not required). Whether this conversion is used as a one-time migration, or the `.feature` file keeps being edited and re-converted repeatedly, is left to the user's own operating practice and not prescribed by this ADR. To allow tracing back to the source at minimum, the `behavior.yml`/`condition.yml` produced by the conversion carries a `source` field recording the original `.feature` file's path and the target Scenario name (Feature name, for a Behavior) (e.g., `source: { path: features/todo/add-todo.feature, scenario: Blank text }`). This `source` is trace-only reference data and is not consumed by `generate.rs`'s test-case generation, but when the same `.feature` file is converted again, the conversion tool can also use it as a matching key — updating the existing Condition/Behavior in place under its existing `uid` rather than minting a new one (this reconciliation mechanism is only needed if the user's operating practice chooses repeated conversion, and its details are left to be decided at implementation time).
  - **Export (markharness → Gherkin, repeatably regenerable)**: a rendering feature that generates `.feature` files from `TestCase`, following the same "generated artifact (committed, regeneration-match verified by CI)" pattern as `generated/testcases/*.yml`.
  To be decided in a separate ADR when implementation begins.

## Implementation notes (not decided by this ADR)

- Which layer rejects an empty array or empty-string elements for each new array field (JSON Schema `minItems`/`items.minLength`, vs. procedural Rust-side checks) is the same open question [0015](./0015-behavior-step-model.md) left open, to be pinned down at implementation time. However, Phase 1's implementation already established a precedent of using `minItems`/`items.minLength` in `behavior.schema.json`, so this implementation should follow that precedent absent a specific reason not to. The one exception is `expected_result.additional_steps`'s "non-empty from the second `expected_result` in a Condition onward" constraint (§1) — a single-file-validated `expected_result.schema.json` cannot express it in principle, so it is necessarily a `validate.rs`-side cross-reference check, with no choice to make.
- The filename order of `expected/*.yml` (`001`, `002`, …) used to be purely cosmetic sorting. With this ADR's introduction of `phases`, it is promoted to **the contract that determines actual execution order**. Because reordering files could silently change a test's meaning, this contract needs to be documented explicitly — in `condition.schema.json`/`expected_result.schema.json`'s doc comments, or in CLI validate messaging.
- Per [0013](./0013-immutable-identity-model.md)'s definition, `case_uid` is derived deterministically from the `requirement_uid`/`feature_uid`/`behavior_uid`/`condition_uid`/`expected_result_uid` set alone; the text content this ADR adds (`preconditions`, `condition.steps`, `additional_preconditions`, `additional_steps`, `results`, etc.) is not part of that hash input. None of these fields affect the UID set, so `compute_case_uid`'s computation itself needs no change (consistent with [0013](./0013-immutable-identity-model.md)'s design that TestCase identity survives content edits). Content-change detection continues to be handled by the existing `changes compute` (tree-SHA diffing over the Feature directory, paper §3.2-3.4); this ADR does not need a new mechanism such as a `content_hash`.
- `knowledge_draft.rs`'s `BehaviorDraft` (`steps` → `preconditions`), plus the new `ConditionDraft`'s `steps`/`additional_preconditions` fields and the new `ExpectedResultDraft`'s `results`/`additional_steps`/`implementation_note` fields, along with their empty-value validation and draft template strings, all need updating.
- The existing test helpers and assertions in `generate.rs::compile_testcases` that assume "`TestCase.steps`/`expected` are single flat arrays" need to be inventoried and rewritten (a full inventory of affected call sites should be done via checklist creation, `checklist-<task>.md`, when implementation begins).
- `[knowledge].schema_version` (ADR 0014) stays at v1. Per [ADR 0014](./0014-knowledge-schema-version-persistence.md)'s decision item 11 ("This ADR's versioning contract does not apply during the prototype (0.x) stage"), `[knowledge].schema_version` is not bumped for Knowledge schema-breaking changes until the project meets its 1.0 criteria (tracked in `PROJECT.md`, including schema stabilization). This policy is recorded as part of ADR 0014's own contract, not something specific to this ADR, so this note only confirms it applies here. In addition, no `behavior.yml`/`condition.yml`/`expected_result.yml` instance data exists anywhere except `examples/bdd-sample/`, so there is also no risk right now of misinterpreting a comparison against old-format real data, for the same reason as [0015](./0015-behavior-step-model.md).
