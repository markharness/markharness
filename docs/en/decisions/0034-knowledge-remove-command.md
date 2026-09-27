# 0034: `knowledge remove` for physical deletion of Knowledge elements

## Status

Accepted (decided 2026-09-27; implemented 2026-09-27). The `knowledge remove` subcommand itself, `identity::recovery`/`feature_ops::roll_forward`'s crash-recoverable delete extension, a regression test proving `generate` already handles it with no extra code, and regression tests covering path-traversal and symlink-ancestor attempts are all done. See `checklist-knowledge-remove.md` for details.

## Context

[0027](0027-declarative-knowledge-reconciliation.md) §4 deliberately deferred deletion: `knowledge reconcile` supports only non-deleting `merge`, and an `exact` mode or `--delete`/`--allow-retire` flags were left unimplemented because "a mistaken scope or AI omission becomes destructive" and "the initial version has no concrete deletion requirement that justifies that risk."

That concrete requirement now exists: authors need to remove a mistaken or obsolete Requirement, Feature, Behavior, or Scenario (and, since [0017](0017-scenario-case-revision-and-execution-evidence.md) §3 makes one Scenario equal to one TestCase, this is also how a TestCase is removed). [0021](0021-identity-retire-simplification.md) already settled the underlying semantics — "When a Feature or TestCase is removed from `knowledge/`, it is simply gone from the Knowledge tree. No UID-reuse prohibition or dedicated `retired` state transition is tracked" — so this ADR does not need to invent a retire/tombstone model; it only needs to decide how deletion is triggered, scoped, and made crash-safe.

The Knowledge tree has two distinct reference shapes that behave differently under deletion:

- **Optional many-to-many references** — `Feature.requirement_uids` and `Scenario.requirement_uids` ([0031](0031-scenario-level-requirement-contribution.md)) — are a value collection ([0027](0027-declarative-knowledge-reconciliation.md) §5) that can simply omit the deleted UID.
- **Mandatory single-parent references** — `Behavior.feature` and `Scenario.behavior` (`src/knowledge.rs`) — are required `String` fields with no representable "no parent" state. A parent's deletion cannot leave these dangling.

Requirement and Feature `id` values are unique across the whole project (`.markharness/knowledge/requirements/<id>/`, `.markharness/knowledge/features/<id>/`), but Behavior and Scenario `id` values are unique only within their parent (`features/<feature_id>/<behavior_id>/`, `features/<feature_id>/<behavior_id>/<scenario_id>/`), so the same slug can legitimately exist under two different parents.

Existing multi-file Knowledge mutations ([0027](0027-declarative-knowledge-reconciliation.md)) are not written directly to the working tree. They go through `identity::recovery`'s staging protocol (`begin_batch_with_payload` → `commit_batch` → `feature_ops::roll_forward` → `finish`), so a crash between writing one file and another always converges to either the pre-mutation or the post-mutation state, never a mix. `identity::recovery::PendingKnowledgeFile` currently represents only "write this file's full contents"; it has no way to represent "this file no longer exists."

## Decision

### 1. Add `markharness knowledge remove <type> <key>` as its own subcommand

```text
markharness knowledge remove <requirement|feature|behavior|scenario> <key>
    [--feature <id>] [--behavior <id>] [--dir <path>] [--json]
```

This is a new `KnowledgeCommand::Remove` variant alongside `Reconcile` (`src/cli.rs`), not an `exact`/`--delete` mode of `reconcile`. A single-item, explicitly ID-targeted removal does not carry the "mistaken scope or AI omission" risk ADR 0027 §4 flagged against implicit, omission-based retirement of a whole plan; it is a separate, narrower operation and gets a separate Interface. `reconcile`'s `merge`-only, non-deleting contract is unchanged.

No confirmation prompt is added, matching the CLI's existing non-interactive convention ([0028](0028-consolidate-knowledge-authoring-commands.md) §3) and `axes prune`'s explicit-flag-required (not prompted) precedent. Because the operation is always a single explicitly named element, it executes immediately.

