# 0042: `traceability` shows each Requirement's cases by the rule `coverage` uses

## Status

Accepted (decided 2026-10-06).

## Background

The GUI wants to show, in its Requirement list, how many cases relate to each Requirement and what they are called. To show the work in progress it reads `traceability`, which reads the working tree ([0033](./0033-traceability-defaults-to-working-tree.md)). But the link between a Requirement and its cases existed only in `coverage`'s `requirements[].cases`, and `coverage` reads committed content only.

If the GUI computes the link from `traceability`'s `relations`, it disagrees with `coverage`. A case relates to a Requirement by this rule: when a Scenario has at least one `contributes_to`, it relates only to the Requirements that Scenario names; otherwise it falls back to its Feature's `contributes_to` ([0031](./0031-scenario-level-requirement-contribution.md)). Adding the Feature-derived and Scenario-derived relations in `relations` does not match this rule, and the case counts did disagree in practice (Issue #113). If the GUI copied the rule, it would drift whenever the core rule changed.

## Decision

### 1. Add `case_uids` to each item of `requirements[]`

`case_uids` is an array of the `case_uid` of every TestCase related to the Requirement, sorted. It is empty for a Requirement whose `requirement_uid` is `null`. A TestCase with no `case_uid` yet (`identity migrate` not run) cannot be named by `case_uid`, so it is not listed.

`traceability` emits it whether or not `--at` is given. Without `--at` it reads the working tree, so an uncommitted edit is reflected.

### 2. The rule is one function that `coverage` also calls

"Which TestCases relate to which Requirement" lives in exactly one place, `generate::testcases_for_requirement`. Both `coverage` and `traceability` call it. It matches against each TestCase's effective `requirement_uids` (a Scenario-level override already resolved). A test pins that the two outputs agree.

### 3. Do not add it to `relations`

`relations` returns the relations as written: a Feature or Scenario's `contributes_to` declaration, and a TestCase's `generated_from` pointing at its Scenario. The Requirement-to-case link is derived, with Scenario overrides resolved. Mixing it into the same array would erase the difference between declared and derived relations and invite summing the `contributes_to` entries.

### 4. Output contract

`requirement` in `schema/traceability-read-model.schema.json` gains a required `case_uids`. Existing fields do not change. `schema_version` stays 1 ([0039](./0039-read-output-schema-version-frozen-in-prototype.md)).

## Alternatives considered and rejected

- **Add a new `relations` kind from Requirement to case (for example `covered_by`)**: rejected for reason 3 above.
- **Let the GUI compute it from `relations`**: as Issue #113 showed, the rules disagree. Avoiding a duplicated rule is the GUI's policy.
- **Make `coverage` read the working tree**: `binding` and `reference_status` assume a commit, so the scope is wide. Handle it when it is needed.
