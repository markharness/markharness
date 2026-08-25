# 0014: Persist the Knowledge schema version into Git history

## Status

Accepted (2026-08-25)

## Background

[`changes compute`](../cli-manual.md) detects Feature changes by comparing each Feature directory's Git tree SHA between two refs (`src/changes.rs`). If a Knowledge schema migration mechanically rewrites every `feature.yml`/`behavior.yml`/etc. without changing their meaning, every Feature's tree SHA still changes, so every Feature gets a `ChangeEvent` and every generated test case lands in `impacted_testcases` — indistinguishable from a real content change.

A future canonical-model converter (out of scope here — see below) needs to know, for an arbitrary Git ref, which Knowledge schema version was in force at that ref, before it can decide whether two refs are even comparable. Nothing in the repository records that today: `.markharness/knowledge/*.yml` files carry no version marker, and `.markharness/config.toml`'s only field is a marker-file-format `schema_version` that [0010](./0010-project-root-auto-detection.md) explicitly deferred reading for compatibility checks ("that consumer-side logic is deferred until actually needed (YAGNI)"). [Issue #29](https://github.com/markharness/markharness/issues/29) is that need becoming concrete.

[0013](./0013-immutable-identity-model.md) already established the pattern this ADR follows: a second, narrowly-scoped `schema_version` living under its own table in `config.toml` (`[identity].schema_version`), independent of the marker file's own top-level `schema_version`. The repository also already carries several other independent `schema_version`-shaped values — the versioned JSON output envelope (`src/presentation.rs`, `"schema_version":1`), and the id-resolution/identity-registry cache keys' `CANONICALIZATION_RULE_VERSION` / `ID_INDEX_SCHEMA_VERSION` (`src/id_cache.rs`, `src/identity/registry.rs`). None of these are shared with each other; this ADR adds one more, scoped to Knowledge content itself.

## Decision

### 1. A dedicated `[knowledge].schema_version` in `config.toml`

```toml
schema_version = 1

[knowledge]
schema_version = 1
```

Scoped separately from the top-level marker-file `schema_version`, `[identity].schema_version`, the JSON output envelope's `schema_version`, and the id-cache's `CANONICALIZATION_RULE_VERSION`/`ID_INDEX_SCHEMA_VERSION` — none of these are read or written together.

### 2. The ref's own `config.toml` is authoritative

`changes compute <from> <to>` resolves `[knowledge].schema_version` from `from`'s and `to`'s own committed `config.toml` (via `git ls-tree` + `git cat-file`, not a working-tree read), never from the running CLI's version and never from `milestone.yml`. This lets the same resolution work uniformly for milestone tags, arbitrary commits, and a future PR base/head comparison.

### 3. `milestone.yml` gets an audit copy, not a second authority

`milestone init` additionally resolves and records `commit_oid` and `knowledge_schema_version`:

```yaml
id: v2
commit_oid: 0123456789abcdef...
knowledge_schema_version: 1
```

These are audit/display copies of the tag-resolved values, not consulted by `changes compute`. `milestone init` remains idempotent — an existing `milestone.yml` is left untouched, exactly as before this change.

### 4. `changes compute` and `backfill run` resolve versions before diffing

Both go through the same `changes::compute_changes_between_refs` (`ChangeAnalyzer::compute` → `compute_changes`, `backfill_run_with_policy` calls the latter directly), so the version gate lives in one place and applies to both automatically.

### 5. An unsupported version combination fails closed

When `from` and `to` report different known versions, or either reports a version newer than this CLI build knows about, `compute_changes_between_refs` returns `io::Error` with `ErrorKind::Unsupported` before computing anything. No `ChangeEvent` is generated and the caller (`application::compute_changes`) never reaches its `replace_file` write, so an existing `changes/<to>.yaml` is left untouched. This reuses the existing `error: {err}` → stderr → exit 1 path (`src/main.rs`) rather than adding a second, JSON-specific error channel.

`backfill run` treats the same `Unsupported` error as a per-pair skip rather than aborting the whole run: the pair is recorded in `BackfillReport.incompatible`, not in `git notes`, so a later run retries it automatically — once a converter exists, the retry succeeds without any manual re-triggering step.

### 6. A ref with no recorded version is legacy schema version 1

