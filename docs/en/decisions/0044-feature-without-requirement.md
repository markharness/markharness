# 0044: Make a Feature without a Requirement a valid state

## Status

Accepted (decided 2026-10-07). Rejects reading [0027](./0027-declarative-knowledge-reconciliation.md) §5's "must satisfy each field's required and non-empty rules" as a non-empty rule on `contributes_to`.

## Background

`knowledge reconcile` accepts a new Feature that omits `contributes_to`, and a Feature with `contributes_to: []`, and writes a `feature.yml` whose `requirement_uids` is empty. `knowledge remove` also leaves the Features that referenced the last Requirement with an empty `requirement_uids` once it is removed ([0034](./0034-knowledge-remove-command.md) §3). But `feature.schema.json` set `minItems: 1` on `requirement_uids`, so `markharness validate` run right after these paths failed with `[] has less than 1 item at /requirement_uids` (Issue #84).

`minItems: 1` was carried over mechanically when the old Feature format, which had one required `requirement`, became an array; no ADR decided it. The ER diagram in [paper §3.1](../git-native-model-for-test-knowledge-management.md) shows the Feature–Requirement relation as `}o--o{` (zero or more on both sides), and the same diagram marks required relations with `||`. [0034](./0034-knowledge-remove-command.md) also calls `requirement_uids` an "optional many-to-many reference". 0027 §5, meanwhile, was worded so that value collections, `contributes_to` included, read as having to satisfy "each field's required, non-empty, and reference rules".

## Decision

### 1. A Feature's `requirement_uids` holds zero or more entries

Remove `minItems: 1` from `feature.schema.json`. A Feature without a Requirement is a valid state, and `knowledge reconcile` (both omission and `[]`), `knowledge remove`, and `markharness validate` follow the same rule for it.

### 2. No feature is added to report a Feature without a Requirement

`traceability`, `coverage`, and `validate` do not report this state as a warning or a gap. It is added when it is needed, to fit the requirement then (YAGNI).

### 3. Assume newly created projects, and do not raise `[knowledge].schema_version`

This change keeps data that was valid under the old schema valid under the new one. There is no risk of the wrong-format comparison that [0014](./0014-knowledge-schema-version-persistence.md) guards against in the prototype stage.

### 4. Rewrite 0027 §5 to match the actual rules

0027 §5 now states that `phases`, `steps`, and `results` need at least one entry, and that `axis`, `contributes_to`, and `procedures` have no non-empty rule.

## Consequences

- A Feature without a Requirement does not appear in the impact search that starts from a changed Requirement. The search that starts from a changed Feature reaches it (design doc §6.1 step 1).
- With the `feature.schema.json` change, `validate` in a newly `markharness init`ed project accepts a Feature without a Requirement.

## Alternatives considered and rejected

- **Require at least one Requirement on a Feature**: reconcile would have to reject omission and `[]`, and `knowledge remove` would have to refuse to remove the last Requirement or cascade into the Features that reference it. The in-progress state of linking a Requirement later could not be expressed, and it does not fit the paper's ER `}o--o{`.
- **Report a Feature without a Requirement as a gap**: there is no requirement for it now, and the shape of the report is undecided (YAGNI).
