# 0021: Drop the strict retire/restore/release guarantee and simplify retirement

## Status

Accepted (design agreed, implementation pending). Based on the redesign grilling session (2026-09-11) for [markharness-v2-design.md](../design/markharness-v2-design.md). Supersedes the retire, restore, release, and reissue portions of [0013](0013-immutable-identity-model.md) (explicit restore under the same UID, releasing a retired human-readable id). Everything else in [0013](0013-immutable-identity-model.md) — UID issuance, the id/UID split, preserving UID across rename — remains in effect.

## Context

[0013](0013-immutable-identity-model.md) made an append-only event log under `.markharness/identity-events/` the sole canonical source for identity lifecycle, implementing a strict retire/restore/release/reissue model (`src/identity/`, roughly 9,000 lines). This makes it possible to restore a retired element under the same UID, reassign a retired human-readable id to a different UID (release), and audit the consistency of both (`IdentityAuditor`).

Re-deriving the design from the North Star — deliberately without being anchored to the existing design or vocabulary, per the grilling session — led to this conclusion:

- Today's requirements (impact confirmation, missed-update detection, past execution scope, release-impact judgment) all deal with "the Features/TestCases that exist now and how they changed." No concrete scenario surfaced that needs strictly restoring identity after retirement.
- A plain "once retired, treat it as gone" rule still satisfies all four North Star answers.
- The cost of keeping the implemented strict guarantee (~9,000 lines, event replay, `IdentityAuditor`'s audit logic) has no concrete justification from the current requirements.

This follows the YAGNI principle in [CLAUDE.md](../../../CLAUDE.md) — "might need it someday" is not a reason to implement something.

## Decision

### 1. Retirement is treated as deletion

When a Feature or TestCase is removed from `knowledge/`, it is simply gone from the Knowledge tree. No UID-reuse prohibition or dedicated `retired` state transition is tracked.

### 2. Restoring under the same UID is not guaranteed

If a Feature/TestCase with the same content as a deleted one is added again, it is treated as a new, distinct element. No explicit `restore` operation carries the old UID forward. If a past relationship (e.g. `feature.requirement_uids`) is needed on the new element, a human sets it again on that element. There is no logic that carries the pre-deletion UID forward: content-match-based automatic restoration would contradict the "new, distinct element" rule stated above.

What this decision guarantees is **the creation side**: creating an element through the CLI issues a new UID, and matching content never implies the old UID. Restoring a deleted Knowledge file straight from Git history is different — the file's `uid:` comes back, so the original UID returns, and with it the Case UID deterministically derived from the Scenario UID ([0017](0017-scenario-case-revision-and-execution-evidence.md) §3). That is a Git history operation rather than a markharness restore feature; this ADR neither prevents nor detects it, and makes no stronger claim that re-adding the same content always yields a different UID. Preserving a UID across a rename (an id change without deletion) remains in effect per [0013](0013-immutable-identity-model.md) and is out of this ADR's scope.

### 3. The `release` event and id-reservation mechanism are dropped

Reassigning a retired element's human-readable id (`id:`) to a different element no longer requires an explicit `release` operation or reservation record. The UID-issuance rules from [0013](0013-immutable-identity-model.md) governing id collisions still apply, but no dedicated logic decides whether a retired id may be reused.

### 4. The append-only event model under `.markharness/identity-events/` shrinks to UID issuance and rename

The only identity-lifecycle events recorded are new UID issuance and rename (preserving UID across an id change). The retire/restore/release/reissue event kinds, and the corresponding replay logic in `IdentityAuditor`, are not implemented.

## Impact

- The retire/restore/release/reissue portions of `src/identity/recovery.rs` (742 lines) and `src/identity/audit.rs` (816 lines) become candidates for reduction under this decision (tracked separately as implementation work). The UID-issuance/rename/migration portions remain.
- [0013](0013-immutable-identity-model.md)'s Status section is updated to point here. The ADR body itself is left unchanged as a historical decision record, per the ADR-management policy in [release-and-license instructions](../../../.github/instructions/release-and-license.instructions.md).

## Alternatives considered and rejected

- **Keep the existing strict guarantee as-is** — a safe choice in the sense of not discarding a working, implemented asset, but there is no way to justify it against today's North Star, leaving only code complexity and maintenance cost behind. Better to wait for a concrete restore/id-reuse need to surface, then design against that need's actual shape (the same reasoning [0004](0004-feature-id-change-migration.md) applied to a similar situation).
- **Keep retire/restore but drop release/reissue only** — considered as a middle ground, but since "identity after retirement" never came up as a scenario in the first place, there is no strong reason to keep part of it. Simplifying it all at once, and reintroducing individual pieces only when a concrete need appears, keeps the decision clean.
