# Review Policy

This document is the source of truth for code-review decisions in this repository. Reviewers must apply it when reviewing code, branches, diffs, pull requests, or implementations against a checklist or specification.

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
