# 0003: Response policy for the related-work coverage finding (GTM / tmt-fmf)

## Status

Accepted

## Background

The external evaluation review pointed out that GTM (testmanagement.com's "Git Test Management" tool) and tmt/fmf (an OSS test execution framework from Red Hat) were missing from the related work section (§2.4).

Independently re-verifying primary sources via WebFetch confirmed that the finding was factually valid.

- **GTM**: Version control is a manual integer scheme via appending v1/v2/v3 to the end of a filename and a version field (an optional feature). Change history depends on manual entries in CHANGELOG.md and Git's branch/PR review, and no automatic ChangeEvent-equivalent derivation mechanism could be confirmed in the specification.
- **tmt/fmf**: The `adjust` attribute is "dynamic modification of test metadata according to context such as product, distribution, architecture, etc.," a mechanism that handles spatial variation across environments. The Core specification contains no description of version history, change history, release/milestone concepts, or functionality equivalent to change-impact analysis.

For both tools, the results bore out the finding document's own hedged conclusion, "as far as could be confirmed within the scope of public documentation."

Section 3-1 of the finding document raised as a concern the possibility that "the two-way comparison (existing TMS vs. naive Git operation) may be oversimplified." However, the difference this paper claims is neither "version-history comparison of individual test cases" nor "being managed under Git" — it is **automatic derivation of `ChangeEvent` at milestone boundaries** and **version-history queries spanning multiple Features**. GTM depends on manual integer versioning (the very scheme this paper explicitly rejects in §3.2) and manual bidirectional linking, and has no automatic derivation mechanism. tmt's `adjust` is a mechanism along the spatial axis (across environments) rather than the temporal axis, so the point at issue is orthogonal. In other words, even adding GTM and tmt as comparison targets, both remain outside this paper's point of differentiation in that neither provides the "automatically derived temporal version history" that this paper claims.

## Decision

The response to this finding was framed not as weakening the novelty claim but as **refining the two-way comparison into a three-way comparison (commercial TMS / structured Git-managed / naive Git operation) and strengthening the claim's specificity and defensibility by naming and excluding the most confusable neighboring products**. The following was carried out.

1. **Restructuring §2.4**: The single-paragraph two-way comparison was reorganized into three categories: (1) commercial TMS / self-hosted TMS (existing description kept), (2) naive Git operation (the existing §1.1 description explicitly re-stated), and (3) a newly added structured-metadata-plus-Git-managed category (GTM, tmt/fmf), stating explicitly that GTM claims the same keyword ("Git-native test management") as this paper and that tmt is a highly recognized OSS tool in both academia and practice, while noting that neither has automatic version-history derivation or change-impact analysis.
2. **Adding a comparison table**: A table was added at the end of §2.4 comparing TestRail et al. / GTM / tmt-fmf / naive Git operation / this work across five axes: storage format, versioning-key scheme, automatic version-history derivation, change-impact analysis at milestone boundaries, and primary purpose. A footnote was added noting that GTM's manual integer scheme is exactly the scheme rejected in §3.2 of this paper.
3. **Treatment of GTMS**: Since GTMS is not a direct comparison target, it was not included in the body of §2.4; instead, one sentence was added at the end of Appendix A.1 (reasons for not adopting the LLM-utilization pivot proposal) noting that "reviewers may recall it as a neighboring product in the same domain."
4. **Additions to sources**: Six primary-source references related to GTM, GTMS, and tmt were added to the reference list.
5. **§1.3, §2.1–2.3, and Chapter 5 (evaluation plan) were treated as out of scope** and left unchanged. The sentence in §1.3, "unlike the individual test-case history-comparison/restoration functionality provided by existing TMS (TestRail, etc.) (§2.4)," was judged not to need changing, since what it refers to is automatically updated once §2.4 itself is refined.

## Impact / Conditions for future reconsideration

- The Plans/Stories/Policy specifications and plugin ecosystem of tmt, and the source implementation of GTM, remain unverified. The newly added paragraphs in §2.4 retain hedging expressions such as "as far as could be confirmed from public documentation" and avoid making definitive claims. This limitation carries over directly from the limitation (§5) of the finding document.
- If an opportunity arises in the future to verify the source code or non-public specifications of these tools, the descriptions in §2.4 and this decision record should be updated.
- This response (2026-08-13) has already been recorded in the changelog of `docs/git-native-model-for-test-knowledge-management.md`.
