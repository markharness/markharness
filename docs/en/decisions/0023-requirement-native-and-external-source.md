# 0023: Give Requirement two ownership modes, native and external

## Status

Accepted (2026-09-11; design agreed, implementation pending). It keeps the native Requirement defined by [0017](0017-scenario-case-revision-and-execution-evidence.md) §1/§3 (many-to-many Feature↔Requirement, with the Feature side owning the relation) and adds a mode in which an external spec document owns the content.

## Background

The 2026-09-11 rethink of [markharness-v2-design.md](../design/markharness-v2-design.md) settled on StrictDoc (`.sdoc`, Git-managed) as the source of truth for requirements, with markharness keeping only a fixed reference (`source_locator` / `source_revision`). Making that the *only* shape of `requirement.yml`, however, creates three problems.

1. **markharness becomes unusable without StrictDoc.** None of the four North Star questions (impact, missed updates, past verification scope, release impact) presupposes an external spec document.
2. **The fixed reference does nothing during the MVP.** Ingesting and parsing `.sdoc` is M3 (future) in that design; M0–M2 never read the external requirement itself. Requiring `source_locator` before anything reads it leaves teams pinning blob OIDs of files nobody parses.
3. **The current implementation and current usage are native.** `knowledge/requirements/<id>/requirement.yml` is a native entity with `label`, `description`, `axis`, and a UID (`src/knowledge.rs`), and it is used without StrictDoc today — including in this repository.

## Decision

### 1. Add `source` to `requirement.yml`

`source: native | external`, defaulting to `native` when absent.

### 2. Native mode

markharness owns `label` (required) and `description` (optional). `source_locator` / `source_revision` are not allowed. A spec-side change is detected from the base/head diff of `requirement.yml` itself — per-Requirement granularity, no external tool involved.

### 3. External mode

`source_locator` (a `.sdoc` path inside the same Git repository as markharness) and `source_revision` (a pinned Git blob OID) are required. `label` / `description` are not allowed (no duplication of external content — P1 in [markharness-v2-design.md](../design/markharness-v2-design.md)). A spec-side change is detected by comparing the pinned reference against the blob OID at head (§6.1 of that design).

### 4. Mixing is rejected

A `requirement.yml` that carries fields from both modes, or that is incomplete for either, is rejected by `validate`.

### 5. `axis` is kept in both modes

`axis` is markharness's own classification, not a copy of external content, so it stays even in external mode.

## Consequences

- Existing `requirement.yml` files remain valid as native (`source` omitted); no conversion is needed.
- `markharness requirement repin` (advancing the pinned reference) applies to external mode only.
- The shape of Change Impact / Release Coverage output is the same in both modes; only the per-Requirement detection method differs.
- The interactive authoring flow (`src/interactive.rs`, `knowledge_draft.rs`) is unchanged for native, and switches to a `source_locator` prompt only when external is chosen.

## Options considered and rejected

- **External only (ownership always pinned to StrictDoc)**: the most faithful reading of P1, but teams without StrictDoc could not use markharness at all, and since `.sdoc` parsing is M3 the MVP itself would not stand up.
- **Native only (add external once demand appears)**: the decision to keep spec truth in StrictDoc is already made, and defining what the alignment check ([0019](0019-alignment-check-commit-trailer.md)) operates on requires fixing the external reference's shape now. The implementation difference between the modes is confined to a `validate` branch and a detection branch — not speculative abstraction.
- **Keep native `label` *and* an external reference on the same Requirement**: that duplicates external content and forces a new rule for deciding which side wins. Making the modes exclusive removes the need for such a rule.
