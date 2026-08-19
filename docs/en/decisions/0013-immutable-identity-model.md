# 0013: Separate mutable IDs from immutable Knowledge identity

## Status

Proposed

## Background

[0004](./0004-feature-id-change-migration.md) required reconsideration when users presented a concrete need to retain Feature history across an `id:` change. [Issue #17](https://github.com/markharness/markharness/issues/17) supplies that need through domain terminology changes, naming-convention migrations, external-system imports, repository integration, and organizational or product restructuring.

The first proposal added old-ID `aliases` to `feature.yml`; a later proposal added an immutable `uid` only to Feature. Both address only part of the problem:

- The current Knowledge tree cannot detect reuse of an ID or UID belonging to a deleted Feature.
- Feature IDs are consumed directly by ChangeEvents, lineage, verification, execution, canonical import, the derived index, and server presentation.
- TestCase IDs embed Feature IDs, so rename breaks their relationship with execution evidence.
- A Git diff alone cannot distinguish an intentional rename from ordinary manual editing.
- Requirement, Behavior, Condition, and ExpectedResult can eventually face the same rename problem.

The root problem is assigning both a mutable human-facing name and machine identity to one string. Without treating implementation cost as a decision factor, this ADR separates those roles throughout the Knowledge model. Results remain deterministically recomputable from two arbitrary refs' Git snapshots (paper §3.2–3.4), but the inputs have distinct roles: content `ChangeEvent`s remain derived from the two Knowledge snapshots, while rare identity-lifecycle declarations resolve the logical entity across those snapshots. Identity events are not a log of ordinary edits. The input scope therefore generalizes from only the Feature tree SHA to the committed `.markharness` snapshot.

In the sense that it requires no external database process or dedicated server, this design satisfies the letter of paper §1.4's "no dedicated DB" positioning. In substance, however, it builds a lightweight event-sourcing storage engine — a Git-tracked, append-only identity-event log plus replay-based derivation and a crash-recovery protocol — on top of Git-managed files. That steps outside what the phrase implies: avoiding the complexity a dedicated DB itself brings. This trade-off is justified on usefulness grounds by Issue #17's requirement (identity tracking that survives deletion). Paper §1.4 therefore describes the precise boundary: Git is the sole persistence boundary, with a lightweight identity event store inside the repository and no Git-external canonical persistence service.

## Decision

### 1. Give every persistent domain element an immutable UID

Requirement, Feature, Behavior, Condition, and ExpectedResult receive an immutable `uid`. The CLI issues a 26-character ULID when an element is created.

```yaml
uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
id: task-management
label: Task Management
```

The roles are separated as follows:

| Value | Purpose | Mutable |
|---|---|---|
| `uid` | Internal identity, relations, external integration | No |
| `id` | CLI input and human-readable external identifier | By explicit operation |
| `label` | Free-form display name | Yes |
| path | Git layout | Yes |

ChangeEvents, lineage, verification, execution, canonical import, the derived index, and server history use UIDs as identity keys. An ID is resolved to a UID at the relevant ref. UIDs and IDs must each be unique within one snapshot.

#### Shared implementation structure for the five entity kinds

Do not implement the same identity lifecycle independently for all five entity kinds. UID issuance, rename, retirement, restoration, release, reissue, event replay, Registry derivation, migration, and common validation belong in the Implementation of one deep Identity Module. Its small Interface passes at least these common domain types:

- `EntityKind` (`Requirement`, `Feature`, `Behavior`, `Condition`, `ExpectedResult`)
- typed `EntityUid` and `EntityId`
- `IdentityHeader` (`uid`, `id`, and kind)
- `IdentityMutation` (`Issue`, `Rename`, `Retire`, `Restore`, `ReleaseId`, `Reissue`)
- `IdentityEvent` and an `IdentityEngine` Interface that validates and produces mutation plans

Differences among entity kinds belong in a declarative `EntityDescriptor`, such as parent kind, marker file, schema name, and ID policy. Put a thin Adapter only where kind-specific reading or writing genuinely varies; do not duplicate lifecycle rules in Adapters. Do not introduce an abstract Seam for behavior that has only one Implementation.

Knowledge parent-child references also use UIDs, rather than mutable IDs, as canonical values. Persist `requirement_uid`, `feature_uid`, `behavior_uid`, and `condition_uid`; IDs remain display and CLI-resolution projections. A parent rename therefore does not require relation rewrites in descendant files.

Do not maintain Rust domain types, distributed JSON Schemas, and schemas emitted by `markharness init` as separate manually synchronized authorities. Rust domain types containing the shared `IdentityHeader` are the single source of truth for schemas; distributed and init schemas are generated deterministically. If generated artifacts are committed, CI rejects regeneration diffs.

Apply the same contract test suite to every `EntityKind`. At minimum, test required UIDs, duplicate rejection, rename events, agreement between event replay and Knowledge, cache equivalence, migration idempotence, crash recovery, and descriptor/schema/fixture coverage for each kind. When a kind is added, one exhaustiveness test detects a missing `EntityKind`, descriptor, schema, or fixture entry.

### 2. Make identity declarations canonical and keep the Registry derived

Append-only events under `.markharness/identity-events/` are the sole canonical source for identity lifecycle, including identities that have disappeared from the current Knowledge tree. They are ordinary Git-tracked files, not an external database or in-tool state. They record only intent that final content snapshots cannot recover—issuance, rename, retirement, restoration, release, and reissue. Ordinary Knowledge edits are not identity events and continue to produce content `ChangeEvent`s retrospectively from a two-snapshot diff.

The Identity Registry under `.markharness-cache/identities/` is a non-committed materialized view deterministically obtained by replaying the identity events present at the selected ref, following the same design principle as the existing id-resolution cache. Deleting it must allow reconstruction from the events in that same ref. Knowledge YAML is the current Knowledge projection. Validation compares event replay directly with current Knowledge values; when a Registry cache is present, it is trusted only after its content-addressed cache key and replay result agree. For rename, creation, retirement, restoration, release, and reissue, the CLI appends the event and updates Knowledge YAML as one crash-recoverable identity operation, then regenerates or invalidates the cache.

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

The Registry is a derived snapshot of UID issuance, lifecycle, and ID history. Its absence is valid and causes reconstruction; a present but stale or inconsistent cache is discarded. A deleted element remains `retired` in the replayed result.

Each entity's lifecycle events form a causal graph. An issuance event has no predecessor; an ordinary later event contains `previous_identity_event_uid` and refers to that entity's current head in its snapshot. A conflict-resolution event contains `previous_identity_event_uids` and joins every divergent head it resolves. These predecessor references, not filenames, `recorded_at`, filesystem iteration order, or ULID timestamp order, define replay order. Independent entity graphs may be replayed in any order and must produce byte-for-byte identical output. Two events extending the same head are a branch divergence: each branch snapshot is valid independently, but a snapshot containing both requires an explicit resolution event or is rejected as ambiguous.

Under this design, the input `changes compute` reads generalizes from "the Feature tree SHA under `knowledge/`" to "the committed `.markharness` snapshot at a point in time (including `knowledge/`, `identity-events/`, migration manifests, and required generated artifacts)." Comparing any two refs replays the cumulative events contained in each snapshot and never walks Git commit history. If the same entity UID occurs in both snapshots, its root issuance event UID and canonical payload must match, and every event UID shared by both snapshots must have byte-identical canonical content. A different root or rewritten shared event is an identity conflict, never continuity. Branch-only suffixes are replayed independently; their union is validated when branches are integrated.

Results from core modules such as `ChangeAnalyzer` and verification are determined only by the two compared refs' committed `.markharness` snapshots, explicit options, the identity canonicalization version, and the tool version. They do not depend on the working tree, current HEAD, an external database or service, wall-clock time, randomness, an uncommitted cache, a third ref, or additional history. A cache must produce byte-for-byte identical results to a cache-free run.

An `IdentityAuditor` that walks Git commit history detects deletion of events absent from both selected snapshots, historical rewriting outside their shared event set, and UID reuse elsewhere in repository history. It is separate from 2-ref comparison. `ChangeAnalyzer` guarantees snapshot consistency plus matching issuance roots and shared events for the two selected refs; it does not claim repository-wide append-only integrity or cross-branch history coverage. Core paths such as `changes compute` and `verify` do not depend on `IdentityAuditor`, and their output must state this narrower audit boundary.

Replaying the events in a snapshot makes UID duplication, mutation and reuse, old-ID reuse, reappearance of deleted elements, and repository-integration conflicts detectable. Once an ID has been issued to a UID, it cannot be assigned to another UID unless an explicit `release` event (below) lifts that reservation; restoring the same UID is the only other exception. `IdentityAuditor` detects deletion or rewriting of events in past commits.

### 3. Persist rename and lifecycle changes as first-class events

A rename is an explicit CLI domain operation, not ordinary YAML editing. It appends an event under `.markharness/identity-events/`.

```yaml
identity_event_uid: 01ARZ3NDEKTSV4RRFFQ69G5FE1
previous_identity_event_uid: 01ARZ3NDEKTSV4RRFFQ69G5FE0
type: feature_renamed
entity_uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
from_id: todo-management
to_id: task-management
recorded_at: 2026-08-20T12:34:56Z
```

`markharness feature rename-id <old> <new>` preserves the UID, validates ID uniqueness, updates Knowledge YAML, appends the event, invalidates the Registry cache, and runs full validation as one identity operation. A manual ID edit lacks the matching event transition and is therefore a validation error.

Creation, retirement, restoration, release, import reissue, and similar lifecycle changes use the same event model. `release` explicitly lifts the reuse reservation on an old ID tied to a `retired` UID, allowing that ID to be issued to a different UID afterward. Replay detects invalid ordering within a snapshot; `IdentityAuditor` detects modification or deletion of events in past commits.

#### Crash-recovery policy

The design does not require a truly atomic multi-file OS write or mandate rollback of every file to its old value after an ordinary error. The required guarantee is that intermediate state is never exposed as normal state and that, after an ordinary error, process kill, or system crash, the next startup converges to either the old state or the committed new state.

An identity operation has at least a transaction intent, staging area, one logical commit point, and recovery information. An incomplete operation before the commit point is discarded or restored to the old state. After the commit point, the system idempotently rolls Knowledge projections and generated artifacts forward from the canonical identity event and invalidates or regenerates derived caches. A mismatch with a corresponding committed operation is recoverable; a mismatch without an operation record is an unsupported manual edit and a validation error.

Normal commands detect incomplete operations at startup and do not proceed until recovery completes. A lock controls concurrent operations. Recovery itself must be idempotent if interrupted. The best-effort per-file deletion in the existing `knowledge_apply::apply_batch` is not considered a transaction primitive satisfying this guarantee.

### 4. Track TestCases and Executions by immutable identity

Generated TestCases receive an immutable `case_uid`. `case_id` remains a human-readable projection but is not used for matching.

```yaml
case_uid: 01ARZ3NDEKTSV4RRFFQ69G5FT1
case_id: tc-task-management-create-task-empty-title
generated_from:
  requirement_uid: 01ARZ3NDEKTSV4RRFFQ69G5FAA
  feature_uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
  behavior_uid: 01ARZ3NDEKTSV4RRFFQ69G5FB1
  condition_uid: 01ARZ3NDEKTSV4RRFFQ69G5FC1
  expected_result_uids:
    - 01ARZ3NDEKTSV4RRFFQ69G5FD1
```

`case_uid` is derived deterministically from the set of `requirement_uid`, `feature_uid`, `behavior_uid`, `condition_uid`, and `expected_result_uid` (sorted into canonical order) — for example, a deterministic hash over their concatenation. This is a pure function requiring no new persistent store (no TestCase Identity Registry), and it preserves `generate`'s existing determinism and purity (the same Knowledge snapshot always produces the same output). Regenerating from the same provenance set always yields the same `case_uid`. ID, label, path, and content changes on the same source elements preserve TestCase identity; changing the provenance UID set itself, such as by splitting a Condition, creates a new TestCase.

Execution records store `execution_uid`, `case_uid`, `feature_uid`, the execution-time `case_id`, and the verified Feature tree SHA. Verification matches re-execution by `case_uid`.

### 5. Give ChangeEvents immutable identity

`change_event_uid` becomes the internal reference key; the existing `event_id` becomes a human-readable display value. A recomputation never issues a new ULID for a ChangeEvent: it derives `change_event_uid` deterministically.

```yaml
change_event_uid: 8f8a3c5d-2df5-5ca7-95ef-11e405455a07
event_id: task-management--v2--v3
feature_uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
feature_id_at_from: todo-management
feature_id_at_to: task-management
from_milestone: v2
to_milestone: v3
```

The derivation uses UUIDv5 or an equivalent versioned hash over a canonical encoding with type tags and lengths. Inputs include a domain separator, identity canonicalization/algorithm version, from/to snapshot identities, target `feature_uid`, canonical change payload, and explicit result-affecting options. The general tool version is not included directly; the algorithm version changes only when UID semantics change. Identical inputs always yield the same UID and encoding-boundary collisions are impossible. Annotations, related events, and verification relations store `change_event_uid`. Historical and current display IDs remain as audit context.

### 6. Define copy, import, and repository-integration semantics

- Copy/import preserves a UID only when the same element continues.
- Importing as a distinct element issues a new UID and records a reissue event.
- Before integrating repositories in which different elements share a UID, one element is explicitly reissued.
- External-system mappings persist UID as canonical and retain ID only as a display snapshot.

## Migration

Implement in this order: (1) the shared Identity Module and crash-recovery mechanism, (2) an end-to-end vertical slice using Feature, (3) descriptors/Adapters for the other four kinds, (4) migration of every entity, and (5) the public schema-version-2 cutover. The Feature vertical slice is an internal stage for early validation of the shared design and Interface; do not publish or permanently support an intermediate format in which only Features use UID mode. Keep the existing schema version until public cutover, then switch all five kinds together.

A project-level marker is added to `.markharness/config.toml`:

```toml
[identity]
schema_version = 2
mode = "uid"
```

`markharness identity migrate` performs one crash-recoverable identity operation:

1. Issue UIDs for every Knowledge element.
2. Create initial issuance events and derive the non-committed Identity Registry cache.
3. Assign `case_uid` to existing TestCases.
4. Create legacy mappings for existing ChangeEvents and executions.
5. Update schemas, cache/index canonicalization versions, and the project marker.
6. Regenerate artifacts and run full validation.

Dry-run reports planned UIDs, conflicts, and changed files. A failure before the logical commit point does not enable UID mode; a failure after commit is completed by idempotent roll-forward at the next startup. Partial migration is never exposed to normal commands as a valid state.

A migration manifest maps legacy snapshot identity (tree SHA), entity kind, old ID, old path/content locator, and old case ID to new UIDs. Regardless of comparison direction, manifests present in both snapshots are collected symmetrically and resolved through snapshot-qualified keys. A missing mapping or multiple remaining candidates is a deterministic error. After UID migration, introducing a UID-less element makes ordinary commands fail and requires an explicit repair/import operation. Migration state is determined by the project marker, not by counting Features or UID fields.

## Validation rules

- UID syntax, uniqueness, immutability, prohibition on retired-UID reuse, and prohibition on assigning a historical ID to another UID unless it has been `release`d.
- Agreement between identity-event replay and Knowledge YAML; a Registry cache, when present, has a matching cache key and replay result, otherwise it is discarded and rebuilt.
- Exactly one corresponding rename event for every ID transition.
- Identity events form unambiguous causal chains and replay without contradiction within the selected snapshot; filenames and timestamps never determine order.
- For an entity UID present in both compared snapshots, the root issuance event and every shared event are identical.
- Agreement between a generated `case_uid` and the deterministic derivation from its provenance UID set.
- No new UID-less Knowledge, generated artifact, or execution in UID mode.
- No comparison across the migration boundary without a migration manifest.
- Core paths such as `changes compute` and `verify` depend only on the two refs' `.markharness` snapshots, explicit options, and canonicalization/tool versions.
- Only `IdentityAuditor` walks Git commit history to validate repository-wide append-only event history and detect deletion or rewriting outside the two selected snapshots.

## How this addresses 0004 and Issue #17

- `rename-id` plus a durable rename event makes an ID change explicit and auditable.
- `changes compute` resolves old and new IDs as one element through UID.
- TestCases, executions, ChangeEvents, and external mappings retain continuity through UID.
- Alias, cycle, and alias-reuse rules become unnecessary.
- Replay of retained identity events makes UID and old-ID reuse detectable after deletion; the Registry is only a rebuildable cache.
- The migration marker and manifest keep existing projects and historical artifacts readable.
- Separating identity declarations from derived content changes and generalizing the input scope to the `.markharness` snapshot guarantees identity while preserving deterministic two-ref recomputation. Ordinary content changes still require no editing-operation log; only identity intent that snapshots cannot infer is declared explicitly (paper §3.2–3.4).
- Feature splitting and merging require separate lifecycle/derivation events and remain out of scope.

## Conditions for acceptance

- Finalize JSON Schemas and locations for identity events and migration manifests, plus the cache format and key for the derived Registry.
- Finalize issuance roots, `previous_identity_event_uid`, resolution-event `previous_identity_event_uids`, branch divergence/conflict resolution, and canonical replay rules without timestamp or filesystem-order dependence.
- Finalize a crash-recovery protocol for multi-file identity operations as an independent design gate, including transaction intent, staging, one commit point, locking, flush/durability, disposal of uncommitted operations, idempotent post-commit roll-forward, blocking normal commands during recovery, and Windows/Unix guarantee differences.
- Detail the mutation plans submitted to that protocol by UID issuance, rename, retirement, restoration, release, reissue, and migration, including each operation's logical commit boundary.
- Add process-kill injection tests at every write and recovery stage, verifying that restart converges to the old or committed new state and that normal processing never observes an intermediate state.
- Validate legacy ChangeEvent, TestCase, and execution resolution with golden fixtures.
- Define the implementation order for migrating all consumers to UID and the removal condition for temporary compatibility adapters.
- Finalize `EntityKind`, `EntityUid`, `EntityId`, `IdentityHeader`, `IdentityMutation`, `IdentityEvent`, and the `IdentityEngine` Interface, and confirm in design review that issuance, rename, lifecycle, replay, and migration rules are not duplicated per entity kind.
- Restrict differences among entity kinds to `EntityDescriptor` or thin Adapters, and finalize a concrete test structure that applies the same identity contract suite to every kind.
- Finalize the generation path that makes Rust domain types the single source of truth for schemas, synchronization checks for distributed and init schemas, and the CI mechanism that rejects one-sided changes.
- Put the implementation order—shared foundation, Feature vertical slice, remaining four kinds, all-entity migration, and public schema-version-2 cutover—and the gate preventing publication of a Feature-only format into the implementation checklist.
- Before acceptance, verify that the Japanese and English papers remain synchronized with the finalized design: Git is the sole persistence boundary; Knowledge and identity events are canonical repository data; the Registry is a disposable cache; the implementation-status table distinguishes the current implementation from this Proposed design; and the identity-lifecycle causal graph remains distinct from any future persistent `derived_from` Version DAG. Separately finalize and apply the impact list for the CLI manual, schemas, and examples.
- Decide the execution conditions, authorization, and audit requirements for a `release` event.
- Decide domain separators, canonical encodings, and algorithm/version for `case_uid` and `change_event_uid`.
- Decide the full-history-audit module's interface, its separation from core paths such as `changes compute`, and how commands disclose the narrower two-snapshot audit boundary.

## Conditions for future reconsideration

- If Feature splitting/merging or one-to-many/many-to-one identity inheritance becomes necessary, add derivation relations to identity events.
