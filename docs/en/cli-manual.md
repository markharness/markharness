# markharness CLI Manual

**Status**: Implemented (implemented commands are in Chapter 1) / Draft (tentative proposals for unimplemented commands are in Chapter 2)
**Related documents**: [product-operation.md](./product-operation.md) (use case mapping), [testcase-generation-design.md](./design/testcase-generation-design.md) (generation rules for `generate`), [decisions/0027](./decisions/0027-declarative-knowledge-reconciliation.md) (the design of `knowledge reconcile`)

**Purpose**: This document summarizes how to use the `markharness` CLI, divided into **implemented commands** and **unimplemented (planned) commands**. The mapping to use cases (UC1–UC8) is based on the "3. Use Case Descriptions" table in `docs/product-operation.md`. For the concrete generation rules of the implemented commands, see `docs/design/testcase-generation-design.md` (however, the current implementation of `generate`/`verify` has since been overhauled into the 4-tier `feature → behavior → condition → expected` model after that document was written; treat sections 1.3/1.4 of this manual as authoritative for the details).

---

## 1. Implemented Commands

### 1.1 `markharness init` — Project initialization (prerequisite for UC1–UC8)

```text
markharness init
```

**Purpose**: Of the physical directory structure that underpins UC1–UC8 (paper §3.5, lines 244–273), this creates the six directories that need to be created in the target repository, so that subsequent commands can operate.

All six directories are created under a single `.markharness/` namespace, so they don't collide with a pre-existing top-level `knowledge/` or `schema/` in the host project:

```text
.markharness/
├── knowledge/
├── axes/
├── generated/
├── executions/
├── changes/
└── schema/
```

| Directory                 | Corresponding UC                                                                        |
| -------------------------- | ----------------------------------------------------------------------------------------- |
| `.markharness/knowledge/`  | UC1 (describe knowledge) / UC1b (manually describe `forked_from`)                        |
| `.markharness/axes/`       | UC1 (registry of the cross-cutting Axis viewpoint, §3.1)                                  |
| `.markharness/generated/`  | UC2 (deterministically generate TestCase) / UC3 (review and merge generated artifacts)    |
| `.markharness/executions/` | UC4 (tag a milestone; destination for recording execution results)                        |
| `.markharness/changes/`    | UC5 (automatically compute ChangeEvent) / UC6 (run backfill asynchronously)               |
| `.markharness/schema/`     | UC7 (discard/rebuild the id cache; definitions of format/normalization rules)              |

UC8 (importing from existing tools) has no dedicated directory, since it is assumed the converted results are written into `.markharness/knowledge/`, and is therefore out of scope.

**Behavior**

- For each directory: if it does not exist, create it; if it already exists, do nothing (including leaving its contents untouched) — an idempotent operation. Re-running on an already-initialized project does not error; only the missing directories are additionally created.
- Creates `.markharness/config.toml` (containing `schema_version = 1` and `[knowledge]\nschema_version = 1`, [decisions/0014](./decisions/0014-knowledge-schema-version-persistence.md)). Every command other than `init` uses the top-level `schema_version` marker to find its own project root: it searches upward for it when `--dir` is omitted, and validates it's present even when `--dir` is explicit (exiting with a `markharness init`-guidance error if not found). `[knowledge].schema_version` is a separate, independently-scoped value that `changes compute` (section 1.9) resolves per-ref to detect Knowledge schema migrations. It is committed to the repository (not added to `.gitignore`). Left untouched if it already exists.
- On success, prints the created paths to standard output.

**Example**

```console
$ markharness init
initialized .markharness/{knowledge,axes,generated,executions,changes,schema}/ under /path/to/project

$ markharness init
initialized .markharness/{knowledge,axes,generated,executions,changes,schema}/ under /path/to/project
```

**Use case mapping**: Does not explicitly correspond to any single UC, but is a helper command that satisfies the prerequisite for starting all of UC1–UC8.

---

### 1.2 `markharness knowledge reconcile` — Declarative reconciliation of a Knowledge Intent (UC1: describe knowledge)

```text
markharness knowledge reconcile <intent-file> [--check] [--json] [-d, --dir <path>]
markharness knowledge reconcile --print-template
```

**Purpose**: Takes a Knowledge Intent (a YAML document describing the desired state) as a single file, reconciles it against the repository's current state, and applies the resulting creations, updates, and renames of Requirements, Features, Behaviors, and Scenarios in a single transaction. This command is the only write interface for Knowledge authoring; humans and AI agents use the same path ([decisions/0027](./decisions/0027-declarative-knowledge-reconciliation.md), [decisions/0028](./decisions/0028-consolidate-knowledge-authoring-commands.md)).

An Intent describes **desired state, not a procedure**. New elements cross-reference each other through a document-local `key` (never persisted); existing elements are selected by `uid`. Re-running the same Intent leaves matching elements `unchanged` with no write.

**Options**

| Option             | Description                                                                                                                         |
| ------------------ | ----------------------------------------------------------------------------------------------------------------------------------- |
| `<intent-file>`    | Path to the Knowledge Intent YAML. Mutually exclusive with `--print-template` (exactly one is required)                              |
| `--print-template` | Prints a blank Knowledge Intent template to stdout. Cannot be combined with any other option                                         |
| `--check`          | Parses, matches, validates, and builds the mutation plan with the same implementation as a normal run, but **writes nothing**        |
| `-d, --dir <path>` | Target project directory (the parent of `.markharness/knowledge/`). Defaults to the project root (discovered by walking up from cwd) |
| `--json`           | Emits the result and diagnostics as single-line JSON. Otherwise human-readable text is printed                                       |

**Exit codes**

| Code | Meaning                                                                                                                    |
| ---- | -------------------------------------------------------------------------------------------------------------------------- |
| 0    | Success (a normal run applied the Intent; `--check` found nothing to change)                                               |
| 1    | Validation error (diagnostics are reported)                                                                                |
| 3    | Another identity operation is in progress, or a previous operation left recovery pending                                   |
| 4    | `--check` only: applying the Intent would change something, so scripts can detect "changes pending" without parsing output |

`--check` shares the planning implementation but its result is not a permit for a later write. A normal run re-reads the current state just before committing and stops with a stale plan if the input state has changed.

**Knowledge Intent format**

Get a template with `markharness knowledge reconcile --print-template`.

```yaml
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: req_todo # Document-local reference name; never persisted
    id: todo # Required. ASCII slug
    source: native # native | external
    label: TODO management # Required when source is native
    axis: [functional] # Registered axes only; anything else is an unknown_axis error
    description: null # Optional
    related_issues: [] # Optional

features:
  - key: feature_todo_add
    id: todo-add
    contributes_to: [req_todo] # Requirement keys (new) or uids (existing)
    label: Add a TODO
    axis: [functional]
    description: null # Optional
    forked_from: null # Optional. id of the Feature this one is conceptually derived from (section 3.1); must name an existing Feature
    behaviors:
      - id: add
        label: Add
        axis: [functional]
        description: The user adds a TODO item. # A new Behavior requires a description
        procedures: # Optional. Common procedures this Behavior declares (ADR 0017)
          - name: open-app
            steps:
              - Launch the application.
        scenarios:
          - id: empty-list
            label: Empty list
            description: Adding to an empty list
            phases:
              - steps:
                  - use: open-app # Invokes a common procedure declared in procedures
                  - action: Type a title and submit. # One element = one operation
                results:
                  - The item appears in the list. # One element = one observable result
            implementation_note: null # Optional. Implementation rationale note; never used for generation (ADR 0016)
```

`mode` accepts only `merge` in the initial version (it never deletes existing elements). A `format` other than `markharness/knowledge-intent/v1` is an `invalid_format` error.

Supplying a string field as an empty (or whitespace-only) value is distinct from omitting it, and is rejected with `missing_required_field`: `label: ""` is written out as `label: `, which reads back as YAML null and leaves the saved file broken, and an empty `action` or `results` entry is something a Test Executor can neither perform nor observe.

A Requirement's `native` and `external` modes have disjoint field sets (ADR 0023). A `native` Requirement owns its own content, so it requires `label` and cannot carry `source_locator`. An `external` one is owned by the external document, so it can carry neither `label` nor `description`, and requires both `source_locator` and `source_revision: current` — the latter resolved to the current blob OID at run time. A patch that switches `source` drops the fields the new mode cannot carry, and stops with `missing_required_field` when a field the new mode requires is supplied by neither the Intent nor the current value — switching from external to native, for instance, has to supply a `label`.

**Updating and renaming existing elements**

Existing elements are selected by `uid`, not by `key`/`id`. UIDs come from a successful run's output or from a `--json` snapshot.

```yaml
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: 01J8Z... # Selects an existing Feature
    id: todo-create # Changing the display ID performs a rename (the uid is kept)
    label: Create a TODO # Omitted fields keep their current value
    contributes_to: [01J8A..., 01J8B...] # Collections are replaced wholesale
```

