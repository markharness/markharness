# Immutable Identity Model: Implementation Design Specification

**Status**: Design finalized (not yet implemented)
**Related documents**: [decisions/0013-immutable-identity-model.md](../decisions/0013-immutable-identity-model.md) ("ADR 0013" below), [git-native-model-for-test-knowledge-management.md](../git-native-model-for-test-knowledge-management.md)
**Audience**: `markharness` implementers

**Purpose**: This document works out, through a structured design interview (a design-tree "grilling" session), the open items ADR 0013 lists under "Conditions for acceptance." ADR 0013 itself settles the policy (why and what); this document settles how to build it. See Section 2 for the mapping between the ADR's conditions and where each is resolved here.

---

## 1. How the decisions were sequenced

Decisions were made in the order below, each unblocking the ones that depended on it. Parenthetical numbers refer to items in ADR 0013's "Conditions for acceptance."

1. Scope of this pass (design only this time; no code yet)
2. Where new logic lives (a new `src/identity/` module; `id_cache.rs` remains a lower-level utility)
3. Dispatch style for `EntityKind` and the shared Interface (condition 8)
4. The crash-recovery mechanism (condition 3)
5. How branch divergence gets resolved (part of condition 2)
6. Identity-event layout (part of condition 1)
7. Algorithm for `case_uid`/`change_event_uid` (condition 14)
8. Locking mechanism during crash recovery (part of condition 3)
9. Friction level for the `release` event (condition 13)
10. The path that makes schemas derive from Rust domain types (condition 10)
11. `IdentityAuditor`'s CLI surface (condition 15)
12. `recorded_at` during migration (part of condition 6)

## 2. Mapping to ADR 0013's "Conditions for acceptance"

| ADR condition | Resolved in |
|---|---|
| JSON Schema and locations for identity events/migration manifest; Registry cache format/key | §4, §5 |
| Root issuance, `previous_identity_event_uid`, branch divergence, canonical replay rules | §4, §7 |
| Crash-recovery protocol (transaction intent, staging, commit point, locking, etc.) | §6 |
| Mutation plans and each operation's logical commit boundary | §6.2 |
| Process-kill injection tests | §6.3 (folded into the implementation checklist) |
| Legacy resolution rules and golden fixtures | §11 (only `recorded_at` decided here; the fixtures themselves are an implementation-checklist item) |
| Consumer migration order and temporary compatibility adapters | §9 (unnecessary during the vertical slice, since nothing is published yet) |
| Finalizing the `EntityKind` etc. Interface | §3 |
| `EntityDescriptor`/contract-test structure | §3.3 |
| Generation path for schemas as the single source of truth | §8 |
| Turning this into an implementation checklist | `checklist-immutable-identity-model.md` |
| Impact list for the paper, CLI manual, schemas, and examples | Separate (§1.4/§3.2/§3.3/§3.4/§1.3/§2.4/§8 were already reflected in the prior ADR-revision session; §3.6/§7 are deferred to the Accepted transition) |
| Execution conditions, authorization, and audit requirements for `release` | §10 |
| Algorithm for `case_uid`/`change_event_uid` | §7 |
| Full-history-audit module Interface, separation boundary, and disclosure | §11 |

## 3. Module layout

A new `src/identity/` module is introduced. The existing `src/id_cache.rs` (Feature id → tree SHA resolution at a single Git ref, 482 lines) has a narrower, distinct responsibility and is not extended; it remains a lower-level utility the `identity` module uses internally (whether to merge them later is a separate decision).

```
src/identity/
  mod.rs        # public interface
  entity_kind.rs   # EntityKind enum, EntityDescriptor (declarative per-kind differences)
  event.rs         # IdentityEvent, the IdentityMutation enum
  engine.rs        # IdentityEngine (validation, mutation-plan generation)
  registry.rs       # reading/writing/replaying the Identity Registry (non-committed cache)
  recovery.rs       # crash recovery (staging, commit point, roll-forward)
  lock.rs           # application-level lock
  audit.rs          # IdentityAuditor (full-history audit; only the `identity` command depends on it)
```

### 3.1 `EntityKind` and its dispatch style

`EntityKind` is a closed enum with exactly these five values (not something end users can extend):

```rust
pub enum EntityKind {
    Requirement,
    Feature,
    Behavior,
    Condition,
    ExpectedResult,
}
```

