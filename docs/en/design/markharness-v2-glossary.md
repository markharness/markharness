# markharness v2 Terms

markharness lightly tracks Git-native test knowledge (`knowledge/`) against its relationship to an external spec (StrictDoc) — alignment, missed updates, and execution record. The canonical source for these terms is [CONTEXT.md](../../../CONTEXT.md); this document is an excerpt and supplement. Where the two disagree, CONTEXT.md governs.

## Language

**Feature**: A spec-level concept, managed under `knowledge/`, representing a capability offered to users. Related to zero or more Requirements via `contributes_to`.

**Behavior**: An observable behavior under a specific condition. Belongs to one Feature.

**Scenario**: One concrete verification example, with a precondition, operation, and expected result. Belongs to one Behavior, and corresponds one-to-one with a TestCase.

**TestCase**: The unit to be verified, deterministically generated from a Scenario.

**Case revision**: The revision of a TestCase's verification content. Distinct from the revision of the target build or the execution environment.

**Axis**: A fixed, cross-cutting classification/search dimension over Features, TestCases, etc.

**ChangeEvent**: A change record derived from comparing Feature revisions between base and head.

**Requirement**: A requirement under verification. In `source: native` mode markharness owns its `label`/`description`; in `source: external` mode StrictDoc owns the content and markharness keeps only a fixed reference (id, revision), never duplicating or editing the body ([0023](../decisions/0023-requirement-native-and-external-source.md), design §5.2.1).

**Contributes-to relation**: A many-to-many relation from Feature to Requirement. Means only "contributes to realization" — not proof of verification. It is carried by the existing `feature.requirement_uids` field; no new type or store is introduced.

**Alignment check**: When either a Requirement or a TestCase changes, reports the other side's state as one of three values — followed / confirmed / unconfirmed. Bidirectional, and a mere simultaneous change ("followed") is never equated with "confirmed" (design §5.3).

**Spec-Reviewed trailer**: A commit trailer recording that an alignment check concluded "no change required." Added alongside the change's own commit.

**Execution status**: A lightweight record attached to a TestCase that states **the verification method (`automated` / `manual`) and where to look**. It holds no pass/fail detail, timestamp, evidence artifact, or execution environment, so the presence of a value does not mean "executed against the latest revision".

**Change Impact**: The list of affected Features, Requirements, and TestCases computed from the diff between base and head. Used for per-PR review.

**Release scope**: The list of TestCases chosen for verification in a given release (`ReleaseScope`). It holds only a `release_id` and an array of Case UIDs — no timestamp, owner, approval state, or result. A human records it through the CLI; it is a declaration of choice, not evidence of a run ([0024](../decisions/0024-release-scope-selection-list.md)).

**Release Coverage**: A listing of the TestCases and verification methods registered as of a given Git ref, including coverage gaps across the chosen set of Requirements/Features. It is not evidence of what was selected or executed — supporting information for release decisions (design §6.2).

**Retire**: Removing a TestCase or Feature from the current target set. Carries no guarantee of UID reuse, nor any mechanism for explicit restore or id reservation under the same UID.