### 2. Resolve `<key>` as a slug first, falling back to disambiguation

- `requirement` and `feature`: `<key>` is always the project-unique display `id`. No ambiguity is possible.
- `behavior` and `scenario`: `<key>` is the display `id`. If it matches exactly one element, that element is the target. If it matches more than one (same slug under different parents), the command fails with a diagnostic listing every match's full parent path and stops without deleting anything.
- To disambiguate, callers pass either `--feature <id>` (`behavior`; also required together with `--behavior <id>` for `scenario`) to scope the search to one parent, or `<key>` may itself be a `uid` (the immutable identifier from [0013](0013-immutable-identity-model.md)) when one is known. `uid` is not always available (a project that has not run `identity migrate` may have Behaviors/Scenarios with `uid: None`), which is exactly why the parent-path form (`--feature`/`--behavior`) exists as a second, always-available disambiguation path rather than relying on `uid` alone.

Resolution reuses `identity::knowledge_walk::find_by_id`/`find_by_uid` (`src/identity/knowledge_walk.rs`) rather than a new directory walker.

### 3. Cascade-delete mandatory-parent children; detach optional back-references

- Deleting a Requirement cascades to every Feature and Scenario whose `requirement_uids` contains its UID — not by deleting those Features/Scenarios, but by rewriting `requirement_uids` to omit it (value-collection replacement, [0027](0027-declarative-knowledge-reconciliation.md) §5). The Feature or Scenario itself remains.
- Deleting a Feature physically deletes every Behavior whose `feature` names it, and deleting a Behavior physically deletes every Scenario whose `behavior` names it — recursively, since `Behavior.feature`/`Scenario.behavior` are required fields with no representable "no parent" value. Deleting a Requirement, Feature, or Behavior therefore always removes the whole subtree rooted at it: Requirement → Feature/Behavior/Scenario, Feature → Behavior/Scenario, Behavior → Scenario.
- Deleting a Feature or Behavior does not require pre-emptively deleting its children by hand; the command computes and deletes the full cascade in one operation.

### 4. Scope this operation to canonical Knowledge files only

`knowledge remove` deletes only files under `.markharness/knowledge/`. It does not touch:

- `generated/testcases/` — see §5.
- `.markharness/case-definitions/` (append-only execution-evidence records, [0017](0017-scenario-case-revision-and-execution-evidence.md)), execution bindings, or release-scope selections that may reference a deleted Scenario's Case UID. These become dangling references. Detecting or warning about them is explicitly out of scope for this ADR; a future ADR may address it if a concrete need surfaces, matching the YAGNI stance [0021](0021-identity-retire-simplification.md) already took for the identity model generally.

### 5. `generate` needs no new pruning logic

`generate::load_knowledge_snapshot` walks `.markharness/knowledge/` fresh on every call and builds one `TestCase` per Scenario file it finds; `application::generate_testcases` fully replaces the `generated/` directory from that snapshot on every run ([0017](0017-scenario-case-revision-and-execution-evidence.md) §3: one Scenario equals one TestCase). Because of this, a Scenario deleted by `knowledge remove` — directly or by cascade — is already absent from the next `generate` run's output with no additional pruning code. This ADR adds only a regression test proving that existing behavior, not new production code, honoring [CLAUDE.md](../../../CLAUDE.md)'s YAGNI rule.

### 6. Extend `identity::recovery`'s staging protocol to represent deletion

Add a `PendingKnowledgeDelete { relative_path: String }` alongside `PendingKnowledgeFile`, and a new `IntentPayload::KnowledgeRemove { deletes: Vec<PendingKnowledgeDelete>, files: Vec<PendingKnowledgeFile> }` variant (`identity::recovery`) — `deletes` for cascade-removed element files/directories, `files` for back-reference rewrites, mirroring `IntentPayload::KnowledgeReconcile { files, moves }`'s existing shape.

Per [0021](0021-identity-retire-simplification.md) §4, deletion issues no identity event (only UID issuance and rename are tracked). `knowledge remove` therefore calls `begin_batch_with_payload(root, Vec::new(), Some(payload))` with an empty `batch_events`, exactly like a content-only reconcile patch; its logical commit point is `commit_batch`'s `commit_marker_path` write, not an event file.

