# 0027: Declarative Knowledge Reconciliation for AI authoring

## Status

Accepted (decided 2026-09-13; implementation not started).

## Context

The current path for creating Knowledge from an AI or script requires the caller to pass a `KnowledgeDraft` through `knowledge validate` and `knowledge apply`, then run `identity migrate`. Creating a new Requirement together with a Feature that references it additionally requires an earlier interactive operation or a temporary display-ID reference because the Requirement UID does not yet exist.

This path requires the caller to understand Knowledge persistence order, UID issuance timing, the distinction between display IDs and UIDs, and the ordering of `apply` and `migrate`. If migration is omitted or processing stops after a successful `apply`, UID-less Knowledge or a display ID in `requirement_uids` can become visible in the canonical storage area. That is inconsistent with [0013](0013-immutable-identity-model.md)'s invariant that ordinary commands do not introduce new UID-less Knowledge in UID mode.

AI authoring should declare which Knowledge is desired, rather than prescribe its persistence procedure. markharness must reconcile that declaration against the current repository state and complete UID issuance, reference resolution, validation, and atomic persistence itself.

## Decision

### 1. Make `knowledge reconcile` the standard AI-authoring path

Introduce this non-interactive command:

```text
markharness knowledge reconcile <intent-file> [--check] [--json] [--dir <path>]
```

Its input is called a **Knowledge Intent**. A Knowledge Intent is an authoring declaration, not the storage format under `.markharness/knowledge/`. It does not require callers to provide UIDs for new elements, storage paths, identity events, or migration steps.

For one Knowledge Intent, `knowledge reconcile` performs the following as one operation:

1. Parse the input and read the repository's current Knowledge and Axes.
2. Resolve document-local references, existing UIDs, and display IDs.
3. Validate the complete change, including domain rules and rules dependent on current state.
4. Reserve UIDs for every new Requirement, Feature, Behavior, and Scenario.
5. Convert the Feature's contributes-to relationships to Requirement UIDs.
6. Produce a mutation plan containing creations, updates, and no-ops.
7. Persist canonical Knowledge, identity events, and required derived state in one crash-recoverable transaction.
8. Confirm that the committed state satisfies every invariant and return a structured result.

Canonical storage is not changed before validation completes. A failure before the logical commit point converges to the old state; a failure after that point rolls forward idempotently to the committed new state. Ordinary commands never observe UID-less elements, unresolved references, or a one-sided update of Knowledge and identity events.

### 2. Use document-local keys to reference new elements within a Knowledge Intent

New elements in the same Intent refer to each other through document-local `key` values that are never persisted. For example, a Feature records its Requirement relationships in `contributes_to` using Requirement keys. Existing Requirements are referenced by UID; a display ID alone never selects an existing Requirement.

```yaml
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: req_todo
    id: todo
    source: native
    label: TODO management
    axis: [functional]

features:
  - key: feature_todo
    id: todo-management
    contributes_to: [req_todo]
    label: TODO management
    axis: [functional]
    behaviors:
      - key: behavior_add
        id: add-todo
        description: Add a TODO
        scenarios:
          - id: empty-title
            description: An empty title cannot be added
            phases:
              - steps:
                  - action: Attempt to add an empty title
                results:
                  - No TODO is added
```

`key` and `contributes_to` are authoring representations and are not persisted. In the canonical Feature, `requirement_uids` remains the sole source of truth for contributes-to relationships. No new persistent relationship type or generic graph is introduced.

### 3. Distinguish creation from changes to existing elements through UID

New elements omit UID. Only markharness issues UIDs.

A change to an existing element's content or display ID must identify that element by UID in the Knowledge Intent. Existing identity is never inferred or inherited from similar content or a matching display ID alone. This preserves [0017](0017-scenario-case-revision-and-execution-evidence.md)'s distinction between revision and creation and [0021](0021-identity-retire-simplification.md)'s identity rule for reintroduced elements.

Resolution follows these rules:

| Intent specification | Current state | Result |
| --- | --- | --- |
| No UID; no matching kind, scope, and ID | New | Issue a UID and create |
| No UID; matching kind, scope, and ID exists and normalized content is exactly equal | Existing | Report unchanged |
| No UID; matching kind, scope, and ID exists but content differs | Existing | Stop with `ambiguous_identity` and require UID |
| UID supplied; UID and kind agree, and current scope matches Intent scope | Existing | Compare content, then update or report unchanged |
| UID supplied; only the display ID differs | Existing | Process as an explicit rename |
| Scenario UID supplied under a different Feature or Behavior in the Intent | Existing | Process as an explicit reparent while preserving UID |
| UID is unknown, kind differs, or scope conflicts for an element other than Scenario | Inconsistent | Stop fail-closed |

