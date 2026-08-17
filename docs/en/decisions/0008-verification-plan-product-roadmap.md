# 0008: Redefine the product roadmap around the PR Verification Plan

## Status

Proposed (not yet started. This ADR fixes the direction; implementation proceeds in stages per the roadmap in this document).

## Context

`docs/Markharness_改善・実装検討_統合設計文書.md` (dated 2026-08-17, hereafter "the integrated design document"; the file itself has been deleted, since its adoption decisions were transcribed into this ADR and into [verification-plan-canonical-model-design.md](../design/verification-plan-canonical-model-design.md) — recoverable via `git log -- docs/`) examined three directions for extending the current markharness design — a Git-native model that treats test knowledge as the primary artifact and TestCase as a derived artifact, as already implemented per Section 2.8 and Chapter 3 of the paper.

1. A canonical import/normalization foundation accepting Doorstop, StrictDoc, TestRail, Gherkin/Cucumber, Playwright, JUnit, and similar tools as input sources.
2. A feature that generates a Verification Plan — covering both impacted existing tests and newly required tests — from a PR's code/spec/knowledge diff.
3. A GUI (Release Verification Dashboard, Feature History, etc.) that visualizes Markharness's model-specific concepts.

The current markharness CLI already implements, within a single Git repository's `knowledge/`, automatic milestone-boundary `ChangeEvent` computation (`changes compute`) and version-binding of execution evidence (`verify trace`/`verify pending`) (Chapter 3 of the paper `git-native-model-for-test-knowledge-management.md`). On the other hand, PR-scoped (arbitrary base/head) Verification Plan generation, import from external tools, and a GUI are all unstarted.

The primary risk the integrated design document identifies is that pursuing these three directions in parallel without discipline would lead to: (a) the import/normalize feature becoming the public face of the product, making it look like it has shrunk into a "version-aware middleware"; (b) re-implementing TestCase CRUD; and (c) the GUI acquiring its own status model that diverges from the Domain Engine. This ADR decides the order and boundaries in which these three directions are pursued.

## Decision

### 1. Fix the product vision to the following single sentence

> Markharness turns a change into a reviewable verification plan. Git remains the source of truth.

The competitive axis is not the number of features such as dashboards, RBAC, or SSO, but the speed and quality of answering "for this change, what is sufficient to test?" This sentence is treated as the primary source of truth to reference when updating external-facing copy such as the README.

### 2. Fix the order of work to Stage 0 through Stage 3

Extend the existing `changes compute` (milestone-boundary) so that an arbitrary PR base/head pair is treated as a first-class version range, proceeding in the following order. The rationale for this ordering is the dependency that a stable canonical model is the input the Plan needs, and the Plan is what makes the GUI a meaningful read model (Chapter 8 of the integrated design document).

| Stage | Scope | Exit Criteria |
|---|---|---|
| Stage 0 | Fix the current domain model and terminology in an ADR/schema document. Turn a fixture repository into a golden dataset. Define a versioning policy for the CLI JSON contract. | The same canonical snapshot, change, and plan status can be regenerated from the same fixture. |
| Stage 1 | Canonical artifact/version/relation/evidence schema. Markharness native importer and JUnit evidence importer. Distinguish stored/derived trace origin. `import --format json`. | Version-aware plan status can be reproduced in CI from native knowledge and JUnit results. |
| Stage 2 | Base/head diff collection. Affected-existing-tests via stored/derived trace. Rule-based missing-test inspection. Optional AI proposal adapter. `markharness plan --base --head --format json`. | Evaluation results (precision/recall, etc.) comparing the plan against human-selected plans are obtained on a historical PR dataset. |
| Stage 3 | Read-only Release Verification Dashboard and Feature History via `markharness serve`. | The target users can explain a release's remaining verification work faster than with CLI/files alone. |

Stage 4 (external import expansion, PR check/comment integration) and Stage 5 (collaborative SaaS) are conditional, premised on the completion of Stage 0–3 and observed usage; this ADR does not fix their order of work (Section 4).

### 3. Boundaries (making explicit what will not be done)

The following are recorded as boundaries the product will not put at the forefront. If, during implementation, a proposal arises that would cross one of these boundaries, its merit is judged in a new ADR.

