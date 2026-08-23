# Review Policy

This document is the source of truth for code-review decisions in this repository. Reviewers must apply it when reviewing code, branches, diffs, pull requests, or implementations against a checklist or specification.

## Project threat model

`markharness` is a local CLI tool run by a single developer against their own working copy. Unless a specific accepted ADR or design document says otherwise for a specific feature, the baseline threat model is:

- The operating system, filesystem, and other processes running as the same user are trusted. A process with the same privileges as `markharness` itself (able to write anywhere `markharness` can write) is **not** an adversary this project defends against — it already has direct, simpler means to cause equivalent or worse damage than any race condition could.
- The project directory is not shared, over a network filesystem or otherwise, with an untrusted party while `markharness` is running, unless a specific finding's evidence shows this is how the project is actually used.
- Malicious *input* (a crafted repository, a crafted Knowledge YAML file, a crafted CLI argument) is in scope. A malicious *concurrent process racing filesystem operations with attacker-chosen timing* is out of scope by default.

Apply this threat model when scoring `Reachability` and `Required capability`. A finding whose only path to impact requires an actor who already has adversarial, concurrent, arbitrarily-timed write access to the project directory defaults to `Accepted risk` or `No finding`, not `Should fix` or `Must fix`, unless the actor could reach that position with meaningfully less capability than "already able to write to this project's files." Do not treat "theoretically possible with the right timing" as sufficient reachability on its own.

This does not relax `Must fix` items that are reachable through ordinary use, operator error, or normal (non-adversarial) concurrent use of the CLI by the same operator — those remain in scope regardless of this section.

## YAGNI