Changing Scenario content or explicitly reparenting it by UID preserves its Scenario UID and recomputes Case revision from effective content. Reparenting occurs only when an existing Scenario UID is placed under a different Feature or Behavior in the Intent. A same-named Scenario without UID never implies a move. A Scenario newly produced by a split, merge, or copy receives a new UID, and reconciliation never infers a split or merge from content similarity.

### 4. Initially support only non-deleting `merge`

The initial version supports only `mode: merge`. It creates or updates elements named in the Intent and leaves every omitted existing element unchanged. Omission is never interpreted as an instruction to delete or retire.

An `exact` mode that retires omitted elements is not implemented until target scope, authorization and confirmation, and effects on derived Scenarios and TestCases are decided separately. `--delete` and `--allow-retire` are likewise outside this ADR's initial implementation scope.

Knowledge Intent may reference only registered Axes. Creating or deleting Axes from an Intent would mix Knowledge and Axis ownership, so the initial version does not do it. An unknown Axis stops with `unknown_axis`.

### 5. Apply patch semantics to UID-selected existing elements

For an existing element selected by UID, only fields present in the Intent change. Omitted scalars, value collections, and child Knowledge elements retain their current values. A supplied scalar replaces its value. An attempt to clear a required value with `null` or an empty value is rejected according to that field's domain rules.

Collections have two distinct categories:

- **Knowledge-element collections** contain Requirements, Features, Behaviors, or Scenarios. Elements named in the Intent are patched by UID or created by new key, while omitted existing elements remain. An empty collection does not delete or retire existing elements.
- **Value collections** are `axis`, `contributes_to`, `procedures`, `phases`, `steps`, and `results`. Supplying one replaces the whole collection. An empty collection explicitly clears it and differs from omission. The replacement must still satisfy that field's required, non-empty, and reference rules.

Omitting a Feature's `contributes_to` preserves its current `requirement_uids`; supplying it replaces the whole collection with the set resolved from document-local keys or Requirement UIDs. This expresses both addition and removal of relationships without separate commands.

Naming child Knowledge elements of an existing element patches or creates only those children. It never deletes siblings omitted under the same parent. `procedures` and `phases` are values owned by their Behavior or Scenario rather than independently UID-bearing Knowledge elements, so they follow value-collection replacement instead of this child-element rule.

To advance an external Requirement's pinned reference to the current `.sdoc` blob, a UID-selected Requirement explicitly supplies `source_revision: current`. The Reconciliation Module resolves and persists the blob OID addressed by `source_locator`; it stops fail-closed for a native Requirement, a missing locator, or an unresolvable Git state. `current` is an Intent-only instruction value and is never persisted in canonical Knowledge.

### 6. Use the same mutation plan for `--check` and execution

`--check` uses the same implementation as execution for parsing, resolution, validation, and mutation planning, but performs no writes. It succeeds when no change is required; when changes are required, it returns a dedicated machine-detectable exit code and the plan.

Execution rechecks current state immediately before commit and stops with a stale-plan result when state changed after `--check` or during planning. A `--check` result is not treated as authorization for a later write.

The same repository state and Intent produce the same mutation plan except for concrete UID values. Plans represent new UIDs as temporary tokens and assign them within the committing execution. When the same UID-less Intent is rerun after success, an element whose kind, scope, display ID, and normalized content are exactly equal is `unchanged`. An update with content differences or a rename must include the UID obtained from the first success result or a current machine-readable snapshot.

### 7. Return stable machine-readable outcomes and diagnostics

On success, `--json` returns at least `created`, `updated`, and `unchanged`, plus each element's kind, UID, display ID, and changed paths. A failure returns a stable `code`, the location in the Intent, a message, and a remediation where possible. Rust type names and internal error strings are not part of the external contract.

The initial diagnostics include at least:

- `invalid_format`
- `duplicate_key`
- `duplicate_uid`
- `unknown_local_reference`
- `unknown_axis`
- `ambiguous_identity`
- `unknown_uid`
- `conflicting_scope`
- `conflicting_existing_value`
- `invalid_procedure_reference`
- `invalid_source_revision`
- `stale_plan`
- `invariant_violation`

### 8. Do not make `identity migrate` part of authoring

When `knowledge reconcile` succeeds, every canonical Knowledge element it created has a UID and every UID reference is resolved. A subsequent `identity migrate` is not part of its success contract.

`identity migrate` remains available for migration or explicit repair of existing or manually introduced data. AI-facing documentation uses `knowledge reconcile` as the standard path and does not prescribe a procedural sequence of multiple commands.

