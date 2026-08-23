# 0004: Migration policy when a Feature's `id:` changes

## Status

Accepted (reconsideration proposed in [0013](./0013-immutable-identity-model.md))

## Background

The external evaluation review (`テスト知識管理のGit-nativeモデル_評価レビュー_有用性判定と修正指示.md`, P2) pointed out that the constraint whereby changing the `id:` of `feature.yml` breaks tracking of it as the same Feature — while a known fact of the design — was not documented for users.

The current implementation (`src/id_cache.rs`) can follow directory **renames** because it reads a Feature's id from the `id:` field of `feature.yml` (paper §3.3). However, if the value of the `id:` field itself is rewritten, `changes compute` (paper §3.2–3.4) treats it as if the Feature with the old id was deleted and a Feature with the new id was added, with no means of linking the two as the same Feature. Version history (`derived_from`) is severed at that point.

## Options considered

1. **Implement a migration command** (e.g., a `markharness feature rename-id <old> <new>` command that retroactively rewrites `changes/*.yaml` across past milestones, or adding a mapping file such as `id_aliases`).
2. **Add an explicit alias mechanism to the design** (have `feature.yml` hold `aliases: [old-id, ...]`, so that `changes compute` can also follow the history of the old id).
3. **Implement nothing, and merely document the constraint for users**.

## Decision

Option 3 was adopted. Neither a migration command nor an alias mechanism will be implemented.

**Rationale**:

- To date, this constraint has not been an obstacle in actual operation (changing an id is itself a rare operation, and in the structured path management of Feature directories, a rename is usually sufficient in most cases).
- Building an alias mechanism into the design would compromise the simplicity of the "just compare two tree SHAs" model of `changes compute` (a core design advantage described in paper §3.2–3.4). The id-resolution path would branch into multiple routes, adding exception paths to both the cache key (§3.3) and the `true_divergences` determination (§3.4).
- Even judged against this project's policy (decide based on usefulness; the amount of implementation cost is not a factor in the decision), preemptively building "a countermeasure to a problem that has not occurred" into the design cannot be justified from a usefulness standpoint. It is more appropriate to design once an actual need from users to track id changes arises and its shape (a one-off rename, or frequent reorganization) can be observed.

## Response taken

- §1.10 of `docs/cli-manual.md` now explicitly states that version history is severed when `id:` changes, and that no migration procedure currently exists (addressing the risk that a user might mistakenly rewrite `id:` and continue using it without noticing the break in history).

## Conditions for future reconsideration

- If a concrete request emerges from users to keep tracking history even after an `id:` change, options 1 and 2 above should be reconsidered.
