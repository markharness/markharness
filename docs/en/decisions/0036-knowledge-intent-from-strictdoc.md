# 0036: `knowledge intent-from-strictdoc` generates a Knowledge Intent from a StrictDoc JSON export

## Status

Accepted (decided 2026-09-29; implemented).

## Context

[0023](0023-requirement-native-and-external-source.md) defines `source: external` Requirements, and [0030](0030-external-requirement-source-key.md) adds the external-side identifier `source_key` (the StrictDoc MID is recommended). The CLI offers no way to register the requirements of an existing StrictDoc project as those Requirements; the only route is writing one `knowledge reconcile` Intent entry per requirement by hand (issue #88; 358 requirements in a real project). `import` accepts only `native` and `junit`, never writes knowledge, and always emits a CanonicalSnapshot.

`strictdoc export --formats=json` writes the whole project to a single `out/json/index.json` (verified with strictdoc 0.30.1). Sections and requirements nest under `DOCUMENTS[]` via `NODES`; a requirement node has `_NODE_TYPE: "REQUIREMENT"` and a MID. The following facts drive the design.

- **No node carries the path of the source file it belongs to.**
- StrictDoc treats `.sdoc` and Markdown (`.md`, `.markdown`) as first-class document formats. The Markdown format has **no document-level MID** (a MID can be written only on nodes such as requirements), and the JSON carries no document MID for those documents either.
- A MID is emitted only when it is persisted in the source, or when an `.sdoc` document sets `ENABLE_MID: True`; in the latter case, if the source does not contain the MID, a different MID is generated on every export run.
- A MID is not always 32 digits (StrictDoc's own documents contain a 31-digit MID).
- In a real project (StrictDoc's own 358 requirements), two Markdown documents under `spec/` hold 51 of them, and copies of Markdown files containing MIDs sit under `tests/` as fixtures.

## Decision

### 1. `knowledge intent-from-strictdoc` only prints an Intent YAML to stdout and writes nothing

```
markharness knowledge intent-from-strictdoc --input out/json/index.json \
  [--sdoc-root <dir>]... [-d <dir>] | markharness knowledge reconcile -
```

Writing to knowledge stays with the existing `knowledge reconcile`, which provides atomic writes, `--check` and `id`-based idempotency without reimplementation. It is not `import --source strictdoc`, so `import` keeps its property of always emitting a CanonicalSnapshot.

### 2. The only input is a StrictDoc JSON export file

markharness does not run `strictdoc` and does not parse requirement bodies (design principle P5: the Core does not know external formats). The user runs `strictdoc export` and passes the output file via `--input`.

### 3. What is imported and which fields are generated

Walk `DOCUMENTS[].NODES` recursively and import only nodes with `_NODE_TYPE == "REQUIREMENT"` (`SECTION` is only descended into; custom grammar types are out of scope). Each requirement yields a Requirement with:

| Field | Value |
|---|---|
| `id` | `sd-<full MID>` (a MID is lowercase hex, a valid slug) |
| `source` | `external` |
| `source_key` | the MID (as recommended by 0030) |
| `source_locator` | §4 |
| `source_revision` | `current` |
| `axis` | `[]` |

A MID must be lowercase hex of one or more digits (the length is not checked). Because the Intent is assembled by formatting, any other value must not reach `id` (a slug) or the YAML.

### 4. `source_locator` is resolved through an index of requirement MIDs

The JSON has no source path and Markdown documents have no document MID, so the **requirement's own MID** is used to find its file. Scan `*.sdoc`, `*.md` and `*.markdown` under `--sdoc-root` (repeatable; default: the project root) and index the lines that declare a MID:

- `.sdoc`: `MID: <value>` at **column 0**.
- Markdown: `**MID**: <value>` at **column 0** (a trailing ` \` is allowed).
- Indented lines are ignored: StrictDoc's own user guide quotes a `[REQUIREMENT]` block as a code example inside a text node.
- Clear literal regions, where a MID declaration has no meaning, are excluded from the scan: a `.sdoc` multi-line string (`FIELD: >>>` up to `<<<`; its lines are not indented, so a quoted requirement block puts `MID:` at column 0) and a Markdown fenced code block (delimited by three or more backticks or tildes; an unclosed fence runs to the end of the file).
- Only lowercase-hex values are read as MIDs; lines with any other value are not indexed.

If exactly one file declares the requirement's MID, its path is the `source_locator`. It is relative to the project root with `/` separators (the same base `knowledge reconcile` uses when it runs `git hash-object` at the project root). Each `--sdoc-root` must be inside the project root.

This scan does not interpret StrictDoc. It does not read the syntax of requirements or the node structure; it only recognizes lines that declare a MID and the boundaries of the literal regions above.

### 5. Any inconsistency rejects the whole run

If any of the following holds, print diagnostics, exit non-zero, and emit no Intent at all:

- A requirement node has no MID, or its MID is not lowercase hex.
- A requirement's MID is declared in none of the scanned files.
- A requirement's MID is declared in more than one file (the error lists the files and points to `--sdoc-root` to narrow the scan).
- A `--sdoc-root` lies outside the project root.

### 6. Preconditions the user must satisfy

- MIDs must be persisted in the source. If MIDs are merely generated per export via `ENABLE_MID: True`, `id` and `source_key` change on every run and each re-run would create different Requirements. No scanned line matches such a MID, so this state is caught by §5's "declared in none of the scanned files".
- When copies containing MIDs exist (e.g. test fixtures), pass only the document directories via `--sdoc-root` (the equivalent of StrictDoc's `include_doc_paths`).
- The source files must be committed so that `source_revision: current` can resolve (`knowledge reconcile` fails on uncommitted files; ADR 0029).

## Invariants

- The command writes nothing to knowledge or the filesystem.
- The `id` in the emitted Intent depends only on the input MID, not on ordering or count.
- If a single inconsistency exists, no partial Intent is emitted.

## Consequences

- `src/knowledge_strictdoc.rs` (new): JSON + MID index → Intent conversion.
- `src/cli.rs`: `KnowledgeCommand::IntentFromStrictdoc`.
- `tests/knowledge_intent_from_strictdoc_cli.rs` (new).
- `source_locator` in [0023](0023-requirement-native-and-external-source.md) is generalized to a StrictDoc source file (`.sdoc` / `.md` / `.markdown`), and the matching messages in `validate`, reconcile and traceability are aligned.

## Alternatives considered and rejected

- **`import --source strictdoc`**: rejected because it would break the meaning of `import`'s output (CanonicalSnapshot) and of `--bind` / `--format`.
- **A `knowledge` command that writes directly**: rejected because it duplicates the write path of `knowledge reconcile`.
- **Running `strictdoc` as a subprocess**: rejected because it brings in external tool execution and environment differences (e.g. Windows).
- **A custom `.sdoc` / Markdown parser**: rejected because nothing beyond the lines declaring a MID is needed.
- **Matching the document MID against the `.sdoc` header**: the initial design. Rejected because Markdown documents have no document MID, so it cannot handle them.
- **Matching by `TITLE`, one `--source-locator` for all requirements, or a user-supplied mapping table**: rejected because titles collide easily and a single locator is inaccurate for multi-file projects (0023's change detection is a blob diff per source file).
- **An `--exclude` option**: rejected in favor of naming the scanned directories explicitly with `--sdoc-root` (repeatable), which is the safer side: unexpected copies are never picked up.
- **Duplicate detection, updates or deletion detection by `source_key`**: 0030 §3 stays out of scope (YAGNI). Because `id` is deterministic from the MID, `id`-based idempotency already makes re-runs safe.
- **Generalizing to `intent --source <name>`**: unnecessary while there is one input format (design §9.2); revisit renaming when a second format appears.
