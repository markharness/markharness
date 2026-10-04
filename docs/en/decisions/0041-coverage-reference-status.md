# 0041: Show in `coverage` whether a binding's reference resolves

## Status

Accepted (decided 2026-10-05).

## Background

The GUI's Release Coverage view shows each case's verification means from `binding_mode` and `binding_reference` in `case_coverage`. One purpose of review is to find cases whose link to a test implementation does not resolve. But `binding_reference` returns the free-form string written in the `ExecutionBinding` as-is and never checks that its target exists. If the target was deleted or mistyped, the GUI still shows "automated (reference)", and a reference that does not exist is mistaken for one that does (Issue #98).

Whether a reference resolves is a structural fact about the repository, not an execution result. What [0025](./0025-v2-forward-compatible-evolution.md) §1 forbids is reading a binding's presence as "executed" or "passed". Showing whether the target is there does not cross that line.

## Decision

### 1. Add `reference_status` to `case_coverage`

`reference_status` is returned next to `binding_reference`, only when `binding_reference` is present. It has three values.

| Value | Meaning |
|---|---|
| `exists` | The reference matches a path in the Git tree of the commit given by `--at` |
| `missing` | It does not. Also an absolute path, a path containing `..`, or an empty string, which could point outside the repository |
| `not_checked` | The reference is a URL. Reachability is not checked |

When there is no `binding_reference` (no binding, or a binding with no `reference`), `reference_status` is omitted too. "No reference" and "reference not checked" mean different things in the GUI, so they are not merged.

`exists` says only that the target is there. It never says the test ran or passed ([0025](./0025-v2-forward-compatible-evolution.md) §1).

### 2. Judge against the tree at the `--at` commit

Files in the working tree are not consulted. The GUI also reads past commits, so judging against the working tree would make a past view wrong with today's state. This matches the existing rule that the Knowledge, the selection, and the bindings are all read from the `--at` commit ([design doc](../design/cli-read-model-design.md) P3). A file that was never committed does not exist at that commit.

### 3. How a reference is classified

- A string containing `://` is treated as a URL and reported `not_checked`.
- Anything else is a path relative to the project root, looked up with `git ls-tree` in the tree of the `--at` commit. A file or a directory whose name in the tree is exactly the reference is `exists` (a directory name with a trailing `/` does not match), because a manual case may point at a folder of material.
- "Relative to the root" means relative to the directory given by `--dir`. When `--dir` is a subdirectory of the Git repository, `git ls-tree` resolves the pathspec relative to `-C`, so the base is the same.

### 4. Patterns that can be misjudged are part of the specification

`reference` is a free-form string with no kind. In exchange for a simple rule, the following cases return a value that differs from the real situation. This is the scope of the decision, not a defect.

- **Symbol or line suffixes.** In `tests/a.spec.ts::jumps` or `tests/a.spec.ts#L10`, the part after the path is not stripped. The whole string is judged as one path, so a reference to an existing file is reported `missing`.
- **Backslash separators.** `tests\a.spec.ts` is not converted and is reported `missing`.
- **No normalization or expansion.** A reference is compared as an exact string, not as a Git pathspec. `./tests/a.spec.ts`, `tests//a.spec.ts`, `tests/` with a trailing `/`, a wildcard such as `tests/*.spec.ts`, and pathspec syntax such as `:(top)` are not interpreted, normalized, or expanded, and are reported `missing`. A real file name containing `[` or `*`, such as `[a].ts`, matches only that name.
- **A path containing `://`.** An existing path that happens to contain `://` is treated as a URL and reported `not_checked`.
- **Whether a URL is valid.** `not_checked` says nothing about whether the URL works.

`missing` means "the tree at the `--at` commit has no path equal to that string". It does not always mean the reference is wrong.

## Rationale

- It serves the purpose of review directly, and the view has no way to judge this itself (the view does not read `.markharness/` or Git directly; see [0040](./0040-traceability-show-element-detail.md)).
- One `git ls-tree` call is enough and no new dependency is needed.
- Three values are enough for the Issue's request: tell existing, missing, and not-checked apart.
- No reference format with suffixes has been observed in use. Adding a stripping rule before that is generalization nobody has asked for yet. The misjudgments are written down and accepted instead.

## Alternatives considered

- **Judge against the working tree.** Reading a past commit with `--at` would then change with whether today's file exists, which breaks reproducibility. Rejected.
- **Give `reference` a kind (`path`, `url`).** This changes the `ExecutionBinding` format. The request needs only the three-way distinction. Rejected.
- **Strip everything from `::` or `#` before judging.** The forms actually used are not known. Rather than freeze a guessed rule, accept that suffixed references are reported `missing` and decide once real examples exist. Rejected.
- **Check URL reachability.** It depends on the network and breaks reproducibility, and the Issue says it is unnecessary. Rejected.
- **Add the same field to `binding list --json`.** The GUI does not use it. Rejected.

## Consequences

- `reference_status` is added to `schema/release-coverage-read-model.schema.json` and to `tests/fixtures/read-models/release_coverage/v1/representative.json`. Only a field is added and no existing field changes meaning, so neither `rule_version` nor `schema_version` is bumped ([0039](./0039-read-output-schema-version-frozen-in-prototype.md)).
- Design doc §7 and the CLI manual are updated in both languages.