- Value collections (`axis`, `contributes_to`, `procedures`) are **replaced wholesale** when present, keep their current value when omitted, and are cleared by an explicit empty array.
- A rename is just a changed `id` on a `uid`-selected element. The uid and its identity events are preserved.
- Reparenting a Scenario to a different Behavior is expressed by writing the `uid`-selected Scenario under that other Behavior. The file move is reported as `previous_path` in the result.
- Adding and removing Feature-to-Requirement relationships is expressed by replacing `contributes_to` wholesale.
- Re-pinning an external Requirement is expressed with `source_revision: current`, which re-pins it to the current blob OID. Using it on a `source: native` Requirement is an error.

**Output**

In human-readable mode, one `created` / `updated` / `unchanged` line is printed per element (`no changes` when nothing at all differs).

```text
created requirement 'todo' (uid 01J8A...) .markharness/knowledge/requirements/todo.yml
updated feature 'todo-create' (uid 01J8Z...) .markharness/knowledge/features/todo-add.yml -> .markharness/knowledge/features/todo-create.yml
unchanged behavior 'add' (uid 01J8C...) .markharness/knowledge/features/todo-create/behaviors/add.yml
```

`--json` emits `{"ok":true,"created":[...],"updated":[...],"unchanged":[...]}` on one line. Each element carries `kind` / `uid` / `id` / `path`, plus `previous_path` only when the file actually moved. On a validation error it returns the diagnostic code, location, and message in an `{"ok":false,...}` document; human-readable mode prints `error[<code>]: <message> (<location>)` to stderr.

**Atomic persistence**: Knowledge files and identity events are written in a single transaction, so an interruption never exposes UID-less Knowledge or a half-updated state to later commands. When an interruption is detected the command exits 3 reporting that recovery is pending; run it once without `--check` to complete the recovery.

**Prerequisite**: Register every axis the Intent references with `axes add` (section 1.4) beforehand. Unregistered axes are rejected as `unknown_axis` before anything is written.

### 1.3 `markharness generate` — Deterministic generation of TestCase (UC2: deterministically generate TestCase)

```text
markharness generate [--json] [-d, --dir <path>]
```

**Purpose**: Deterministically traverses `.markharness/knowledge/`, mechanically assembles `TestCase` from `Requirement × Feature × Behavior × Condition × ExpectedResult`, and regenerates them as `.yml` files under `.markharness/generated/testcases/`, **one file per Condition**. Each run empties `.markharness/generated/testcases/` before rewriting it, so stale files corresponding to a deleted Condition are automatically removed too.

**Actor**: Nominally the CI Bot (UC2), but manual execution for local pre-checks is also possible.

**Algorithm overview**

- Traverses `.markharness/knowledge/` in the order `requirement.yml` → `feature.yml` → `behavior.yml` → `condition.yml` → `expected/*.yml`, in path sort order (independent of the execution environment or timestamps). No `TestCase` is generated from a `Feature` that has no `Behavior`, or from a `Condition` whose `expected/` is empty (or absent).
- **Aggregation model**: All files under a single `Condition`'s `expected/` are aggregated into the `phases` array of a single `TestCase` (1 Condition = 1 TestCase).
- `case_id = "tc-{requirement.id}-{feature.id}-{behavior.id}-{condition.id}"`. Concatenating all four ids (`requirement`/`feature`/`behavior`/`condition`) makes a `case_id` collision structurally impossible even if a `condition.id` is reused under a different Behavior.
- The output file is written to `.markharness/generated/testcases/{requirement.id}/{feature.id}/{behavior.id}/{condition.id}.yml`, fully mirroring `.markharness/knowledge/`'s own hierarchy (the earlier flat `.markharness/generated/testcases/{condition.id}.yml` naming had a defect where reusing the same `condition.id` under a different Behavior silently overwrote the earlier file).
- [ADR 0016](decisions/0016-behavior-condition-precondition-step-result-model.md) replaced the earlier `title`/`steps`/`expected` fields with `preconditions`/`phases`. `preconditions` = `behavior.preconditions` concatenated with `condition.additional_preconditions` (`behavior.description`/`condition.description` remain human-facing summaries not used for generation). `phases` is built by walking `expected/*.yml` in file-name order and producing one `Phase { steps, results }` per file: the first phase's `steps` is `condition.steps` followed by that `expected/*.yml`'s own `additional_steps` (if any); every later phase's `steps` is that file's `additional_steps` alone (non-empty is enforced by `markharness validate` for every phase after the first). Each phase's `results` is that `expected/*.yml`'s `results`.
- `generated_from` records each of the `requirement` / `feature` / `behavior` / `condition` ids, and the source `expected_results` (the list of `id`s of `expected/*.yml`) that were aggregated.
- `axis`: a list of viewpoints formed by combining (union, deduplicated and sorted) the `axis` of the `Requirement` / `Feature` / `Behavior` (§3.4 "axis inheritance").
- The output is serialized with `serde_yaml_ng`, and always produces the same output for the same input (determinism, a prerequisite for diff verification in CI).
- In addition to `.markharness/generated/testcases/*.yml`, `generate` also regenerates `.markharness/generated/traceability-index.json` at the same time (a machine-readable index holding the Requirement → Feature → Behavior → Condition → TestCase correspondence, as pretty-printed JSON via `serde_json`). `markharness verify` (section 1.4) also includes this file in its diff verification.
- Omitting `--dir` searches upward from the current directory for `.markharness/config.toml` and targets the project root it finds (the same convention every other command follows; `generate` used to be the sole exception, always pinned to the current directory).
- `--json` prints `{"ok":true,"generated":<count>,"written":[<list of written file paths, including traceability-index.json>]}` instead of the human-readable message, so a caller can mechanically reconcile the reported count against the actual written files.

**Example**

```console
$ markharness generate
generated 1 testcase(s) into .markharness/generated/testcases/
$ markharness generate --json
{"ok":true,"generated":1,"written":[".markharness/generated/testcases/req-todo/todo/todo-add-task/todo-add-task-empty-input.yml",".markharness/generated/traceability-index.json"]}
```

`.markharness/generated/testcases/task-management/add-todo/add-task/empty-title.yml`:

```yaml
case_id: tc-task-management-add-todo-add-task-empty-title
generated_from:
  requirement: task-management
  feature: add-todo
  behavior: add-task
  condition: empty-title
  expected_results:
    - empty-title-001
preconditions:
  - "Open the todo app."
phases:
  - steps:
      - "Click the title field."
      - "Press the add button."
    results:
      - "A validation error is shown under the title field."
```

If `.markharness/knowledge/` has nothing in it, `.markharness/generated/testcases/` becomes empty (0 files).

**Use case mapping**: UC2 "deterministically generate TestCase" (`docs/product-operation.md` line 105). Diff verification in CI (UC3) is done by `markharness verify` in section 1.4.

---

### 1.4 `markharness verify` — Diff verification of generated artifacts (UC3: review and merge generated artifacts)

```text
markharness verify [--json] [-d, --dir <path>]
```

**Purpose**: Rebuilds the TestCase and `traceability-index.json` from `.markharness/knowledge/` using the same logic as `generate` (without writing to disk), and compares them against the committed `.markharness/generated/testcases/*.yml` and `.markharness/generated/traceability-index.json`. Intended to be run in CI to check that changes to `.markharness/knowledge/` have not been forgotten to be reflected in `.markharness/generated/` (this command already covers what `generate --check` would have done).

**Actor**: Reviewer / CI Bot (UC3)

**Options**

| Option              | Description                                                         |
| -------------------- | -------------------------------------------------------------------- |
| `-d, --dir <path>`   | Target project directory. Defaults to the project root (auto-detected by searching upward from cwd).        |
| `--json`             | Prints structured JSON instead of the human-readable message (see below). |

**Behavior**

- If there is no diff, prints `.markharness/generated/testcases/ is up to date with .markharness/knowledge/` and exits with code `0`.
- If there is a diff, lists the added, removed, and changed files, labeled `added:` / `removed:` / `changed:`, in file-name sort order, and exits with code `1` (does not show a unified diff of the contents). `.markharness/generated/traceability-index.json` is included in the listing on the same footing as the other generated artifacts (under the file name `traceability-index.json`).
- With `--json`, always prints `{"would_change":<bool>,"added":[...],"changed":[...],"removed":[...]}` regardless of whether there's a diff. Each path is relative to `.markharness/generated/`: TestCase files carry a `testcases/` prefix (e.g. `testcases/task-management/add-todo/add-task/empty-title.yml`), and `traceability-index.json` is listed by its bare name (it lives directly under `.markharness/generated/`, not under `.markharness/generated/testcases/`). Exits `0` when there's no diff (`would_change:false`), `1` when there is (`would_change:true`).

**Example (no diff)**

```console
$ markharness verify
.markharness/generated/testcases/ is up to date with .markharness/knowledge/
$ markharness verify --json
{"would_change":false,"added":[],"changed":[],"removed":[]}
```

**Example (diff present)**

```console
$ markharness verify
added: .markharness/generated/testcases/task-management/add-todo/add-task/empty-title.yml
changed: .markharness/generated/testcases/task-management/add-todo/add-task/max-length.yml
removed: .markharness/generated/testcases/task-management/add-todo/add-task/duplicate-title.yml
$ echo $?
1

$ markharness verify --json
{"would_change":true,"added":["testcases/task-management/add-todo/add-task/empty-title.yml"],"changed":["testcases/task-management/add-todo/add-task/max-length.yml"],"removed":["testcases/task-management/add-todo/add-task/duplicate-title.yml"]}
$ echo $?
1
```

