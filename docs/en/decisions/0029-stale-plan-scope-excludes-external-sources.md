# 0029: Scope `stale_plan` to the state markharness owns

## Status

Accepted (decided 2026-09-14). Settles what "input state" covers in [0027](0027-declarative-knowledge-reconciliation.md) §6.

## Context

[0027](0027-declarative-knowledge-reconciliation.md) §6 requires a normal run to re-check current state immediately before committing and to stop with `stale_plan` if the input state has changed. The implementation takes a fingerprint when it starts building the mutation plan and takes it again just before committing, comparing the two.

§6 does not define what belongs to "input state". The state `knowledge reconcile` reads is of three different kinds.

1. `.markharness/knowledge`: the canonical Knowledge markharness owns and reconcile itself rewrites.
2. `.markharness/axes`: the Axis registry markharness owns, which `unknown_axis` resolves against and the `axes` commands rewrite.
3. The `.sdoc` a `source_locator` points at: the external record. markharness neither owns nor writes it. Per [0023](0023-requirement-native-and-external-source.md), an external Requirement's content lives in that file and `source_revision` pins the blob OID it had at a point in time.

Kinds 1 and 2 are state the identity lock can protect. Kind 3 is not. A `.sdoc` is a file that editors, people, and tools outside markharness change at any moment, and reconcile has neither the authority nor the grounds to lock it.

`source_revision: current` instructs the Reconciliation Module to resolve the current blob OID of `source_locator` and store it ([0027](0027-declarative-knowledge-reconciliation.md) §5). If the `.sdoc` is edited between that resolution and the commit, the stored OID points at the pre-edit content.

## Decision

### 1. Scope `stale_plan` to the state markharness owns

The fingerprint covers `.markharness/knowledge` and `.markharness/axes`. It does not cover the content, or the blob OID, of the `.sdoc` a `source_locator` points at.

The Axis registry belongs in scope because it is what [0027](0027-declarative-knowledge-reconciliation.md) §4's rule — only registered Axes may be referenced — is checked against, and because the `axes` commands rewrite it without taking the identity lock. If an Axis judged registered is removed after that judgement, reconcile would store Knowledge referencing an Axis that no longer exists: a state `markharness validate` rejects. That must be detected.

### 2. State the conditions held out of scope

A change is out of `stale_plan`'s scope when all of the following hold.

- What changed is the `.sdoc` file a `source_locator` points at.
- The change happened between `source_revision: current`'s blob OID resolution and the completion of the commit.
- The only discrepancy the change produces is a mismatch between the stored `source_revision` and the `.sdoc`'s current content.

A `.sdoc` change that undermines the validity of the canonical Knowledge itself — for example, deleting the file a `source_locator` points at — is not covered by this exclusion. That case already stops fail-closed with `invalid_source_revision` at resolution time.

### 3. Accept that a concurrent edit can store a stale blob OID

When a `.sdoc` is edited within that window, `knowledge reconcile` stores the pre-edit content's blob OID as `source_revision` and succeeds. This outcome is accepted.

The grounds are threefold.

- **The window cannot be closed.** A `.sdoc` cannot be locked, so re-resolving immediately before the commit shrinks the window without removing it. An edit an instant after the commit completes leaves the stored OID equally out of step with the current content. "The stored OID always matches the `.sdoc`'s current content" is an invariant that cannot hold for an external record.
- **The resulting state is not invalid.** A pin that does not match current content is a state `markharness validate` accepts. It is the ordinary state of the pinned-reference model [0023](0023-requirement-native-and-external-source.md) established, and it arises routinely whenever a `.sdoc` is updated, with no involvement from reconcile.
- **Detection already exists.** As the next section describes, this state is detected and reported as a stale pin.

This acceptance does not extend to changes under `.markharness/knowledge` or `.markharness/axes`; those are in scope per §1.

### 4. Existing validation and operational mitigations

- **Detection**: `markharness impact` compares an external Requirement's `source_revision` against the blob OID at head and reports any mismatch as a stale pin ([markharness-v2-design.md](../design/markharness-v2-design.md) §6.1 step 3, AC10c and AC19). It is an item independent of spec-change detection, so a lagging pin is never silently lost.
- **Correction**: re-running `knowledge reconcile` with `source_revision: current` on that Requirement advances the pin to the current blob OID. No special recovery command is needed.
- **Avoidance**: do not run reconcile while editing the `.sdoc`. In ordinary use as a single user's local CLI, an edit landing inside that window is unlikely to arise at all.

### 5. Triggers for revisiting this decision

Revisit this decision when any of the following becomes true.

- markharness itself becomes a writer of `.sdoc` files — for example, if a StrictDoc Adapter starts generating or updating them. The ownership changes, and with it the premises for locking and detection.
- `knowledge reconcile` comes to be used beyond a single user's interactive CLI, in concurrently executing environments such as CI jobs or a resident process. The chance of an edit landing in the window stops being negligible in practice.
- A stale pin is actually reported whose origin is reconcile's execution window rather than an ordinary `.sdoc` update.
- A decision is made to change `source_revision`'s meaning from "a pin naming the content as of resolution time" to "a pin guaranteed to match the content as of commit time". That change entails revising [0027](0027-declarative-knowledge-reconciliation.md) §5.

## Consequences

- The invariant `stale_plan` protects becomes explicit: the state markharness owns has not changed since the plan was built.
- Detecting changes to the external record is `impact`'s stale pin, settled as outside `reconcile`'s responsibility.
- No plumbing is needed to carry the Intent's referenced locator set through to the fingerprint, so `Plan`'s structure keeps matching the ownership of the state it read.
- Editing an unrelated `.sdoc` during a reconcile run does not interrupt it; the false-positive interruptions that including `.sdoc` in the fingerprint would produce do not occur.
- That a concurrent edit can store a stale blob OID becomes a recorded decision rather than an implicit premise re-argued at every review.

## Options considered and not taken

- **Re-resolve `source_revision: current` immediately before committing**: the window shrinks from milliseconds to microseconds but does not disappear. No "always matches" guarantee is obtained, and the resolution work is duplicated for an invariant that cannot hold.
- **Include the content of the Intent's referenced `.sdoc` files in the fingerprint**: this detects the change, but interrupts a reconcile with `stale_plan` when a `.sdoc` is edited during the run. Since the resulting state is valid and detection already exists, that interruption is a false positive from the user's point of view.
- **Bring `.sdoc` under the identity lock's protection**: a file markharness does not own cannot be locked against outside editors, and nothing guarantees that other tools would honour such a lock.
- **Give `reconcile` its own stale-pin detection**: this duplicates a judgement `impact` already makes, and breaks the separation in which `reconcile` applies desired state while `impact` analyses base-to-head differences.
