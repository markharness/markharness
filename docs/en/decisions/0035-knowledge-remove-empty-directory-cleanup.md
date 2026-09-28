# 0035: Clean up empty directories left behind by `knowledge remove`

## Status

Accepted (decided 2026-09-29). Not yet implemented; see `checklist-knowledge-remove-empty-dirs.md`.

## Context

[0034](0034-knowledge-remove-command.md) deletes Knowledge YAML files (`requirement.yml`, `feature.yml`, `behavior.yml`, `scenario.yml`) via `fs_safety::remove_file_no_follow`, which removes only the named file. It never removes the directory that file lived in, even when deleting it (or its cascade) leaves that directory with nothing left inside. A `knowledge remove` on a leaf Scenario, or a cascade that empties out an entire Behavior or Feature subtree, therefore leaves an increasingly deep trail of empty directories under `.markharness/knowledge/` that nothing else in the codebase ever cleans up: `identity::knowledge_walk::list_entities` and `generate::load_knowledge_snapshot` both only ever look for a marker file (`requirement.yml`/`feature.yml`/`behavior.yml`/`scenario.yml`), so an empty directory is invisible to every other command, but it is still real clutter a human or a GUI browsing the tree would see.

`markharness init` (`src/init.rs`) creates exactly six directories directly under `.markharness/`: `knowledge`, `axes`, `generated`, `executions`, `changes`, `schema`, and writes a `.gitkeep` into whichever of them (other than `schema`, which `ensure_default_schemas` always populates) would otherwise be empty, so Git can track them. It does not create `.markharness/knowledge/requirements/` or `.markharness/knowledge/features/` — those come into existence lazily, the first time something writes a Requirement or Feature under them. No code anywhere in the project currently detects or removes an empty directory.

## Decision

### 1. Clean up only the directories this `knowledge remove` invocation touched

Cleanup runs automatically, as part of every `knowledge remove` invocation, immediately after its file deletions and back-reference rewrites succeed. There is no separate `cleanup`/`prune` command and no flag to opt in or out. A general sweep of `.markharness/knowledge/` for empty directories left over from other causes (manual `rm`, a future GUI, interrupted operations from before this ADR existed) is out of scope; nothing currently produces such directories other than `knowledge remove` itself, so there is no concrete case to design a sweep against (YAGNI, per [CLAUDE.md](../../../CLAUDE.md) and the same reasoning [0021](0021-identity-retire-simplification.md) applied to identity retirement).

### 2. A directory is a cleanup candidate only when it is completely empty

"Empty" means no entries at all — not even other, still-non-empty subdirectories. A directory that still contains a subdirectory (empty or not) is left alone at this step; it becomes a candidate itself only once that subdirectory has already been removed. This keeps the rule simple (one `read_dir` check, no recursive "is this subtree entirely inert" computation) and correct: an entry that is itself removable will be removed on its own turn.

### 3. Removal cascades upward, stopping at a fixed set of protected roots

After a file's deletion, its parent directory is checked; if that directory is now completely empty, it is removed and its own parent is checked next, repeating until a non-empty directory is reached or a protected root is reached. Protected roots are never removed, even when completely empty:

- `.markharness/knowledge/` itself (created by `init`).
- `.markharness/knowledge/requirements/` and `.markharness/knowledge/features/` — the two collection roots. `init` does not create either (they come into existence lazily), but both are treated as permanent structural fixtures of a Knowledge tree rather than as ordinary content directories: removing, say, `requirements/` after the last Requirement is deleted would mean the next `knowledge reconcile` or `knowledge remove` recreates it from scratch, and — absent the `.gitkeep` mechanism `init` uses for the six top-level directories — Git would stop tracking an empty `requirements/` at all between the last deletion and the next creation. Treating both roots as permanent avoids that churn.

An individual element's own directory — `requirements/<id>/`, `features/<feature_id>/`, `features/<feature_id>/<behavior_id>/`, `features/<feature_id>/<behavior_id>/<scenario_id>/` — is not protected and is removed once empty. This is what actually cleans up after a deletion: removing a whole Feature, for instance, is expected to leave no trace of `features/<feature_id>/` at all once its `feature.yml` and every nested Behavior/Scenario directory are gone.