Extend `identity::feature_ops::roll_forward` — the single dispatcher every caller of `run_startup_recovery` already uses — with a match arm for `IntentPayload::KnowledgeRemove` that idempotently removes each `deletes` path (already-removed is not an error, matching the existing move-replay precedent for the same reason) and `replace_file`s each `files` entry. Because this is the same dispatcher every existing call site already passes to `run_startup_recovery`, no call site other than the new `knowledge_remove` module itself needs to change.

This makes `knowledge remove`'s cascade delete plus back-reference rewrite crash-recoverable exactly like `knowledge reconcile`'s multi-file writes: a crash between removing one file and rewriting another converges to the fully-applied state on the next startup recovery scan, never a partially-applied one.

## Invariants

- A canonical Knowledge element is either present in full (with a valid parent chain, for Behavior/Scenario) or entirely absent; `knowledge remove` never leaves a required parent reference dangling.
- `requirement_uids` never names a UID for a Requirement that no longer exists once a `knowledge remove` operation (and any crash recovery it required) has completed.
- Deleting a Requirement, Feature, or Behavior removes its entire cascade (all descendants whose existence depends on it) as one crash-recoverable operation, never partially.
- `knowledge remove` never issues an identity event ([0021](0021-identity-retire-simplification.md) §4 unchanged).
- `.markharness/case-definitions/`, execution bindings, and release-scope selections are never rewritten or deleted by `knowledge remove`.

## Impact

- `src/identity/recovery.rs`: new `PendingKnowledgeDelete` type and `IntentPayload::KnowledgeRemove` variant.
- `src/identity/feature_ops.rs`: new match arm in `roll_forward` for the variant above.
- New `src/knowledge_remove.rs` (or `src/knowledge_reconcile/remove.rs`): resolution (slug/uid/parent-path, ambiguity diagnostics), cascade computation, back-reference detachment, and the lock/recovery/commit sequence, following `knowledge_reconcile::execute::reconcile_creation`'s existing shape.
- `src/cli.rs`: new `KnowledgeCommand::Remove` variant and dispatch arm.
- A regression test in `generate.rs`/`application.rs`'s test suite proving deleted-Scenario pruning already works, with no production code change there.
- AI-facing documentation (`docs/knowledge-from-code.ai.md`) gains `knowledge remove` alongside `knowledge reconcile` as a standard-path command.

## Options considered and not taken

- **Implement as `reconcile --delete`/`exact` mode, per ADR 0027 §4's original plan**: rejected. `exact` mode's danger is specifically implicit, omission-based retirement of everything not named in a large declarative Intent; a single explicitly ID-targeted removal does not share that failure mode and does not need the same authorization ceremony `exact` would need. A separate, narrower command is simpler to reason about and to test.
- **Require `uid` for Behavior/Scenario removal, erroring when absent**: rejected; it would make removal impossible in a project that has not run `identity migrate`, for no safety benefit over the always-available parent-path (`--feature`/`--behavior`) alternative.
- **Direct filesystem operations (no staging/recovery) for deletion**: rejected. Cascade delete plus back-reference rewriting is the same multi-file-mutation shape `knowledge reconcile` already treats as crash-recoverable; accepting a smaller guarantee here for a structurally identical failure mode would be inconsistent for no clear benefit.
- **Cascade-delete case-definitions/bindings/release-scope selections along with their Scenario**: rejected for now; [0017](0017-scenario-case-revision-and-execution-evidence.md) treats them as append-only historical records, and no concrete need for touching them has surfaced yet (YAGNI, per [0021](0021-identity-retire-simplification.md)'s precedent). Left as a documented dangling-reference possibility for a future ADR.
- **Add an explicit `--force`/confirmation flag**: rejected; the command is already scoped to one explicitly named element, so there is no "large blast radius from omission" risk that a confirmation step would meaningfully guard against, and the CLI has no other interactive confirmation precedent to match.
