# 0030: Add an external key (`source_key`) to external Requirement

## Status

Accepted (decided 2026-09-15). Extends the external Requirement shape (`source_locator` / `source_revision`) defined by [0023](0023-requirement-native-and-external-source.md) with a third pinned field holding StrictDoc's own UID string.

## Background

StrictDoc's UID (the requirement identifier inside `.sdoc`) is free-form text and can contain any characters, including uppercase letters. An attempt to actually hold a StrictDoc UID on the markharness side ran into the following incident.

markharness's `id` (the display id) is restricted by `is_valid_slug` (`src/knowledge.rs`) to lowercase ASCII letters, digits, and hyphens. This is not a Requirement-specific constraint — it is a project-wide safety constraint because `id` is reused as a filesystem path component (`knowledge/requirements/<id>/requirement.yml`, etc.). Writing a StrictDoc UID containing uppercase letters (e.g. `REQ-Login-01`) straight into `id` was rejected by this constraint, forcing the value to be lowercased before it could be saved. At that point the stored value diverged from StrictDoc's original spelling, exact-match search and visual cross-checking against StrictDoc broke, and traceability was significantly harmed.

[markharness-v2-design.md](../design/markharness-v2-design.md) §9.2.1 had already reserved a forward-compatible contract for the future StrictDoc Adapter (M3): "an external Requirement distinguishes an external key, a same-repo locator, and a fixed revision" — but this "external key" itself was never implemented. This ADR implements that contract ahead of schedule, so a StrictDoc UID can be held verbatim without being subjected to markharness's filename-safety constraint.

## Decision

### 1. Add `source_key` to `Requirement`

Add `source_key: Option<String>` to `Requirement` (`src/knowledge.rs`). Treat it exactly like `source_locator` / `source_revision`: required for `source: external`, forbidden for `source: native`. A `requirement.yml` carrying both, or missing either, is rejected by `validate` (the same exclusivity rule as [0023](0023-requirement-native-and-external-source.md) §4).

Under the current design, `source: external` always means StrictDoc (`.sdoc`); anything not using StrictDoc uses `source: native`. Making `source_key` required for external therefore does not foreclose any future non-StrictDoc external source.

### 2. Store the raw value verbatim; no normalization on write

`source_key` stores StrictDoc's UID string as-is, case included. markharness performs no automatic conversion or forced normalization on write, and imposes no character-set restriction (the `id` field's `is_valid_slug` constraint does not apply here). Preserving the original spelling is what makes manual cross-checking and grepping against StrictDoc possible in the first place.

### 3. Comparison (duplicate detection, search) is out of scope for this ADR

`validate` does not detect or reject multiple Requirements sharing the same `source_key` when compared case-insensitively. No `source_key`-based search/lookup command is added. When such comparison is needed in the future, this ADR only fixes the intended policy — compare after uppercase-normalizing — leaving the implementation to a separate ADR once concrete demand is confirmed.

### 4. `source_key` plays no part in Requirement identity

This ADR does not affect Requirement's `uid` handling as defined by [0013](0013-immutable-identity-model.md) in any way. `source_key` is auxiliary information for display and cross-checking; it is never used by identity resolution or by the rename-tolerance logic ([0021](0021-identity-retire-simplification.md)). The new field is an addition to the existing `Requirement` entity; no new `EntityKind` is introduced.

### 5. `schema_version` is unchanged

Per the existing policy in §9.2.2 of [markharness-v2-design.md](../design/markharness-v2-design.md) (the same reasoning as [0018](0018-identity-schema-version-freeze.md)), a field addition alone does not advance `schema_version`. During this prototype period it stays at `1` until an actual comparison/compatibility gate needs it.

## Consequences

- `source_key` is added to the `Requirement` struct and YAML serialization in `src/knowledge.rs`, to `check_requirement_source_mode` in `src/validate.rs`, to `src/knowledge_reconcile/` (Intent schema, plan construction, blank-string checks), and to `schema/requirement.schema.json`.
- No existing `requirement.yml` is affected (this repository currently has no `source: external` Requirement in practice).

## Options considered and rejected

- **Reuse `id` (the display id) to hold the StrictDoc UID**: the option that actually caused this incident. `id` is not Requirement-specific; it is reused, project-wide, for other purposes such as filenames, and its `is_valid_slug` constraint exists for the safety of all of those uses. Loosening it would compromise every other entity and use of `id`. A separate field dedicated to the StrictDoc UID avoids that.
- **Force-uppercase the value on write**: this would diverge from StrictDoc's original spelling and make visual/grep cross-checking harder, not easier. Normalizing only where comparison is actually needed is sufficient; there is no reason to alter the stored value itself.
- **Implement duplicate detection and a search command in the same change**: this ADR is scoped to holding the StrictDoc UID verbatim. Duplicate detection and search are separate features to be considered in their own ADR once demand is confirmed.

## Triggers to revisit

Revisit the scope of this decision (no comparison, no duplicate detection, no search) if any of the following happens:

- A real duplicate registration or missed match caused by spelling variance is reported again.
- Concrete demand for a search command emerges.
- M3 implements the StrictDoc Adapter and Requirement-level key comparison becomes necessary.
