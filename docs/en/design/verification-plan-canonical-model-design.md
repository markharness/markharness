# PR Verification Plan: Canonical Model and Generation Pipeline Design

**Status**: Proposed (not implemented. A design proposal corresponding to the Stage 1–2 scope decided in [decisions/0008](../decisions/0008-verification-plan-product-roadmap.md))
**Related documents**: [git-native-model-for-test-knowledge-management.md](../git-native-model-for-test-knowledge-management.md) (hereafter "the paper"), [decisions/0008](../decisions/0008-verification-plan-product-roadmap.md), `docs/Markharness_改善・実装検討_統合設計文書.md` (hereafter "the design-review document")
**Intended audience**: implementers of `markharness` (to be referenced when starting Stage 1: the canonical import model, and Stage 2: the PR Verification Plan)

**Positioning**: The model in Chapter 3 of the paper — a `ChangeEvent` that uses the tree SHA of a Feature aggregate as its version identity, computed automatically at milestone boundaries — is already implemented, on the premise of a single Git repository's `knowledge/` and milestone tags. This document designs how to generalize that model to an arbitrary PR base/head pair, ingest artifacts originating from external tools, and emit the result as a Verification Plan, concretizing the proposals in the design-review document in terms of markharness's existing vocabulary (`FEATURE`, `ChangeEvent`, `verified_feature_tree_shas`, etc.). [decisions/0008](../decisions/0008-verification-plan-product-roadmap.md) fixes the Stage 0–3 order of work; this document covers the detailed design of Stage 1 (the canonical model) and Stage 2 (the Verification Plan). Stage 0 (turning the fixture into a golden dataset) and Stage 3 (the GUI) are out of scope here.

---

## 1. The Canonical Model

### 1.1 Conceptual Model

To connect artifacts originating from external tools with markharness's existing `FEATURE`/`CONDITION`/`TESTCASE`/`ChangeEvent`/`TESTEXECUTION` (the ER diagram in Section 3.1 of the paper), the following intermediate layer is added.

```text
ExternalArtifact (raw data observed in an external tool)
      │ import/normalize
      ▼
CanonicalArtifact (a logical artifact within markharness; the existing FEATURE, etc. is regarded as one kind of this)
      │
      ├── ArtifactVersion (an immutable snapshot, corresponding to the paper's tree SHA / blob SHA)
      │          │
      │          └── Change (the diff between two ArtifactVersions; a generalization of the paper's ChangeEvent)
      │                 │
      │                 ▼
      │          AffectedArtifact (a candidate impacted by a Change, with rationale, confidence, and derivation path)
      │
      └── Relation (stored | derived; a generalization of the paper's `derived_from`/`forked_from`)

Execution ── Evidence ── binds to an ArtifactVersion (a generalization of the paper's verified_feature_tree_shas)
                          │
                          └── valid | stale | unknown (a generalization of the paper's verify trace/pending determination)
```

In the existing markharness implementation, `FEATURE` corresponds to a `CanonicalArtifact` with kind=`feature`, and the tree SHA of a Feature directory corresponds to the case of an `ArtifactVersion` where source=`markharness-native` and `git_oid` is populated. This generalization therefore does not replace the existing model; it subsumes the existing model as one of its sources.

### 1.2 Separating Logical Identity from Version Identity

```text
logical_identity(A)  = (source, external_id)
version_identity(A,v) = git_oid                      # when source is under Git management
                       | canonical_hash(content(A,v)) # for SaaS/API-originated input outside Git management
```

- `source`: a namespace such as `markharness-native`, `doorstop:<repo>`, `testrail:<instance/project>`.
- `external_id`: an ID that continuously identifies the same artifact at the input source. For markharness-native, this is the `id:` field of `feature.yml` (identical to the existing specification in Section 3.3 of the paper).
- `git_oid`: the tree/blob SHA, when the input is under Git management. A markharness-native Feature always uses this (directly reusing `resolve_feature_versions` from Section 3.1 of the paper).
- `canonical_hash`: a content hash normalized against ordering, whitespace, and source-specific non-semantic fields. Used as a substitute for `git_oid` for input not under Git management, such as a SaaS API (Section 3.7 of the design-review document).

**Stage 1 scope limitation**: The importers handled in Stage 1 (Markharness native and JUnit) are both file-based, so `canonical_hash` can be reduced to `git_oid` by committing the normalized files produced by the JUnit importer and placing them under Git management. `canonical_hash` derived directly from a SaaS API response (without materializing it) is not implemented until Stage 4, where the TestRail importer is undertaken (following the recommendation in Section 3.7 of the design-review document).

