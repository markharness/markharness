# Repository instructions

Before any task that may edit files, create commits, push, or open a pull request, read and follow [GitHub Flow](.github/instructions/github-flow.instructions.md). Work on a short-lived branch rather than `main`; push, pull-request creation, and merge require explicit user authorization.

Follow [CONTRIBUTING.md](CONTRIBUTING.md) for TDD, verification, documentation, licensing, and commit conventions.

Before reviewing code, a branch, a diff, a pull request, or an implementation against a checklist/specification, read and follow [Review Policy](docs/review-policy.md). Apply its decision axes and disposition labels to every finding, and treat its documented accepted risks as non-findings unless the reviewed change expands or invalidates the acceptance conditions.

Scale review verification to the changed behavior and risk. A reviewer need not rerun every Pre-PR check on every review round: use focused tests for the affected paths, rely on passing required CI checks when available, and reserve the full local suite for an initial review, broad/high-risk code changes, or evidence that wider regression coverage is needed. For documentation-only corrections, verify the changed text, links, bilingual synchronization, and agreement with the implementation; do not run the test suite unless the documentation change affects generated artifacts or executable examples. On re-review, verify the corrective diff and the previously failing behavior rather than mechanically repeating unrelated checks. Report which checks were run and which evidence was relied on.

Preserve the chronological review record on pull requests. Unless the user explicitly asks to edit or replace an existing comment, post each review, re-review, resolution, or changed disposition as a new comment that references the earlier conclusion. Edit an existing comment only to correct that comment itself, such as a typo, broken link, exposed secret, or factual mistake discovered immediately after posting; for a later change in evidence or judgment, add a new comment instead of rewriting history.
