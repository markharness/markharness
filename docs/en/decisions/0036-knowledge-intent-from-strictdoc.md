# 0036: `knowledge intent-from-strictdoc` generates a Knowledge Intent from a StrictDoc JSON export

## Status

Accepted (decided 2026-09-29; implemented).

## Context

[0023](0023-requirement-native-and-external-source.md) defines `source: external` Requirements, and [0030](0030-external-requirement-source-key.md) adds the external-side identifier `source_key` (the StrictDoc MID is recommended). The CLI offers no way to register the requirements of an existing StrictDoc project as those Requirements; the only route is writing one `knowledge reconcile` Intent entry per requirement by hand (issue #88; 358 requirements in a real project). `import` accepts only `native` and `junit`, never writes knowledge, and always emits a CanonicalSnapshot.

`strictdoc export --formats=json` writes the whole project to a single `out/json/index.json` (verified with strictdoc 0.30.1). Sections and requirements nest under `DOCUMENTS[]` via `NODES`; a requirement node has `_NODE_TYPE: "REQUIREMENT"` and a MID. **No node carries the path of the `.sdoc` it belongs to.** A MID is emitted only when it is persisted in the `.sdoc` or the document sets `ENABLE_MID: True`; in the latter case, if the `.sdoc` does not contain the MID, a different MID is generated on every export run.

## Decision

### 1. `knowledge intent-from-strictdoc` only prints an Intent YAML to stdout and writes nothing

```
markharness knowledge intent-from-strictdoc --input out/json/index.json [--sdoc-root <dir>] [-d <dir>] \
  | markharness knowledge reconcile -
```

Writing to knowledge stays with the existing `knowledge reconcile`, which provides atomic writes, `--check` and `id`-based idempotency without reimplementation. It is not `import --source strictdoc`, so `import` keeps its property of always emitting a CanonicalSnapshot.

### 2. The only input is a StrictDoc JSON export file

markharness does not run `strictdoc` and does not parse `.sdoc` requirement bodies (design principle P5: the Core does not know external formats). The user runs `strictdoc export` and passes the output file via `--input`.

### 3. What is imported and which fields are generated

Walk `DOCUMENTS[].NODES` recursively and import only nodes with `_NODE_TYPE == "REQUIREMENT"` (`SECTION` is only descended into; custom grammar types are out of scope). Each requirement yields a Requirement with:

| Field | Value |
|---|---|
| `id` | `sd-<full MID>` (a MID is 32 lowercase hex digits, a valid slug) |
| `source` | `external` |
| `source_key` | the MID (as recommended by 0030) |
| `source_locator` | §4 |
| `source_revision` | `current` |
| `axis` | `[]` |

### 4. `source_locator` is resolved by matching the `.sdoc` header MID

Because the JSON has no `.sdoc` path, scan `*.sdoc` under `--sdoc-root` (default: the project root), read the `MID:` line in each file's header, and find the file whose MID equals the document's `MID`. Requirement bodies are not parsed. `source_locator` is a path relative to the project root with `/` separators (the same base `knowledge reconcile` uses when it runs `git hash-object` at the project root). `--sdoc-root` must be inside the project root.

### 5. Any inconsistency rejects the whole run

If any of the following holds, print diagnostics, exit non-zero, and emit no Intent at all:

- A requirement node has no MID.
- A document that contains requirements has no MID.
- The document's MID matches zero or more than one `.sdoc`.
- `--sdoc-root` lies outside the project root.

### 6. Preconditions the user must satisfy

MIDs must be persisted in the `.sdoc` files (both requirement nodes and documents). If MIDs are merely generated per export via `ENABLE_MID: True`, `id` and `source_key` change on every run and each re-run would create different Requirements. A document MID missing from the `.sdoc` header is caught by the §4 matching and reported as an error, but whether requirement-node MIDs are persisted is not verified. The user also commits the `.sdoc` files so that `source_revision: current` can resolve (`knowledge reconcile` fails on uncommitted files; ADR 0029).

## Invariants

- The command writes nothing to knowledge or the filesystem.
- The `id` in the emitted Intent depends only on the input MID, not on ordering or count.
- If a single inconsistency exists, no partial Intent is emitted.

## Consequences

- `src/knowledge_strictdoc.rs` (new): JSON + `.sdoc` header index → Intent conversion.
- `src/cli.rs`: `KnowledgeCommand::IntentFromStrictdoc`.
- `tests/knowledge_intent_from_strictdoc_cli.rs` (new).

## Alternatives considered and rejected

- **`import --source strictdoc`**: rejected because it would break the meaning of `import`'s output (CanonicalSnapshot) and of `--bind` / `--format`.
- **A `knowledge` command that writes directly**: rejected because it duplicates the write path of `knowledge reconcile`.
- **Running `strictdoc` as a subprocess**: rejected because it brings in external tool execution and environment differences (e.g. Windows).
- **A custom `.sdoc` parser**: rejected because nothing beyond the header `MID:` line is needed.
- **Duplicate detection, updates or deletion detection by `source_key`**: 0030 §3 stays out of scope (YAGNI). Because `id` is deterministic from the MID, `id`-based idempotency already makes re-runs safe.
- **Generalizing to `intent --source <name>`**: unnecessary while there is one input format (design §9.2); revisit renaming when a second format appears.
- **Matching by `TITLE`, one `--source-locator` for all requirements, or a user-supplied mapping table**: rejected because titles collide easily and a single locator is inaccurate for multi-`.sdoc` projects (0023's change detection is a blob diff per `.sdoc`).
