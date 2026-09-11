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

**Alignment check**: Detects, when either a Requirement or a TestCase changes, whether the other side followed up or was explicitly confirmed as not needing a change. Bidirectional.

**Spec-Reviewed trailer**: A commit trailer recording that an alignment check concluded "no change required." Added alongside the change's own commit.

**Execution status**: A lightweight record attached to a TestCase that states **the verification method (`automated` / `manual`) and where to look**. It holds no pass/fail detail, timestamp, evidence artifact, or execution environment, so the presence of a value does not mean "executed against the latest revision".

**Change Impact**: The list of affected Features, Requirements, and TestCases computed from the diff between base and head. Used for per-PR review.

**Release Coverage**: A listing, over a given set of Requirements/Features, of their correspondence to TestCases and whether an Execution status exists. Supporting information for release decisions.

**Retire**: Removing a TestCase or Feature from the current target set. Carries no guarantee of UID reuse, nor any mechanism for explicit restore or id reservation under the same UID.
