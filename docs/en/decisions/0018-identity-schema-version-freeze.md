# 0018: Freeze `[identity].schema_version` at 1, correcting ADR 0013's unneeded bump

## Status

Accepted (implemented). This ADR corrects the `schema_version = 2` value that ADR 0013's "Migration" section specified for the `[identity]` marker in `.markharness/config.toml`, changing it to `schema_version = 1`. The rest of ADR 0013's decisions (UID issuance, identity events, the migration procedure, etc.) are out of scope for this ADR and remain in effect unchanged.

## Context

ADR 0013's "Migration" section specified writing the following to `.markharness/config.toml` once the UID-mode public cutover completes:

```toml
[identity]
schema_version = 2
mode = "uid"
```

However, the implementation ([src/identity/marker.rs](../../../src/identity/marker.rs)) shows that cutover-completion detection (`is_uid_mode`) reads only the `mode` field; `schema_version` is written but never read by any code path. Unlike `[knowledge].schema_version`, which `changes compute` actually consults to compare refs ([0014](./0014-knowledge-schema-version-persistence.md)), `[identity].schema_version` has no corresponding compatibility-gating consumer implemented or planned.

[0014](./0014-knowledge-schema-version-persistence.md) rejected bumping `[knowledge].schema_version` during the prototype phase, absent real data or a real comparison use, as diluting the meaning of the ADR history, and froze it at 1 until the schema stabilizes. ADR 0013's bump of `[identity].schema_version` from 1 to 2 is exactly the pattern [0014](./0014-knowledge-schema-version-persistence.md) rejected — advancing a value with no corresponding read-side need — and we judge it to have been an unneeded implementation in violation of YAGNI (the principle stated in [CLAUDE.md](../../../CLAUDE.md)).

## Decision

### 1. Freeze `[identity].schema_version` at 1

The `IDENTITY_SCHEMA_VERSION` constant ([src/identity/marker.rs](../../../src/identity/marker.rs)) is 1, and UID-mode cutover writes `schema_version = 1`, not 2. The field itself is kept for structural symmetry with the other `schema_version` fields in `config.toml` (the top-level marker, `[knowledge].schema_version`), but its value does not move until a real compatibility-checking implementation actually needs it to.

```toml
[identity]
schema_version = 1
mode = "uid"
```

Whether UID-mode cutover has completed is still determined solely by the presence of `mode = "uid"`, as ADR 0013 specifies. `schema_version` plays no part in that determination.

### 2. Conditions for raising `[identity].schema_version` in the future

Only raise `[identity].schema_version` when an actual comparison/compatibility gate that reads its value (analogous to `changes compute`'s `ensure_compatible` for `[knowledge].schema_version`) is implemented, and a breaking change to the identity schema's shape lands at that same time. Do not advance the value in anticipation of some future breaking change with no such consumer yet planned.

### 3. `mode` and `schema_version` keep their separate roles

`mode` remains the sole authoritative flag for cutover completion. `schema_version` exists only to represent a structural version of identity data within UID mode, not whether cutover happened — no implementation should conflate the two (e.g. by gating cutover detection on the `schema_version` value).

## Scope of this correction

- `src/identity/marker.rs`: the `IDENTITY_SCHEMA_VERSION` constant and its tests.
- `tests/identity_cutover.rs`: the "schema version 2 public cutover" phrasing in its doc comment.
- `docs/ja/cli-manual.md` / `docs/en/cli-manual.md`: the description of the value written at cutover.
- `docs/ja/design/immutable-identity-model-design.md` and its English counterpart: the Phase 5 description.
- `README.ja.md`: the description of the post-cutover validation rule.
- Backward compatibility with any project that already migrated under `schema_version = 2` is not a concern here — the project is still in a 0.x prototype phase with no existing project carrying real data, the same reasoning [0014](./0014-knowledge-schema-version-persistence.md)'s background relies on.

## Relationship to 0013

This ADR corrects only the specific `schema_version = 2` value recorded in ADR 0013's "Migration" section. ADR 0013's body is left unedited as the historical decision record; only its status section gets a pointer to this ADR (following the ADR operating policy in [release-and-license instructions](../../../.github/instructions/release-and-license.instructions.md), which rules out moving files or rewriting content).