### 1.3 Stored Trace and Derived Trace

The distinction in Section 3.1 of the paper between `forked_from` (stored, manually recorded) and `derived_from` (derived, computed anew from a `ChangeEvent`'s tree-SHA comparison) is extended to traces originating from external imports as well.

```yaml
relation:
  from: test:checkout-empty-postcode
  type: verifies
  to: condition:postcode-required
  origin:
    kind: derived                # stored | derived
    rule: markharness-generate   # identifier of the generation rule (in the existing implementation, the generates relationship CONDITION → TESTCASE)
    rule_version: "1"
```

For the markharness-native importer, the `generates` relationship (the structural generation graph of Section 3.2(A) of the paper) is the sole source of derived trace, and this directly reuses the existing implementation's `markharness generate`. Stored trace corresponds either to a requirement–test association brought in by an external importer (e.g., a Doorstop/StrictDoc link), or to `forked_from` manually recorded on the markharness side.

### 1.4 Required Fields of the Canonical Schema

| Area | Example fields | Correspondence with the existing implementation |
|---|---|---|
| Identity | `source`, `external_id`, `canonical_id` | The `id:` field of `feature.yml` (Section 3.3 of the paper) |
| Version | `git_oid`, `canonical_hash`, `observed_at` | The Feature directory's tree SHA (Section 3.1 of the paper) |
| Type | `feature`/`requirement`/`condition`/`expected_result`/`test_case`/`external_requirement`, etc. | Each entity in the paper's ER diagram |
| Relations | `type`, `origin.kind`, `origin.rule`, `confidence` | `derived_from`/`forked_from` (Section 3.1 of the paper) |
| Provenance | `importer`, `importer_version`, `source_locator` | (New. Unnecessary in the existing implementation, which is fixed to markharness-native.) |
| Evidence | `result`, `executed_at`, `bound_versions` | `verified_feature_tree_shas` (Section 3.7 of the paper) |

Normalization must be deterministic (the same canonical hash from the same semantic input, every time — the same design philosophy as the paper's `canonicalization_rule_version` in Section 3.3). Non-deterministic elements such as timing, retrieval order, or API-response order are excluded from the hash target.

---

## 2. The Verification Plan Generation Pipeline

### 2.1 Processing Stages

```text
PR (base..head)
      │
      ├── code diff
      ├── spec diff
      └── knowledge diff ── generalizes the existing Feature tree-SHA comparison
              │              (Sections 3.2–3.4 of the paper) to an arbitrary base/head pair
              ▼
     Structured Changes (reuses the existing ChangeEvent schema, with a base/head ref pair instead of a milestone_id)
              │
      ┌───────┴────────┐
      ▼                ▼
trace/derivation    gap analysis
(existing generates    (new. Inspects for missing tests corresponding to a
 relationship +          new condition/boundary/error behavior in knowledge)
 stored trace)
      │                │
      ▼                ▼
Affected Existing   New Required Tests (a proposal; becomes required only once accepted by a human)
Tests
      └───────┬────────┘
              ▼
       Verification Plan
              │
      Evidence resolution (reuses the existing verify trace/pending valid/stale/unknown determination)
              ▼
 Passed / Pending / Failed / Stale
```

Stage 2 implements the four stages "Structured Changes," "Affected Existing Tests (stored + derived trace)," "Evidence resolution," and "Plan emission." "Gap analysis (missing-test discovery)" and the "AI proposal adapter" are added as optional variants within Stage 2, evaluated against the rule-based baseline (see Section 5 of [decisions/0008](../decisions/0008-verification-plan-product-roadmap.md)).

### 2.2 Extension Points from the Existing `changes compute`

The current `markharness changes compute` takes two Git tags, `from_milestone`/`to_milestone`, as arguments (Section 3.4 of the paper). The Verification Plan generalizes this to accept an arbitrary pair of Git refs.

```rust
// Conceptual signature (a proposed generalization of the existing changes::compute)
fn compute_changes(from_ref: &str, to_ref: &str, mode: ComputeMode) -> Vec<ChangeEvent>
```

Aside from lifting the naming and the restriction to milestone tags (`from_milestone`/`to_milestone`), the tree-SHA-comparison logic itself (Sections 3.2–3.4 of the paper, including the two modes `historical`/`--current-tree`) is unchanged. The existing `markharness changes compute --from <tag> --to <tag>` remains backward-compatible as a special case of this generalized API (when milestone tags are passed).

### 2.3 Verification Plan Output Schema (Proposal)

```yaml
schema_version: 1
base: v2.3
head: 4c2e81a
summary:
  changed_features: 3
  affected_tests: 17
  new_tests: 4
  obsolete_tests: 2
  passed: 9
  pending: 6
  failed: 2
  stale_evidence: 5

changed_features:
  - id: feature:checkout
    from_tree_sha: 3a1b...
    to_tree_sha: 4c2e...
    confidence: 1.0          # The existing tree-SHA comparison (deterministic) always has confidence 1.0;
                              # only AI-proposal-originated changed_features can have a value below 1.0.

affected_existing_tests:
  - id: test:checkout-valid-address
    reason: "derived from modified condition: postcode-required"
    origin: derived           # stored | derived (Section 1.3)
    status: pending           # directly reuses the existing verify pending determination

new_required_tests:
  - proposal_id: new-test:checkout-empty-postcode
    behavior: "empty postal code is rejected"
    reason: "new mandatory constraint has no negative test"
    confidence: 0.88           # only for rule-based/AI-originated candidates; not fixed at 1.0
    decision: proposed         # proposed | accepted | rejected | deferred (Section 2.5)

obsolete_tests:
  - id: test:checkout-postcode-optional
    reason: "asserts behavior removed by this change"
```

Entries in `changed_features` mechanically derived from a tree-SHA comparison are fixed at `confidence: 1.0`; only AI-proposal-originated entries can have a value below 1.0. This distinction prevents a Plan consumer (CI, GUI) from conflating a deterministic result with an AI-originated suggestion.

### 2.4 CLI Commands (Proposal)

```bash
# Generate a Plan between base and head (a generalization of the existing changes compute)
markharness plan --base origin/main --head HEAD --format json \
  --output .markharness/verification-plan.json

# Review the Plan, accepting/rejecting proposals in new_required_tests
markharness plan review .markharness/verification-plan.json

# Resolve evidence, reusing the existing verify trace/pending as-is
markharness plan status --plan .markharness/verification-plan.json
```

Exit codes are kept consistent with the existing `verify pending --fail-on-pending`.

| Exit code | Condition |
|---|---|
| 0 | All required tests have valid evidence |
| 1 | There is a failed test |
| 2 | There is a pending/stale/unaccepted proposal |
| 3 | An input, schema, or identity-resolution error |

### 2.5 The Decision State Model for new_required_tests

The state model in Section 6.3 of the design-review document is adopted in a form that does not collide with markharness's existing vocabulary (`verify pending`'s pending/stale).

```text
proposed ── human accepts ── accepted (once the TestCase is created, treated as an ordinary TESTCASE)
    │                             │
    ├── rejects ── rejected       └── while awaiting execution, pending (the existing verify pending determination)
    └── defers  ── deferred
```

`proposal decision` (accepted/rejected/deferred) and `execution status` (pending/passed/failed/stale) are not conflated. A new required test becomes a TestCase only once accepted, after which it becomes subject to the existing `verify trace`/`verify pending` (reusing the same logic as "Evidence resolution" in the second stage).

---

## 3. Bounded Components (Implementation Split Guidance)

| Component | Responsibility | Correspondence with the existing implementation |
|---|---|---|
| Importer SDK | Reading external formats, attaching identity/provenance | New (Stage 1) |
| Canonical Store | Canonical files and schema migration | New (Stage 1). However, the output destination of the markharness-native importer is the existing `knowledge/` directory itself. |
| Change Engine | Semantic diff between versions/snapshots | Generalizes the existing `markharness changes compute` (Section 2.2) |
| Impact Engine | Deriving affected artifacts from stored/derived trace | Reuses the existing `generates` relationship (Section 3.2(A) of the paper) |
| Gap Analyzer | Missing/new/obsolete test proposals | New (Stage 2, optional variant) |
| Evidence Engine | Result ingestion, version binding, freshness determination | Reuses the existing `verify trace`/`verify pending` (Section 3.7 of the paper) |
| Plan Engine | Assembling the required set and status | New (Stage 2). A thin layer that composes the output of the Engines above. |
| Presentation | CLI, (in Stage 3) GUI, CI summary | Extends the existing CLI output format |

The essence of this split is that **the Change Engine and Evidence Engine directly reuse the existing implementation, and new implementation is confined to the Importer SDK, Gap Analyzer, and Plan Engine**. This structurally lowers the risk that the Stage 1–2 implementation diverges from the existing `ChangeEvent`/`verified_feature_tree_shas` model (Chapter 3 of the paper, the very object of the not-yet-empirically-verified RQ1 evaluation).

## 4. Invariants

1. The same deterministic output is obtained from the same canonical input, rule version, and base/head (the same requirement as the existing `markharness changes compute`'s `historical` mode, Section 3.5 of the paper).
2. A derived artifact carries provenance and a generator version (reusing the `rule_version` of the existing `generates` relationship).
3. Evidence carries at least test identity, result, execution context, and the verified version (the same requirement as the existing `verified_feature_tree_shas`).
4. `valid` on a Plan means not "last passed" but "there is sufficient evidence for the version set currently required over the base/head interval" (the same semantics as the existing `verify pending`'s stale determination).
5. Ambiguity in identity (rename/split/merge) is not implicitly resolved. It goes through human review as `identity_resolution: proposed` (not handled in the paper; Section 3.6 of the design-review document).
6. Plan items are traceable via reason/source/confidence (the schema in Section 2.3).
7. Cache/index/GUI state is reconstructible from Git and external snapshots (Section 3, "Do not make a GUI-only DB the source of truth," of [decisions/0008](../decisions/0008-verification-plan-product-roadmap.md)).

---

## 5. What Must Be Fixed in Stage 0 (a Premise of This Document)

This document is the design for Stage 1–2, and presupposes Stage 0 (golden datasets and a CLI JSON-contract versioning policy) as fixed by [decisions/0008](../decisions/0008-verification-plan-product-roadmap.md). For ChangeEvent, `tests/fixtures/stage0/changes-m1-m2.golden.yml` is the golden contract. Canonical snapshots and plan status are not implemented yet, so their fixtures will be added in the first vertical slice of Stages 1 and 2 respectively.

CLI JSON contracts are versioned as follows:

- Every JSON object newly published as a stable contract must have a top-level integer `schema_version`, starting at `1`.
- Within one version, field meaning, type, and requiredness do not change. Only optional fields may be added.
- Removing or renaming a field, changing its type, or changing its meaning increments `schema_version`; readers retain the previous version for at least one minor release. This data-contract rule also applies during the 0.x period.
- Existing `--json` output is treated as an unversioned legacy contract. It migrates to a versioned envelope when the shared Presenter is introduced in Phase 2; its shape remains unchanged until then.
- Golden tests normalize environment-dependent values such as timestamps, temporary paths, and commit SHAs, then compare the remaining JSON/YAML document exactly.

## 6. Options Considered but Not Adopted

- **Implementing `Change` as a new entity distinct from `ChangeEvent`**: The design-review document uses the name `Change` at the conceptual-model level, but it is functionally identical to markharness's existing `ChangeEvent` (a diff via tree-SHA comparison), so it is not implemented as a separate entity; instead, `ChangeEvent`'s from/to is generalized from being restricted to milestone tags to accepting an arbitrary ref (Section 2.2). Splitting the entity would mean the object of empirical evaluation in Chapter 3 of the paper (RQ1) and the Verification Plan evaluation would be looking at different models, preventing Stage 2's evaluation results from feeding back into the paper's model.
- **Implementing SaaS-API-originated `canonical_hash` ahead of schedule in Stage 1**: Authentication, pagination, and rate-limit handling for TestRail-like APIs cost more to implement than a file-based importer, and would delay Stage 1's purpose (validating the canonical schema). This is deferred to Stage 4 (Section 4 of [decisions/0008](../decisions/0008-verification-plan-product-roadmap.md)).
- **Folding the `new_required_tests` confidence score into `generated_by`/`verified_by` (the productization-proposal fields in Section 3.5 of the paper)**: `generated_by`/`verified_by` are metadata on an ExpectedResult, which is settled knowledge, whereas a `proposed`-state proposal has different semantics (settled knowledge vs. an unapproved candidate). Mixing the two would break the premise the paper states explicitly — "everything under `knowledge/` is verified, settled knowledge" (Section 3.5) — so a proposal is kept only on the `.markharness/verification-plan.json` side, and is reflected into ordinary knowledge only once accepted.
