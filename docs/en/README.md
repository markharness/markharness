# docs/en/ Document Index

Japanese version: [docs/ja/README.md](../ja/README.md)

Documents in this directory fall into four layers: "research design (the paper)", "product design", "CLI specification/manual", and "design decision records". The reading order and dependencies between documents are summarized below. Responses to external evaluation reviews are recorded as decisions under `decisions/` as they happen; once a response is complete, the review document itself is deleted (see "Cleanup log" below).

## Suggested reading order

1. **[A Git-Native Model for Test Knowledge Management: Integrated Edition](./git-native-model-for-test-knowledge-management.md)** — The research design (paper draft) that underlies the whole project. Every other document assumes this one. The Changelog section at the end summarizes responses to external evaluation reviews, with references into `decisions/`.
2. **[product-operation.md](./product-operation.md)** — Translates the paper's design into a product operation picture (UC1–UC8, actors, file creation order).
3. **[cli-manual.md](./cli-manual.md)** — List of implemented/unimplemented CLI commands. Correspondence with use cases refers to the UC numbers from item 2.
4. Detailed design of individual commands (referenced from cli-manual):
   - **[knowledge-apply-cli-spec.md](./design/knowledge-apply-cli-spec.md)** — Specification of `knowledge validate`/`apply` (non-interactive knowledge registration).
   - **[testcase-generation-design.md](./design/testcase-generation-design.md)** — Specification of `generate` (deterministic TestCase generation).
   - **[change-event-verification-tracking-spec.md](./design/change-event-verification-tracking-spec.md)** — Specification of `verify trace`/`verify pending` (automatic reconciliation of execution results and ChangeEvents).
   - **[verification-plan-canonical-model-design.md](./design/verification-plan-canonical-model-design.md)** — (Status: Proposed, not implemented) Canonical-model and pipeline design for PR Verification Plan generation. Corresponds to Stage 1–2 of the roadmap decided in [decisions/0008](./decisions/0008-verification-plan-product-roadmap.md).
5. **[gap-analysis-mh-sample-test-case.md](./gap-analysis-mh-sample-test-case.md)** — An investigation that verifies the gap between design and implementation against real data from the case-study operating repository `mh-sample-test-case` (including on-the-ground confirmation of tree-SHA-based detection, and branch/merge scenario verification). Treated as reference material / an audit log.
6. **[decisions/](./decisions/)** — Records of "why we decided this" — responses to external evaluation reviews, design trade-offs, etc. Reading in number order lets you follow the history. Managed as a single directory with a single sequential number space; each file's `## Status` section at the top expresses its lifecycle (Proposed/Accepted/Rejected/Deprecated/Superseded, or an in-progress state such as `Accepted (partially executed)`). Undecided or partially-unexecuted documents also live here rather than being moved to a separate directory (following Michael Nygard's "Documenting Architecture Decisions" and MADR practice; a previous separate `docs/internal-notes/` directory was discontinued because it fragmented the number space and caused path staleness, and was folded into `decisions/0007`). [decisions/0008](./decisions/0008-verification-plan-product-roadmap.md) (Status: Proposed) is the decision on a product roadmap centered on the PR Verification Plan, based on the review in `Markharness_改善・実装検討_統合設計文書.md`.

## On document freshness

- **git-native-model-for-test-knowledge-management.md** has a "Note (on implementation status)" and "§3.6 Implementation Status Summary" in its body, already noting known differences from the CLI implementation. See `gap-analysis-mh-sample-test-case.md` for a detailed cross-check.
- `cli-manual.md`, `knowledge-apply-cli-spec.md`, `testcase-generation-design.md`, and `change-event-verification-tracking-spec.md` each carry a "Status: Implemented" style status line and an "Additions/changes made during implementation" section, managing the diff between the initial draft and the implementation self-containedly within the document body.
- `gap-analysis-mh-sample-test-case.md` is a "snapshot at investigation time" and must be read while distinguishing the point-in-time findings from the current implementation state.

## File naming convention

All documents except the paper (`git-native-model-for-test-knowledge-management.md`) use English kebab-case (`foo-bar.md`).

## Cleanup log

Once a document's response to an external evaluation review is complete, the rationale is transcribed into `decisions/` or the paper's Changelog, and the document itself is deleted (recoverable via `git log -- docs/`). The same practice applies to documents whose purpose has been served, such as one-off bug-fix instruction sheets.

**2026-08-13(2)**:

- `nested-project-dir-git-path-fix-spec.md` — Fix instructions for a bug where `execution record` etc. failed when the project directory was a subdirectory of a git repository. Since the response is complete (fixed, tested, integration-verified), the design decision (formal support for subdirectory placement) and the key points of the response were transcribed into [decisions/0006](./decisions/0006-nested-project-directory-support.md) before deletion. References to it in `cli-manual.md` were also repointed to the same decision.

**2026-08-13**:

- `テスト知識管理のGit-nativeモデル_評価レビュー.md` — The body of the 2026-08-13 external evaluation review. The response policy for its findings was judged in `テスト知識管理のGit-nativeモデル_評価レビュー_有用性判定と修正指示.md`, and the results are reflected in the paper's Changelog and [decisions/0005](./decisions/0005-review-2026-08-13-triage.md), so it was deleted.
- `テスト知識管理のGit-nativeモデル_評価レビュー_有用性判定と修正指示.md` — The usefulness-judgment document for the above review. Judgment criteria and rejection reasons were transcribed into [decisions/0005](./decisions/0005-review-2026-08-13-triage.md), so it was deleted.
- `improvement-prompts.md` — A collection of execution prompts for responding to the 2026-08-12 review. Items 1–6 and 11 are reflected in [decisions/0001](./decisions/0001-version-dag-to-changeevent-model.md), [decisions/0002](./decisions/0002-changes-compute-historical-default.md), and the paper's Changelog; item 8 was addressed in [decisions/0003](./decisions/0003-related-work-gtm-tmt.md); items 9 and 10 were rejected in [decisions/0005](./decisions/0005-review-2026-08-13-triage.md); item 7 (importer, large-scale case study) was carried forward into the paper's Chapter 7 Future Work. Deleted accordingly.

**2026-08-12**:

- `review-data-model-improvement-proposals.md` — A review of an external data-model analysis report. The improvement proposals adopted were implemented and reflected in the paper via `improvement-prompts.md`, and it was concluded that no further paper revision was needed, so it was deleted.
- `gap-analysis-mm-folder.md` — The oldest gap-analysis document. Most of its findings had already been resolved via in-body "additions", and its content overlapped with the successor `gap-analysis-mh-sample-test-case.md`, so it was deleted.
