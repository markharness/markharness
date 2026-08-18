# Specification: `markharness knowledge validate` / `apply` — Non-interactive Knowledge Registration Commands

**Status**: Implemented(`src/knowledge_draft.rs` / `src/knowledge_apply.rs` / `src/cli.rs`). Additions/changes made at implementation time relative to this specification are noted in the "Notes Added at Implementation Time" subsection at the end of each section.
**Created**: 2026-08-08
**Related documents**: [testcase-generation-design.md](./testcase-generation-design.md), [product-operation.md](../product-operation.md), `src/interactive.rs`, `src/knowledge.rs`, `src/cli.rs`

**Positioning**: A design specification for extracting the internal logic (candidate enumeration, validation, writing) of the interactive `markharness knowledge add` into TTY-independent `validate`/`apply` subcommands. It is intended to be used as a common execution engine by human interactive CLI, AI agents, and a future GUI backend.

---

## 1. Background and Purpose

The current `markharness knowledge add` presumes sequential prompting on a TTY (stdin, one line at a time), and has the following constraints.

- It proceeds one-directionally through the five levels Requirement → Feature → Behavior → Condition → ExpectedResult, **writing to files as each level is confirmed** (no going back; partial writes can remain).
- Numeric selection (reuse of an existing item) and entry of a new id/label are mixed into the same prompt, and the existence of choices is not made explicit in help text.
- Removal of a redundant Condition id prefix (`strip_redundant_condition_prefix`) is performed without confirmation.
- axis (cross-cutting perspective) values are not cross-checked against the `axes/*.yml` registry.

In addition, this command is expected going forward to also be used from **non-interactive invocation by AI agents such as Claude Code**, and from a **future GUI implementation**. Both of these require an operational model of "validate and commit a batch of input all at once," and cannot depend on sequential prompting over a TTY.

This specification defines a design for extracting the internal logic (candidate enumeration, validation, writing) of the existing interactive `knowledge add` — while leaving it as-is — into **TTY-independent `validate` / `apply` subcommands**. The human interactive CLI, AI agents, and a future GUI backend will all use this `validate` / `apply` as a common execution engine.

## 2. Scope

In scope:

- `markharness knowledge validate <draft-file>` (new)
- `markharness knowledge apply <draft-file> [flags]` (new)
- Schema definition of the draft YAML
- Machine-readable error output format
- axis registry cross-check rules
- Change in handling of the Condition id's redundant prefix

Out of scope (not covered by this specification):