- **Do not make individual TestCase CRUD the primary UI**: TestCase is treated only as an exceptional edit of a derived artifact (consistent with, and unchanged from, the current paper model).
- **Do not make canonical import the star of the product**: Import/normalize stays an internal foundation and plugin boundary; user-facing messaging always states "keep your existing test assets and build a per-PR Verification Plan."
- **Do not auto-commit AI proposals**: AI is used only to generate candidate behavior changes, missing tests, and obsolete tests; reflecting them into canonical YAML/files always requires human review and a Git diff.
- **Do not make a GUI-only DB the source of truth**: The GUI (Stage 3) is a viewer/editor for the Git repository; even if it keeps a search index or cache, that state must be reconstructible from Git.

### 4. Stage 4 and Stage 5 are conditional

Work on Stage 5 (collaborative SaaS: RBAC, SSO, a shared DB, etc.) is considered only once the following conditions are confirmed. It is not started as of this ADR.

- Sustained demand for a shared dashboard/assignment feature from multiple teams.
- A collaboration problem clearly exists that Git/CI integration alone cannot solve.
- A design can be maintained in which canonical state can still be exported to Git even without hosted metadata.

Stage 4 (an importer from an existing TMS such as TestRail, GitHub/GitLab PR check integration) is started after the Stage 2 Verification Plan PoC shows practically useful accuracy (precision/recall). The TestRail importer is deferred until demand is confirmed, and the Stage 1 importer rollout order is "Markharness native → JUnit XML → Gherkin → Playwright → Doorstop/StrictDoc → TestRail" (file-based sources first, with SaaS-API-specific authentication, pagination, and rate-limit handling deferred to later stages; Section 3.8 of the integrated design document).

### 5. Treatment of existing functionality

Based on the proposals in Section 9 of the integrated design document, the following maintain/reduce policy is adopted.

**Maintain and strengthen**: structured test knowledge, deterministic TestCase generation, Feature version via Git tree SHA, milestone/snapshot diff, change-to-affected-TestCase derivation, version-binding of execution evidence, derived pending/re-verification, the file/CLI workflow. All of these are core to the current implementation (Chapter 3 of the paper) and remain unchanged.

**Reduce and redesign**: Generalize the milestone-only UX into a common version range that adds PR base/head as first-class (undertaken in Stage 2). Do not rely solely on human-oriented text output; give stable JSON/schema equal or greater weight (undertaken in Stage 0–1). Consolidate the simple PASS/FAIL display so that it uses, as tracking input, the valid/stale/unknown classification already provided by `verify trace`/`verify pending` (Section 3.7 of the paper).

## Consequences

- Making the roadmap's priority order explicit makes it structurally easier to avoid the risk the integrated design document flags, of over-weighting the import/normalize feature.
- Because Stage 0 fixes the current domain model in an ADR/schema document, the subsequent Stage 1–3 implementations can each be checked for consistency with the current paper model (`ChangeEvent`, `verified_feature_tree_shas`, etc.).
- Setting explicit entry conditions for Stage 4 and 5 prevents premature investment in a collaborative SaaS that would "undermine the advantage of being Git-native."
- This ADR fixes only the scope and order of Stage 0–3; the detailed canonical schema, Verification Plan JSON contract, and bounded components for each stage are left to a separate design document ([verification-plan-canonical-model-design.md](../design/verification-plan-canonical-model-design.md)).

## Options considered but not adopted

- **Starting with the GUI (Stage 3) first**: Visualizing Markharness's model-specific concepts is compelling, but building a GUI before the Plan's JSON contract is stable carries a high risk that the GUI acquires its own status model that diverges from the Domain Engine (Section 5.7 of the integrated design document). The canonical model is stabilized, then the Plan, before starting the GUI.
- **Implementing an importer for a major TMS such as TestRail as the top priority**: Demand for this is clear, but SaaS-API authentication, pagination, and rate-limit handling cost more to implement than file-based importers (native/JUnit/Gherkin), and would slow down Stage 1's validation speed. The canonical schema is validated with file-based importers first.
- **Making AI proposals the default information source for the Verification Plan early on**: This would directly improve missing-test discovery accuracy, but making AI the default before there is a comparative evaluation against the baselines (stored trace, derived trace, rule-based gap analysis) would undermine the Plan's reproducibility and explainability. In Stage 2, AI is added as an optional variant, and its differential effect against the baseline is measured.