**Use case mapping**: UC3 "review and merge generated artifacts" (`docs/product-operation.md` line 106). When a diff is detected, judging whether its content is intentional and merging it is the Reviewer's role (a point of human judgment).

---

### 1.5 `markharness axes list` — List the axis registry

```text
markharness axes list [--json] [-d, --dir <path>]
```

**Purpose**: Prints the list of viewpoints registered under `.markharness/axes/*.yml`, in ascending id order. A reference command for pre-emptively avoiding `unknown_axis` errors from `knowledge reconcile`.

**Behavior**: Without `--json`, prints `id (label)` (or just id if the label equals the id) one per line, and prints `no axes registered under .markharness/axes/` if there are zero registered. With `--json`, prints `[{"id":...,"label":...|null}]` as single-line JSON.

**Example**

```console
$ markharness axes list --dir tmp/todo-sample
gameplay (Gameplay)
ui

$ markharness axes list --dir tmp/todo-sample --json
[{"id":"gameplay","label":"Gameplay"},{"id":"ui","label":null}]
```

**Use case mapping**: A helper command that does not explicitly correspond to any UC.

---

### 1.6 `markharness axes add` — Non-interactive axis registration

```text
markharness axes add <id> [--label <label>] [--json] [-d, --dir <path>]
```

**Purpose**: Creates `.markharness/axes/<id>.yml`. Every axis a Knowledge Intent references must already be registered (an unregistered axis is rejected as an `unknown_axis` error before anything is written), and `axes add` is the standalone write command for that, symmetric with the other resources (Requirement/Feature/Behavior/Scenario).

**Behavior**

- `<id>` follows the same slug constraint as `condition.id` etc. (lowercase alphanumerics and hyphens only). An invalid id exits with code `2`.
- Omitting `--label` defaults `label` to the same value as `<id>` (the same "id doubles as label when omitted" convention every other command follows).
- If `.markharness/axes/<id>.yml` already exists, it is **not overwritten**. An error message is printed and the command exits with code `2` (edit the existing file directly if you need to change it).
- With `--json`, prints `{"ok":true,"written":[".markharness/axes/<id>.yml"]}`.

**Example**

```console
$ markharness axes add persistence --dir tmp/todo-sample
created tmp/todo-sample/.markharness/axes/persistence.yml

$ markharness axes add persistence --dir tmp/todo-sample
error: axis 'persistence' already exists under .markharness/axes/
$ echo $?
2

$ markharness axes add security --label Security --dir tmp/todo-sample --json
{"ok":true,"written":["tmp/todo-sample/.markharness/axes/security.yml"]}
```

**Use case mapping**: Like `markharness axes list` (section 1.5), a helper command that does not explicitly correspond to any UC.

---

### 1.7 `forked_from` (UC1b: manually describe a conceptual derivation from another Feature)

Write the id of the source Feature into a Feature's `forked_from` in a Knowledge Intent for `knowledge reconcile` (section 1.2) (§3.1). If the referenced Feature does not exist anywhere under `.markharness/knowledge/`, it stops with an `unknown_forked_from` error. Because this is domain knowledge that cannot be automatically derived from Git history, unlike `derived_from` (the version history of the same Feature, §3.2–3.4), only validation is performed and no automatic computation is done.

```yaml
feature:
  id: player-double-jump
  label: player-double-jump
  axis: [gameplay]
  forked_from: player-jump # Conceptual derivation source (existing Feature id). Optional.
```

---

### 1.8 `markharness cache rebuild` — Discarding the id cache (UC7: discard/rebuild the id cache)

```text
markharness cache rebuild [-d, --dir <path>]
```

**Purpose**: Deletes `.markharness-cache/` entirely (the uncommitted cache of Feature id→tree SHA resolution results used by `changes compute` in section 1.9. It is keyed by a content-addressing scheme, and is automatically recomputed on load whenever the content of `.markharness/knowledge/` or the tool version changes, so explicit `rebuild` is normally unnecessary). Does not perform an immediate recomputation (it is computed lazily on the next `changes compute` run). No error occurs if the cache directory does not exist (idempotent).

**Example**

```console
$ markharness cache rebuild
removed .markharness-cache/ under /path/to/project
```

**Use case mapping**: UC7 "discard/rebuild the id cache" (`docs/product-operation.md`). A fail-safe for cases where id-resolution inconsistency is suspected.

**Note when changing a Feature's `id:` (for users, paper §3.3)**: The Feature id is tracked using the `id:` field of each `feature.yml` as the canonical source. If the value of `id:` itself is rewritten, the tool treats this as "the original Feature was deleted and a Feature with a new id was added," and `changes compute` cannot recover the `derived_from` relationship with past milestones (the version history is broken). **Renaming** a Feature directory (a path change) remains trackable as long as `id:` does not change, but this CLI has no migration procedure for a change to `id:` itself (such as recording an old-id→new-id alias); currently, users must strictly follow the practice of "never change `id:`." See [decisions/0004](./decisions/0004-feature-id-change-migration.md) for the status of consideration.

**On the cache key's version fields**: The `canonicalization_rule_version`/`id_index_schema_version` (paper §3.3) that make up the cache key in `.markharness-cache/` are currently fixed at `"1"` in the implementation. Since no normalization-rule revision or id-index format revision that would actually bump these values has yet occurred, it has not been empirically verified whether the cache is correctly discarded when the values are bumped.

---

### 1.9 `markharness changes compute` — Computing ChangeEvents (UC5: automatically compute ChangeEvent)

```text
markharness changes compute <from-milestone> <to-milestone> [--no-cache] [--current-tree] [--granularity <feature|behavior|condition>] [-d, --dir <path>]
```