`.markharness/knowledge/` predates this feature everywhere it hasn't been touched yet, so a ref whose `config.toml` has no `[knowledge]` table (or no `config.toml` at all) is treated as legacy schema version 1, and `changes compute` surfaces that assumption as a warning rather than silently guessing. The warning is a structured field (`CommandOutcome::ChangesComputed.warnings: Vec<String>`) rendered by both `HumanPresenter` (`warning: ...` lines) and `JsonPresenter` (a `"warnings"` array in the existing JSON envelope) — adding a field to an already-versioned JSON contract is additive and does not require bumping that envelope's own `schema_version` (`docs/en/design/verification-plan-canonical-model-design.md`'s existing convention: only removing/renaming a field does).

### 7. `milestone.yml`'s audit copy is verified, not just written

Both `changes compute` and `backfill run` resolve each ref's Knowledge schema version through a single shared function, `compute_changes_with_warnings`, which — right after resolving each ref's version (§8 below; the audit check reuses that same resolution rather than re-resolving) — also checks each ref's own `.markharness/executions/<name>/milestone.yml` (when one exists for that name) against the tag it claims to describe: its recorded `commit_oid` and `knowledge_schema_version` must agree with what the tag actually resolves to right now (the version-resolution policy table's "`milestone.yml` とtag内の正本が不一致 | エラーとして報告する" row, previously unimplemented). A `milestone.yml` with no such fields (predating this ADR) is not checked — the tag alone is trusted, per the table's next row. A mismatch (a moved tag, or a hand-edited file) is a hard `InvalidData` error, not a fail-closed `Unsupported` skip: `backfill run` therefore does not silently skip it and retry later the way it does an unsupported schema-version pair — a stale or tampered audit copy needs a human to look at it, not a converter.

### 8. Version resolution happens once per ref, not once per concern

`compute_changes_with_warnings` resolves each ref's Knowledge schema version exactly once and reuses that same `ResolvedSchemaVersion` for all three of its consumers — the §7 `milestone.yml` audit check, the fail-closed gate, and the legacy-warning text — rather than each one re-resolving independently (a Standards-review finding, initially caught between `application::compute_changes` and `changes::compute_changes`, and found a second time between `milestone::verify_audit_matches_tag` and its caller: duplicate Git reads, and a real risk of two of these three disagreeing if the resolutions ever produced different `ResolvedSchemaVersion`s — e.g. under a concurrent tag update between calls). `verify_audit_matches_tag` therefore takes the caller's already-resolved `ResolvedSchemaVersion` as a parameter instead of resolving it itself.

### 9. `warnings` is an optional JSON field, and `backfill run` reports the same information `changes compute` does

The `"warnings"` key is omitted from `changes compute --json`'s output entirely when there is nothing to warn about, rather than emitted as `"warnings":[]` — the design doc's JSON contract rule (§5) permits only *optional* field additions within one `schema_version`, and a field that is always present (even as an empty array) is a required field in practice, changing the v1 contract's shape for every existing consumer.

`backfill run` collects the same legacy-schema-version warnings `changes compute` does (one call to `compute_changes_with_warnings` per pair), and — since it currently has no `--json` mode — prints them as `warning: ...` lines and lists each skipped-as-incompatible pair by name, matching `changes compute`'s policy rather than only its fail-closed gate. Because a version-incompatible pair leaves real work undone even though the rest of the run succeeded, `backfill run` exits `1` (not `0`) whenever `BackfillReport.incompatible` is non-empty, instead of reporting a clean success that would let the gap go unnoticed.

### Out of scope

Matches issue #29's stated exclusions: cross-schema converters, semantic diffing that ignores schema-only migrations, semantic-hash/canonicalization-rule-version changes, an `--allow-raw-schema-diff` escape hatch, and a command that rewrites existing Knowledge into a new schema version. This ADR only makes the version resolvable and makes an unsupported comparison fail safely instead of silently.

## Response taken

- `src/knowledge_schema.rs` (new): `resolve` (ref → `ResolvedSchemaVersion { version, is_legacy }`, hard-erroring on a malformed — non-integer, out-of-`u32`-range, or non-table `[knowledge]` — recorded value rather than silently treating it as absent), `ensure_compatible` (the fail-closed gate, rejecting version `0` alongside differing/future versions), `legacy_warning`, and `CURRENT_KNOWLEDGE_SCHEMA_VERSION`.
- `src/git.rs`: added `resolve_commit_oid` for `milestone.yml`'s audit `commit_oid`.
- `src/milestone.rs`: `milestone_init` now writes `commit_oid` and `knowledge_schema_version` alongside `id`; unchanged idempotency behavior. Added `verify_audit_matches_tag`, checking a `milestone.yml`'s recorded audit fields against the tag's live resolution.
- `src/changes.rs`: added `compute_changes_with_warnings` (resolves each ref's schema version once, runs `milestone::verify_audit_matches_tag` and `knowledge_schema::ensure_compatible`, and returns both the `ChangeEvent`s and any legacy-version warnings); `compute_changes`/`compute_changes_between_refs` are now thin wrappers over it, so every caller shares one resolution path.
- `src/backfill.rs`: `BackfillReport` gained `incompatible: Vec<String>` and `warnings: Vec<String>`; `backfill_run_with_policy` calls `compute_changes_with_warnings`, skips a pair on `ErrorKind::Unsupported` instead of propagating it, and collects its warnings.
- `src/application.rs` / `src/presentation.rs`: `CommandOutcome::ChangesComputed` gained `warnings: Vec<String>`, rendered by both presenters — `JsonPresenter` omits the key entirely when empty rather than emitting `"warnings":[]`.
- `src/cli.rs`: `backfill run` prints each collected warning and each incompatible-pair name, and exits `1` when any pair was skipped as incompatible.
- `src/init.rs`: `markharness init` now writes `[knowledge]\nschema_version = 1` alongside the existing top-level `schema_version = 1`.