Apply YAGNI (You Aren't Gonna Need It) to both implementation and review: build and request only what the current, concrete requirement needs.

- Do not propose or request generalization, configurability, abstraction layers, or extension points for a need that does not yet exist. "This might be needed later" is not, by itself, grounds for a finding or a remediation.
- Do not propose hardening against a capability or scenario excluded by [Project threat model](#project-threat-model), even if it is technically possible to close. A closable gap is not automatically worth closing — weigh it against `Remediation cost` and `Change risk` like any other finding, and default to `Accepted risk`/`No finding` when the requesting actor's capability is already out of scope.
- When a finding's proposed remediation would add a new abstraction, dependency, or configuration surface, prefer the version of the fix that solves only the reported condition. Note any broader generalization as a `Follow-up` at most, never as part of the required remediation.

## Review axes

Review changes independently along both axes. Report findings under the corresponding heading; do not let one axis conceal the other.

### Standards

Determine whether the change follows repository instructions, `CONTRIBUTING.md`, accepted ADRs, design-document invariants, documented module interfaces, and established safety constraints. A documented project decision overrides a generic code-smell preference.

### Spec

Determine whether the change implements the requested checklist, issue, or specification completely and correctly. Report missing or partial requirements, incorrect behavior, and material behavior outside the requested scope.

## Decision axes

Evaluate every candidate finding using all applicable axes:

| Axis | Question |
|---|---|
| Contract | Does it violate an explicit specification, invariant, accepted ADR, or documented interface? |
| Impact | Can it cause data/evidence corruption, incorrect results, writes outside the project root, loss of recovery, denial of service, or misleading diagnostics? |
| Reachability | Is it reachable through ordinary use, operator error, abnormal environment state, or only adversarial filesystem/process control? |
| Required capability | What access must an actor already possess, and could that access cause equal or greater harm more directly? |
| Recoverability | Is recovery automatic, manual and reliable, uncertain, or impossible? |
| Likelihood | How plausible is the required timing, state, platform, and usage pattern? |
| Remediation cost | Is the fix local and testable, or does it require substantial platform-specific code, FFI, dependencies, or architectural complexity? |
| Change risk | Could the proposed fix introduce greater portability, correctness, or maintenance risk than the condition being fixed? |
| Verification | Can a deterministic automated test demonstrate the failure and constrain the fix? |
| Scope | Did this change introduce, worsen, or newly expose the problem, or is it an unrelated pre-existing condition? |

Do not rely on a numerical score alone. State the evidence and make a reasoned disposition from these axes.

## Dispositions

Assign every reported item exactly one disposition.

### Must fix

The change must be corrected before merge. Use this for specification violations and credible risks of data/evidence corruption, writes outside the project root, unrecoverable state, broken crash convergence, or ordinary concurrent operations violating identity invariants.

### Should fix

The remediation is proportionate and its benefit exceeds its implementation and change risk. The item does not block merge automatically; identify the concrete fix and verification needed.

### Accepted risk

The condition is real, but its likelihood, required capability, impact, or remediation cost makes acceptance reasonable. Acceptance requires a durable record containing:

- the condition and possible impact;
- the required capability and reachability;
- existing mitigations;
- the rejected mitigation and its cost/risk;
- the reason for acceptance; and
- an explicit trigger for reconsideration.

Record feature-specific accepted risks in the relevant accepted ADR or design document. A reviewer must not report a documented accepted risk again unless the reviewed change expands its reachability or impact, violates an acceptance condition, invalidates a mitigation, or makes a substantially cheaper and safer mitigation available.

### Follow-up

The issue is credible but outside the reviewed scope and was not introduced, worsened, or newly exposed by the change. Report it separately from blocking findings and identify where it will be tracked.

### No finding

Do not report stylistic preference, tooling-enforced formatting, unsupported speculation, or behavior already covered by an applicable accepted-risk record. If a comment or design document promises a stronger guarantee than the implementation provides, that mismatch remains a finding even when the underlying risk could otherwise be accepted: either strengthen the implementation or narrow the documented guarantee.

## Repository defaults

Apply these defaults unless a more specific accepted ADR or design document says otherwise:

- Corruption or misassociation of Knowledge, identity events, generated evidence, or execution evidence is `Must fix`.
- Failure of crash recovery to converge deterministically is `Must fix`.
- A race reachable through ordinary concurrent CLI use that violates a documented invariant is `Must fix`.
- Following a symlink or junction to write outside the project root is `Must fix`.
- Loss of error cause that turns a persistent filesystem or Git failure into a normal/retryable result is at least `Should fix` and becomes `Must fix` when it can permit unsafe continuation.
- A low-cost, portable, deterministic, and testable mitigation normally favors `Should fix` over risk acceptance.
- A narrow platform-specific residual risk may be accepted when closing it requires unstable or high-risk FFI or substantial dependencies, the actor already needs destructive write access, and the remaining guarantee is documented precisely.
- A pre-existing issue is `Follow-up` unless the reviewed change worsens it, newly makes it reachable, or depends on the violated behavior for correctness.
- Adding raw platform FFI (a manual `extern` declaration, direct syscalls, or a new low-level dependency like `libc`/`windows-sys`) to close a race is a `Change risk` signal, not a free action. Prefer `Accepted risk` over adding or expanding raw FFI when the only actor who can trigger the condition already needs the level of access described in [Project threat model](#project-threat-model). Reserve FFI-based hardening for conditions reachable through ordinary use, operator error, or an actor with meaningfully less access than "can already write to this project."

## Avoiding review churn

- When this policy itself changes, or a new requirement (such as the six accepted-risk elements) is introduced, check every existing accepted-risk record against it in one pass and report the complete set of gaps as a single finding (or one finding per affected document, not one per missing element or per language). Do not spread a single policy-compliance gap across multiple sequential review rounds.
- A finding whose entire content is "this accepted-risk record's prose doesn't literally restate one of the required elements," where the underlying risk determination (condition, capability, reachability, mitigation, acceptance reasoning) is otherwise unchanged and correct, is `No finding` once that determination has already been recorded once in substance — do not require a second round to reformat it into stricter headings or bullet structure.
- Before opening a new review round on a change that is itself the fix for the previous round's finding, confirm the previous finding's evidence no longer applies. If the fix is correct, the review is done for that finding; do not use the same evidence location to open an adjacent, narrower finding unless it describes a genuinely different condition.

### Complete-pass and re-review discipline

A review should give the author one complete, actionable picture of the current change, not reveal independently discoverable problems over a sequence of rounds.

- At the start of a review, enumerate the behaviors changed by the diff and check each one against both the Standards and Spec axes, including its ordinary failure path and any platform behavior that is material to an explicit contract. Treat observations made before that pass finishes as interim, not as the final review result.
- Complete the full pass before issuing the review conclusion, and report all findings discovered in that pass together. A later round must not turn an already-inspected, unchanged behavior into a new finding merely to pursue a nearby edge case.
- Apply YAGNI to remediation as well as implementation. When a narrow fix satisfies the current issue and preserves the applicable invariants, recommend that fix instead of requiring a general mechanism. If the author nevertheless introduces a broader contract, review the newly promised behavior, but do not require the broader contract to be retained.
- For an issue-scoped change, review Standards and Spec as required above, but make a blocking finding only for a violation introduced, worsened, or newly exposed by the change. Unrelated pre-existing problems are `Follow-up` at most.
- Reserve `Must fix` for the conditions defined in this policy. Do not promote a portability edge case or unusual filesystem object to `Must fix` unless it is credibly reachable in ordinary use and violates an explicit requirement or invariant. Prefer narrowing an unnecessary guarantee over completing a general solution for every edge case.

On a re-review of a corrective change:

1. First determine whether every previous finding is resolved, still present, or deliberately addressed by narrowing the implementation or its documented contract.
2. Review all behavior newly added or changed by the correction in the same pass. A new finding is appropriate when the correction introduced, worsened, or newly exposed the condition.
3. If a new finding concerns behavior that was unchanged and available for inspection in the previous completed review, label it explicitly as a reviewer omission. Do not present it as a consequence of the author's latest correction. Consolidate any remaining independently discoverable issues from that behavior in the same round.
4. End with an explicit merge disposition: `mergeable`, `mergeable with non-blocking follow-up`, or `not mergeable`, with the blocking findings identified. When no blocking finding remains, say so directly.

## Finding format

Each finding must be self-contained and include:

```text
- [Disposition][Severity] Short title
  - Evidence: file and line, failing test, or reproducible behavior
  - Violated contract: specification, invariant, standard, or interface
  - Reachability and required capability
  - Impact and recoverability
  - Proposed remediation
  - Remediation cost and trade-off
  - Verification required
```

Use severity (`Critical`, `High`, `Medium`, or `Low`) to describe impact and urgency; use disposition to state what should happen. They are separate judgments.

If an axis has no findings, state that explicitly. End the review with the number of findings per axis and the highest-severity disposition within each axis. Review work is complete only when every changed behavior and every applicable requirement has been considered under both axes.
