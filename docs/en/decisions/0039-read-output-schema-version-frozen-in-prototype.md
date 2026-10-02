# 0039: Do not raise the read outputs' `schema_version` during 0.x

## Status

Accepted (decided 2026-10-02). The criterion for raising `schema_version` is itself decided in a separate ADR once the 1.0 criteria are met (see "Conditions for revisiting" at the end).

## Background

The output JSON of `traceability`, `impact`, and `coverage` carries `schema_version` and `record_kind`. An external tool can use these values to decide whether it can read the JSON it received.

`traceability`'s `behaviors[]` did not list a Behavior that has no Scenario (design doc cli-read-model-design.md §5.3). `knowledge reconcile` can create such a Behavior, so an external tool that later wanted to add a Scenario to it could not look up its `behavior_uid`. Fixing this adds elements to `behaviors[]`. No existing field changes, but it forces the question of whether `schema_version` should be raised.

The criterion for raising `schema_version` has not been decided.

## Decision

**During 0.x, do not raise `schema_version` of the read output JSON (`traceability`, `impact`, `coverage`).** This holds whether a change adds or removes a field, adds or removes elements, or corrects a meaning.

The `behaviors[]` fix (list every Behavior that exists in Knowledge) is the first application of this decision, and `traceability`'s `schema_version` stays at 1.

## Rationale

- The [release-and-license instructions](../../../.github/instructions/release-and-license.instructions.md) allow breaking compatibility between 0.x minor versions. A version number stamped during a period with no compatibility guarantee gives an external tool nothing to rely on.
- The Knowledge schema version is not raised during the prototype stage for the same reason ([0014](./0014-knowledge-schema-version-persistence.md) §11). Treating the output JSON differently would blur what each version number guarantees.
- Raising the version without a criterion means deciding on the spot, at every later change, whether to raise it, so the number would not mean the same thing twice.

## Alternatives considered

- **Raise `traceability` from 1 to 2 with this change.** An external tool that stops on a version mismatch would stop on a change that only added elements. A number raised without a criterion cannot serve as a precedent for the next change. Rejected.
- **Decide now on "raise only when an existing field's meaning changes".** There is no real data from external tools that need a compatibility contract, so the criterion cannot be checked against anything. It will be decided against the requirements at the time it is needed (YAGNI). Rejected.

## Consequences

- An external tool must assume the read outputs can change during 0.x. A matching `schema_version` does not guarantee the output is unchanged.
- When an output changes, the design doc and the CLI manual are updated in the same change.

## Conditions for revisiting

Decide the criterion for raising `schema_version` once the 1.0 criteria defined in [PROJECT.md](../../../PROJECT.md) are met. One starting point is "raise when an existing field's meaning or type changes, or when an existing way of reading becomes wrong, and treat added elements as compatible". This is undecided and is not adopted by this ADR.