The protected-root check is a fixed list of paths built from `project_root::MARKHARNESS_DIR`, not a check for the presence of `.gitkeep`: a `.gitkeep` can be deleted by hand without that being a signal that the directory it was in stopped being structural, so relying on it here would be fragile in exactly the case this decision needs to be reliable.

### 4. Report every directory removed

`RemoveOutcome` gains a `removed_directories: Vec<String>` field (root-relative, forward-slash-normalized, like `deleted`/`detached`'s paths), populated with every directory this invocation actually removed, deepest first. `--json` includes it alongside `deleted`/`detached`; the human-readable presenter lists each one.

### 5. This cleanup step is best-effort, not part of the crash-recoverable batch

[0034](0034-knowledge-remove-command.md) §6 makes file deletion and back-reference rewriting part of `identity::recovery`'s crash-recoverable staging protocol, replayed identically by `feature_ops::roll_forward` on the happy path and during crash-recovery. This decision deliberately does not extend that protocol to directory cleanup: it runs as a plain step in `knowledge_remove::remove_element`, after the recoverable batch has already committed and been rolled forward, with no durable record of "cleanup is still pending" and no replay on a later command's startup recovery scan.

This is safe because a leftover empty directory carries no data and is invisible to every reader in the codebase (§ Context). A crash between the recoverable batch completing and this step running leaves, at worst, an empty directory that a later `knowledge remove` touching the same subtree will likely clean up as a side effect anyway, or that a human can remove by hand with no consequence. Extending the recoverable protocol to guarantee this step would add real complexity (a new payload shape, or deriving cleanup targets from `deletes` inside `feature_ops::roll_forward`, which already threads two other unrelated ADRs' payload variants through one function) for a guarantee whose absence nothing can ever observe.

## Invariants

- `.markharness/knowledge/`, `.markharness/knowledge/requirements/`, and `.markharness/knowledge/features/` are never removed by `knowledge remove`, regardless of how empty they become.
- A directory is only ever removed by this cleanup when, at the moment of the check, it has no entries at all.
- Every directory `knowledge remove` removes this way is reported in its outcome.

## Impact

- `src/fs_safety.rs`: new `remove_dir_if_empty_no_follow` primitive (no existing "remove only if empty" primitive exists; the module currently offers only unconditional single-file and recursive-unconditional-directory removal).
- `src/knowledge_remove.rs`: new upward-walking cleanup step in `remove_element`, and a new `removed_directories` field on `RemoveOutcome`.
- `src/cli.rs`: `report_remove_outcome` reports `removed_directories` in both human and `--json` output.

## Options considered and not taken

- **A general `knowledge cleanup`/`prune` command, mirroring `axes prune`'s report-only-by-default-plus---delete shape**: rejected for now. Nothing currently leaves behind an empty directory except `knowledge remove` itself, so a general sweep has no concrete case to clean up that this ADR's automatic, scoped cleanup does not already cover (YAGNI). Revisit if a concrete source of stray empty directories (e.g. a future GUI, or manual filesystem edits) surfaces.
- **Sweep the whole `.markharness/knowledge/` tree instead of only the directories this invocation touched**: rejected; broader scope than the concrete problem, and slower for no benefit given nothing else produces stray empty directories today.
- **Recurse into non-empty-looking directories to check whether every descendant is itself removable, removing a whole inert subtree in one pass**: rejected as unnecessary complexity; the upward walk already achieves the same end state one level at a time, correctly, as each level's own turn comes up.
- **Treat `.gitkeep` presence, rather than a fixed path list, as the signal for "this directory is protected"**: rejected; a `.gitkeep` can be deleted by hand or fail to exist for a directory this ADR still wants protected (`requirements/`/`features/`, which `init` never gives one), making it an unreliable boundary for a check whose whole job is reliability.
- **Extend the crash-recoverable batch to cover directory cleanup too**: rejected; see Decision §5. Nothing downstream can observe the difference between a directory removed immediately and one a crash left behind temporarily, so the extra machinery buys no observable guarantee.