`IdentityEngine`/`EntityDescriptor` do not use dynamic dispatch via trait objects (`Box<dyn EntityDescriptor>`). Only closed enum-dispatch that matches on `EntityKind` is used. This follows ADR 0013's rule against introducing an abstract Seam for behavior that has only one Implementation, and matches the fact that the five kinds form a fixed set that will not grow or shrink.

### 3.2 `EntityDescriptor`

Per-kind differences (parent kind, marker-file name, schema name, ID policy) are captured in a declarative `EntityDescriptor` value — data, not a trait object.

```rust
struct EntityDescriptor {
    kind: EntityKind,
    parent_kind: Option<EntityKind>,
    file_name: &'static str,       // e.g. "feature.yml"
    schema_name: &'static str,     // e.g. "feature.schema.json"
}

const DESCRIPTORS: [EntityDescriptor; 5] = [ /* Requirement, Feature, Behavior, Condition, ExpectedResult */ ];
```

Thin functions are added only where kind-specific reading or writing genuinely differs (e.g., `ExpectedResult`'s parent is `Condition`, and its files are plural under `expected/*.yml`); lifecycle rules (issuance, rename, retirement, etc.) are never duplicated per kind.

### 3.3 Contract tests

The same contract test suite runs against every `EntityKind`. Built on top of closed enum-dispatch, this takes the shape of a single test function iterating `for kind in EntityKind::ALL { ... }`. At minimum, each kind is checked for:

- UID is required (a UID-less Knowledge element is a validation error under UID mode)
- Duplicate UID and duplicate ID are rejected
- Rename events are generated and reflected in the Registry
- Event-replay results agree with Knowledge YAML
- Results are equivalent whether or not a Registry cache is present
- Migration is idempotent
- Crash recovery converges correctly after a process kill mid-operation

When a new `EntityKind` value is added, one exhaustiveness test detects that `DESCRIPTORS`, a schema file, or a fixture is missing, by cross-checking `EntityKind::ALL`'s element count against the key sets of each table.

## 4. Identity events

### 4.1 Layout

`.markharness/identity-events/` is grouped by kind and then by entity:

```
.markharness/identity-events/
  features/
    01ARZ3NDEKTSV4RRFFQ69G5FAV/
      01ARZ3NDEKTSV4RRFFQ69G5FE0.yml   # issued
      01ARZ3NDEKTSV4RRFFQ69G5FE1.yml   # renamed
  requirements/
    .../
```

Rationale: replay's primary access pattern is "fetch all events for one entity." A flat layout would force scanning every event file and filtering by `entity_uid` on every replay, which does not scale. Grouping mirrors the Identity Registry cache's own layout (`.markharness-cache/identities/features/<uid>.yml`), making the correspondence between the two easy to see.

### 4.2 Event kinds

`IdentityMutation` (the event kind) has seven variants:

| type | Meaning | Key fields |
|---|---|---|
| `issued` | New UID issuance | (root; no predecessor) |
| `renamed` | `id` change | `from_id`, `to_id` |
| `retired` | UID retirement on deletion | - |
| `restored` | Restoration of a retired UID | - |
| `released` | Lift the reuse reservation on a retired id | `released_id` |
| `reissued` | New UID issuance on copy/import | `source_uid` (optional) |
| `resolved` | Explicit resolution of a branch divergence | `previous_identity_event_uids` (plural), `winning_event_uid` |

Every kind other than `issued` has a predecessor. An ordinary event carries a single `previous_identity_event_uid` pointing at that entity's current head. Only `resolved` carries `previous_identity_event_uids` (plural), joining every divergent head it resolves.

```yaml
identity_event_uid: 01ARZ3NDEKTSV4RRFFQ69G5FE1
previous_identity_event_uid: 01ARZ3NDEKTSV4RRFFQ69G5FE0
type: renamed
entity_uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
from_id: todo-management
to_id: task-management
recorded_at: 2026-08-20T12:34:56Z
```

### 4.3 Replay order

Replay order never depends on filename, `recorded_at`, filesystem iteration order, or ULID timestamp order — only on predecessor references (`previous_identity_event_uid`, or `previous_identity_event_uids` for `resolved`). Independent entities' event graphs may be replayed in any order and must produce byte-for-byte identical results.

A snapshot containing two events that both extend the same head is a branch divergence, and is an ambiguity error unless a `resolved` event is present (see Section 7).

## 5. The Identity Registry (non-committed cache)

`.markharness-cache/identities/<kind>/<uid>.yml` holds the materialized view obtained by replaying the identity events present at the selected ref. It follows the same design principle as the existing id-resolution cache (`.markharness-cache/<ref>.json`, `id_cache.rs`):

- Absence is normal and triggers reconstruction on read.
- A present but stale or inconsistent cache is silently discarded and rebuilt (detected via a content-addressed cache-key mismatch).
- The canonical source is always the Git-tracked `.markharness/identity-events/`; the Registry cache must be reconstructible from that same ref's events alone even after deletion.

```yaml
# .markharness-cache/identities/features/01ARZ3NDEKTSV4RRFFQ69G5FAV.yml
uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
kind: feature
status: active
current_id: task-management
id_history:
  - id: todo-management
    from_identity_event_uid: 01ARZ3NDEKTSV4RRFFQ69G5FE0
  - id: task-management
    from_identity_event_uid: 01ARZ3NDEKTSV4RRFFQ69G5FE1
```

## 6. The crash-recovery mechanism

### 6.1 Staging and the commit point

No new write-ahead-log (WAL)-style mechanism is invented — but implementation surfaced a problem with the original plan (a single `fs_safety::replace_dir_from_staging` swap committing everything at once): what one identity operation writes lands in physically separate directories (`knowledge/` and `.markharness/identity-events/`), and no single rename can commit both at the same time.

Instead, the mechanism exploits a property the identity-event model already has: Knowledge YAML is a projection of event replay (Section 2). Only one point genuinely needs single-operation atomicity.

1. Write `intent.yml` first, under `.markharness/.identity-staging/<operation-id>/`, via `fs_safety::create_new_no_follow`. This is durable proof that an operation was attempted.
2. **The single logical commit point**: write the new identity-event file directly to its final location (`.markharness/identity-events/<kind>/<uid>/<event_uid>.yml`) via `fs_safety::replace_file` (the existing single-file atomic write). Whether this file exists is the only thing that decides whether the operation happened.
3. Update Knowledge YAML (e.g. `feature.yml`'s `id:`) via `replace_file`, with content deterministically derived from the replay result. This is an idempotent roll-forward — writing the same content again has no side effect.
4. Invalidate the Registry cache (delete it).
5. Delete `.markharness/.identity-staging/<operation-id>/`, marking the operation complete.

The startup recovery scan, for each leftover `.markharness/.identity-staging/<operation-id>/`, does the following:

- If the identity-event file named by `intent.yml` does **not** exist at its final location: step 2 was never reached (before the commit point). The old state is already correct; just delete `.identity-staging/<operation-id>/`.
- If it **does** exist: the commit point was reached. Idempotently redo steps 3–4 (roll forward Knowledge YAML, invalidate the Registry cache), then delete `.identity-staging/<operation-id>/`.

This gives crash recovery a simpler invariant than "one rename commits everything": only the identity-event file's existence decides whether an operation happened: everything else is deterministically re-derivable from it.

### 6.2 Mutation plans and logical commit boundaries

`issued`, `renamed`, `retired`, `restored`, `released`, `reissued`, and `resolved` all follow the same procedure (write intent → atomically write the commit-point event → roll projections forward). What differs per operation kind is the projection produced from replay; the commit-point mechanism itself is shared. Project-wide migration uses the batch form described in Section 12: its durable intent contains every planned issuance event, and the first event is the single logical commit point from which recovery completes the remainder.

### 6.3 Process-kill injection tests

For each operation kind, a test kills the process at three points — just before the commit point, during the rename (a window OS-level atomicity protects), and just after the commit point — and verifies that recovery on next startup always converges to either the old state or the committed new state. This is folded into the implementation checklist as its own item.

## 7. Resolving branch divergence

When two branches independently generate identity events for the same entity and a merge leaves two divergent heads (both extending the same predecessor) present in one snapshot, no custom merge driver auto-resolves it.

- An ordinary `git merge` proceeds as normal.
- `markharness validate` (and core paths such as `changes compute`) detect the divergent heads and stop with an ambiguity error.
- A human runs `markharness identity resolve <entity-uid>`. This command takes an argument specifying which divergent head wins (or a fresh `id` to use instead), and issues a new `resolved` event whose `previous_identity_event_uids` lists both heads' event UIDs.

Why not a merge driver: it requires per-developer local registration, is hard to test, and breaks the "clone and it just works" assumption Git otherwise provides. This project already treats identity-affecting operations — `rename-id`, for instance — as always explicit CLI commands; conflict resolution follows the same policy.

## 8. Algorithm for `case_uid`/`change_event_uid`

Both use standard UUIDv5 (RFC 4122, SHA-1-based). The `uuid` crate's v5 generation is used as-is (reuse the existing dependency if present; otherwise run a license check before adding it); no bespoke hash scheme is designed.

- `case_uid`: derived from a namespace UUID plus a name built from the set `{requirement_uid, feature_uid, behavior_uid, condition_uid, expected_result_uid}`, sorted into canonical order and concatenated.
- `change_event_uid`: derived from a namespace UUID plus a name built by canonically encoding and concatenating a domain separator, the identity canonicalization/algorithm version, the from/to snapshot identities, `feature_uid`, the canonical change payload, and any explicit, result-affecting options.

## 9. Friction level for the `release` event

`markharness identity release <uid> <old-id>` runs directly with no confirmation flag, matching `rename-id`. This project consistently relies on the command's execution itself, plus the resulting Git diff and identity event, as the audit trail for identity-affecting operations, so `release` is not singled out for extra friction. It is also reversible in effect (issuing a new UID again effectively undoes it).

## 10. The single source of truth for schemas

No code-generation crate such as `schemars` is added. `schema/*.schema.json` (and the `.markharness/schema/` mirror) continue to be hand-maintained, and a new test verifies that each Rust struct's field set matches its JSON Schema's `properties`. This avoids the license-check and maintenance burden of a new dependency, and integrates naturally with the existing `schema::validate_yaml` machinery.

The five kinds' structs and schema files, including the shared `IdentityHeader` (`uid`, `id`, `kind`), are covered by this consistency test.

## 11. `IdentityAuditor`'s CLI surface

Full-history auditing is a new, independent top-level command, `markharness identity audit`, rather than a subcommand under the existing `changes` family. `IdentityAuditor` walks the entire Git commit history — a heavyweight operation with a different cost profile from `changes compute`'s lightweight two-ref comparison.

Core paths limited to two-snapshot comparison — `changes compute`, `verify`, and similar — include a machine-readable `audit_scope: "two_snapshot"` field in their JSON output, so this narrower audit boundary can be detected from a CI gate. Documentation alone cannot be checked automatically.

## 12. `recorded_at` during migration

When `markharness identity migrate` backfills initial issuance events for pre-existing elements, it captures the migration operation's UTC start time once and uses that same value as every event's `recorded_at`. The eventual Git commit does not exist while the CLI is preparing the working-tree changes, so its timestamp cannot honestly be used as an input. No attempt is made to retroactively infer the element's true first-commit time via `git log --follow` or similar. Since `id` changes could not be tracked before UID was introduced — the very problem ADR 0013 exists to solve — there is no guarantee that history before a rename can be correctly followed, and an inferred value risks looking more accurate than it is. The shared operation timestamp honestly states: "the true creation time is unknown; this records when tracking began."

The complete set of planned UID/event assignments is written to one durable batch intent before the first event reaches its final path. The first event is the batch's logical commit point. If a crash occurs after that point, startup recovery writes every remaining planned event and rolls every Knowledge projection forward before normal commands resume; partial migration is never exposed as a normal state. `identity migrate --dry-run` reports the planned UID assignments without writing the lock, staging data, events, or Knowledge files.

## 13. Implementation order

The order follows ADR 0013's "Migration" section as-is:

1. The shared Identity Module (Sections 3–6 above) and the crash-recovery mechanism
2. An end-to-end vertical slice using Feature (an internal stage, never published or permanently supported)
3. Descriptors/adapters for the remaining four kinds (Requirement, Behavior, Condition, ExpectedResult)
4. Migration of every entity
5. The public schema-version-2 cutover (all five kinds switch together)

Because nothing is published during the vertical-slice stage (2), no temporary compatibility adapter is needed (this resolves the ADR condition on "the implementation order for migrating all consumers to UID and the removal condition for temporary compatibility adapters").

See `checklist-immutable-identity-model.md` for the concrete task breakdown.
