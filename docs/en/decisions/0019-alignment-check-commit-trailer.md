# 0019: Record spec/test-case alignment checks as a commit trailer

## Status

Accepted (design agreed, implementation pending). Based on the redesign grilling session (2026-09-11) for [markharness-v2-design.md](../design/markharness-v2-design.md).

## Context

When a developer changes a feature, a Requirement, or a test case, the team wants to know whether the other side followed up, or was reviewed and confirmed as not needing a change. Neither "both files changed in the same PR" nor "the re-run passed" is enough to establish that a human actually confirmed semantic alignment.

The earlier draft (old [markharness-v2-design.md](../design/markharness-v2-design.md) §6.7) introduced a dedicated `AlignmentObligation`/`AlignmentDecision` domain type carrying actor, rationale, and review_reference, persisted under Git management. The grilling session asked for this record to be "as lightweight as possible" — "recording it via a git comment or note would be enough."

`git notes` was considered and rejected, for these reasons:

- It is not included in `git push`/`git fetch` by default; without an explicit refspec it never reaches the rest of the team.
- It never shows up in GitHub's web UI, so it is invisible where PR review actually happens.
- It does not follow a commit across a rebase unless `notes.rewriteRef` is configured.

markharness itself already uses git notes for `backfill run`'s progress tracking, but that is internal state nobody needs to read directly — a different use case from a record the team is meant to see during review.

## Decision

### 1. Record the check as a commit trailer

A commit that includes a Requirement change or a test-case change carries a trailer stating the check's outcome:

```text
Spec-Reviewed: no-change-required
```

Write `no-change-required` when no change is needed. A trailer is only required to make explicit that someone looked and deliberately chose not to change anything.

When the corresponding edit is made in another commit, that edit only establishes that the other side *also changed* — not that a human checked semantic agreement (the premise this ADR starts from). The Core therefore reports **followed / confirmed / unconfirmed** as three distinct states and never equates a simultaneous update with a confirmation ([markharness-v2-design.md](../design/markharness-v2-design.md) §5.3).

A trailer's validity is scoped to the target's content as of the commit carrying it. If, within the same base/head range, the target's effective content (Case revision for a TestCase; `requirement.yml` or the `.sdoc` blob for a Requirement) changes again in a later commit, the confirmation becomes void and the item returns to "unconfirmed." Deciding this by commit order rather than by making authors write a revision into the trailer keeps the authoring burden low while preventing a stale confirmation from masking a newer change.

The trailer's value must **identify the element it refers to** (e.g. `Spec-Reviewed: no-change-required (req-login-01)`). When one commit touches several Requirements or TestCases, a trailer without a target makes it impossible to tell which alignment check was actually done, and the automatic check would drop a genuinely unconfirmed item.

The check scans every commit body in `git log base..head`. In a squash-merged PR the original trailer line can end up in the middle of the merge commit body, so an implementation that inspects only the final trailer line is not acceptable.

The exact key name, value vocabulary, and multi-entry notation are left to implementation design. This ADR fixes the storage location, the requirement that the target be identifiable, and the reasoning behind both — not the final syntax.

### 2. Assume the check happens alongside the change's own commit

The default workflow has the person making the change add the trailer to that same commit. Commit trailers get full push/fetch/GitHub-display support like any other commit text, but they cannot be appended after the fact (a new commit is required). Given that test-case edits tend to precede spec edits in practice (per the requirements session), this constraint is accepted for now.

### 3. No dedicated domain type or approval workflow

No `AlignmentObligation`/`AlignmentDecision`-style domain type, assignee, or approval-status transition is introduced. Core still computes when an alignment check is needed (automatic detection), but the confirmed record itself lives in Git's own commit history — no dedicated persistent store.

## Alternatives considered and rejected

- **git notes** — rejected for the reasons in Context.
- **PR description/comments** — a natural place during review, but reading it back mechanically from markharness requires an external API integration (GitHub, etc.), which conflicts with staying Git-only. Revisit via a separate ADR if external integration becomes necessary.
- **A dedicated file in the repository** (e.g. under `.markharness/alignment/`) — stays under Git management, but introduces a new file format/schema to manage, more complex than a commit trailer. Held in reserve as a fallback if "can't append after the fact" becomes a real operational problem.

## If the "can't append after the fact" constraint becomes a problem

If recording "a check noticed after the commit" turns out to be needed frequently, switching to a dedicated in-repo file becomes worth a separate ADR. As of this ADR there is no evidence that need has actually arisen, so a dedicated file format is not introduced preemptively (the YAGNI principle in [CLAUDE.md](../../../CLAUDE.md)).
