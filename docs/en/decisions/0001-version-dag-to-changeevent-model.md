# 0001: Scale back the Version DAG claim to "ChangeEvent model" wording (Option A)

## Status

Accepted

## Background

`docs/git-native-model-for-test-knowledge-management.md` positioned a persistent graph of version nodes/edges with `derived_from` (Version DAG) as a core contribution (§1.1 Figure 1, §1.2 RQ1, §3.4 Figure 3, etc.).

Meanwhile, in the CLI implementation the `ChangeEvent` struct (`src/changes.rs`) has no `derived_from` field; what is actually persisted is only `from_tree_sha`/`to_tree_sha` (a linear diff between two milestones) and `true_divergences` (audit information for two-parent merges). An independent persistent DAG with version nodes/edges is not implemented (P0 finding from the external evaluation review as of 2026-08-12).

To resolve this discrepancy, the following two options were considered.

- **Option A (adopted)**: Leave the already-implemented functionality untouched and scale back the paper's claims to match the implementation. Unify the wording "derived_from DAG" / "Version DAG" into "a ChangeEvent model derived from milestone intervals in Git history."
- **Option B (not adopted, held as future work)**: Actually implement a persistent graph structure with version nodes/edges, and bring the implementation in line with the paper's claims.

## Decision

Option A was adopted.

- Throughout the paper (§0, §1.1–1.3, §3.1–3.5, §4.1, §4.5, §7, §8), the wording "version history DAG" / "Version DAG" was replaced with "ChangeEvent model" wording that corresponds to the actual implementation (comparison of `from_tree_sha`/`to_tree_sha` at milestone boundaries, `true_divergences`).
- The term `derived_from` itself remains as a **conceptual name** referring to how a Feature changed between consecutive milestones. However, it was made explicit — in a note immediately after the ER diagram, in §3.2(B), and in the Figure 3 description (three locations) — that this is not persisted as a self-referencing edge on `FEATURE`, but rather a relationship derived on demand via tree SHA comparison of `ChangeEvent`.
- `forked_from` (a conceptual branch manually recorded by a human as domain knowledge that does not appear in Git history) is a different concept from `derived_from` (automatically derived version history), and was kept as out of scope for this revision.
- Figures 1 and 3 were redrawn as linear/divergent diff logs of ChangeEvent.
- Option B (extension to a persistent DAG) was not rejected but recorded as a one-sentence item of future work in §7 Future Work.

## Rationale

- Leaves already-implemented functionality (the current implementation's value) untouched, and eliminates the risk of claiming an unimplemented DAG as if it were implemented.
- Option B would require newly implementing a persistent graph structure with version nodes/edges, which exceeds the scope of this review response in both effort and design decisions.

## Impact / Conditions for future reconsideration

- Descriptions in §1.3 (Contributions) and the related-work comparison (improvement prompt items 2, 8) of `docs/git-native-model-for-test-knowledge-management.md` should be written on the premise of the "ChangeEvent model" terminology fixed by this decision.
- The `derived_from`-related description in `PROJECT.md` (improvement prompt item 5) should also be corrected in line with this decision to match the actual implementation (from_tree_sha/to_tree_sha/true_divergences).
- If a persistent graph structure with version nodes/edges (equivalent to Option B) is implemented in the future, a new decision record superseding this one should be created, and the paper's description should again be expanded to match the implementation.

## Addendum (2026-08-12): Response to improvement prompt item 11, the "merely a git diff/log wrapper" concern

### Background of item 11

As a result of Option A scaling back the Version DAG claim to the ChangeEvent model, the external review raised a new concern that "the paper appears to merely wrap git diff/git log." Upon checking the implementation (`src/id_cache.rs`), it was confirmed that an already-implemented algorithmic mechanism exists that differs from a simple path-based git diff/git log --follow.

### The three points selected as the core to emphasize

1. Path-independent ID resolution: because a Feature's id is read from the `id:` field of `feature.yml` rather than from a directory name or path, the same Feature can be tracked even after directory rename/relocation, without relying on Git's path tracking (`git log --follow`) (`id_cache.rs` lines 10–22, 84–97).
2. Directory-level tree SHA comparison: because the comparison target is the tree SHA of the entire Feature directory rather than the blob SHA of `feature.yml` itself, changes to only Condition/ExpectedResult can be detected without going through feature.yml.
3. A content-addressed id-resolution cache: the cache key consists of the tree SHA of `knowledge/` plus the versions of the rules/schema/tools, adopting the same design philosophy as Git's `commit-graph` auxiliary cache (§3.3).

All three of these points have been verified in the existing implementation (`src/id_cache.rs`); no new implementation was done.

### Chosen vocabulary and rationale

The phrase "theoretical core" was avoided in favor of "core design mechanism" / "algorithmic core." Reason: the ingenuity described in this section is a combination of known techniques — content addressing (Git's blob/tree SHA) and ancestor search via `git merge-base` — not a theoretical result accompanied by formal proof or complexity analysis. Calling it "theoretical" would reintroduce, under different vocabulary, the same kind of overstatement that was corrected for item 1 (Version DAG) and item 2 (asserting an existing TMS).

### Where reflected

The description in §1.3 (Contributions) was rewritten into the explicit form of the three points above, adding one sentence contrasting it with path-based history tracking (`git diff`/`git log --follow`). The same one-sentence contrast was also added to the introductions of §1.1 (Abstract-equivalent), §3.1 (Structure), and §3.3 (id resolution) respectively (the existing technical descriptions themselves were not changed).
