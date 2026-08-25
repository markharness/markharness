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

### Out of scope

Matches issue #29's stated exclusions: cross-schema converters, semantic diffing that ignores schema-only migrations, semantic-hash/canonicalization-rule-version changes, an `--allow-raw-schema-diff` escape hatch, and a command that rewrites existing Knowledge into a new schema version. This ADR only makes the version resolvable and makes an unsupported comparison fail safely instead of silently.

## Response taken

- `src/knowledge_schema.rs` (new): `resolve` (ref → `ResolvedSchemaVersion { version, is_legacy }`), `ensure_compatible` (the fail-closed gate), `legacy_warning`, and `CURRENT_KNOWLEDGE_SCHEMA_VERSION`.
- `src/git.rs`: added `resolve_commit_oid` for `milestone.yml`'s audit `commit_oid`.
- `src/milestone.rs`: `milestone_init` now writes `commit_oid` and `knowledge_schema_version` alongside `id`; unchanged idempotency behavior.
- `src/changes.rs`: `compute_changes_between_refs` calls `ensure_compatible` before any tree-SHA comparison.
- `src/backfill.rs`: `BackfillReport` gained `incompatible: Vec<String>`; `backfill_run_with_policy` skips a pair on `ErrorKind::Unsupported` instead of propagating it.
- `src/application.rs` / `src/presentation.rs`: `CommandOutcome::ChangesComputed` gained `warnings: Vec<String>`, rendered by both presenters.
- `src/init.rs`: `markharness init` now writes `[knowledge]\nschema_version = 1` alongside the existing top-level `schema_version = 1`.
