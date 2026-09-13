# 0028: Consolidate Knowledge authoring into `knowledge reconcile`

## Status

Accepted (decided 2026-09-13; implementation not started). Execute this decision after the "ADR 0028 start gate" in [0027](0027-declarative-knowledge-reconciliation.md) is satisfied.

## Context

[0027](0027-declarative-knowledge-reconciliation.md) consolidates parsing a Knowledge Intent, state-dependent validation, UID issuance, reference resolution, updates, renames, and atomic persistence behind the single `knowledge reconcile` Interface.

The current `knowledge add`, `knowledge scaffold`, `knowledge validate`, and `knowledge apply` commands divide the same Knowledge-authoring job into procedural stages for interactive input, an old KnowledgeDraft template, validation, and persistence. `feature rename-id` also describes the same state transition as a UID-selected rename in an Intent: select an existing Feature by UID and change its display ID.

Keeping both Interfaces would require maintaining two input schemas, two validation rule sets, two atomic write paths, and multiple recommended workflows. Following [0026](0026-module-inventory-and-plan-removal.md)'s no-backward-compatibility rule, no duplicate remains after its replacement is complete.

## Decision

### 1. Remove every duplicated authoring command

After the "ADR 0028 start gate" in [0027](0027-declarative-knowledge-reconciliation.md) is satisfied, remove all of the following in the same change:

- `markharness knowledge add`, including `--edit`
- `markharness knowledge scaffold`
- `markharness knowledge validate`
- `markharness knowledge apply`, including `--batch` and `--dry-run`
- `markharness feature rename-id`
- `markharness requirement link`
- `markharness requirement unlink`
- `markharness requirement repin`

Do not provide aliases, a deprecation period, compatibility wrappers, or hidden paths accepting old arguments. After removal, the Knowledge-authoring Interface is only:

```text
markharness knowledge reconcile <intent-file> [--check] [--json] [--dir <path>]
markharness knowledge reconcile --print-template
```

Multiple Knowledge elements belong in one Intent, so there is no separate equivalent of old `--batch`. `--check` subsumes old `knowledge validate` and `knowledge apply --dry-run`. `--print-template` replaces old `knowledge scaffold`.

### 2. Remove the old KnowledgeDraft implementation

Remove the KnowledgeDraft types, parser, validator, apply path, editor loop, template, reference schema, tests, and dedicated documentation used only by the old commands. With current filenames, candidates include at least the following, but the removal change verifies actual reachability by searching references:

- `src/knowledge_draft.rs`
- `src/knowledge_apply.rs`
- `src/knowledge_edit.rs`
- `docs/knowledge_draft.schema.json`
- unit tests, CLI integration tests, and example drafts that exercise only the old KnowledgeDraft

Domain validation, Knowledge parsers and serializers, filesystem safety, identity replay, and crash recovery needed by the Reconciliation Module remain. Do not preserve the old Module merely for the new Module to call; move required rules to the location matching their current responsibility, then delete the old Module.

### 3. Use the same Interface for human authoring

Do not retain interactive prompts or `$VISUAL`/`$EDITOR` launching as built-in Knowledge-authoring features. Humans obtain and edit an Intent template, then run `knowledge reconcile`. Launching an editor belongs to the shell or editor.

Feature rename also uses a Knowledge Intent containing the target UID and new display ID. No dedicated `feature rename-id` mutation path remains. The Reconciliation Module produces the same rename outcome while preserving the existing identity-event and crash-recovery invariants.

Adding or removing a Feature-to-Requirement relationship uses a patch that replaces the UID-selected Feature's `contributes_to` collection. Updating an external Requirement's pinned reference uses `source_revision: current` on a UID-selected Requirement. No dedicated mutation path remains for `requirement link`, `unlink`, or `repin`.

### 4. Keep identity maintenance and audit commands and Axis management

The following do not duplicate Knowledge authoring and remain:

- `identity migrate`: migration or repair of existing or manually introduced data
- `identity audit`: full-Git-history identity-event audit
- `identity resolve`: explicit resolution of branch divergence
- `identity sync`: disaster recovery that re-derives a Knowledge file from identity events
- `axes list`, `axes add`, and `axes prune`: Axis-registry operations managed outside the initial Knowledge Intent

`markharness validate` validates canonical Knowledge, Axes, and related persisted state as a whole. It is distinct from old `knowledge validate`, which validates an unpersisted Intent, and therefore remains.

### 5. Fix the implementation order

Implement in this order:

1. Implement [0027](0027-declarative-knowledge-reconciliation.md)'s Reconciliation Module, Knowledge Intent schema, CLI, and recovery tests.
2. Satisfy ADR 0027's "ADR 0028 start gate", verifying updates, renames, explicit Scenario reparenting, adding and removing Requirement relationships, repinning external Requirements, multiple elements, `--check`, and template output.
3. Switch README, AI-facing documentation, the Japanese and English CLI manuals, and examples to `knowledge reconcile`.
4. In one change, remove the old commands and old KnowledgeDraft implementation according to §1 and §2.
5. Run all tests, lint, formatting, license, generated-artifact self-verification, and confirm that no old command remains reachable through the CLI.

Do not remove old commands first and temporarily leave the product without an authoring path. Conversely, do not ship old and new paths together across multiple releases once the replacement is complete. [0027](0027-declarative-knowledge-reconciliation.md) makes completion of this removal an acceptance condition.

## Consequences

- Users learn one write Interface for Knowledge authoring.
- UID issuance, reference resolution, validation, rename, and atomic-persistence rules are localized in the Reconciliation Module.
- TTY interaction, the editor loop, one-chain Drafts, batch file-order dependencies, and post-apply migration disappear.
- External scripts using the old KnowledgeDraft stop working. This is an intentional breaking change with no compatibility path.
- Keeping maintenance commands such as `identity migrate` preserves repair and audit capabilities for existing repositories.

## Options considered and not taken

- **Keep old commands as deprecated**: this is kinder during migration but retains two schemas and mutation paths and works against converging on the best Interface.
- **Keep old commands as wrappers over the Reconciliation Module**: this reduces implementation duplication but preserves the old KnowledgeDraft constraints and multiple CLI surfaces.
- **Keep only `knowledge add --edit` for humans**: this creates separate human and AI authoring paths with divergent validation, errors, and retry behavior. Editing a template is sufficient.
- **Keep `feature rename-id` as a convenience shortcut**: the operation is concise, but it leaves a separate Interface and mutation path only for rename. A UID-bearing Intent is smaller overall.
- **Keep `requirement link`, `unlink`, and `repin` as convenience shortcuts**: replacing a relationship collection and advancing an external Requirement's pinned reference are Knowledge updates, so dedicated mutation paths contradict the single-Interface decision.
- **Fold identity maintenance commands into `reconcile`**: migration, history audit, branch-divergence resolution, and disaster recovery have different authority, inputs, and failure modes from desired-state authoring and do not belong in one Interface.
