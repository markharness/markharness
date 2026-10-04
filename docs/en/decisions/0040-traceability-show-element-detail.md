# 0040: Read the content of one chosen element with `traceability show`

## Status

Accepted (decided 2026-10-05).

## Background

The GUI is laid out as one tree rooted at the requirements, plus a detail pane for the chosen element. The tree is built from the light `traceability` output. The detail pane shows the content of the chosen element.

Each Node in `traceability` carries only an identifier and a `label`. Axes, descriptions, `procedures`, a Scenario's `phases`, and the steps of a generated TestCase appear in no read output. The [design doc](../design/cli-read-model-design.md) §11 also says that view does not read Knowledge or `.markharness/` directly and takes only CLI output as input. The GUI therefore has no way to obtain the content of its detail pane (Issue #95).

Design doc §12 listed a TestCase detail model as something to add "once view shows a concrete need", and said that reading the generated file directly could stand in for it meanwhile. The detail pane is that concrete need, so the statement must be revisited. Reading the file directly also contradicts the boundary that view does not read `.markharness/` directly.

## Decision

### 1. Add a new command, `traceability show`

```text
markharness traceability show --uid <UID> [--at <git-ref>] [-d, --dir <path>]
```

It writes exactly one JSON document, holding the content of the chosen element, to standard output. `traceability` stays light; only the detail carries content. `--at` means what it means for `traceability`: the working tree when omitted, the given Git ref otherwise ([0033](./0033-traceability-defaults-to-working-tree.md)). If the tree and the detail were read at different points, the GUI would show contradictory values.

The existing use of `traceability` with `--at` and no subcommand does not change. `show` is a subcommand of it.

### 2. Fields per element

| Element | Fields |
|---|---|
| Requirement | `requirement_id`, `axis`, `description` |
| Feature | `feature_id`, `axis`, `description` |
| Behavior | `behavior_id`, `axis`, `description`, `procedures` |
| Scenario | `scenario_id`, `description`, `phases` (each `steps` item as `action` or `use`, and `results`), `implementation_note` |
| TestCase | `case_id`, `case_revision`, `phases` (with `use:` expanded: what actually runs) |

Every element also carries `schema_version`, `record_kind`, `at`, `kind`, and `uid`.

- `kind` is one of `requirement`, `feature`, `behavior`, `scenario`, `test_case`. The caller looks an element up by `uid` alone and does not need to know its kind in advance.
- There is one `record_kind` (`traceability_detail`). The JSON Schema expresses the per-kind difference in fields with `oneOf`.
- A field that does not exist is omitted, not set to `null`. This matches how `traceability` treats `label`, and design doc §11's rule that a reader keeps working when fields are added.
- A Requirement with `source: external` has no `description`, because the external document owns that content. This is the same reason it has no `label` ([0023](./0023-requirement-native-and-external-source.md)).
- A Scenario's `phases` are returned as written in Knowledge, with `use` left unexpanded. A TestCase's `phases` are returned expanded. The first is the current value for editing, the second is for checking what actually runs; the purposes differ, so the shapes differ.

### 3. A TestCase is looked up by `case_uid` and returns its current revision

A TestCase's `uid` is `traceability`'s `test_cases[].case_uid`. `case_revision` is not an input. If the GUI passes the same `--at` it used for the tree, the revision returned matches the tree.

### 4. An unknown `uid` is a failure

If `--uid` matches no element at that point, the command exits with code 2 and writes nothing to standard output. The error message names the `uid` and the point it was read at (`working-tree` or the Git ref). The GUI recovers by re-reading the tree.

### 5. `schema_version` is 1

Following [0039](./0039-read-output-schema-version-frozen-in-prototype.md), it is not raised during 0.x.

### 6. Scope

These fields are not used for display, so they are left out for now. If the editing scope later needs them, they are decided separately.

- A Requirement's `source_revision` and `related_issues`
- A Feature's `forked_from`

Design doc §12's "TestCase detail model" is treated as added by this decision.

## Rationale

- The detail pane is a concrete need, which satisfies the condition in design doc §12.
- To keep the boundary that view does not read `.markharness/` directly, the content must come from the CLI.
- Putting content in the tree would read every body on every tree read and make the tree heavy. Returning only the chosen element keeps the tree light.
- The expansion of `use:` is the output of the generation logic. If the GUI reproduced it, it could drift from that logic.

## Alternatives considered

- **Add an argument such as `--detail` to `traceability` and emit all content at once.** This makes the tree heavy and contradicts the premise "light tree, one element's detail on demand". Not adopted.
- **Split `record_kind` by element kind (five kinds).** The caller would have to know the kind in advance and could not look an element up by `uid` alone. Not adopted.
- **Require both `case_uid` and `case_revision` to select a TestCase.** If the GUI passes the same `--at` as for the tree, the revision already matches, so selecting one is unnecessary (YAGNI). Not adopted.
- **Allow the GUI to read generated files directly, as an exception in the design doc.** This breaks view's boundary for part of `traceability` and requires reproducing the expansion of `use:`. Not adopted.
- **Return a normal JSON document with `found: false` for an unknown `uid`.** The success/failure distinction moves into the output, and every caller must check that field. It also differs from how `impact` and `coverage` report failures. Not adopted.

## Consequences

- Add §5.5 to `docs/*/design/cli-read-model-design.md` and update §9, §12, and §14.1. Add the command to the CLI manual too.
- Add the JSON Schema (`schema/traceability-detail-read-model.schema.json`) and per-kind fixtures (`tests/fixtures/read-models/traceability_detail/v1/`).
- `knowledge reconcile` and the Intent format do not change. The detail output only supplies the current values the GUI needs when it builds an editing Intent.