**Purpose**: Between two milestones (using the git tag name as-is; milestone boundaries are determined purely by tag-name match, and correspondence with `.markharness/executions/*/milestone.yml` is the caller's responsibility), compares the tree SHA of each Feature directory under `.markharness/knowledge/` via `git ls-tree -r <tag> -- .markharness/knowledge`, computes a `ChangeEvent` for each changed Feature, and writes it to `.markharness/changes/<to-milestone>.yaml`. The Feature id uses the `id:` field of each `feature.yml` as the canonical source, and is tracked independently of the directory name (paper §3.3).

The target project directory (`-d`/`--dir`, the parent of `.markharness/knowledge/`) may be any directory within a git repository (it need not be the root of the repository itself). There used to be a known issue where this command would fail when the project directory was a subdirectory of the repository, due to a specification constraint of the `git show <ref>:<path>` syntax, but this has been resolved by switching to an `ls-tree`/`cat-file`-based implementation (details: [decisions/0006](./decisions/0006-nested-project-directory-support.md)).

**Actor**: CI Bot (UC5)

**Behavior**

- Before comparing anything, resolves `[knowledge].schema_version` from `from-milestone`'s and `to-milestone`'s own `.markharness/config.toml` ([decisions/0014](./decisions/0014-knowledge-schema-version-persistence.md)). A ref with no recorded version is treated as legacy schema version 1, and a warning to that effect is included in the output (see below). If the two resolved versions differ, or either is newer than this CLI build knows about, the command exits with an error and writes nothing — no `ChangeEvent` is generated and any existing `.markharness/changes/<to-milestone>.yaml` is left untouched (fail closed, since a raw tree-SHA diff across schema versions could otherwise misreport a schema-only migration as a Feature change).
- For each Feature, compares `from_blob`/`to_blob`; if they match, nothing happens. If it exists in only one, it is an addition/deletion; if it exists in both with differing values, it is a change, and one `ChangeEvent` is generated.
- `impacted_testcases` lists the `TestCase.case_id`s originating from the changed Feature, enumerated from the same generation graph as `generate` (section 1.3) (the structural generation graph of §3.2(A); version history is not used). Which point in time's `.markharness/knowledge/` this generation graph is built from splits into two modes as of 2026-08 (as of 2026-08-12; see also [change-event-verification-tracking-spec.md](./design/change-event-verification-tracking-spec.md) §2.4).
  - **Default (`--current-tree` not given)**: Built by loading the `.markharness/knowledge/` tree pointed to by the `to-milestone` tag directly from Git blobs. Recomputing the same interval later always yields the same result.
  - **When `--current-tree` is given**: Built from `.markharness/knowledge/` in the current working tree (legacy behavior). As long as the working tree keeps changing, recomputation results for the same interval can also change.
- **`--granularity <feature|behavior|condition>` (default: `feature`)**: Selects the unit `impacted_testcases` is narrowed down to (issue #15).
  - **`feature` (default)**: Unchanged from before this option existed — includes every TestCase originating from a changed Feature as a candidate (conservative, safe-side).
  - **`behavior`**: Compares tree SHAs per Behavior directory (`behavior.yml`) under the Feature, and includes only the TestCases originating from a Behavior that actually changed (or was added/removed). TestCases from an untouched sibling Behavior are excluded.
  - **`condition`**: Narrows further still, at the Condition directory (`condition.yml`) level.
  - `behavior`/`condition` do not affect Feature-level change *detection* itself (which Feature gets a `ChangeEvent`, rename tracking, `true_divergences`) — only the narrowing of `impacted_testcases`.
  - **Caveat (false-negative risk)**: The Behavior/Condition schema has no field expressing dependencies between siblings, and this command does not detect or infer any. A Feature boundary may encode an author's implicit coupling (shared setup, preconditions, etc.) that `behavior`/`condition` deliberately ignores in exchange for precision over recall. Since the tool cannot guarantee this trade-off is safe for any given project, the choice is left to the user's judgment of their own project.
  - The chosen granularity, and the evidence for the narrowing, are recorded on each computed `ChangeEvent`'s `impact_reason` field (`granularity` and `changed_paths`; see the output examples below). `changed_paths` is only populated for `behavior`/`condition`: the marker-file paths (`behavior.yml`/`condition.yml`) of the Behaviors/Conditions whose tree SHA actually changed (or was added/removed). It is empty for `feature`, since that granularity doesn't resolve individual Behaviors/Conditions.
- `change_type` (spec change / bug fix, etc.) is output as `null` at the time of computation. The practice is for a human to fill it in afterward via `markharness changes annotate` (section 1.15) (§3.5).
- Unless `--no-cache` is given, Feature tree SHA resolution results are read from and written to `.markharness-cache/` (section 1.8), keyed by content-addressing.
- On success, human output appends one `warning: ...` line per side that fell back to legacy schema version 1; `--json` output includes the same messages as a `"warnings"` array in the existing JSON envelope. Neither appears when both refs have a recorded `[knowledge].schema_version` — the JSON `"warnings"` key is omitted entirely rather than emitted as `[]`, since only optional field additions are allowed within one `schema_version` (§5 of [verification-plan-canonical-model-design.md](./design/verification-plan-canonical-model-design.md)).
- If either `from-milestone` or `to-milestone` has a `.markharness/executions/<name>/milestone.yml` whose recorded `commit_oid`/`knowledge_schema_version` disagrees with what that tag now resolves to, the command errors out before computing anything (a moved tag, or a hand-edited file — [decisions/0014](./decisions/0014-knowledge-schema-version-persistence.md)). A `milestone.yml` predating those fields is not checked.
- The `from-milestone..to-milestone` interval is traversed with `git rev-list --ancestry-path`, and for every two-parent merge commit present within the interval, the section 1.16 `lineage` determination logic is internally run using `git merge-base` (oldest first). If a target Feature is judged a `true_divergence` (true divergence) at any of the merges, an entry consisting of `merge_commit` (the merge commit SHA, for auditing) and `parent_tree_shas: [P1, P2]` is appended to the `true_divergences` field, in the order they occurred (§3.2). If the same Feature undergoes true divergence multiple times within the interval, all of them are recorded. For a normal linear history, or when there is no merge within the interval, it remains an empty array.
- **Note on branch-strategy dependence**: The `from_tree_sha`/`to_tree_sha` diff detection itself does not depend on the branch strategy (merge/squash/rebase/fast-forward), but `true_divergences` presupposes that a two-parent merge commit actually remains within the milestone interval; with squash merges, rebases, or fast-forward merges, the divergence relationship of the original branch is lost from the commit graph, so it is not detected (remains an empty array; paper §3.4 Table 2).

**Output example** (`.markharness/changes/m2.yaml`, linear history case)

```yaml
- event_id: player-jump--m1--m2
  feature_id: player-jump
  from_milestone: m1
  to_milestone: m2
  from_tree_sha: 1a2b3c...
  to_tree_sha: 4d5e6f...
  impacted_testcases:
    - tc-ground-001
  impact_reason:
    granularity: feature
    changed_paths: []
  change_type: null
  true_divergences: []
```

**Output example** (a case where a true divergence was detected in a merge within the interval)

```yaml
- event_id: player-jump--m1--m2
  feature_id: player-jump
  from_milestone: m1
  to_milestone: m2
  from_tree_sha: 1a2b3c...
  to_tree_sha: 7c8d9e...
  impacted_testcases:
    - tc-ground-001
  impact_reason:
    granularity: feature
    changed_paths: []
  change_type: null
  true_divergences:
    - merge_commit: 9f8e7d...
      parent_tree_shas:
        - 2b3c4d...
        - 5e6f7a...
```

**Output example** (with `--granularity behavior`, when only some Behaviors under the Feature changed)

```yaml
- event_id: player-jump--m1--m2
  feature_id: player-jump
  from_milestone: m1
  to_milestone: m2
  from_tree_sha: 1a2b3c...
  to_tree_sha: 4d5e6f...
  impacted_testcases:
    - tc-ground-001
  impact_reason:
    granularity: behavior
    changed_paths:
      - .markharness/knowledge/controls/player-jump/jump/behavior.yml
  change_type: null
  true_divergences: []
```

**Use case mapping**: UC5 "automatically compute ChangeEvent." A simplified implementation of this model's core contribution (§3.2–3.4).

---

### 1.10 `markharness backfill run` — Batch processing of past milestones (UC6: run backfill asynchronously)

```text
markharness backfill run [--no-cache] [--max-pairs <count>] [--time-budget <duration>] [-d, --dir <path>]
```

**Purpose**: Targets the milestones for which `.markharness/executions/*/milestone.yml` exists, orders them newest-first by the commit date (committer date) of the corresponding git tag, and runs processing equivalent to `changes compute` (section 1.9) for each pair of adjacent milestones, generating `.markharness/changes/<milestone>.yaml`. A single run processes all pairs and then exits (it is not a resident daemon; intended for periodic execution from CI, etc.).

**Behavior**

- The oldest milestone has nothing to compare against, so it is skipped.
- Completion of processing for each milestone (the "to" side) is recorded in `git notes --ref=markharness-backfill`; on the next run, the same pair is not recomputed and is skipped (§4.3).
- A pair whose Knowledge schema versions can't be compared safely (the same fail-closed check as `changes compute`, section 1.9, [decisions/0014](./decisions/0014-knowledge-schema-version-persistence.md)) is skipped rather than aborting the whole run — the rest of the pairs still get processed. This skip is *not* recorded in `git notes`, so a later run retries it automatically (e.g. once a converter for that schema version exists). Each skipped pair is printed as `skipped <to-milestone>: <reason>`, where `<reason>` is the same fail-closed error `changes compute` would show for that pair verbatim (both sides' schema versions, and that a CLI update or migration is needed — issue #29 §5) — not a generic message, so the operator can tell why without re-running `changes compute` by hand. The command exits with code `1` if any pair was skipped this way — a run that leaves an incompatible pair unprocessed is not reported as a clean success.
- A `milestone.yml` whose recorded `commit_oid`/`knowledge_schema_version` disagrees with what its tag now resolves to (a moved tag, or a hand-edited file) is a hard error for the pair involving it — this is *not* the fail-closed skip above; the whole run stops, since a stale or tampered audit copy needs a human to look at it rather than being retried automatically.
- Any legacy-schema-version warning encountered while processing a pair (the same warning `changes compute` would show for that ref) is printed as a `warning: ...` line.
- Unless `--no-cache` is given, it shares the same `.markharness-cache/` as `changes compute`.
- `--max-pairs` limits newly processed pairs per run; already-processed skipped pairs do not consume the limit.
- `--time-budget` checks the remaining budget before starting each unprocessed pair. Units are `ms`, `s`, `m`, and `h` (for example `30s` or `5m`). It does not interrupt a pair in progress.

The constraint for when the target project directory (`-d`/`--dir`) is a subdirectory of the git repository is resolved the same way as in section 1.9 ([decisions/0006](./decisions/0006-nested-project-directory-support.md)).

**Exit codes**

| Code | Meaning                                                                    |
| ---- | --------------------------------------------------------------------------- |
| 0    | Success — every pair was either processed or already up to date            |
| 1    | At least one pair was skipped as Knowledge-schema-incompatible             |

**Example**

```console
$ markharness backfill run
backfilled .markharness/changes/2026-08-release.yaml
backfill: 1 processed, 2 already up to date
```

**Use case mapping**: UC6 "run backfill asynchronously" (a simplified implementation of §4.1–4.3; the milestone-only scope and progress management via git notes follow the paper as written, but asynchronous workerization has been deferred).

---

### 1.11 `markharness milestone init` — Creating `.markharness/executions/<tag>/milestone.yml` (a helper for UC4: tag a milestone)

```text
markharness milestone init <tag> [--json] [-d, --dir <path>]
```

**Purpose**: Creates `.markharness/executions/<tag>/milestone.yml` corresponding to an existing `git tag <tag>`. UC4 itself (making the release-timing decision by putting down a `git tag`) remains a point of human judgment and is out of scope for this command, but this mechanically scaffolds that tag into the form that `backfill run` (section 1.10) can recognize (a directory name under `.markharness/executions/<name>/milestone.yml` that matches the tag name, [src/backfill.rs:21-22](../../src/backfill.rs#L21-L22)).

**Options**

| Option              | Description                                                                                                                                                     |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `<tag>`             | (required) The target `git tag` name. Used as-is as the directory name of `.markharness/executions/<tag>/` (no additional normalization/validation is performed).            |
| `-d, --dir <path>` | Target project directory (any directory within a git repository; need not be the repository's own root). Defaults to the project root (auto-detected by searching upward from cwd).                     |
| `--json`            | Prints the result as single-line JSON. If omitted, prints human-readable text.                                                                                    |

**Behavior**

- If the target `tag` does not exist as a `git tag`, prints an error message prompting the user to first run `git tag <tag>`, and exits with code `2` (no file is created).
- If the tag exists and `.markharness/executions/<tag>/milestone.yml` has not yet been created, writes `id: <tag>` plus two audit-only fields resolved from the tag itself, `commit_oid` (the full commit SHA the tag points at) and `knowledge_schema_version` (the `[knowledge].schema_version` recorded in the tag's own `.markharness/config.toml`, or `1` if the tag predates that field — [decisions/0014](./decisions/0014-knowledge-schema-version-persistence.md)). Neither field is treated as authoritative by `changes compute`/`backfill run`, which always re-resolve directly from the ref being compared; they exist only so a human reading `milestone.yml` can see what was recorded at `init` time.
- If `.markharness/executions/<tag>/milestone.yml` already exists, its contents are left unchanged; a message stating it is "already initialized" is printed, and it exits with code `0` (the same idempotent pattern as `markharness init`).

**Exit codes**

| Code | Meaning                                                              |
| ---- | --------------------------------------------------------------------- |
| 0    | Success (newly created, or idempotent exit when already initialized) |
| 2    | The target `git tag` does not exist                                  |
| 3    | Filesystem error                                                      |

**Example (new creation)**

```console
$ git tag 2026-08-release
$ markharness milestone init 2026-08-release
initialized .markharness/executions/2026-08-release/milestone.yml
```

**Example (error when the tag has not been created)**

```console
$ markharness milestone init 2026-08-release
error: git tag '2026-08-release' not found. Run `git tag 2026-08-release` first, then retry.
$ echo $?
2
```

**Example (idempotent)**

```console
$ markharness milestone init 2026-08-release
.markharness/executions/2026-08-release/milestone.yml is already initialized
$ echo $?
0
```

**Use case mapping**: Helps scaffold the destination for recording the results of UC4 "tag a milestone" (`docs/product-operation.md` line 107). The tagging decision itself continues to be made by a human.

---

### 1.12 `markharness binding set` / `list` — Declare how a TestCase is verified (ADR 0020, ADR 0025)

```text
markharness binding set --case-uid <case-uid> --mode <automated|manual> [--reference <text>] [--json] [-d, --dir <path>]
markharness binding list [--json] [-d, --dir <path>]
```

**Purpose**: Declares whether a TestCase is verified by automated or manual means, and where that verification lives. Stored one file per Case at `.markharness/bindings/<case-uid>.yml`.

**An `ExecutionBinding` is not a record of an execution.** It carries no timestamp, result (pass/fail), Case revision, target build, environment, attempt count, or evidence, and **its presence must never be read as "executed" or "passed"** (ADR 0025 §1 and §2). Detailed execution evidence is outside markharness's responsibility; it belongs to whatever `reference` points at (the test code, or a separate tool).

**Options**

| Option | Description |
| --- | --- |
| `--case-uid <case-uid>` | (required) The TestCase's Case UID, never its display id (ADR 0013 — so renaming a display id cannot break the record) |
| `--mode <value>` | (required) `automated` or `manual` |
| `--reference <text>` | Free-text pointer to the verification itself (a test file path, a URL). markharness never interprets it |
| `-d, --dir <path>` | Target project directory. Defaults to the project root (auto-detected upward from cwd) |
| `--json` | Emit JSON instead of human-readable text |

**Behavior**

- `set` **replaces** any existing binding for the same Case UID. A binding is a current declaration, not an append-only log, so there is one file per Case and it is overwritten.
- The stored form is exactly `schema_version: 1`, `record_kind: execution_binding`, `case_uid`, `mode`, and an optional `reference`. `schema_version` is fixed at `1` for every record kind and is never bumped (ADR 0026 §7).
- A binding file carrying unknown fields is **rejected when read**. Hand-adding execution-fact fields such as `result`, `executed_at`, `build`, or `environment` is never silently ignored (ADR 0025 §2).
- A file whose `case_uid` disagrees with the Case UID its name encodes is **also rejected when read**. Once the one-file-per-Case invariant breaks, one Case's verification declaration would be read as another's.
- A binding whose `schema_version` is not `1` is rejected too. The version is fixed, so any other value means a hand-edit or a record kind this reader does not know.
- A Case UID is the sole component of the file's path, so an empty value, `.`, `..`, a leading dot, a path separator (`/`, `\`), or a drive specifier is refused **before any file is created** — the same rule `generate` applies to `id:`.
- Writes go through the atomic replacement path in `src/fs_safety.rs`.

**Exit codes**

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 2 | The Case UID is unusable as a file name, or a stored binding is malformed (e.g. carries unknown fields) |
| 3 | Filesystem error |

**Example**

```console
$ markharness binding set --case-uid 01ARZ3NDEKTSV4RRFFQ69G5FAV --mode automated --reference tests/login.spec.ts
bound 01ARZ3NDEKTSV4RRFFQ69G5FAV as automated in .markharness/bindings/01ARZ3NDEKTSV4RRFFQ69G5FAV.yml
```

`.markharness/bindings/01ARZ3NDEKTSV4RRFFQ69G5FAV.yml`:

```yaml
schema_version: 1
record_kind: execution_binding
case_uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
mode: automated
reference: tests/login.spec.ts
```

**Use case mapping**: `ExecutionBinding` in [the markharness v2 design](./design/markharness-v2-design.md) §5.2. Per-release verification scope lives in `ReleaseScope` (a separate command).

---

### 1.13 `markharness impact` — Change Impact and the alignment check (ADR 0019, design §5.3 and §6.1)

```text
markharness impact --base <git-ref> --head <git-ref> [--format json] [--fail-on-findings] [-d, --dir <path>]
```

**Purpose**: For every Requirement the `base..head` range changed, reports the related Features and TestCases and whether a human confirmed that the two still correspond.

**`--base` is required.** Inferring it from the local branch layout would make the same range produce different results on different machines, breaking reproducibility (design principle P3, AC37). In CI, pass something explicit such as `origin/main`.

**How a confirmation is recorded: the `Spec-Reviewed` commit trailer**

```text
Spec-Reviewed: requirement=<requirement-id> case=<case-id> reason=no-change-required
```

- Written at the end of the commit body, **starting at column 0**, **one line per pair**. An indented line is not counted: a fenced code block or a quotation explaining the syntax must not be mistaken for a declaration (the same position git's own trailer parsing takes). Trailing whitespace is ignored. Several confirmations are several lines; a single comma-separated line is not used, because it could not express one pair being invalidated while another still stands.
- Both `requirement=` and `case=` are **required**. A one-sided trailer, or one with no target at all, is not counted: when a commit touches several Requirements or TestCases, there would be no way to tell which pair was confirmed (AC12, AC16).
- The identifiers are **display ids**, resolved to UIDs against the Knowledge as it stood at the commit carrying the trailer. One that cannot be resolved is not counted and appears in `rejected_trailers` with the reason.
- `reason` may be omitted (defaults to `no-change-required`). An unrecognized value is not counted.
- The check scans the **whole body** of each commit in `git log base..head`. A squash merge embeds the original trailers partway through the merge commit's message, so a "final lines only" reader would miss them.

**The three-valued verdict** (design §5.3)

| status | Meaning |
| --- | --- |
| `confirmed` | A still-valid `Spec-Reviewed` exists for that exact pair |
| `followed_up` | Both sides changed in the range, but nothing records a confirmation. That the TestCase moved is evidence of work, not of a human judging the two to still agree |
| `unconfirmed` | The spec side changed, and there is neither a matching TestCase change nor a confirmation |

**When a confirmation lapses**: if a later commit in the same range changes the effective content of either side of the pair — the Case revision for a TestCase, the `requirement.yml` or `.sdoc` blob for a Requirement — that pair's confirmation is void (AC14, AC29). A confirmation is never reused for another pair, nor extended to a case added later (AC30, AC31).

**Detecting a spec-side change**: for `source: native`, the base/head diff of `requirement.yml` itself; for `source: external`, the base/head diff of the `.sdoc` blob the locator names (ADR 0023). A fixed reference that does not match head's blob OID is reported **separately** under `stale_pins` and never as a spec change (AC10c). A repin does not cancel detection (AC18, AC19).

**When history is unavailable**: if `base..head` cannot be walked — a shallow clone, an unreachable ref — the command exits `2` with a diagnostic. Missing history is never reported as "confirmed" or "unchanged" (AC17).

**Exit codes**

| Code | Meaning |
| --- | --- |
| 0 | Success (regardless of findings; with `--fail-on-findings`, no findings) |
| 2 | Findings present (only with `--fail-on-findings`), or unavailable history / invalid input |
| 3 | Filesystem error |

`--fail-on-findings` is off by default: whether one unconfirmed pair should fail CI is the team's policy, not this tool's. Findings include any pair that is not `confirmed`, plus `stale_pins` and `rejected_trailers`.

**Output**: carries `schema_version: 1`, `record_kind: change_impact`, `rule_version`, and the fully resolved commit ids (`base_commit`/`head_commit`). The same input at the same `rule_version` reproduces the same verdict (AC06, AC37).

---

### 1.14 `markharness release scope` / `markharness coverage` — Release selection lists and Release Coverage (ADR 0024, design §6.2)

```text
markharness release scope set --release <release-id> --case-uid <case-uid> [--case-uid ...] [-d, --dir <path>]
markharness release scope show --release <release-id> [--at <ref>] [--format json] [-d, --dir <path>]
markharness coverage --requirements <ids-or-all> [--release <release-id>] [--at <ref>] [--format json] [-d, --dir <path>]
```

**Purpose**: Records what a release chose to verify, and lists — for a chosen set of Requirements — whether a means of verification exists and whether anything looks left out of the selection.

**What `ReleaseScope` does not carry** (ADR 0024 §2): selection timestamp, chooser, approval state, pass/fail, execution result, target build, environment, or a structured reason. The Git history of the file records how the selection came about. **Being in a selection is a declaration that it was chosen — never that it ran, and never that it passed** (ADR 0024 §5). The output never conflates "selected" with "executed".

**Storage**: `.markharness/releases/<release-id>.yml`. Keeping it under Git is what makes `--at <ref>` reproduce a past point in time (design principle P3). The `release-id` becomes the sole component of that path, so only **ASCII lowercase letters, digits, hyphens, and dots** are allowed; an empty value, `.`, `..`, a leading dot, a path separator, or an uppercase letter is refused **before any file is created**. Ordinary tag names such as `v1.2.0` or `2026-08-release` pass.

**Everything `coverage` reads comes from `--at`**: the Knowledge, the selection list, and the bindings are all read from that ref's commit, so uncommitted changes are not reflected (including under the default `--at HEAD`). Reading any one of them from the working tree instead would let a query about a past ref change with today's work, breaking reproducibility (design principle P3, AC11).

**`--requirements` is what bounds the answer**: missed-selection candidates (`unselected_case_uids`) are the TestCases reachable from the requested Requirements that the selection does not include. The bound is deliberately not derived from the selection itself — doing so would hide a Requirement that was left out whole, which is the most dangerous omission. Pass `--requirements all` to look at everything.

**Output**

| Field | Meaning |
| --- | --- |
| `requirements[].cases[].binding_mode` / `binding_reference` | That TestCase's verification means (section 1.12). **Its presence does not mean anything ran** |
| `requirements[].cases[].selected` | Only with `--release`: whether the selection includes it |
| `gaps[].kind = requirement_has_no_feature` | No Feature contributes to this Requirement (AC08) |
| `gaps[].kind = feature_has_no_case` | A Feature contributes, but nothing underneath it produces a TestCase (AC21) |
| `release.selected_case_uids` | Selected, and present in the Knowledge at that ref |
| `release.unselected_case_uids` | In scope of the requested Requirements but not selected (missed-selection candidates, AC25) |
| `release.absent_case_uids` | Selected, but absent from the Knowledge at that ref (AC26). The selection is never rewritten automatically |

**A release with no recorded selection**: if the release passed to `--release` has no scope on record, the `release` field is omitted and only the registered state is returned. A selection that was never recorded is never inferred (ADR 0024 §4).

**Exit codes**

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 2 | The `release-id` is unusable as a file name, a requested Requirement does not exist, or a stored record is malformed |
| 3 | Filesystem error |

**Example**

```console
$ markharness release scope set --release v1.2.0 --case-uid 01ARZ... --case-uid 01BRZ...
recorded 2 case(s) for v1.2.0 in .markharness/releases/v1.2.0.yml

$ markharness coverage --requirements all --release v1.2.0 --at v1.2.0
```

**Use case mapping**: design §1's question 3, "which tests were in the verification scope of the last release". Only a release that recorded a selection can be answered down to *what was chosen*; for one that did not, the answer reaches the registered state at that point and stops.

---

### 1.15 `markharness changes annotate` — Post-hoc entry of change_type / related_events (§3.5)

```text
markharness changes annotate <event_id> [--type <spec-change|bug-fix|refactor|other>] [--related <event_id>]... [-d, --dir <path>]
```

**Purpose**: Lets a human set, after the fact, the `change_type` and `related_events` of a `ChangeEvent` computed by `changes compute` (section 1.9). Since it searches across all `*.yaml` files under `.markharness/changes/` by `event_id`, the caller does not need to know in advance which milestone interval's file contains it.

**Behavior**

- `--type` and `--related` are independent, additive fields; either may be specified alone (it is an error to omit both — at least one must be specified).
- Specifying `--type` rewrites the `change_type` of the first file with a matching `event_id`. Other `ChangeEvent`s in the same file are left unchanged.
- `--related <event_id>` may be specified multiple times, and each is appended to the target event's `related_events` (existing values are kept; this appends rather than overwrites).
- If `--related` is given, it is verified, before any writing, that the target `event_id` and all `event_id`s given via `--related` exist somewhere in `.markharness/changes/*.yaml`. If any is not found, the write does not happen — even if `--type` was also specified — and it errors with exit code `3` (`--type` and `--related` are independent additive fields, but the command as a whole either writes everything or writes nothing).
- When only `--type` is specified (i.e., `--related` is not given), it errors with exit code `3` if the target `event_id` is not found.

**Example**

```console
$ markharness changes annotate player-jump--m1--m2 --type spec-change
set change_type on player-jump--m1--m2

$ markharness changes annotate player-jump--m2--m3 --related player-jump--m1--m2
set related_events on player-jump--m2--m3
```

**Use case mapping**: Part of UC5 "automatically compute ChangeEvent" (§3.5; corresponds to the design intent that both `change_type` and `related_events` are entered by a human after the fact, rather than computed).

---

### 1.16 `markharness changes lineage` — Lineage audit via merge-base ancestor search (§3.2, secondary feature)

```text
markharness changes lineage --commit <merge-commit-sha> [--json] [-d, --dir <path>]
```

**Purpose**: For a given merge commit, compares the tree SHA of its two parents (P1, P2) and the merge base (B) via `git merge-base`, and for each Feature id, determines and outputs the §3.2 case classification (`linear` / `true_divergence` / `single_parent`) — an audit-only command. `changes compute` (section 1.9) internally invokes the same determination logic as this command for every two-parent merge commit present within the `from-milestone..to-milestone` interval, and reflects the result in `true_divergences`. To manually audit/verify an individual merge commit by itself, run this command independently. This command itself does not write to `.markharness/changes/*.yaml` (it is a read-only audit command). In repositories operated with squash merges, rebases, or fast-forward merges, the target two-parent merge commits simply do not exist on the commit graph in the first place, so there is nothing this command can audit (paper §3.4 Table 2).

**Behavior**

- If `<merge-commit-sha>` does not have two parents (i.e., is not a merge commit), it errors with exit code `2`.
- The determination result is output as human-readable text (`<feature_id>: <kind>`) or as a JSON array with `--json`.

**Example**

```console
$ markharness changes lineage --commit a1b2c3d
player-jump: linear
```

**Use case mapping**: An implementation of the "detailed lineage tool (for auditing, secondary feature)" in §3.2. Not included among the evaluation targets of RQ1 (the primary lineage; see the note in §1.3).

---

### 1.17 `markharness validate` — Structural validation of .markharness/knowledge/, .markharness/axes/, .markharness/bindings/ (§3.5/§3.6)

```text
markharness validate [--json] [-d, --dir <path>]
```

**Purpose**: Performs JSON Schema validation of all YAML under `.markharness/knowledge/` (`requirement.yml` / `feature.yml` / `behavior.yml` / `condition.yml` / `expected/*.yml`), `.markharness/axes/*.yml`, and `.markharness/executions/<milestone>/results.yml`, against the corresponding `.markharness/schema/*.schema.json` (a default set placed by `markharness init`; section 1.1). In addition, it validates cross-reference constraints that cannot be expressed by JSON Schema alone: whether `axis` tags are registered in `.markharness/axes/*.yml`, and whether `feature.yml`'s `forked_from` points to an actually existing Feature id.

**Binding validation**: `.markharness/bindings/*.yml` is checked for being readable as an `ExecutionBinding` (section 1.12). A binding carrying execution-fact fields such as `result`, `executed_at`, `build`, or `environment` is rejected as having unknown fields (ADR 0025 §2).

**Additional validation in UID mode (ADR 0013, design doc §13 Phase 5)**: For a project whose `.markharness/config.toml` `[identity]` marker is `mode = "uid"` (written by `identity migrate`, section 1.21, once every kind has finished migrating), any Requirement/Feature/Behavior/Condition/ExpectedResult that lacks a `uid:` is reported as a validation issue, naming the file and prompting a run of `markharness identity migrate`. This guards against a uid-less element being introduced after cutover (via copy/import/hand-editing); it does not apply to a project that hasn't cut over yet (no marker).

**Behavior**

- If there are zero problems, exits with code `0`. In human-readable mode, prints `.markharness/knowledge/ and .markharness/axes/ are valid`; with `--json`, prints `{"ok":true}`.
- If there are problems, lists a message for each file and exits with code `1`.

**Example**

```console
$ markharness validate
.markharness/knowledge/controls/player-jump/feature.yml: axis 'not-registered' is not registered under .markharness/axes/
$ echo $?
1
```

**Use case mapping**: An implementation of the §3.5 constraint "restrict, via schema validation, values not defined in `.markharness/axes/*.yml` from being usable in front matter."

---

### 1.18 `markharness --version` / `-V` — Display version

```text
markharness --version
markharness -V
```

**Purpose**: Prints the `version` from `Cargo.toml` (embedded at build time as `CARGO_PKG_VERSION`). `Cargo.toml` is the single source of truth for the version number (per the CLAUDE.md operating rule).

**Example**

```console
$ markharness --version
markharness 0.3.1
```

---

### 1.19 `markharness axes prune` — Detect/delete unused axes

```text
markharness axes prune [--delete] [--json] [-d, --dir <path>]
```

**Purpose**: Detects axes registered under `.markharness/axes/*.yml` that are not referenced by any Requirement/Feature/Behavior's `axis:` list anywhere under `.markharness/knowledge/` (orphaned axes). `condition.yml`/`expected/*.yml` have no `axis` field, so they are not scanned.

**Behavior**

- Report-only by default (`.markharness/axes/*.yml` is never deleted unless `--delete` is given).
- With `--delete`, actually deletes `.markharness/axes/<id>.yml` for every unused axis found. No second confirmation (e.g. an additional `--yes`) is required — passing `--delete` itself is treated as explicit consent, since only orphaned axes with no reference anywhere are ever a candidate, so the risk of losing anything important is low.
- With `--json`, prints `{"axes":[<ids of unused axes>],"deleted":<bool>}`. `deleted` reflects whether `--delete` was given; the `axes` key and structure are the same regardless of `--delete`, so a caller doesn't need separate parsing logic for the two modes.

**Example (report only)**

```console
$ markharness axes prune --dir tmp/todo-sample --json
{"axes":["legacy-ui"],"deleted":false}
```

**Example (delete)**

```console
$ markharness axes prune --delete --dir tmp/todo-sample --json
{"axes":["legacy-ui"],"deleted":true}
$ markharness axes list --dir tmp/todo-sample --json
```

(`legacy-ui` is removed from `.markharness/axes/` and no longer appears in `axes list`)

**Use case mapping**: A companion command to `markharness axes add` (section 1.6). Does not map explicitly to any UC.

---

### 1.20 `markharness import` — Emit a canonical snapshot

```text
markharness import --source <native|junit> [--input <junit.xml>] [--git-ref <ref>] [--bind <artifact-id=version>]... --format json [-d, --dir <path>]
```

`native` normalizes `.markharness/knowledge/` at the selected Git ref into artifacts carrying Feature tree SHAs and derived traces. `junit` normalizes JUnit XML TestCases and PASS/FAIL/SKIP results into evidence, with `--bind` supplying versions under verification. A JUnit `markharness.condition` property creates a stored trace. Output carries `schema_version: 1` and conforms to `.markharness/schema/canonical_snapshot.schema.json`. The command does not modify the input or `.markharness/knowledge/`.

---

### 1.21 `markharness identity migrate` — Bulk-issue uids for every Knowledge element kind (ADR 0013, design doc §12 and §13 Phase 4/5)

```text
markharness identity migrate [--json] [--dry-run] [-d, --dir <path>]
```

**Purpose**: Issues a fresh uid, and records a root `Issued` identity event, for every Requirement/Feature/Behavior/Condition/ExpectedResult under `.markharness/knowledge/` that doesn't have one yet. Idempotent — safe to re-run after copy/import/hand-editing introduces new uid-less elements. Also records TestCase `case_id` → `case_uid` mappings (the migration manifest, `.markharness/identity-migration-manifest.yml`).

Once every one of the five kinds has zero uid-less elements left, writes `schema_version = 1` / `mode = "uid"` into `.markharness/config.toml`'s `[identity]` marker, completing the public cutover to UID mode (design doc §13 Phase 5). Cutover completion is determined by `mode` alone, not `schema_version` (ADR 0018). After cutover, `markharness validate` (section 1.17) starts reporting any newly introduced uid-less element as a validation issue.

**Precondition**: The target directory must already be a Git repository. To record the legacy snapshot identity (the tree SHA of `.markharness/knowledge`) into the migration manifest, this internally performs a `git write-tree`-equivalent operation against a disposable temporary index (the repository's real staging area is never touched).

**Behavior**

- `--dry-run`: writes no lock, staging, identity event, or Knowledge file — only shows the planned UID assignments and the list of files that would change.
- Normal run: processes all five kinds in two passes (detect id/uid conflicts first; if none, record every kind's `Issued` events as one crash-recoverable batch). Reusing the same id across different kinds is allowed; a duplicate id within one kind is rejected as a conflict (exit code `2`).
- Exits with code `2` if a concurrent identity operation is detected.
- Exits with code `3` on a filesystem error.
- `--json`: prints `{"audit_scope":"working_tree","dry_run":bool,"migrated":[{"kind","id","uid"}],"conflicts":[string],"changed_files":[string]}`. `audit_scope` is a machine-readable field (design doc §11) contrasting with `"two_snapshot"` (`changes compute`/`verify`, sections 1.4/1.9) and `"full_history"` (`identity audit`, section 1.23) — it marks `identity migrate` as inspecting only the current working tree.

**Example**

```console
$ markharness identity migrate --dry-run
would migrate requirement 'req-todo' -> uid 01M0M862TX3X878T44WXBCQDQF
would migrate feature 'todo' -> uid 01M0M862TYP26CAAB5RWHKWC2B
would migrate behavior 'todo-add-task' -> uid 01M0M862TYD5B5H95VGXAXYKN3
would migrate condition 'todo-add-task-empty-input' -> uid 01M0M862TYDND4EQJT6A25KAG4
would migrate expected_result 'todo-add-task-empty-input-001' -> uid 01M0M862TYQKXDCTGDYPE8BBWY
would change .markharness/knowledge/req-todo/requirement.yml
would change .markharness/identity-events/requirements/01M0M862TX3X878T44WXBCQDQF/01M0M862TYKGXCZV3TECPDQGWS.yml
... (every changed file, across all five kinds, is listed the same way)

$ markharness identity migrate
migrated requirement 'req-todo' -> uid 01M0M8632NP9SY6T1X1NK7Z9XE
migrated feature 'todo' -> uid 01M0M8632N73PB010A2TQQYG84
migrated behavior 'todo-add-task' -> uid 01M0M8632N0KDPK15MAK34TZKC
migrated condition 'todo-add-task-empty-input' -> uid 01M0M8632N94PXJREJEJNMKETY
migrated expected_result 'todo-add-task-empty-input-001' -> uid 01M0M8632NWZAW5VM0HZ2AWNMV

$ markharness identity migrate --json
{"audit_scope":"working_tree","changed_files":[],"conflicts":[],"dry_run":false,"migrated":[]}
```

(The second `--json` run is a no-op response with an empty `migrated`, since every element is already migrated.)

**Use case mapping**: ADR 0013's "Migration" section; design doc §12 (`recorded_at` and crash-recovery during migration) and §13 Phase 4 (migrating all elements) / Phase 5 (public cutover to UID mode).

---

### 1.22 `markharness identity resolve` — Explicitly resolve a branch divergence (ADR 0013, design doc §7)

```text
markharness identity resolve <KIND> <UID> --keep <EVENT_UID> [-d, --dir <path>]
```

`<KIND>` is one of `requirement` / `feature` / `behavior` / `condition` / `expected-result`.

**Purpose**: When one entity has multiple identity events that diverged from the same predecessor (a branch divergence, design doc §7), explicitly picks which one's outcome (id) wins and records a `Resolved` identity event. Divergence can arise when independent identity operations (rename, etc.) on different branches are later merged. It rarely occurs under ordinary single-branch use; this command exists as the recovery path for that merge scenario.

**Behavior**

- On success, prints `resolved divergence for <uid>, keeping <keep>` and exits with code `0`.
- Exits with code `2` if: the target entity has no unresolved divergence right now; the event uid passed to `--keep` is not one of the divergent heads (the error message lists the candidates); or a concurrent identity operation is detected.
- Exits with code `3` on a filesystem error.

**Use case mapping**: ADR 0013 design doc §7 (resolving branch divergence).

---

### 1.23 `markharness identity audit` — Full commit-history identity audit (IdentityAuditor, ADR 0013, design doc §11)

```text
markharness identity audit [--json] [--ref <ref>] [-d, --dir <path>]
```

**Purpose**: Walks the entire first-parent history of `<ref>` (default `HEAD`) and verifies two properties `.markharness/identity-events/` is supposed to hold: (1) identity events are append-only (an event file committed once must never disappear or change content in a later commit), and (2) the event set at every commit still replays without a causal-chain contradiction. `changes compute`, `verify`, and `identity migrate` (sections 1.4/1.9/1.26) are all lightweight comparisons that look at no more than two `.markharness` snapshots; `identity audit` is the one command that walks the entire Git commit history, and is kept as its own separate top-level command for that reason (design doc §11).

The walk is limited to the first-parent history of the currently checked-out branch (equivalent to `git log --first-parent`). Changes that only ever existed on a not-yet-merged side branch are not this project's published history, and are excluded.

**Behavior**

- Exits with code `0` if there are zero violations. In human-readable mode, prints `no identity-history violations found (<N> commits scanned)`; with `--json`, `violations` is an empty array.
- Exits with code `1` if there are violations, printing one line per violation.
- `--json`: prints `{"audit_scope":"full_history","commits_scanned":<N>,"violations":[...]}`. Each element of `violations` is tagged by a `type` field (`event_disappeared` / `event_content_changed` / `causal_chain_contradiction`).
- An underlying infrastructure failure (e.g. a Git object read failure) makes the command itself exit with an error, rather than being misreported as a contradiction in the audited history.

**Example**

```console
$ markharness identity audit
no identity-history violations found (3 commits scanned)

$ markharness identity audit --json
{"audit_scope":"full_history","commits_scanned":3,"violations":[]}
```

When history has been tampered with, e.g. an identity event file was later deleted:

```console
$ markharness identity audit
event disappeared: feature '01M0M8632N73PB010A2TQQYG84' event '01M0M8632N666JSS1BXY1NCH30' is missing as of commit a021aed5d2159dbe718b111e9aaf679130ee823b (.markharness/identity-events/features/01M0M8632N73PB010A2TQQYG84/01M0M8632N666JSS1BXY1NCH30.yml)
causal chain contradiction: feature '01M0M8632N73PB010A2TQQYG84' at commit a021aed5d2159dbe718b111e9aaf679130ee823b: NoRootEvent
$ echo $?
1
```

**Use case mapping**: ADR 0013's verification rules ("only `IdentityAuditor` walks the full Git commit history, verifying repository-wide event append-only-ness and any deletion/past alteration outside the two selected snapshots"), design doc §11.

---

### 1.24 `markharness identity sync` — Re-derive a Knowledge file's id:/uid: from its identity event log

```text
markharness identity sync <KIND> <UID> [-d, --dir <path>]
```

**Purpose**: Replays `<UID>`'s identity events to their current state and writes the resulting `id` back into whatever Knowledge file currently carries it — filling in a missing `uid:` or correcting a stale one. Records no new identity event; it only re-derives file state from the already-durable event log. This is the same "resync Knowledge file via roll-forward" side effect every other identity operation (including `identity migrate`) already performs internally, exposed on its own.

**Precondition**: Meant to cover cases where no other operation's side effect performed the sync — most notably, restoring or re-creating a Knowledge file from Git history. A rename through `knowledge reconcile` (section 1.2) selects its target by `uid`, so it cannot serve as a resync for a still-uid-less file; `identity sync` supports all five kinds and works regardless of whether the file currently has a `uid:`.

**Behavior**

- On success, prints `synced <uid>` and exits with code `0`.
- Exits with code `2` if: the target entity has no `uid`, or a concurrent identity operation is detected.
- Exits with code `3` on a filesystem error.

**Example**

```console
$ markharness identity sync feature 01M0MJQ5C4CJ3HHVG7PBYAQEBR
synced 01M0MJQ5C4CJ3HHVG7PBYAQEBR
$ cat .markharness/knowledge/req-todo/todo/feature.yml
id: todo
requirement: req-todo
label: todo
axis: []
uid: 01M0MJQ5C4CJ3HHVG7PBYAQEBR
```

**Use case mapping**: A general cleanup for cases such as restoring a Knowledge file from Git history.

---

### 1.25 `markharness traceability` — Read Requirement/Feature/Behavior/Scenario/TestCase relations (ADR 0032/0033, design doc cli-read-model-design.md §5)

```text
markharness traceability [--at <git-ref>] [--format json] [-d, --dir <path>]
```

**Purpose**: Reads Requirement/Feature/Behavior/Scenario/TestCase relations, read-only, from Knowledge and generated TestCases. Lets external tools such as `markharness-view` take this output as their only input, without reading Knowledge or `.markharness/` directly (ADR 0032). Like `impact` and `coverage`, it never goes through `CommandOutcome`/`Presenter`; its own module's struct is serialized directly to JSON.

**`--at` is optional. Omitting it reads the working tree** — the uncommitted, current content — the same way `generate`/`verify` already do (ADR 0033). Giving `--at <ref>` reads that Git ref's committed content instead. Unlike `impact`/`coverage`, `traceability` has no two-point-comparison or release-auditing requirement, so it never demands a commit first. It never writes any artifact, unlike `generate`.

**Output**: `schema_version: 1`, `record_kind: traceability`, `at` (the fixed string `"working-tree"` when `--at` is omitted, otherwise the given string as-is; ADR 0033), plus `requirements` (`requirement_id`, `requirement_uid`, `source`, `source_locator`, `source_key` — the latter two are present only when `source` is `"external"`, and point to the underlying StrictDoc (or similar) content; always `null` for `"native"`), `features` (`feature_id`, `feature_uid`), `behaviors` (`behavior_id`, `feature_id`; `behavior_uid` is always `null` today — the current Knowledge-reading path has no way to obtain it), `scenarios` (`scenario_id`, `scenario_uid`, `behavior_id`), `test_cases` (`case_id`, `case_uid`, `case_revision`, `relative_path`, `scenario_id`), and `relations` (`from_uid`, `to_uid`, `kind`, where `kind` is one of `contributes_to` — Feature or Scenario to Requirement — or `generated_from` — TestCase to Scenario). An element with no UID yet (`identity migrate` not run) still appears as a Node, but never in `relations`.

**Behavior**

- Every Feature and Requirement in Knowledge is reported regardless of whether it has a generated TestCase underneath (the same reasoning as coverage's AC21: a Feature with nothing to verify it is still made visible).
- Behaviors, Scenarios, and TestCases are derived from every generated TestCase (`generate` rejects a Scenario with empty phases, so an existing Scenario always corresponds to exactly one TestCase).
- A Requirement whose `source` (native/external) disagrees with its `source_locator`/`source_key` (e.g. `source: native` that still carries `source_locator`) is rejected (exit code 2) — the same constraint `validate` enforces (ADR 0023), checked again here because `traceability` cannot assume `validate` has already run.

**Exit codes**

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 2 | A Knowledge file has a syntax or content error |
| 3 | Filesystem error |

**Example**

```console
$ markharness traceability
{
  "schema_version": 1,
  "record_kind": "traceability",
  "at": "working-tree",
  ...
}
```

To check committed state at a specific point instead, give `--at`:

```console
$ markharness traceability --at HEAD
{
  "schema_version": 1,
  "record_kind": "traceability",
  "at": "HEAD",
  "requirements": [
    { "requirement_id": "controls", "requirement_uid": "01ARZ3NDEKTSV4RRFFQ69G5FAV", "source": "native", "source_locator": null, "source_key": null }
  ],
  "features": [
    { "feature_id": "player-jump", "feature_uid": null }
  ],
  "behaviors": [
    { "behavior_id": "jump", "behavior_uid": null, "feature_id": "player-jump" }
  ],
  "scenarios": [
    { "scenario_id": "ground", "scenario_uid": "01ARZ3NDEKTSV4RRFFQ69G5FB1", "behavior_id": "jump" }
  ],
  "test_cases": [
    { "case_id": "tc-player-jump-jump-ground", "case_uid": "...", "case_revision": "...", "relative_path": "player-jump/jump/ground.yml", "scenario_id": "ground" }
  ],
  "relations": [
    { "from_uid": "01ARZ3NDEKTSV4RRFFQ69G5FB1", "to_uid": "01ARZ3NDEKTSV4RRFFQ69G5FAV", "kind": "contributes_to" }
  ]
}
```

**Use case mapping**: [cli-read-model-design.md](./design/cli-read-model-design.md) §5's `TraceabilityReadModel`, [ADR 0032](./decisions/0032-cli-read-model-seam.md) / [ADR 0033](./decisions/0033-traceability-defaults-to-working-tree.md).

---

## 2. Unimplemented (Planned) Commands

The following are commands planned for future implementation, based on the use case diagram and use case descriptions in `docs/product-operation.md`. The command names and options are tentative proposals and may change at implementation time.

| #   | Use case                                | Planned command (tentative)                                           | Actor                    | Overview                                                                                     |
| --- | ---------------------------------------- | ----------------------------------------------------------------------- | ------------------------ | ---------------------------------------------------------------------------------------------- |
| UC4 | Tag a milestone                          | No dedicated command (`git tag <milestone>` is used directly)          | Release Manager          | This is the release-timing decision itself, and remains a point of human judgment (Figure 3). |

These are currently not yet started; implementation ordering is managed separately via a checklist (`/plan-checklist`).

---

## 3. Verification / Testing

Unit tests for the implemented commands can be run with `cargo test` (see the `#[cfg(test)] mod tests` in `src/init.rs` / `src/knowledge.rs` / `src/knowledge_reconcile/` / `src/generate.rs` / `src/verify.rs` / `src/axes.rs` / `src/traceability_index.rs` / `src/git.rs` / `src/id_cache.rs` / `src/changes.rs` / `src/backfill.rs`, as well as `tests/knowledge_reconcile_cli.rs`, which verifies the exit codes and output of `knowledge reconcile`). Because the tests in `git.rs`/`id_cache.rs`/`changes.rs`/`backfill.rs` actually run `git init`/`commit`/`tag` in a temporary directory, the `git` command is required in the test environment. Following the Pre-PR checklist (`CONTRIBUTING.md`), run the following before committing:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo audit
```