- Implementation of `knowledge add --edit`, which launches `$EDITOR` (designed separately as a wrapper that calls this specification's API; only interface requirements are noted in §9.3 of this document)
- Removal/replacement of the existing `knowledge add` (the sequential-prompt version). It is retained for now.
- Implementation of the GUI itself.

## 3. Command Specification

### 3.1 `markharness knowledge validate <draft-file>`

No side effects. Reads the draft file, validates schema and consistency, and returns only the result.

```
markharness knowledge validate <draft-file> [--json] [-d, --dir <path>]
```

| Argument/flag | Required | Description |
|---|---|---|
| `<draft-file>` | Yes | Path to the draft YAML file |
| `-d, --dir <path>` | - | Target project root (the parent of `knowledge/`). Defaults to the current directory if omitted |
| `--json` | - | Output errors/results as a single line of JSON (see §6). Human-readable text if omitted |

Exit code: follows the table in §3.4. Performs no file writes whatsoever.

### 3.2 `markharness knowledge apply <draft-file> [flags]`

Validates the draft, and if there are no issues, writes files under `knowledge/` **atomically**.

```
markharness knowledge apply <draft-file> [--json] [-d, --dir <path>] [--strip-redundant-prefix] [--dry-run]
```

| Flag | Description |
|---|---|
| `--json` | Same as §3.1 |
| `--dir` | Same as §3.1 |
| `--strip-redundant-prefix` | If the Condition id has a redundant prefix overlapping with the Behavior id, adopts the stripped id without confirmation. If not specified, stops with a validation error, presenting the stripped candidate id within the error (§7). |
| `--dry-run` | Synonymous with `validate` (only validates, does not write). An alias intended for use in CI, etc. |

**Atomicity requirement**: Even when creating only some of the five levels (Requirement through ExpectedResult) as new, writing occurs all together only after validation has fully passed. If an I/O error occurs during writing, each file is written via temp-file + rename to the extent possible, and on failure, files already succeeded are also rolled back (deleted). At minimum, a state where "only some files get written due to a validation error" must not occur.

### 3.3 `markharness knowledge add --edit` (Reference — Detailed Separately)

A thin wrapper for humans. Generates a template draft into a temporary file → launches `$EDITOR` → after saving, internally calls the equivalent of `apply`. On a validation error, reopens the editor for correction. The interface is designed on the premise of calling this specification's API (the `apply_draft` function in §9.2), and the implementation is a separate ticket.

### 3.4 Exit Codes

| Code | Meaning |
|---|---|
| 0 | Success (validate: no errors, apply: write succeeded) |
| 1 | Validation error present (the error list of §6 output to stderr) |
| 2 | Usage error (file not found, unparseable YAML, invalid flag) |
| 3 | Filesystem error (e.g. write failure; apply only) |

## 4. Draft File YAML Schema

A tree structure with a one-to-one correspondence to the existing structs in `knowledge.rs` (`Requirement`/`Feature`/`Behavior`/`Condition`/`ExpectedResult`). One `apply` registers one chain (Requirement→Feature→Behavior→Condition→multiple ExpectedResults). Bulk registration of multiple chains is out of scope (see §10, "Not Supported").

```yaml
requirement:
  id: controls              # required. ASCII slug ([a-z0-9-]+)
  label: controls           # optional. If omitted, id is used as label
  axis: [gameplay]           # required when newly creating; may be omitted when reusing an existing one (see "Reusing an Existing id" below)
  description: null         # optional

feature:
  id: player-jump
  label: player-jump
  axis: [gameplay, animation]

behavior:
  id: jump
  label: jump
  axis: [gameplay]
  description: Player presses jump.   # required (description is required only for Behavior, per the existing schema)

condition:
  id: ground
  label: ground
  description: Jump from the ground and land

expected:
  - description: lands safely
  - description: takes fall damage if height > 3m
```

Field specifications conform to each struct definition in `src/knowledge.rs` (the type and required/optional status of `id`/`label`/`axis`/`description` follow the current structs as-is). Only `expected` is an array (multiple ExpectedResults can be added in one apply; this follows the existing sequential numbering logic `{condition_id}-{seq:03}`).

## 5. List of Validation Rules

Each rule corresponds to an error code in §6.

| # | Target | Rule | Error code |
|---|---|---|---|
| 1 | All ids | Must satisfy `is_valid_slug` (lowercase alphanumerics and hyphens only) | `invalid_slug` |
| 2 | requirement/feature/behavior | When newly created, `axis` must have 1 or more entries | `missing_axis` |
| 3 | behavior | `description` must not be empty | `missing_description` |
| 4 | condition | `description` must not be empty | `missing_description` |
| 5 | expected[] | `description` must not be empty (each element) | `missing_description` |
| 6 | axis in general | Value must match an id already registered in `axes/*.yml` (§8) | `unknown_axis` |
| 7 | condition.id | If it has a redundant prefix (`{behavior_id}-`) overlapping with the Behavior id, stops unless `--strip-redundant-prefix` is specified (§7) | `redundant_prefix` |
| 8 | When reusing an existing id | When the provided `axis`/`description`/`label` does not match the value in the existing file (§10.2) | `conflicting_existing_value` |
| 9 | requirement/feature/behavior/condition | The parent reference (e.g. feature.requirement) must exist. If newly created within the draft, must be consistent with the draft's own value | `parent_not_found` |
| 10 | feature.forked_from | If a value is specified, it must match the `id` of some Feature under `knowledge/` (added at implementation time; the `forked_from` of paper §3.1, not noted in the initial version of this specification) | `unknown_forked_from` |
| 11 | label on requirement/feature/behavior/condition | Must not contain a newline. `label` is serialized as a single-line plain scalar, so a multi-line value would corrupt the output YAML (added at implementation time) | `multiline_label` |

**Notes added at implementation time**: Rule #10 (`unknown_forked_from`) was not in the initial version of this specification, but was added by `knowledge_draft.rs::feature_id_exists` to match the implementation of the `feature.forked_from` field (`knowledge.rs`). Since Feature ids are assumed to be unique across the entire repository even when nested under `requirement`, the search is exhaustive under `knowledge/` across `requirement` levels.

**Notes added at implementation time**: Rule #11 (`multiline_label`) was also not in the initial version of this specification. It was added via `knowledge_draft.rs::push_multiline_label` to close a known gap (previously tracked as a TODO comment) where `knowledge.rs::serialize_requirement` and its siblings emit `label` as a plain scalar rather than a block scalar, so a multi-line value would corrupt the output YAML. Unlike `description`, `label` is a short identifying label with no semantic need to support multiple lines, so the fix is a fail-fast check in `validate_draft` rather than switching `label` to a block scalar.

**Notes added at implementation time**: Rule #2 (`missing_axis`) populates `suggestion` with the registered axes (comma-separated, sorted) whenever `axes/*.yml` has at least one registered; when none are registered, there is nothing to suggest, so `suggestion` stays `null` and `message` instead points the caller at `markharness axes add` (`knowledge_draft.rs::missing_axis_error`). The blank draft template `knowledge add --edit`/`knowledge scaffold` emit (`EDIT_TEMPLATE`) used to write `axis: []`; it now writes a blank scalar (`axis:`) like the template's other unfilled fields (`id:`, `label:`) do, since `axis: []` looked filled-in at a glance even though it still triggers this rule for a new entry.

## 6. Error Output Format

**Human-readable (default)**: One error per line to `stderr`.

```
error: unknown_axis: axis "validdation" is not registered (path=behavior.axis[0])
error: redundant_prefix: condition.id "jump-ground" starts with behavior.id "jump-" (suggested="ground", path=condition.id)
```

**Machine-readable (`--json`)**: An array of error objects output as a single line of JSON to `stdout` (for piping/agent parsing).

```json
{
  "ok": false,
  "errors": [
    {
      "code": "unknown_axis",
      "path": "behavior.axis[0]",
      "value": "validdation",
      "message": "axis \"validdation\" is not registered",
      "suggestion": "validation"
    },
    {
      "code": "redundant_prefix",
      "path": "condition.id",
      "value": "jump-ground",
      "message": "condition.id starts with behavior.id prefix",
      "suggestion": "ground"
    }
  ]
}
```

The `suggestion` field is set only when possible (a close-match candidate or corrective proposal). Agents can use this to build automatic retries. On success, `{"ok": true, "written": ["knowledge/controls/player-jump/jump/ground/expected/002.yml", ...]}` is output (apply only).

## 7. Handling of the Condition id's Redundant Prefix

The current interactive CLI (`interactive.rs`) strips the prefix and writes without confirmation. `apply` disables this by default.

- `--strip-redundant-prefix` not specified: Stops with a `redundant_prefix` error (presenting the stripped id in `suggestion`, in the format of §6). The caller (human/agent) either corrects `condition.id` or re-runs with the flag attached.
- `--strip-redundant-prefix` specified: Applies the current logic (`strip_redundant_condition_prefix`) as-is and writes with the stripped id. A warning message is output, but it does not stop.
- If an existing directory already exists with a name that has the redundant prefix (legacy data), it is "reused as-is without stripping, preferring the existing one," same as the current interactive CLI (following the behavior of the `legacy_condition_dir_with_redundant_prefix_is_reused_without_stripping` test).

## 8. axis Registry Cross-Check

`axes/<id>.yml` (`id`/`label`/`description`) is read at startup, and each value of `requirement.axis`/`feature.axis`/`behavior.axis` is required to exactly match a registered id.

- An unregistered axis stops with an `unknown_axis` error (not merely a warning; in agent-driven use, "passing through unnoticed" is more harmful than "noticing that the axis registry should be updated").
- The `markharness axes list [--json]` command is implemented (`docs/cli-manual.md` §1.7). Agents can use this to obtain the axis list in advance.

## 9. Internal Architecture

### 9.1 Proposed Module Structure

```
src/
├── knowledge.rs        # existing. Structs, parse/serialize, slug utilities (unchanged)
├── knowledge_draft.rs  # new. KnowledgeDraft struct, YAML parsing, validate()
├── knowledge_apply.rs  # new. apply_draft() (validation + atomic write)
├── interactive.rs      # existing. To be refactored in the future to assemble knowledge_draft::KnowledgeDraft and call knowledge_apply::apply_draft() (out of direct scope of this specification; see §10)
└── cli.rs               # add knowledge validate / knowledge apply subcommands
```

### 9.2 Core Function Signatures (Proposal)

```rust
// knowledge_draft.rs
pub struct KnowledgeDraft {
    pub requirement: RequirementDraft,
    pub feature: FeatureDraft,
    pub behavior: BehaviorDraft,
    pub condition: ConditionDraft,
    pub expected: Vec<ExpectedDraft>,
}

pub struct ValidationError {
    pub code: ValidationErrorCode,
    pub path: String,
    pub value: Option<String>,
    pub message: String,
    pub suggestion: Option<String>,
}

pub fn parse_draft(yaml: &str) -> Result<KnowledgeDraft, DraftParseError>;

pub fn validate_draft(
    root: &Path,
    draft: &KnowledgeDraft,
    options: &ValidateOptions, // strip_redundant_prefix: bool
) -> Vec<ValidationError>;

// knowledge_apply.rs
pub struct ApplyResult {
    pub written_paths: Vec<PathBuf>,
}

pub fn apply_draft(
    root: &Path,
    draft: &KnowledgeDraft,
    options: &ApplyOptions, // strip_redundant_prefix: bool
) -> Result<ApplyResult, ApplyError>; // internally calls validate_draft, aborting before writing if there are errors
```

`validate_draft` is a pure function with no dependency whatsoever on TTY/stdin/stdout (completely separated from the `prompt_*` function group currently in `interactive.rs`). This allows the `validate` subcommand, the `apply` subcommand, the future `add --edit` wrapper, and a future GUI backend to all share the same function.

### 9.3 Relationship to `add --edit` (Reference)

`add --edit` is expected to be implemented with the following pseudocode (separate ticket).

```
draft = generate_template_draft(root)  // YAML string with existing candidate lists embedded as comments
tmp_file = write_temp(draft)
loop:
    open_editor(tmp_file)
    parsed = parse_draft(read(tmp_file))
    errors = validate_draft(root, parsed, options)
    if errors.is_empty():
        apply_draft(root, parsed, options)
        break
    else:
        print_errors_as_comments_in(tmp_file, errors)  // prompt re-editing
```

## 10. Correspondence with Existing Code and Migration Policy

| Existing element | Handling policy |
|---|---|
| `interactive.rs::run_add` | Unchanged (this specification is additive only). A separate ticket will be filed for a future refactor to replace it with a form that internally uses `knowledge_draft`/`knowledge_apply`. |
| `strip_redundant_condition_prefix` (`knowledge.rs`) | No signature change. Shared, called from `apply`/`add --edit`. |
| `is_valid_slug` / `normalize_slug_candidate` / `romanize_label` | Shared. Also reused for automatic id proposal in the draft (at template generation time in §9.3). |
| `list_candidate_ids` (`interactive.rs`) | Moved to `knowledge_draft.rs`, used for existing-id lookup within `validate_draft` and for the "parent reference existence check" in §9. |

**Not supported (explicitly out of scope)**:

- Bulk registration of multiple chains (registering multiple Features/Behaviors simultaneously in one file) is not handled in this specification. If needed, it will be considered as a separate specification, arraying it similarly to `expected`.
- Updating existing files (changing label or axis) is not supported. When reusing an existing id, only "verify whether it matches" is performed (§5, rule #8), no rewriting occurs. If updates are needed, a separate command (e.g. `knowledge edit`) will be designed separately.

## 11. Test Plan

Corresponding to the existing tests in `interactive.rs` (the FULL_INPUT-based scenario group, `interactive.rs:290-846`), the following are implemented in `knowledge_draft.rs`/`knowledge_apply.rs`.

- Normal case: bulk creation of all 5 new levels (equivalent to the existing `creates_new_requirement_feature_behavior_condition_and_expected_from_scratch`)
- Reusing an existing id: succeeds if values match, `conflicting_existing_value` error if they do not (new test; the existing interactive CLI has no equivalent functionality)
- Unregistered axis: `unknown_axis` error is returned (new)
- Redundant prefix: error without the flag, stripped with the flag (rewritten as a non-interactive version of the existing `auto_dedup_strips_redundant_condition_prefix_and_notifies`)
- Legacy directory precedence (equivalent to the existing `legacy_condition_dir_with_redundant_prefix_is_reused_without_stripping`)
- Atomicity: no files are written at all upon validation failure (new, apply only)
- Schema verification of `--json` output (new)
- CLI integration tests: verification of exit codes and stdout/stderr for `markharness knowledge validate`/`apply` (added to `cli.rs`)

## 12. Open Items (to Be Decided Before Implementation) — Resolved by Implementation

1. Whether to accept the discrepancy between the current spec of making `expected` an array and the existing interactive CLI, which creates only one ExpectedResult per run. → **Resolved**: implemented under the premise that it is accepted (`knowledge_apply.rs`, continuing sequential numbering from `existing_expected_count`).
2. Whether to include the addition of the `axes list` command in this ticket or make it a separate ticket. → **Resolved**: implemented as `markharness axes list` (see §8).
3. The comparison granularity of `conflicting_existing_value` (whether to require exact match of label/axis/description, or to treat an omitted field as "unspecified" and ignore it). → **Resolved**: implemented as per the tentatively adopted policy. `knowledge_draft.rs`'s `push_conflicting_value`/`push_conflicting_axis` cross-checks against the existing value only when the corresponding field on the `draft` side is `Some`; when it is `None` (omitted), the comparison itself is not performed.
