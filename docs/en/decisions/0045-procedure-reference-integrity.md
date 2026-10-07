# 0045: Keep procedure references consistent in validate and reconcile

## Status

Accepted (decided 2026-10-08). Establishes that [0027](./0027-declarative-knowledge-reconciliation.md) §5's "each element of the replacement must satisfy the reference rules" extends to other Scenarios that reference a replaced `procedures` entry through `use:`.

## Background

A Scenario's `use:` step names a procedure declared in its owning Behavior's `procedures` ([0017](./0017-scenario-case-revision-and-execution-evidence.md) §2). A procedure has no UID of its own; it is a value of the Behavior. So when `knowledge reconcile` supplies `procedures`, the whole collection is replaced (0027 §5).

Replacing the collection so that it drops or renames a procedure leaves every Scenario that used it with a reference that no longer resolves. The implementation detected neither case:

- `knowledge reconcile` checked `use:` only for Scenarios the Intent listed. A Scenario the Intent did not list stayed unchecked and was saved as is after `procedures` was replaced.
- `markharness validate` did not match `procedures` against `use:`.

So `reconcile` and `validate` both succeeded, and then `generate`, `verify`, and `traceability`, which expand procedures, failed with `phase step references unknown procedure`. The state in which `validate` passes did not match the state in which later commands succeed.

0027 §5's "each element of the replacement must satisfy the reference rules" speaks of the elements of the replaced collection itself, and did not say whether it covers other Scenarios that reference a replaced procedure.

## Decision

### 1. `validate` matches every Scenario's `use:` against its Behavior's `procedures`

A `use:` that names something not in the owning Behavior's `procedures` is reported as a problem in that Scenario's file. Hand edits that bypass `reconcile` are caught by the same rule.

### 2. `reconcile` also checks the Scenarios the Intent does not list, for a Behavior whose `procedures` change

When an existing Behavior's `procedures` differ from its current content, `use:` in the Scenarios under it that this Intent does not list is checked against the replacement `procedures`. If one does not resolve, `reconcile` rejects with `invalid_procedure_reference` and writes nothing. The boundary of the check is:

- A Scenario the Intent lists is checked, as before, against its patched content. Listing the Scenario with `use:` dropped in the same Intent lets the procedure be removed.
- A Scenario this Intent moves to another Behavior is checked at its destination, so it is left out of the check at its source. The result does not depend on the order of entries in the Intent.
- A Behavior that omits `procedures`, and one whose `procedures` content does not change, is not checked. The procedures do not change, so no reference newly breaks.

### 3. No automatic rewriting of `use:`

A procedure rename is not distinguished from replacing it by removing the old name and adding a new one. No feature is added that rewrites the `use:` of the referencing Scenarios to follow. When rejected, the user either keeps the procedure or lists the Scenario update in the same Intent. It is a feature nobody asked for, and is added when it is needed, to fit the requirement then (YAGNI).

### 4. One function decides the rule

Whether a `use:` resolves is decided by one function in the `knowledge` module (`unresolved_procedure_uses`), and both `validate` and `reconcile` use the same rule. The error `generate` raises when it expands steps stays as it is.

## Consequences

- An Intent that removes or renames a procedure is rejected while a Scenario still uses it. In an existing project, a Scenario that already holds a broken reference is newly reported by `validate`.
- The rejecting diagnostic points at a Scenario that has no position in the Intent, so its message carries the Scenario file's path and the Scenario's id, the procedure name, and the position.

## Alternatives considered and rejected

- **Leave `reconcile` as it is and detect only in `validate`**: a broken state would be saved first and noticed afterwards. It also does not fit `reconcile`'s fail-closed, atomic saving ([0027](./0027-declarative-knowledge-reconciliation.md)).
- **Rewrite the referencing Scenarios' `use:` on a rename**: a procedure is a value keyed by name, and no operation expresses a rename. It would add a mechanism to follow renames without a requirement for it (YAGNI).
- **Always check a Behavior that supplies `procedures`, even when the content does not change**: when the procedures do not change, no reference newly breaks, and an existing broken reference would become the reason to reject an unrelated update.