Existing authoring commands whose responsibilities overlap `knowledge reconcile` are removed according to [0028](0028-consolidate-knowledge-authoring-commands.md), after this ADR's Reconciliation Module and replacement Interface are complete. No backward compatibility with the old paths is provided.

## Invariants

- Only new elements in an unpersisted Knowledge Intent may omit UID.
- Every committed Requirement, Feature, Behavior, and Scenario has a UID.
- `requirement_uids` stores only Requirement UIDs, never display IDs or Intent keys.
- Knowledge and identity events change as one logical commit.
- One Scenario equals one TestCase, and Case UID is derived deterministically from Scenario UID.
- The Feature remains the sole source of truth for contributes-to relationships to Requirements.
- Identity is never inferred from content similarity.
- Reconciliation never implicitly changes or retires elements outside the Intent or omitted from it.
- An external Requirement's body does not become canonical content owned by markharness.

## Impact

- Add an authoring-specific Knowledge Intent schema, clearly separated from the canonical Knowledge schemas.
- Consolidate current-state loading, reference resolution, validation, UID reservation, mutation planning, and crash recovery behind one Reconciliation Module.
- Keep the CLI as a thin Adapter over the Reconciliation Module so that a future GUI can use the same Interface.
- Treat the Module Interface as the primary test surface, covering a complete new graph, existing updates, rename, ambiguous identity, unknown Axis, reruns, write failure, and pre-commit/post-commit recovery.
- Update `docs/knowledge-from-code.ai.md` to make `knowledge reconcile` the standard workflow when implementation is complete.
- Do not weaken [0013](0013-immutable-identity-model.md)'s prohibition on UID-less Knowledge. If the implementation reuses or extends that ADR's crash-recovery protocol, it still must not expose intermediate state to ordinary commands.

## ADR 0028 start gate

All of the following functional-readiness conditions must hold before beginning the removals in [0028](0028-consolidate-knowledge-authoring-commands.md). This section is an intermediate gate for starting ADR 0028, not the acceptance conditions for ADR 0027 as a whole.

- Creating a complete new Knowledge graph, recognizing unchanged existing elements, UID-selected patches and renames, explicit Scenario reparenting, and multi-element operations work.
- Replacing `contributes_to` expresses both addition and removal of Requirement relationships.
- `source_revision: current` on an external Requirement performs the same blob-OID validation as existing `requirement repin`.
- `--check`, machine-readable outcomes, an Intent template, and crash-recoverable atomic persistence work.
- Intent represents every Knowledge value expressible through the current KnowledgeDraft without information loss.
- Tests of every replacement behavior pass while the old commands still exist.

## Acceptance conditions

This ADR is implemented completely when all of the following hold:

- `knowledge reconcile` supports creating a complete new Knowledge graph, recognizing unchanged existing elements, UID-selected updates and renames, multi-element operations, `--check`, machine-readable outcomes, an Intent template, and crash-recoverable atomic persistence.
- Knowledge Intent can represent, without information loss, every Requirement, Feature, Behavior procedure, and Scenario phase expressible by the current KnowledgeDraft.
- Pre-commit and post-commit/pre-roll-forward recovery tests prove that ordinary commands never observe UID-less Knowledge, unresolved references, or one-sided updates of Knowledge and identity events.
- [0028](0028-consolidate-knowledge-authoring-commands.md) is executed after this ADR's functionality is implemented, removing the duplicated legacy authoring commands and implementations.
- AI-facing documentation, the Japanese and English CLI manuals, README, schemas, and examples are updated to present only `knowledge reconcile` as the standard authoring path.

## Options considered and not taken

- **Run `identity migrate` once after `knowledge apply`**: this is a small change from the current implementation, but temporarily persists a display ID in `requirement_uids` and exposes UID-less canonical Knowledge and command ordering to the caller.
- **Extend only the internals of `knowledge apply`**: it could complete in one operation, but it would mix the current one-chain, create-or-reuse Draft contract with a declaration of the desired state of multiple elements. An explicit authoring-intent seam is preferable to silently changing the meaning of an existing command, and the duplicated old Interface is removed after migration completes.
- **Have AI write canonical Knowledge files directly, then normalize them**: this makes storage layout and internal reference representation part of the AI-facing Interface and first exposes invalid canonical data in the worktree.
- **Derive deterministic UIDs from content hashes**: this removes issuance but breaks continuity across content changes and renames.
- **Extend the interactive flow**: it can serve humans but leaves AI, CI, and scripts dependent on standard input, resumption state, and procedural error correction.
- **Provide exact synchronization and implicit retirement immediately**: it is a complete declarative-sync model, but a mistaken scope or AI omission becomes destructive. The initial version has no concrete deletion requirement that justifies that risk.
