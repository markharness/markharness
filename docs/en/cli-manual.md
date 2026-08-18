# markharness CLI Manual

**Status**: Implemented (implemented commands are in Chapter 1) / Draft (tentative proposals for unimplemented commands are in Chapter 2)
**Related documents**: [product-operation.md](./product-operation.md) (use case mapping), [testcase-generation-design.md](./design/testcase-generation-design.md) (generation rules for `generate`), [knowledge-apply-cli-spec.md](./design/knowledge-apply-cli-spec.md) (detailed design of `knowledge validate`/`apply`)

**Purpose**: This document summarizes how to use the `markharness` CLI, divided into **implemented commands** and **unimplemented (planned) commands**. The mapping to use cases (UC1–UC8) is based on the "3. Use Case Descriptions" table in `docs/product-operation.md`. For the concrete generation rules of the implemented commands, see `docs/design/testcase-generation-design.md` (however, the current implementation of `generate`/`verify` has since been overhauled into the 4-tier `feature → behavior → condition → expected` model after that document was written; treat sections 1.5/1.6 of this manual as authoritative for the details). For the detailed design of `knowledge validate`/`apply` (the non-interactive, TTY-independent versions, sections 1.3/1.4), treat `docs/design/knowledge-apply-cli-spec.md` as authoritative.

---

## 1. Implemented Commands

### 1.1 `markharness init` — Project initialization (prerequisite for UC1–UC8)

```text
markharness init
```

**Purpose**: Of the physical directory structure that underpins UC1–UC8 (paper §3.5, lines 244–273), this creates the six directories that need to be created in the target repository, so that subsequent commands can operate.

| Directory     | Corresponding UC                                                                        |
| ------------- | ----------------------------------------------------------------------------------------- |
| `knowledge/`  | UC1 (describe knowledge) / UC1b (manually describe `forked_from`)                        |
| `axes/`       | UC1 (registry of the cross-cutting Axis viewpoint, §3.1)                                  |
| `generated/`  | UC2 (deterministically generate TestCase) / UC3 (review and merge generated artifacts)    |
| `executions/` | UC4 (tag a milestone; destination for recording execution results)                        |
| `changes/`    | UC5 (automatically compute ChangeEvent) / UC6 (run backfill asynchronously)               |
| `schema/`     | UC7 (discard/rebuild the id cache; definitions of format/normalization rules)              |

UC8 (importing from existing tools) has no dedicated directory, since it is assumed the converted results are written into `knowledge/`, and is therefore out of scope.

**Behavior**

- For each directory: if it does not exist, create it; if it already exists, do nothing (including leaving its contents untouched) — an idempotent operation. Re-running on an already-initialized project does not error; only the missing directories are additionally created.
- On success, prints the created paths to standard output.

**Example**

```console
$ markharness init
initialized knowledge/, axes/, generated/, executions/, changes/, schema/ under /path/to/project

$ markharness init
initialized knowledge/, axes/, generated/, executions/, changes/, schema/ under /path/to/project
```

**Use case mapping**: Does not explicitly correspond to any single UC, but is a helper command that satisfies the prerequisite for starting all of UC1–UC8.

---

### 1.2 `markharness knowledge add` — Interactive description of knowledge (UC1: describe knowledge, in the order Requirement → Feature → Behavior → Condition → ExpectedResult)

```text
markharness knowledge add [--dir <path>]
```

**Purpose**: Lets a Test Designer describe the five tiers `Requirement` → `Feature` → `Behavior` → `Condition` → `ExpectedResult` interactively (sequential prompts on standard input), creating `.yml` files under `knowledge/`. `Requirement` is the requirement unit that is the parent of a Feature, and `Feature` references its parent via its own `requirement:` field. `Behavior` is a required intermediate tier expressing "how the feature behaves," and becomes the source of the `steps` in the TestCase that `generate` assembles.

**Options**

| Option              | Description                                                                                                    |
| ------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `-d, --dir <path>` | Specifies the target project directory (the parent of `knowledge/`). If omitted, targets the current directory. |

**Example (targeting a directory other than the current one)**

```console
$ markharness knowledge add --dir tmp/todo-sample
Requirement name (e.g. task-management): task-management
Requirement axis (comma separated, e.g. ui, validation): workflow
Feature name (e.g. add-todo): add-todo
Axis (comma separated, e.g. ui, validation): ui, validation
Behavior name (e.g. add-task): add-task
Behavior axis (comma separated, e.g. ui, validation): ui
Behavior description (e.g. User adds a new task to the list.): User adds a new task to the list.
Condition name (e.g. empty-title): empty-title
Scenario (e.g. Submit the todo form with an empty title): Submit the todo form with an empty title
Expected result (e.g. shows a validation error): shows a validation error
```

→ Files are created under `tmp/todo-sample/knowledge/task-management/add-todo/...`.

**Actor**: Test Designer (`docs/product-operation.md` UC1)

**Flow**

1. `Requirement name (e.g. task-management):` — enter the Requirement's slug (lowercase alphanumerics and hyphens only), or a Japanese-language label
   - If one or more existing Requirements exist under `knowledge/`, a numbered list in the form `N) id` is displayed before the prompt. Entering a number selects the corresponding Requirement, and typing an existing id directly also works for reuse. If there are zero candidates, no list is shown.
   - If an existing `knowledge/<requirement_id>/requirement.yml` exists, it is reused and the flow skips to the next prompt.
   - Only for a new Requirement, the axis is entered as a comma-separated list at `Requirement axis (comma separated, e.g. ui, validation):`, and `requirement.yml` is newly created.
2. `Feature name (e.g. add-todo):` — enter the Feature's slug (lowercase alphanumerics and hyphens only), or a Japanese-language label
   - If one or more existing Features exist under the selected Requirement, a numbered list is shown the same way, and either number selection or direct entry works.
   - If an existing `knowledge/<requirement_id>/<feature_id>/feature.yml` exists, it is reused and the flow skips to the next prompt.
   - Only for a new Feature, the axis is entered as a comma-separated list at `Axis (comma separated, e.g. ui, validation):`, and `feature.yml` is newly created (the `requirement:` field automatically records the id of the selected/created Requirement).
3. `Behavior name (e.g. add-task):` — enter the Behavior's slug, or a Japanese-language label
   - If one or more existing Behaviors exist under the selected Feature, a numbered list is shown the same way, and either number selection or direct entry works.
   - If an existing `knowledge/<requirement_id>/<feature_id>/<behavior_id>/behavior.yml` exists, it is reused and the flow skips to the next prompt.
   - Only for a new Behavior, `Behavior axis (...)` and `Behavior description (...)` are entered, and `behavior.yml` is newly created.
4. `Condition name (e.g. empty-title):` — enter the Condition's slug, or a Japanese-language label
   - If one or more existing Conditions exist under the selected Behavior, a numbered list is shown the same way, and either number selection or direct entry works.
   - If the newly created Condition id begins with `{behavior_id}-` (i.e., the Behavior id was accidentally duplicated in it), that prefix is automatically stripped before creation, and the fact is reported (e.g., entering the Condition id `add-task-empty-title` under Behavior `add-task` creates it as `empty-title`). However, if a directory with the id exactly as entered already exists, it is not stripped and is reused as-is (to avoid breaking data that was previously created manually with a duplicated name).
   - If an existing `knowledge/<requirement_id>/<feature_id>/<behavior_id>/<condition_id>/condition.yml` exists (judged using the id after stripping), it is reused and the flow skips to the next prompt.
   - Only for a new Condition, the condition's description is entered at `Scenario (e.g. Submit the todo form with an empty title):`, and `condition.yml` is newly created.
5. `Expected result (e.g. shows a validation error):` — enter the expected-result text and create `expected/NNN.yml` (3-digit sequence number, existing file count + 1).

**On the prompt wording**: Each prompt internally determines the `id` of a Feature/Behavior/Condition (the directory name and the `id` field in the YAML), but to avoid the human operator getting confused by the abstract notion of "id," it is presented with easy-to-understand English phrasing and examples such as `Feature name` / `Behavior name` / `Condition name`. The internal data model (the `id` field, notification messages, variable names in the code) is unchanged.

**Japanese-label input (Feature name / Behavior name / Condition name)**

For each name prompt, if the input contains non-ASCII characters (e.g., a Japanese-language label), the following romanization flow is used instead of direct id entry. This does not apply to ExpectedResult's id, since it is an automatic sequence number.

1. Determine whether the input string contains non-ASCII characters.
2. If it does, convert the input to romaji using the [`kakasi`](https://crates.io/crates/kakasi) crate, then present a single id candidate after normalization (lowercasing, hyphenation of whitespace, collapsing consecutive hyphens, stripping leading/trailing hyphens, removing disallowed symbols).
3. At the prompt `id候補: <candidate> (Enterで採用、編集する場合は入力):` ("id candidate: <candidate> (press Enter to accept, or type to edit):"), sending only an empty input (Enter) accepts that candidate as-is as the id. Typing any string instead runs that input through the same normalization rules and adopts it as the id (free editing).
4. If the normalized id collides with an existing candidate in the numbered list, a warning is shown and id entry for that tier must be redone (there is no automatic reuse of an existing id; intentional reuse of an existing id is done via number selection).
5. For a Requirement/Feature/Behavior/Condition newly created via Japanese-label input, the `label` field stores the entered Japanese string as-is. For direct ASCII input, or when an existing entry is reused via number selection, the input value itself (i.e., the same string as the id) is stored as `label`. ExpectedResult has no `label` field, since its id is an automatic sequence number and is not something the user names (the entered description text is stored directly in `description`).

**Input validation**

- The id (Feature id / Behavior id / Condition id) allows only lowercase alphanumerics and hyphens. Invalid input prompts for re-entry.
- When a candidate list is displayed, entering an integer between 1 and the number of candidates (inclusive) selects the corresponding candidate. An out-of-range integer or a non-numeric value is treated as normal id input (or as a Japanese-language label if it contains non-ASCII characters).
- For all prompts, empty input (empty after trimming) prompts for re-entry. However, empty input in response to the id-candidate prompt after Japanese-label conversion means "accept the candidate as-is" and is not subject to re-entry.

**Generated files** (example: `task-management` / `add-todo` / `add-task` / `empty-title` / first entry)

```
knowledge/task-management/requirement.yml
knowledge/task-management/add-todo/feature.yml
knowledge/task-management/add-todo/add-task/behavior.yml
knowledge/task-management/add-todo/add-task/empty-title/condition.yml
knowledge/task-management/add-todo/add-task/empty-title/expected/001.yml
```

`requirement.yml`:

```yaml
id: task-management
label: task-management
axis: [workflow]
```

`feature.yml`:

```yaml
id: add-todo
requirement: task-management
label: add-todo
axis: [ui, validation]
```

`behavior.yml`:

```yaml
id: add-task
feature: add-todo
label: add-task
axis: [ui]
description: |
  User adds a new task to the list.
```

`condition.yml`:

```yaml
id: empty-title
behavior: add-task
label: empty-title
description: |
  Submit the todo form with an empty title
```

`expected/001.yml` (id is `{condition_id}-{3-digit sequence}`):

```yaml
id: empty-title-001
condition: empty-title
description: |
  shows a validation error
```

**Example (first session)**

```console
$ markharness knowledge add
Requirement name (e.g. task-management): task-management
Requirement axis (comma separated, e.g. ui, validation): workflow
Feature name (e.g. add-todo): add-todo
Axis (comma separated, e.g. ui, validation): ui, validation
Behavior name (e.g. add-task): add-task
Behavior axis (comma separated, e.g. ui, validation): ui
Behavior description (e.g. User adds a new task to the list.): User adds a new task to the list.
Condition name (e.g. empty-title): empty-title
Scenario (e.g. Submit the todo form with an empty title): Submit the todo form with an empty title
Expected result (e.g. shows a validation error): shows a validation error
```

**Example (adding a second ExpectedResult to an existing Requirement/Feature/Behavior/Condition, via number selection)**

```console
$ markharness knowledge add
Requirement name (e.g. task-management):
  1) task-management
1
Reusing existing Requirement 'task-management'.
Feature name (e.g. add-todo):
  1) add-todo
1
Reusing existing Feature 'add-todo'.
Behavior name (e.g. add-task):
  1) add-task
1
Reusing existing Behavior 'add-task'.
Condition name (e.g. empty-title):
  1) empty-title
1
Reusing existing Condition 'empty-title'.
Expected result (e.g. shows a validation error): highlights the title field in red
```

→ `knowledge/task-management/add-todo/add-task/empty-title/expected/002.yml` is created. Typing `task-management` / `add-todo` / `add-task` / `empty-title` directly instead of the numbers produces the same result.

**Example (automatic stripping of a duplicated Condition id prefix)**

```console
$ markharness knowledge add
Requirement name (e.g. task-management): task-management
Reusing existing Requirement 'task-management'.
Feature name (e.g. add-todo): add-todo
Reusing existing Feature 'add-todo'.
Behavior name (e.g. add-task): add-task
Reusing existing Behavior 'add-task'.
Condition name (e.g. empty-title):
  1) empty-title
add-task-max-length
Stripping the prefix 'add-task' (duplicating the Behavior id) from Condition id 'add-task-max-length'; creating as 'max-length'.
Scenario (e.g. Submit the todo form with an empty title): Submit the todo form with a title longer than 200 characters
Expected result (e.g. shows a validation error): shows a length validation error
```

→ `knowledge/task-management/add-todo/add-task/max-length/condition.yml` and `knowledge/task-management/add-todo/add-task/max-length/expected/001.yml` are created (the `add-task-max-length/` directory is not created).

**Use case mapping**: Supports UC1 "describe knowledge" (manual description, `docs/product-operation.md` line 103) via an interactive form.

---

### 1.3 `markharness knowledge validate` — Validation of draft YAML (UC1: describe knowledge, non-interactive, TTY-independent)

```text
markharness knowledge validate <draft-file> [--json] [-d, --dir <path>]
markharness knowledge validate --batch <dir> [--json] [-d, --dir <path>]
```

**Purpose**: Without depending on the sequential TTY prompts assumed by `knowledge add` (section 1.2), this validates the schema and consistency of one Requirement→Feature→Behavior→Condition→ExpectedResult chain given as a single draft YAML file. **It has no side effects and performs no file writes whatsoever.** It is intended for non-interactive invocation by AI agents such as Claude Code, and for use from a future GUI implementation. `docs/design/knowledge-apply-cli-spec.md` is authoritative for the detailed design intent and the full list of validation rules.

**Options**

| Option              | Description                                                                                                  |
| ------------------- | ------------------------------------------------------------------------------------------------------------- |
| `<draft-file>`     | Path to the draft YAML file. Mutually exclusive with `--batch` (exactly one of the two is required)          |
| `--batch <dir>`    | Treats every `*.yml` directly under `<dir>` as a draft file and validates them cumulatively in ascending file-name order. See "Batch mode" below |
| `-d, --dir <path>` | Target project directory (the parent of `knowledge/`). Defaults to the current directory.                    |
| `--json`           | Print errors/results as single-line JSON. If omitted, prints human-readable text.                             |

**Batch mode (`--batch <dir>`)**: Validates multiple drafts the same cumulative way `knowledge apply --batch` (section 1.4) does — in ascending file-name order, a later draft may reuse a Requirement/Feature/Behavior that an **earlier draft in the same batch would newly create**, the same way it could reuse one an earlier draft actually applied. Unlike `apply --batch`, though, one draft's failure does not stop the run: every file in the batch is checked through to the end before results are reported together (this is the point of the command — surfacing every error before anything is written). A failed draft does not contribute to the cumulative state seen by later drafts (they are checked as though it were never in the batch). With `--json`, any failures print `{"ok":false,"failures":[{"file":"...","errors":[...]}, {"file":"...","error":"..."}]}` (`errors` for validation errors, `error` for a parse error). Human-readable mode likewise prints every failing file's errors, prefixed with its file name. `{"ok":true}` when every file is valid. Nothing is ever written to the real project directory (internally, `knowledge/` and `axes/` are copied into a temp directory and validated there). If `<dir>` has no `*.yml` files directly under it (including when it only contains `.yaml`-extension drafts), this is an error: exit code 2, with `{"ok":false,"error":"no *.yml files found in batch directory <dir>"}` (same behavior as section 1.4's "Batch mode").

**Draft YAML format** (a single run validates one chain). A blank template is available via `markharness knowledge scaffold` (section 1.21). See `docs/knowledge_draft.schema.json` for a reference schema meant for IDE autocompletion (a static reference file not used for actual validation — the table below and `docs/design/knowledge-apply-cli-spec.md` are authoritative for `knowledge validate`/`apply`'s own validation rules).

```yaml
requirement:
  id: controls # required. ASCII slug
  label: controls # optional (omittable when reusing an existing id)
  axis: [gameplay] # required when creating new; omittable when reusing an existing id
  description: null # optional

feature:
  id: player-jump
  label: player-jump
  axis: [gameplay, animation]

behavior:
  id: jump
  label: jump
  axis: [gameplay]
  description: Player presses jump. # description is required only for Behavior (when newly created)

condition:
  id: ground
  label: ground
  description: Jump from the ground and land

expected:
  - description: lands safely
  - description: takes fall damage if height > 3m
```

`axis`/`label`/`description` can be omitted when reusing an existing id (a Requirement/Feature/Behavior/Condition for which a file already exists under `knowledge/`). Omitted fields are excluded from comparison against the existing value; only specified fields are checked against the existing file's values (`conflicting_existing_value` error).

**Validation rules (summary; see spec §5 for details)**

| Error code                   | Meaning                                                                                                                    |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `invalid_slug`               | The id contains characters other than lowercase alphanumerics and hyphens                                                 |
| `missing_axis`               | `axis` is empty/unspecified for a newly created Requirement/Feature/Behavior. When `axes/*.yml` has at least one axis registered, `suggestion` lists the registered axes (comma-separated); when none are registered, `suggestion` stays `null` and `message` points the caller at `axes add` instead |
| `missing_description`        | `description` is empty for a newly created Behavior/Condition, or for any ExpectedResult                                  |
| `unknown_axis`               | An axis value not registered in the `axes/*.yml` registry (a close match, if any, is offered in `suggestion`)             |
| `redundant_prefix`           | `condition.id` starts with `{behavior.id}-` (when `--strip-redundant-prefix` is not given to `knowledge apply`; see 1.4)  |
| `conflicting_existing_value` | When reusing an existing id, the specified `label`/`axis`/`description` does not match the existing file's value          |
| `parent_not_found`           | The parent reference recorded in an existing file (e.g., `requirement:` in `feature.yml`) contradicts the draft's chain    |

**Exit codes**

| Code | Meaning                                                                                           |
| ---- | --------------------------------------------------------------------------------------------------- |
| 0    | Success (no errors)                                                                                  |
| 1    | Validation errors present (error content on stderr; on JSON stdout when `--json` is given)          |
| 2    | Usage error (file not found, YAML unparsable, `--batch <dir>` has no `*.yml` files)                 |

**Example (success, human-readable)**

```console
$ markharness knowledge validate draft.yml --dir tmp/todo-sample
$ echo $?
0
```

(Neither stdout nor stderr prints anything)

**Example (success, `--json`)**

```console
$ markharness knowledge validate draft.yml --dir tmp/todo-sample --json
{"ok":true}
```

**Example (failure, human-readable)**

```console
$ markharness knowledge validate draft.yml --dir tmp/todo-sample
error: unknown_axis: axis "validdation" is not registered (path=behavior.axis[0])
error: redundant_prefix: condition.id "jump-ground" starts with behavior.id "jump-" prefix (suggested="ground", path=condition.id)
$ echo $?
1
```

**Example (failure, `--json`)**

```console
$ markharness knowledge validate draft.yml --dir tmp/todo-sample --json
{"ok":false,"errors":[{"code":"unknown_axis","path":"behavior.axis[0]","value":"validdation","message":"axis \"validdation\" is not registered","suggestion":"validation"}]}
$ echo $?
1
```

**Example (`--batch`, validating several drafts, one fails)**

```console
$ markharness knowledge validate --batch drafts/ --dir tmp/todo-sample --json
{"ok":false,"failures":[{"file":"01-broken.yml","error":"failed to parse draft: ..."},{"file":"03-air.yml","errors":[{"code":"missing_description","path":"condition.description","value":null,"message":"condition.description must not be empty","suggestion":null}]}]}
$ echo $?
1
```

A valid file such as `02-*.yml` is not included in `failures`. `03-air.yml` is still checked through to the end even though `01-broken.yml` failed to parse first.

**Use case mapping**: Supports UC1 "describe knowledge" (`docs/product-operation.md` line 103) in a TTY-independent way. Shares the same validation logic as `knowledge add` in section 1.2.

---

### 1.4 `markharness knowledge apply` — Validation + write of draft YAML (UC1: describe knowledge, non-interactive, TTY-independent)

```text
markharness knowledge apply <draft-file> [--json] [-d, --dir <path>] [--strip-redundant-prefix] [--dry-run]
markharness knowledge apply --batch <dir> [--json] [-d, --dir <path>] [--strip-redundant-prefix] [--dry-run]
```

**Purpose**: Performs the same validation as `knowledge validate` (section 1.3), and if there are no problems, writes **atomically** under `knowledge/`. Even when only some of the five tiers (Requirement through ExpectedResult) are newly created, the write happens all at once after all validation passes (temp file + rename; if an I/O error occurs mid-write, even the files already succeeded are rolled back). Files for an existing id (reuse) are not overwritten.

**Options**

| Option                      | Description                                                                                                                                                                                                                                                                                          |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `<draft-file>`              | Path to the draft YAML file. Same format as section 1.3. Mutually exclusive with `--batch`.                                                                                                                                                                                                          |
| `--batch <dir>`             | Treats every `*.yml` directly under `<dir>` as a draft file, validating and applying them one at a time in ascending file-name order. Mutually exclusive with `<draft-file>`. See "Batch mode" below.                                                                                              |
| `-d, --dir <path>`         | Same as section 1.3                                                                                                                                                                                                                                                                                   |
| `--json`                    | Same as section 1.3. On success, prints the list of written files (see below).                                                                                                                                                                                                                       |
| `--strip-redundant-prefix` | When `condition.id` starts with `{behavior.id}-`, adopts the id with the prefix stripped, without confirmation. If not given, stops with a `redundant_prefix` error (see section 1.3). If a directory of the same name as the stripped id already exists (legacy data), it is reused as-is without stripping, just as with `knowledge add`. |
| `--dry-run`                 | Synonymous with `knowledge validate` (validates only, does not write). A separate name intended for use in CI, etc.                                                                                                                                                                                  |

**Batch mode (`--batch <dir>`)**: Instead of manually looping `validate` → `apply` over each Condition one at a time, applies every draft YAML accumulated in a scratch directory in one call.

- Each draft is validated and applied in ascending file-name order (e.g. `01-empty-title.yml`, `02-max-length.yml`, ...). A later draft can reuse a Requirement/Feature/Behavior that an **earlier draft in the same batch just created**, by referencing it via id alone — the same way it could reuse one that already existed on disk before a single `apply`. No dependency resolution is performed, so name files so a parent-creating draft sorts before the drafts that reuse it.
- **All-or-nothing overall**: if any one draft fails with a validation or parse error, every file already written earlier in this batch call is deleted, and `knowledge/` ends up exactly as it was before the batch ran. Note, however, that each draft is validated against `knowledge/`'s state immediately before *that* draft is applied (reflecting the results of earlier drafts in the batch) — this is not a single upfront validation pass across every draft before any writing begins.
- `--dry-run --batch <dir>` is a thin alias that calls the exact same implementation as `knowledge validate --batch` (section 1.3) and never writes. Like section 1.3, it is cumulative (it does simulate every other draft in the batch being applied first) and checks every file (one failure does not stop the run), so it can never disagree with what a real (non-dry-run) run would do. See section 1.3's "Batch mode" for the `--json` output shape and exit code.
- If `<dir>` has no `*.yml` files directly under it (e.g. the directory only holds `.yaml`-extension drafts), this is an error: exit code 2, with `{"ok":false,"error":"no *.yml files found in batch directory <dir>"}` under `--json` (plain text on stderr otherwise). Treating a zero-match batch as a silent success would let an extension mistake or an empty directory pass unnoticed.
- Validation/parse error `--json` output (without `--dry-run`, when an actual write attempt fails) adds `"file":"<name>"` to the single-draft shape: `{"ok":false,"file":"...","errors":[...]}` for a validation failure, or `{"ok":false,"file":"...","error":"..."}` for a parse failure. Unlike `--dry-run` (section 1.3's collect-everything behavior), this stops at the first failure — since a write-mode failure requires rolling back everything already written, there is no point validating further. The human-readable mode likewise prefixes each error line with the file name.

**Exit codes**

| Code | Meaning                                                                            |
| ---- | ------------------------------------------------------------------------------------ |
| 0    | Success (write succeeded; no error when `--dry-run` is given)                       |
| 1    | Validation errors present (same format as section 1.3; no files are written at all) |
| 2    | Usage error (file not found, YAML unparsable, `--batch <dir>` has no `*.yml` files)  |
| 3    | Filesystem error (e.g., write failure)                                              |

**Example (success, `--json`)**

```console
$ markharness knowledge apply draft.yml --dir tmp/todo-sample --json
{"ok":true,"written":["knowledge/controls/player-jump/jump/ground/expected/002.yml"]}
$ echo $?
0
```

`written` lists only the files newly written (files skipped due to reuse of an existing id are not included), as paths relative to the target directory (`--dir`).

**Example (stripping a duplicated Condition id prefix with `--strip-redundant-prefix`)**

```console
$ markharness knowledge apply draft.yml --dir tmp/todo-sample --strip-redundant-prefix
$ echo $?
0
```

`draft.yml`'s `condition.id: add-task-max-length` (duplicating Behavior id `add-task`) is written as `max-length`. Same behavior as the automatic stripping in `knowledge add` (section 1.2).

**Example (applying multiple drafts at once with `--batch`)**

```console
$ ls drafts/
01-empty-title.yml  02-max-length.yml  03-duplicate-title.yml
$ markharness knowledge apply --batch drafts/ --dir tmp/todo-sample --json
{"ok":true,"written":["knowledge/req-todo/todo/add-task/empty-title/condition.yml","knowledge/req-todo/todo/add-task/empty-title/expected/001.yml","knowledge/req-todo/todo/add-task/max-length/condition.yml","knowledge/req-todo/todo/add-task/max-length/expected/001.yml","knowledge/req-todo/todo/add-task/duplicate-title/condition.yml","knowledge/req-todo/todo/add-task/duplicate-title/expected/001.yml"]}
```

`02-max-length.yml`/`03-duplicate-title.yml` reference `req-todo`/`todo`/`add-task` — newly created by `01-empty-title.yml` — by id alone, reusing them just as a single `apply` reuses an existing parent.

**Example (`--batch` rejects the whole batch on a validation error)**

```console
$ markharness knowledge apply --batch drafts/ --dir tmp/todo-sample
error: 02-max-length.yml: missing_description: condition.description must not be empty (path=condition.description)
$ echo $?
1
```

(No files remain under `knowledge/`, including the ones `01-empty-title.yml` had already written)

**Example (`--dry-run`)**

```console
$ markharness knowledge apply draft.yml --dir tmp/todo-sample --dry-run --json
{"ok":true}
$ echo $?
0
```

(No files are written)

**Example (write rejected due to a validation error)**

```console
$ markharness knowledge apply draft.yml --dir tmp/todo-sample
error: missing_description: behavior.description must not be empty (path=behavior.description)
$ echo $?
1
```

(No files are created under `knowledge/` at all)

**Use case mapping**: Supports UC1 "describe knowledge" (`docs/product-operation.md` line 103) in a TTY-independent way. The common entry point through which an AI agent or a future GUI implementation finalizes and registers knowledge. The human-facing `$EDITOR`-launching wrapper is implemented as `knowledge add --edit` (section 1.10).

---

### 1.5 `markharness generate` — Deterministic generation of TestCase (UC2: deterministically generate TestCase)

```text
markharness generate [--json] [-d, --dir <path>]
```

**Purpose**: Deterministically traverses `knowledge/`, mechanically assembles `TestCase` from `Requirement × Feature × Behavior × Condition × ExpectedResult`, and regenerates them as `.yml` files under `generated/testcases/`, **one file per Condition**. Each run empties `generated/testcases/` before rewriting it, so stale files corresponding to a deleted Condition are automatically removed too.

**Actor**: Nominally the CI Bot (UC2), but manual execution for local pre-checks is also possible.

**Algorithm overview**

- Traverses `knowledge/` in the order `requirement.yml` → `feature.yml` → `behavior.yml` → `condition.yml` → `expected/*.yml`, in path sort order (independent of the execution environment or timestamps). No `TestCase` is generated from a `Feature` that has no `Behavior`, or from a `Condition` whose `expected/` is empty (or absent).
- **Aggregation model**: All files under a single `Condition`'s `expected/` are aggregated into the `expected` array of a single `TestCase` (1 Condition = 1 TestCase; a change from the earlier model that made a separate TestCase per expected file).
- `case_id = "tc-{requirement.id}-{feature.id}-{behavior.id}-{condition.id}"`. Concatenating all four ids (`requirement`/`feature`/`behavior`/`condition`) makes a `case_id` collision structurally impossible even if a `condition.id` is reused under a different Behavior.
- The output file is written to `generated/testcases/{requirement.id}/{feature.id}/{behavior.id}/{condition.id}.yml`, fully mirroring `knowledge/`'s own hierarchy (the earlier flat `generated/testcases/{condition.id}.yml` naming had a defect where reusing the same `condition.id` under a different Behavior silently overwrote the earlier file).
- `title` = `condition.description`, `steps` = `[behavior.description]`, `expected` = the `description` of each `expected/*.yml`, listed in file-name sort order.
- `generated_from` records each of the `requirement` / `feature` / `behavior` / `condition` ids, and the source `expected_results` (the list of `id`s of `expected/*.yml`) that were aggregated.
- `axis`: a list of viewpoints formed by combining (union, deduplicated and sorted) the `axis` of the `Requirement` / `Feature` / `Behavior` (§3.4 "axis inheritance").
- The output is serialized with `serde_yaml_ng`, and always produces the same output for the same input (determinism, a prerequisite for diff verification in CI).
- In addition to `generated/testcases/*.yml`, `generate` also regenerates `generated/traceability-index.json` at the same time (a machine-readable index holding the Requirement → Feature → Behavior → Condition → TestCase correspondence, as pretty-printed JSON via `serde_json`). `markharness verify` (section 1.6) also includes this file in its diff verification.
- Omitting `--dir` targets the current directory (the same convention every other command follows; `generate` used to be the sole exception, always pinned to the current directory).
- `--json` prints `{"ok":true,"generated":<count>,"written":[<list of written file paths, including traceability-index.json>]}` instead of the human-readable message, so a caller can mechanically reconcile the reported count against the actual written files.

**Example**

```console
$ markharness generate
generated 1 testcase(s) into generated/testcases/
$ markharness generate --json
{"ok":true,"generated":1,"written":["generated/testcases/req-todo/todo/todo-add-task/todo-add-task-empty-input.yml","generated/traceability-index.json"]}
```

`generated/testcases/task-management/add-todo/add-task/empty-title.yml`:

```yaml
case_id: tc-task-management-add-todo-add-task-empty-title
generated_from:
  requirement: task-management
  feature: add-todo
  behavior: add-task
  condition: empty-title
  expected_results:
    - empty-title-001
title: |
  Submit the todo form with an empty title
steps:
  - |
    User adds a new task to the list.
expected:
  - |
    shows a validation error
```

If `knowledge/` has nothing in it, `generated/testcases/` becomes empty (0 files).

**Use case mapping**: UC2 "deterministically generate TestCase" (`docs/product-operation.md` line 105). Diff verification in CI (UC3) is done by `markharness verify` in section 1.6.

---

### 1.6 `markharness verify` — Diff verification of generated artifacts (UC3: review and merge generated artifacts)

```text
markharness verify [--json] [-d, --dir <path>]
```

**Purpose**: Rebuilds the TestCase and `traceability-index.json` from `knowledge/` using the same logic as `generate` (without writing to disk), and compares them against the committed `generated/testcases/*.yml` and `generated/traceability-index.json`. Intended to be run in CI to check that changes to `knowledge/` have not been forgotten to be reflected in `generated/` (this command already covers what `generate --check` would have done).

**Actor**: Reviewer / CI Bot (UC3)

**Options**

| Option              | Description                                                         |
| -------------------- | -------------------------------------------------------------------- |
| `-d, --dir <path>`   | Target project directory. Defaults to the current directory.        |
| `--json`             | Prints structured JSON instead of the human-readable message (see below). |

**Behavior**

- If there is no diff, prints `generated/testcases/ is up to date with knowledge/` and exits with code `0`.
- If there is a diff, lists the added, removed, and changed files, labeled `added:` / `removed:` / `changed:`, in file-name sort order, and exits with code `1` (does not show a unified diff of the contents). `generated/traceability-index.json` is included in the listing on the same footing as the other generated artifacts (under the file name `traceability-index.json`).
- With `--json`, always prints `{"would_change":<bool>,"added":[...],"changed":[...],"removed":[...]}` regardless of whether there's a diff. Each path is relative to `generated/`: TestCase files carry a `testcases/` prefix (e.g. `testcases/task-management/add-todo/add-task/empty-title.yml`), and `traceability-index.json` is listed by its bare name (it lives directly under `generated/`, not under `generated/testcases/`). Exits `0` when there's no diff (`would_change:false`), `1` when there is (`would_change:true`).

**Example (no diff)**

```console
$ markharness verify
generated/testcases/ is up to date with knowledge/
$ markharness verify --json
{"would_change":false,"added":[],"changed":[],"removed":[]}
```

**Example (diff present)**

```console
$ markharness verify
added: generated/testcases/task-management/add-todo/add-task/empty-title.yml
changed: generated/testcases/task-management/add-todo/add-task/max-length.yml
removed: generated/testcases/task-management/add-todo/add-task/duplicate-title.yml
$ echo $?
1

$ markharness verify --json
{"would_change":true,"added":["testcases/task-management/add-todo/add-task/empty-title.yml"],"changed":["testcases/task-management/add-todo/add-task/max-length.yml"],"removed":["testcases/task-management/add-todo/add-task/duplicate-title.yml"]}
$ echo $?
1
```

**Use case mapping**: UC3 "review and merge generated artifacts" (`docs/product-operation.md` line 106). When a diff is detected, judging whether its content is intentional and merging it is the Reviewer's role (a point of human judgment).

---

### 1.7 `markharness axes list` — List the axis registry

```text
markharness axes list [--json] [-d, --dir <path>]
```

**Purpose**: Prints the list of viewpoints registered under `axes/*.yml`, in ascending id order. A reference command for pre-emptively avoiding `unknown_axis` errors from `knowledge validate`/`apply`.

**Behavior**: Without `--json`, prints `id (label)` (or just id if the label equals the id) one per line, and prints `no axes registered under axes/` if there are zero registered. With `--json`, prints `[{"id":...,"label":...|null}]` as single-line JSON.

**Example**

```console
$ markharness axes list --dir tmp/todo-sample
gameplay (Gameplay)
ui

$ markharness axes list --dir tmp/todo-sample --json
[{"id":"gameplay","label":"Gameplay"},{"id":"ui","label":null}]
```

**Use case mapping**: A helper command that does not explicitly correspond to any UC (`docs/design/knowledge-apply-cli-spec.md` §8).

---

### 1.8 `markharness axes add` — Non-interactive axis registration

```text
markharness axes add <id> [--label <label>] [--json] [-d, --dir <path>]
```

**Purpose**: Creates `axes/<id>.yml`. `knowledge add --edit` (section 1.10) auto-registers unregistered axes as part of its interactive edit flow, but that is aimed at an interactive user who can launch `$VISUAL`/`$EDITOR` — it isn't usable by an AI agent or other caller driving the CLI non-interactively off JSON output. `axes add` is the standalone write command for that case, symmetric with the other resources (Requirement/Feature/Behavior/Condition).

**Behavior**

- `<id>` follows the same slug constraint as `condition.id` etc. (lowercase alphanumerics and hyphens only). An invalid id exits with code `2`.
- Omitting `--label` defaults `label` to the same value as `<id>` (the same "id doubles as label when omitted" convention every other command follows).
- If `axes/<id>.yml` already exists, it is **not overwritten**. An error message is printed and the command exits with code `2` (edit the existing file directly if you need to change it).
- With `--json`, prints `{"ok":true,"written":["axes/<id>.yml"]}`.

**Example**

```console
$ markharness axes add persistence --dir tmp/todo-sample
created tmp/todo-sample/axes/persistence.yml

$ markharness axes add persistence --dir tmp/todo-sample
error: axis 'persistence' already exists under axes/
$ echo $?
2

$ markharness axes add security --label Security --dir tmp/todo-sample --json
{"ok":true,"written":["tmp/todo-sample/axes/security.yml"]}
```

**Use case mapping**: Like `markharness axes list` (section 1.7), a helper command that does not explicitly correspond to any UC.

---

### 1.9 `forked_from` (UC1b: manually describe a conceptual derivation from another Feature)

There is no dedicated command; instead, the operational practice is to write the id of the source Feature directly into the `forked_from` field of `feature.yml` (§3.1). The draft YAML for `knowledge validate`/`apply` (sections 1.3/1.4) also accepts `feature.forked_from`; if the referenced Feature does not exist anywhere under `knowledge/`, it stops with an `unknown_forked_from` error. Because this is domain knowledge that cannot be automatically derived from Git history, unlike `derived_from` (the version history of the same Feature, §3.2–3.4), only validation is performed and no automatic computation is done.

```yaml
feature:
  id: player-double-jump
  label: player-double-jump
  axis: [gameplay]
  forked_from: player-jump # Conceptual derivation source (existing Feature id). Optional.
```

---

### 1.10 `markharness knowledge add --edit` — `$EDITOR` editing of draft YAML (UC1: describe knowledge)

```text
markharness knowledge add --edit [-d, --dir <path>]
```

**Purpose**: Instead of the interactive prompts of `knowledge add` (section 1.2), writes an empty draft YAML template (the same format as section 1.3) to a temporary file and launches `$VISUAL` (or `$EDITOR` if unset). When the file is saved and the editor exits, the same validation and write as `knowledge apply` (section 1.4) is performed; if there is a validation error, the error content is displayed and the same file is reopened in the editor (a loop). If neither `$VISUAL` nor `$EDITOR` is set, an error is displayed and the process exits with code `2`.

**On Windows / the `code` command**: VS Code's `code` command is actually a `.cmd` (batch file), and Rust's `std::process::Command` does not perform extension resolution (PATHEXT), so `EDITOR=code --wait` results in `program not found`. Specify it to launch via `cmd /c`, e.g. `EDITOR="cmd /c code --wait"`.

**Automatic axis registration**: If `requirement.axis` / `feature.axis` / `behavior.axis` includes a value not registered in `axes/*.yml`, only values satisfying all of the following conditions are automatically newly registered as `axes/<value>.yml` (with both `id` and `label` set to that value), and a message is displayed.

- There is no close match (edit distance / Levenshtein distance of 2 or less) against a registered axis (a value that might be a typo is not auto-registered, and remains as an `unknown_axis` error as before, with the close match presented via `suggested="..."`).
- It is in a valid `id` format (lowercase alphanumerics and hyphens only).

When a single validation has multiple unregistered axis values, each axis is judged independently (some may be auto-registered while only those with a close match remain as errors). This auto-registration is exclusive to `knowledge add --edit`; interactive `knowledge add` and non-interactive `knowledge validate`/`apply` continue to stop with only an `unknown_axis` error, as before.

**Example**

```console
$ EDITOR="cmd /c code --wait" markharness knowledge add --edit
axis 'state' を新規登録しました (axes/state.yml)
wrote knowledge/controls/player-jump/jump/ground/expected/001.yml
```

**Use case mapping**: UC1 "describe knowledge" (`docs/product-operation.md` line 103). Reuses `knowledge apply`'s non-interactive validation logic as-is.

---

### 1.11 `markharness cache rebuild` — Discarding the id cache (UC7: discard/rebuild the id cache)

```text
markharness cache rebuild [-d, --dir <path>]
```

**Purpose**: Deletes `.markharness-cache/` entirely (the uncommitted cache of Feature id→tree SHA resolution results used by `changes compute` in section 1.12. It is keyed by a content-addressing scheme, and is automatically recomputed on load whenever the content of `knowledge/` or the tool version changes, so explicit `rebuild` is normally unnecessary). Does not perform an immediate recomputation (it is computed lazily on the next `changes compute` run). No error occurs if the cache directory does not exist (idempotent).

**Example**

```console
$ markharness cache rebuild
removed .markharness-cache/ under /path/to/project
```

**Use case mapping**: UC7 "discard/rebuild the id cache" (`docs/product-operation.md`). A fail-safe for cases where id-resolution inconsistency is suspected.

**Note when changing a Feature's `id:` (for users, paper §3.3)**: The Feature id is tracked using the `id:` field of each `feature.yml` as the canonical source. If the value of `id:` itself is rewritten, the tool treats this as "the original Feature was deleted and a Feature with a new id was added," and `changes compute` cannot recover the `derived_from` relationship with past milestones (the version history is broken). **Renaming** a Feature directory (a path change) remains trackable as long as `id:` does not change, but this CLI has no migration procedure for a change to `id:` itself (such as recording an old-id→new-id alias); currently, users must strictly follow the practice of "never change `id:`." See [decisions/0004](./decisions/0004-feature-id-change-migration.md) for the status of consideration.

**On the cache key's version fields**: The `canonicalization_rule_version`/`id_index_schema_version` (paper §3.3) that make up the cache key in `.markharness-cache/` are currently fixed at `"1"` in the implementation. Since no normalization-rule revision or id-index format revision that would actually bump these values has yet occurred, it has not been empirically verified whether the cache is correctly discarded when the values are bumped.

---

### 1.12 `markharness changes compute` — Computing ChangeEvents (UC5: automatically compute ChangeEvent)

```text
markharness changes compute <from-milestone> <to-milestone> [--no-cache] [--current-tree] [-d, --dir <path>]
```

**Purpose**: Between two milestones (using the git tag name as-is; milestone boundaries are determined purely by tag-name match, and correspondence with `executions/*/milestone.yml` is the caller's responsibility), compares the tree SHA of each Feature directory under `knowledge/` via `git ls-tree -r <tag> -- knowledge`, computes a `ChangeEvent` for each changed Feature, and writes it to `changes/<to-milestone>.yaml`. The Feature id uses the `id:` field of each `feature.yml` as the canonical source, and is tracked independently of the directory name (paper §3.3).

The target project directory (`-d`/`--dir`, the parent of `knowledge/`) may be any directory within a git repository (it need not be the root of the repository itself). There used to be a known issue where this command would fail when the project directory was a subdirectory of the repository, due to a specification constraint of the `git show <ref>:<path>` syntax, but this has been resolved by switching to an `ls-tree`/`cat-file`-based implementation (details: [decisions/0006](./decisions/0006-nested-project-directory-support.md)).

**Actor**: CI Bot (UC5)

**Behavior**

- For each Feature, compares `from_blob`/`to_blob`; if they match, nothing happens. If it exists in only one, it is an addition/deletion; if it exists in both with differing values, it is a change, and one `ChangeEvent` is generated.
- `impacted_testcases` lists the `TestCase.case_id`s originating from the changed Feature, enumerated from the same generation graph as `generate` (section 1.5) (the structural generation graph of §3.2(A); version history is not used). Which point in time's `knowledge/` this generation graph is built from splits into two modes as of 2026-08 (as of 2026-08-12; see also [change-event-verification-tracking-spec.md](./design/change-event-verification-tracking-spec.md) §2.4).
  - **Default (`--current-tree` not given)**: Built from the `knowledge/` tree pointed to by the `to-milestone` tag (expanded into a temporary `git worktree`). Recomputing the same interval later always yields the same result.
  - **When `--current-tree` is given**: Built from `knowledge/` in the current working tree (legacy behavior). As long as the working tree keeps changing, recomputation results for the same interval can also change.
- `change_type` (spec change / bug fix, etc.) is output as `null` at the time of computation. The practice is for a human to fill it in afterward via `markharness changes annotate` (section 1.16) (§3.5).
- Unless `--no-cache` is given, Feature tree SHA resolution results are read from and written to `.markharness-cache/` (section 1.11), keyed by content-addressing.
- The `from-milestone..to-milestone` interval is traversed with `git rev-list --ancestry-path`, and for every two-parent merge commit present within the interval, the section 1.17 `lineage` determination logic is internally run using `git merge-base` (oldest first). If a target Feature is judged a `true_divergence` (true divergence) at any of the merges, an entry consisting of `merge_commit` (the merge commit SHA, for auditing) and `parent_tree_shas: [P1, P2]` is appended to the `true_divergences` field, in the order they occurred (§3.2). If the same Feature undergoes true divergence multiple times within the interval, all of them are recorded. For a normal linear history, or when there is no merge within the interval, it remains an empty array.
- **Note on branch-strategy dependence**: The `from_tree_sha`/`to_tree_sha` diff detection itself does not depend on the branch strategy (merge/squash/rebase/fast-forward), but `true_divergences` presupposes that a two-parent merge commit actually remains within the milestone interval; with squash merges, rebases, or fast-forward merges, the divergence relationship of the original branch is lost from the commit graph, so it is not detected (remains an empty array; paper §3.4 Table 2).

**Output example** (`changes/m2.yaml`, linear history case)

```yaml
- event_id: player-jump--m1--m2
  feature_id: player-jump
  from_milestone: m1
  to_milestone: m2
  from_tree_sha: 1a2b3c...
  to_tree_sha: 4d5e6f...
  impacted_testcases:
    - tc-ground-001
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
  change_type: null
  true_divergences:
    - merge_commit: 9f8e7d...
      parent_tree_shas:
        - 2b3c4d...
        - 5e6f7a...
```

**Use case mapping**: UC5 "automatically compute ChangeEvent." A simplified implementation of this model's core contribution (§3.2–3.4).

---

### 1.13 `markharness backfill run` — Batch processing of past milestones (UC6: run backfill asynchronously)

```text
markharness backfill run [--no-cache] [-d, --dir <path>]
```

**Purpose**: Targets the milestones for which `executions/*/milestone.yml` exists, orders them newest-first by the commit date (committer date) of the corresponding git tag, and runs processing equivalent to `changes compute` (section 1.12) for each pair of adjacent milestones, generating `changes/<milestone>.yaml`. A single run processes all pairs and then exits (it is not a resident daemon; intended for periodic execution from CI, etc.).

**Behavior**

- The oldest milestone has nothing to compare against, so it is skipped.
- Completion of processing for each milestone (the "to" side) is recorded in `git notes --ref=markharness-backfill`; on the next run, the same pair is not recomputed and is skipped (§4.3).
- Unless `--no-cache` is given, it shares the same `.markharness-cache/` as `changes compute`.

The constraint for when the target project directory (`-d`/`--dir`) is a subdirectory of the git repository is resolved the same way as in section 1.12 ([decisions/0006](./decisions/0006-nested-project-directory-support.md)).

**Example**

```console
$ markharness backfill run
backfilled changes/2026-08-release.yaml
backfill: 1 processed, 2 already up to date
```

**Use case mapping**: UC6 "run backfill asynchronously" (a simplified implementation of §4.1–4.3; the milestone-only scope and progress management via git notes follow the paper as written, but asynchronous workerization has been deferred).

---

### 1.14 `markharness milestone init` — Creating `executions/<tag>/milestone.yml` (a helper for UC4: tag a milestone)

```text
markharness milestone init <tag> [--json] [-d, --dir <path>]
```

**Purpose**: Creates `executions/<tag>/milestone.yml` corresponding to an existing `git tag <tag>`. UC4 itself (making the release-timing decision by putting down a `git tag`) remains a point of human judgment and is out of scope for this command, but this mechanically scaffolds that tag into the form that `backfill run` (section 1.13) can recognize (a directory name under `executions/<name>/milestone.yml` that matches the tag name, [src/backfill.rs:21-22](../../src/backfill.rs#L21-L22)).

**Options**

| Option              | Description                                                                                                                                                     |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `<tag>`             | (required) The target `git tag` name. Used as-is as the directory name of `executions/<tag>/` (no additional normalization/validation is performed).            |
| `-d, --dir <path>` | Target project directory (any directory within a git repository; need not be the repository's own root). Defaults to the current directory.                     |
| `--json`            | Prints the result as single-line JSON. If omitted, prints human-readable text.                                                                                    |

**Behavior**

- If the target `tag` does not exist as a `git tag`, prints an error message prompting the user to first run `git tag <tag>`, and exits with code `2` (no file is created).
- If the tag exists and `executions/<tag>/milestone.yml` has not yet been created, writes content consisting only of `id: <tag>` (committer date, etc. is not stored, so as not to change the existing design of fetching it from git each time, [src/backfill.rs:41-48](../../src/backfill.rs#L41-L48)).
- If `executions/<tag>/milestone.yml` already exists, its contents are left unchanged; a message stating it is "already initialized" is printed, and it exits with code `0` (the same idempotent pattern as `markharness init`).

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
initialized executions/2026-08-release/milestone.yml
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
executions/2026-08-release/milestone.yml is already initialized
$ echo $?
0
```

**Use case mapping**: Helps scaffold the destination for recording the results of UC4 "tag a milestone" (`docs/product-operation.md` line 107). The tagging decision itself continues to be made by a human.

---

### 1.15 `markharness execution record` — Recording a TestCase execution result (UC4: destination for recording execution results)

```text
markharness execution record <case_id> --milestone <name> --result <pass|fail|skip> --executor <name> [--note <text>] [--json] [-d, --dir <path>]
```

**Purpose**: Appends one execution result, for a given milestone, for one of the `TestCase`s (identified by `case_id`) in `generated/testcases/`, to `executions/<milestone>/results.yml`. Intended to be invokable via the same interface for both automated test execution by CI and manual testing by QA (the write destination and schema are shared).

**Options**

| Option                | Description                                                                                             |
| --------------------- | ------------------------------------------------------------------------------------------------------- |
| `<case_id>`           | (required) The `case_id` of the target TestCase (a value contained in one of `generated/testcases/*.yml`) |
| `--milestone <name>` | (required) The name of the destination milestone. The corresponding `executions/<name>/milestone.yml` must exist. |
| `--result <value>`    | (required) One of `pass` / `fail` / `skip`                                                               |
| `--executor <name>`  | (required) A free-text description of the executor (a person's name, or a CI identifier like `ci-github-actions`) |
| `--note <text>`       | An optional free-text note                                                                                |
| `-d, --dir <path>`   | Target project directory. Defaults to the current directory.                                              |
| `--json`              | Prints the result as single-line JSON. If omitted, prints human-readable text.                            |

**Behavior**

- If `executions/<milestone>/milestone.yml` does not exist, prints an error message prompting the user to first run `markharness milestone init <milestone>`, and exits with code `2`.
- If `case_id` is not found in any of the current (HEAD's) `generated/testcases/*.yml`, prints an error message prompting the user to first run `markharness generate`, and exits with code `2`. Since the file name under `generated/testcases/` is the `condition.id`, which differs from `case_id` ([section 1.5](#15-markharness-generate--deterministic-generation-of-testcase-uc2-deterministically-generate-testcase)), this check is done by reading the contents of each file (the `case_id` field). It does not go back to the content as of a past milestone; it always validates against the current HEAD.
- Once validation passes, one entry consisting of `case_id` / `result` / `executor` / `note` (not output if omitted) / `executed_at` (ISO8601, UTC) is appended to `executions/<milestone>/results.yml`. Existing entries are left unchanged and the new one is appended at the end (past execution history and records of re-execution are also preserved).
- The write uses the same "temp file + rename" atomic method as `knowledge apply` (section 1.4) (all entries are re-read and written together).
- Since the computation of `verified_feature_tree_shas` (see near section 1.17) goes through the same Feature tree SHA resolution process as `changes compute`, the constraint for when the target project directory is a subdirectory of a git repository is likewise resolved ([decisions/0006](./decisions/0006-nested-project-directory-support.md)).

**Exit codes**

| Code | Meaning                                                                       |
| ---- | -------------------------------------------------------------------------------- |
| 0    | Success (entry appended)                                                        |
| 2    | The specified milestone is uninitialized, or `case_id` was not found            |
| 3    | Filesystem error                                                                 |

**Example**

```console
$ markharness execution record tc-ground-001 --milestone 2026-08-release --result pass --executor yamada
recorded pass for tc-ground-001 into executions/2026-08-release/results.yml
```

`executions/2026-08-release/results.yml`:

```yaml
- case_id: tc-ground-001
  result: pass
  executor: yamada
  executed_at: 2026-08-08T03:15:00Z
```

**Example (error when specifying an uninitialized milestone)**

```console
$ markharness execution record tc-ground-001 --milestone 2099-01-01 --result pass --executor yamada
error: milestone '2099-01-01' not found. Run `markharness milestone init 2099-01-01` first.
$ echo $?
2
```

**Use case mapping**: UC4 "tag a milestone, destination for recording execution results" (the `executions/` directory correspondence table in `docs/cli-manual.md`, and the `TESTEXECUTION` in §3.1 of `docs/git-native-model-for-test-knowledge-management.md`). Aggregating/reporting results, bulk ingestion from CI test report formats (`--from-report`), and validation against `generated/testcases/` as of a past milestone are not implemented (future work).

---

### 1.16 `markharness changes annotate` — Post-hoc entry of change_type / related_events (§3.5)

```text
markharness changes annotate <event_id> [--type <spec-change|bug-fix|refactor|other>] [--related <event_id>]... [-d, --dir <path>]
```

**Purpose**: Lets a human set, after the fact, the `change_type` and `related_events` of a `ChangeEvent` computed by `changes compute` (section 1.12). Since it searches across all `*.yaml` files under `changes/` by `event_id`, the caller does not need to know in advance which milestone interval's file contains it.

**Behavior**

- `--type` and `--related` are independent, additive fields; either may be specified alone (it is an error to omit both — at least one must be specified).
- Specifying `--type` rewrites the `change_type` of the first file with a matching `event_id`. Other `ChangeEvent`s in the same file are left unchanged.
- `--related <event_id>` may be specified multiple times, and each is appended to the target event's `related_events` (existing values are kept; this appends rather than overwrites).
- If `--related` is given, it is verified, before any writing, that the target `event_id` and all `event_id`s given via `--related` exist somewhere in `changes/*.yaml`. If any is not found, the write does not happen — even if `--type` was also specified — and it errors with exit code `3` (`--type` and `--related` are independent additive fields, but the command as a whole either writes everything or writes nothing).
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

### 1.17 `markharness changes lineage` — Lineage audit via merge-base ancestor search (§3.2, secondary feature)

```text
markharness changes lineage --commit <merge-commit-sha> [--json] [-d, --dir <path>]
```

**Purpose**: For a given merge commit, compares the tree SHA of its two parents (P1, P2) and the merge base (B) via `git merge-base`, and for each Feature id, determines and outputs the §3.2 case classification (`linear` / `true_divergence` / `single_parent`) — an audit-only command. `changes compute` (section 1.12) internally invokes the same determination logic as this command for every two-parent merge commit present within the `from-milestone..to-milestone` interval, and reflects the result in `true_divergences`. To manually audit/verify an individual merge commit by itself, run this command independently. This command itself does not write to `changes/*.yaml` (it is a read-only audit command). In repositories operated with squash merges, rebases, or fast-forward merges, the target two-parent merge commits simply do not exist on the commit graph in the first place, so there is nothing this command can audit (paper §3.4 Table 2).

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

### 1.18 `markharness validate` — Structural validation of knowledge/, axes/, executions/ (§3.5/§3.6)

```text
markharness validate [--json] [-d, --dir <path>]
```

**Purpose**: Performs JSON Schema validation of all YAML under `knowledge/` (`requirement.yml` / `feature.yml` / `behavior.yml` / `condition.yml` / `expected/*.yml`), `axes/*.yml`, and `executions/<milestone>/results.yml`, against the corresponding `schema/*.schema.json` (a default set placed by `markharness init`; section 1.1). In addition, it validates cross-reference constraints that cannot be expressed by JSON Schema alone: whether `axis` tags are registered in `axes/*.yml`, and whether `feature.yml`'s `forked_from` points to an actually existing Feature id.

**Schema of `executions/*/results.yml`**: `execution_result.schema.json` requires `case_id` / `result` (`pass`/`fail`/`skip`) / `executor` / `executed_at`, and treats `note` / `verified_feature_tree_shas` as optional fields (section 1.15). `verified_feature_tree_shas` is absent from execution records written before this specification was introduced, but since it is defined as an optional field, such past records still pass schema validation as-is. In this case, `verify trace`/`verify pending` (change-event-verification-tracking-spec.md §6) does not retroactively backfill the record, and treats it as "unknown."

**Behavior**

- If there are zero problems, exits with code `0`. In human-readable mode, prints `knowledge/ and axes/ are valid`; with `--json`, prints `{"ok":true}`.
- If there are problems, lists a message for each file and exits with code `1`.

**Example**

```console
$ markharness validate
knowledge/controls/player-jump/feature.yml: axis 'not-registered' is not registered under axes/
$ echo $?
1
```

**Use case mapping**: An implementation of the §3.5 constraint "restrict, via schema validation, values not defined in `axes/*.yml` from being usable in front matter."

---

### 1.19 `markharness --version` / `-V` — Display version

```text
markharness --version
markharness -V
```

**Purpose**: Prints the `version` from `Cargo.toml` (embedded at build time as `CARGO_PKG_VERSION`). `Cargo.toml` is the single source of truth for the version number (per the CLAUDE.md operating rule).

**Example**

```console
$ markharness --version
markharness 0.3.0
```

---

### 1.20 `markharness axes prune` — Detect/delete unused axes

```text
markharness axes prune [--delete] [--json] [-d, --dir <path>]
```

**Purpose**: Detects axes registered under `axes/*.yml` that are not referenced by any Requirement/Feature/Behavior's `axis:` list anywhere under `knowledge/` (orphaned axes). `condition.yml`/`expected/*.yml` have no `axis` field, so they are not scanned.

**Behavior**

- Report-only by default (`axes/*.yml` is never deleted unless `--delete` is given).
- With `--delete`, actually deletes `axes/<id>.yml` for every unused axis found. No second confirmation (e.g. an additional `--yes`) is required — passing `--delete` itself is treated as explicit consent, since only orphaned axes with no reference anywhere are ever a candidate, so the risk of losing anything important is low.
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

(`legacy-ui` is removed from `axes/` and no longer appears in `axes list`)

**Use case mapping**: A companion command to `markharness axes add` (section 1.8). Does not map explicitly to any UC.

---

### 1.21 `markharness knowledge scaffold` — Print a blank draft YAML template

```text
markharness knowledge scaffold [--out <path>]
```

**Purpose**: Prints the same blank draft YAML chain (`EDIT_TEMPLATE`) that `knowledge add --edit` (section 1.10) writes into `$VISUAL`/`$EDITOR`, without spawning an editor. For non-interactive callers — AI agents and the like — that just want a draft file's starting point. Same five-tier (Requirement through ExpectedResult) blank chain as the "Draft YAML format" in section 1.3. See `docs/knowledge_draft.schema.json` for a reference schema meant for IDE autocompletion (not used for actual validation — see the note at the top of section 1.3).

**Options**

| Option         | Description                                                                                              |
| -------------- | ----------------------------------------------------------------------------------------------------------- |
| `--out <path>` | Write to this path instead of stdout. Refuses to overwrite an existing file at that path (exit code `2`) |

**Example (stdout)**

```console
$ markharness knowledge scaffold > drafts/01-new-condition.yml
```

**Example (`--out`)**

```console
$ markharness knowledge scaffold --out drafts/01-new-condition.yml
$ markharness knowledge scaffold --out drafts/01-new-condition.yml
error: cannot write drafts/01-new-condition.yml: ...(refuses to overwrite the existing file)
$ echo $?
2
```

**Use case mapping**: Supports UC1 "describe knowledge." Intended to pair with `knowledge apply --batch <dir>` (section 1.4): run `scaffold --out drafts/NN-xxx.yml` repeatedly, then apply the whole directory at once.

---

### 1.22 `markharness import` — Emit a canonical snapshot

```text
markharness import --source <native|junit> [--input <junit.xml>] [--git-ref <ref>] [--bind <artifact-id=version>]... --format json [-d, --dir <path>]
```

`native` normalizes `knowledge/` at the selected Git ref into artifacts carrying Feature tree SHAs and derived traces. `junit` normalizes JUnit XML TestCases and PASS/FAIL/SKIP results into evidence, with `--bind` supplying versions under verification. A JUnit `markharness.condition` property creates a stored trace. Output carries `schema_version: 1` and conforms to `schema/canonical_snapshot.schema.json`. The command does not modify the input or `knowledge/`.

---

### 1.23 `markharness plan` — Build a PR Verification Plan

```text
markharness plan --base <git-ref> --head <git-ref> --format json [--evidence <canonical.json>]... [--output <path>] [-d, --dir <path>]
```

Compares Feature tree SHAs across arbitrary base/head refs and emits changed Features, affected tests from stored/derived traces, `passed`/`failed`/`pending`/`stale` status from version-bound evidence, and rule-based proposals for changed Features without traces. Repeat `--evidence` for canonical snapshots emitted by `import`. The JSON contract is `schema_version: 1` under `schema/verification_plan.schema.json`. Exit code is 1 for failures, 2 for pending/stale/unreviewed proposals, and 0 when all required tests are verified.

---

## 2. Unimplemented (Planned) Commands

The following are commands planned for future implementation, based on the use case diagram and use case descriptions in `docs/product-operation.md`. The command names and options are tentative proposals and may change at implementation time.

| #   | Use case                                | Planned command (tentative)                                           | Actor                    | Overview                                                                                     |
| --- | ---------------------------------------- | ----------------------------------------------------------------------- | ------------------------ | ---------------------------------------------------------------------------------------------- |
| UC4 | Tag a milestone                          | No dedicated command (`git tag <milestone>` is used directly)          | Release Manager          | This is the release-timing decision itself, and remains a point of human judgment (Figure 3). |

These are currently not yet started; implementation ordering is managed separately via a checklist (`/plan-checklist`).

---

## 3. Verification / Testing

Unit tests for the implemented commands can be run with `cargo test` (see the `#[cfg(test)] mod tests` in `src/init.rs` / `src/knowledge.rs` / `src/interactive.rs` / `src/knowledge_draft.rs` / `src/knowledge_apply.rs` / `src/knowledge_edit.rs` / `src/generate.rs` / `src/verify.rs` / `src/axes.rs` / `src/traceability.rs` / `src/git.rs` / `src/id_cache.rs` / `src/changes.rs` / `src/backfill.rs`, as well as `tests/knowledge_cli.rs`, which verifies the exit codes and output of `knowledge validate`/`apply`). Because the tests in `git.rs`/`id_cache.rs`/`changes.rs`/`backfill.rs` actually run `git init`/`commit`/`tag` in a temporary directory, the `git` command is required in the test environment. Following the Pre-PR checklist (`CONTRIBUTING.md`), run the following before committing:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo audit
```
