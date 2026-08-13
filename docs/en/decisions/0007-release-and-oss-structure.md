# 0007: Overall design of the release structure and OSS publication structure

## Status

Accepted (partially executed; Section 7 not yet executed).

- The operational practice for ADR numbering/placement was changed to lifecycle management via a single `docs/decisions/` directory plus a Status line as in this section (previously: unconfirmed documents were moved to a separate `docs/internal-notes/` directory, but this caused a fragmented number space and path staleness (described below), so it was discontinued. For details, see [Michael Nygard, "Documenting Architecture Decisions"](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions) and the practice of MADR (https://adr.github.io/madr/), which this follows). This 0007 itself was moved back from `docs/internal-notes/` to `docs/decisions/`, as the first application of this change.

- Section 1 (unifying into a single repository), Section 2 (license correction), Section 3 (versioning), Section 4 (new CI), Section 5 (documentation structure), and Section 6 (governance files) have been executed and committed locally within this repository (2026-08-14). However, regarding the deletion of `docs/gap-analysis-mh-sample-test-case.md` in Section 1, since it was actually being actively referenced as an audit log from README.md, the paper body, and design/change-event-verification-tracking-spec.md, the decision was changed to keep it rather than delete it. The relevant passages in Section 1 and Section 7 have been corrected accordingly (they previously diverged from the actual state).
- The initial policy in Section 1, "there is no necessity to hide `CLAUDE.md`/`PROJECT.md`/`.claude/` etc., so they will be published," was reversed during local execution at the user's request (minimal public footprint). The judgment that these files contain nothing confidential remains unchanged, but since they are template-operation files of little general interest to readers of a public repository, the policy was changed to exclude them from tracking via `.gitignore` and keep them under private local operation, and this has been executed (`git rm --cached`). The substantive rules (build/test commands, the Pre-PR checklist, license/versioning/docs-placement rules) have already been ported to CONTRIBUTING.md/README.md/SECURITY.md/docs/product-operation.md.
- For the release pipeline in Section 4, adopting `cargo-dist` was considered at the design stage, but to avoid adding a dependency during local execution, equivalent functionality (cross-platform builds, checksums, CHANGELOG generation) was implemented with a hand-rolled matrix build using standard actions such as `actions/upload-artifact`. The targets were also narrowed to four, excluding `x86_64-pc-windows-gnu` (`x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`) (no demand was confirmed for GNU-toolchain Windows builds; it will be added if and when needed). The body of Section 4 has been corrected accordingly.
- Section 7 (deleting and recreating the existing public repository `markharness/markharness`, history rewriting via `git filter-repo`, force push, and reconfiguring Pages) has not been started, as it is an irreversible operation affecting externally shared state (stars, forks, existing links). Execution requires final confirmation by the user themselves on GitHub.
- Once Section 7 is complete, this section (Status) of this file need only be updated to `Accepted (execution complete)`. The previous practice (after completion, "transcribing" into `docs/decisions/` and deleting the original file in `docs/internal-notes/`, as done for 0006) failed to preserve the identity of the number/file, and in fact once caused a bug where this very file referenced the path `docs/decisions/0005-review-2026-08-13-triage.md` (which presupposed a move) while its actual location (`docs/internal-notes/`) diverged from that. After switching to the single-directory-plus-Status-line practice, this kind of move-induced staleness can no longer occur structurally.
- Section 5 (documentation structure) was updated further within 2026-08-14: every document under `docs/` (the paper, cli-manual, product-operation, gap-analysis, decisions, design) was split into per-language directories `docs/ja/` and `docs/en/`, with the English side newly written in full. The root `README.md`/`README.ja.md` was actually split as the original table in Section 5 had already anticipated (suffix scheme, not a directory split), with `README.md` as the English (default) version and `README.ja.md` as the Japanese version. The READMEs under `examples/todo-minimal/` were split the same way. The table in Section 5 has been updated to match the current layout.

## Context

markharness is currently published in the following state.

- The public repository `markharness/markharness` and the development repository (this repository, the full set including `.github/` / `PROJECT.md` / `docs/decisions/` / `tests/` / `examples/` originating from the template) are separate.
- The public repository's `Cargo.toml` has `version = "0.1.0"` (SemVer), but the git tag is `v2026.08.13` (CalVer) — two version identifier schemes coexist.
- Neither the public nor the development repository has a `.github/workflows/` directory; there is no CI or release automation.
- The dependency crate `kakasi 0.1.0` is GPL-3.0 licensed, which conflicts with the MIT license declared for the project as a whole (because static linking makes it a combined work).
- `markharness.com` is simply README.md auto-rendered as-is by GitHub Pages, with no additional build configuration.
- The GitHub Wiki has not been created (existence could not be confirmed; this includes some speculation).
- The development repository's `docs/` contains a number of documents not present in the public repository (`cli-manual.md`, `product-operation.md`, `testcase-generation-design.md`, etc.).

Given this, the release process and the OSS publication structure are redesigned from scratch in their best form, without regard for backward compatibility.

## Decision

### 1. Repository structure: consolidate into a single repository

The current two-repository structure is discontinued, and development and publication are done in a single repository.

Rationale:

- The public repository's commit history consists only of curation-purpose commits (e.g., "Create CNAME"), so the implementation history cannot be traced via `git blame`. This hinders external contributors' understanding of the code.
- Drift caused by manual synchronization has already occurred (the version mismatch between Cargo.toml and the git tag).
- The public repository lacks `tests/`/`examples/`, so external contributors cannot access assets on the development-repository side. There is effectively no destination to submit a PR to.
- Internal operational files such as `PROJECT.md`/`CLAUDE.md`/`.claude/` contain no confidential information such as credentials (confirmed), so there is no actual harm in publishing them. This is not, by itself, a reason to avoid consolidating into a single repository, but as noted in the Status section above, exactly how much is actually published (included in tracking) was separately changed to exclude them from tracking, per the minimal-public-footprint policy.

Operational rules:

- Development continues on the `main` branch. Commits that fail CI are not merged.
- Binary release artifacts are generated only on push of a `v*` tag (Section 4). However, since Pages (`markharness.com`) continues to auto-render the README.md of the main branch as it currently does (Section 5), updates to the docs site are reflected immediately on every push to main, not on tag triggers. These two are clearly distinguished as separate things with different generation triggers. Immature changes are visible to users only via this Pages-based README/docs; binary distribution artifacts are always a stable snapshot as of when a tag was cut.
- ADR numbering/directory is fixed to a single `docs/decisions/` directory, and unconfirmed/partially-executed documents are also placed here. The lifecycle (Proposed / Accepted / Rejected / Deprecated / Superseded, plus item-specific annotations such as `Accepted (partially executed)` indicating mid-execution status) is expressed in the `## Status` section at the top of each file, and the file is not moved to a different directory when its state changes (following the practice of Michael Nygard's "Documenting Architecture Decisions" and MADR; see "fragmented number space" and "link staleness from moves" for the reasons). Individual files that should be kept private at OSS publication time (e.g., working notes that only reflect rejection decisions and do not contribute to product decisions) are removed on a per-file basis via `git filter-repo` in Section 7, rather than via directory separation.
- Documents not directly related to the product or without lasting relevance, such as `docs/template-readme.md`, are deleted or archived. `docs/gap-analysis-mh-sample-test-case.md` was initially included in this category, but since it is actively referenced as an audit log from README.md, the paper body, and `design/change-event-verification-tracking-spec.md`, it is excluded from this treatment and kept.

### 2. License: correct dependency consistency as the top priority

Replace `kakasi` (GPL-3.0) with an MIT/Apache-2.0-compatible alternative, or implement the relevant functionality in-house. This is undertaken with priority over release automation. If left unaddressed, distributing a binary declared under the MIT license would continue to fail to satisfy the license terms.

After the correction, incorporate a `cargo-deny` `licenses` check into CI, failing CI the moment a dependency not on the allow-list (MIT, Apache-2.0, BSD family, etc.) is introduced.

### 3. Versioning: unify on SemVer

Discontinue the CalVer tag (`v2026.08.13`) practice, and make the `version` field in `Cargo.toml` and the git tag (`vX.Y.Z`) match as a single source of truth.

- Continue on the 0.x series, with UC8 (existing-TMS importer) implementation and schema stabilization as the criteria for reaching 1.0.
- The tag must always match the `version` in `Cargo.toml`. A CI step verifying "does the tag name match the version in Cargo.toml" is added, failing the release job on mismatch.

### 4. CI: separate the PR gate from the release pipeline

**PR gate (triggered on push/PR)**

- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt --check`
- `cargo audit` (vulnerabilities)
- `cargo deny check licenses` (license consistency, to prevent recurrence of the kakasi issue)
- `markharness verify` (checking consistency between `generated/testcases/*.yml` and `knowledge/`)

The existing Pre-PR checklist (manually operated) is translated directly into CI job definitions.

**Release pipeline (triggered on push of a `v*` tag)**

1. Verify that the tag name matches the `version` in `Cargo.toml`.
2. Cross-platform builds (`x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`). Adopting `cargo-dist` was originally envisioned, but to avoid adding a dependency it was implemented as a hand-rolled matrix build using standard actions such as `actions/upload-artifact`. `x86_64-pc-windows-gnu` was excluded from targets since no demand was confirmed (to be added if it becomes necessary).
3. Generate checksums (SHA256SUMS). No installer script is generated at this time (not implemented; to be added if needed).
4. Attach to GitHub Releases. Auto-generated CHANGELOG from Conventional Commits (`git-cliff`) is attached as release notes.
5. (Optional) Publish to crates.io via `cargo publish`. Not mandatory unless usage as a library is anticipated. Not implemented.

### 5. Documentation structure

| Type | Location | Notes |
|---|---|---|
| Landing page, quick start, command overview | `README.md` (English, default) / `README.ja.md` (Japanese) | Unchanged. GitHub Pages (`markharness.com`) auto-renders `README.md`. |
| Design background (the paper) | `docs/ja/テスト知識管理のGit-nativeモデル_統合版.md` / `docs/en/git-native-model-for-test-knowledge-management.md` | Already published. Originally a single directory (directly under `docs/`); on 2026-08-14 it was split into per-language directories `docs/ja/`/`docs/en/`, and a full English translation was added. |
| Full CLI command reference (including implemented/unimplemented) | `docs/ja/cli-manual.md` / `docs/en/cli-manual.md` | Newly published. Explicitly marking unimplemented items prevents duplicate issues for the same request. |
| Operational picture | `docs/ja/product-operation.md` / `docs/en/product-operation.md` | Newly published. Needed for adoption decisions. |
| Implementation design specs | `docs/ja/design/` / `docs/en/design/` (newly created, moving `testcase-generation-design.md` etc. here) | Newly published. For contributors. Check for divergence from the implementation before publishing. |
| Design decision records (ADR) | `docs/ja/decisions/` / `docs/en/decisions/` | Managed per language as a single directory plus a `## Status`/`## ステータス` section (Nygard/MADR approach); numbering is shared across both languages. With all files kept at the same path, those with `Status` of `Accepted` (0001, 0002, 0003, 0004, 0005, 0006, 0007) are left as-is at publication time too. Working notes that do not contribute to product decisions (e.g., a response to an external review consisting only of a rejection determination) that should be kept private, if any, are removed on a per-file basis via `git filter-repo` in Section 7, without separating directories. |
| Community knowledge (FAQ, troubleshooting) | (Not created. GitHub Discussions in the future if needed) | Wiki will not be created. Editing flows that bypass PR review are inconsistent with this project's TDD/ADR practices. |

`docs/ja/` and `docs/en/` are maintained as mirrors of each other — a change to one is mirrored to the other in the same PR (see the "Simultaneous update of Japanese/English documentation" rule in CLAUDE.md for details).

Pages (`markharness.com`) continues to auto-render README as it currently does; a dedicated documentation-site platform (mdBook, etc.) is not adopted. Links between README and docs are judged sufficient at this stage.

### 6. New governance files

- `CONTRIBUTING.md` — build instructions, how to submit a PR, the Pre-PR checklist (excerpted from the relevant part of PROJECT.md)
- `CODE_OF_CONDUCT.md` — a standard template such as the Contributor Covenant
- `SECURITY.md` — vulnerability-reporting contact and process
- `.github/ISSUE_TEMPLATE/`, `PULL_REQUEST_TEMPLATE.md`

## Consequences

- Consolidating into a single repository concentrates issues/PRs/commit history in one place, lowering the barrier to entry for external contributors.
- Resolving the license contradiction ensures the legal consistency of distributed artifacts.
- Automating versioning and releases structurally prevents drift caused by manual operation (the version mismatch discovered this time) from recurring.
- Not adopting a Wiki and keeping Pages as-is consolidates the "source of truth" for documentation into `docs/`.

## 7. Deleting and recreating the existing public repository

As a premise, deleting the existing public repository `markharness/markharness` and recreating it from scratch is deemed acceptable. Rather than simply "tidying up the existing repository," the reasons for choosing deletion and recreation are as follows.

- The existing public repository's commit history consists only of curation-purpose commits (e.g., "Create CNAME") and has no substantive value. Building on a valuable history (the actual history of the development repository) as the foundation achieves a cleaner result than grafting history on afterward.
- The coexistence of the CalVer tag (`v2026.08.13`) and SemVer (`Cargo.toml`'s `0.1.0`) can be resolved all at once without worrying about backward compatibility. Starting a new repository with the correct tagging scheme from the outset is less error-prone than re-cutting existing tags (force update).
- The state with the kakasi (GPL-3.0) issue corrected can be made the "first commit." In the existing repository's history, the fact remains that a binary containing a license-problematic dependency was already released, but this can be avoided in a new repository (though the possibility that the old repository's Release artifacts have already been downloaded externally cannot be eliminated; see below).

### Execution order

Deletion is performed last. Preparation is completed in the following order before deleting and recreating.

1. **Correct the kakasi issue within the development repository** (Section 2). At this point, Cargo.toml's license declaration matches reality.
2. **Create a clean history with non-public paths removed from the development repository's history**. Using `git filter-repo` (filter-repo is recommended over BFG, for its greater flexibility in history rewriting and because GitHub officially recommends it), remove paths unsuitable for publication — `docs/template-readme.md`, `CLAUDE.md`, `PROJECT.md`, `.claude/`, `.github/copilot-instructions.md`, `.github/instructions/`, `.github/prompts/`, `.github/skills/`, `checklist-*.md` (per the minimal-public-footprint policy, see the Status section), etc. — along with their history. `docs/gap-analysis-mh-sample-test-case.md` is actively referenced as an audit log and is not included in the removal targets. The ADRs (0001–0007) under `docs/decisions/` are all `Status: Accepted` and are not included in the removal targets (Section 5). This allows publication not as a mere copy of the latest snapshot but **while preserving the actual development history** (gaining the "history transparency" benefit of OSS described in Section 1, in full-history form).
3. **Reconstruct tags**. Do not use the existing CalVer tags; renumber with SemVer starting from `v0.1.0` (or alternatively, start from a version that reflects the accumulated development to date, such as `v0.2.0`).
4. **Incorporate CI/CD at this point**. Ensure that the PR-gate CI and release pipeline (Section 4) function from the new repository's very first commit. Being able to have it "built in from the start" rather than "added later" is a clear advantage of recreating the repository.
5. **Include the governance files (Section 6)** added, as part of the initial set of commits.
6. Delete the existing `markharness/markharness`.
7. Create a new repository under the same name and push the filtered history.
8. Reconfigure the GitHub Pages custom domain setting (`markharness.com`) on the new repository. The DNS side (CNAME record) can be reused as-is, but the "Pages enabled" setting on the repository side is lost by deletion and must be reconfigured immediately after recreation.

### Irreversible impacts to be aware of

These are known side effects of deletion that are technically impossible to work around, and should be understood before execution.

- **The association with stars, watch counts, and forks is lost**. Since delete-then-recreate is treated as a different repository, existing stars and forks are reset. This repository has been public only a short time (observed commits are within a few days) and the number of actual users is presumed small, but the exact number could not be confirmed from this session due to GitHub API limitations. It is recommended to check the repository's Insights (Stargazers / Forks / Traffic) before deletion to judge whether the scale is acceptable.
- **If users have already downloaded the existing Release artifacts (binaries), those binaries will retain the kakasi (GPL-3.0) issue**. Deleting the repository does not recall already-downloaded binaries. The actual harm is thought to be small (experimental-stage declared software), but not zero.
- **Any external links to the existing repository (blog posts, social-media posts, crates.io, etc.) will 404**. Whether such external references exist could not be confirmed at this time.

## Options considered but not adopted

- **Keeping the two-repository structure + automating synchronization**: Feasible, but does not gain the "development history transparency" advantage compared to single-repository consolidation. If synchronization automation cost is going to be paid anyway, single-repository consolidation is structurally simpler and has greater effect.
- **Adopting a GitHub Wiki**: There is currently no accumulated community knowledge, and an editing flow that bypasses PR review is inconsistent with this project's quality discipline. May be reconsidered in the future with Discussions.
- **Making the entire project GPL-3.0**: Considered as a solution to the kakasi issue, but rejected because it imposes strong constraints on users (obligation to disclose derivative-work source) and raises the adoption barrier as a CLI tool. Priority is given to replacing the dependency instead.
- **Tidying up and reusing the existing public repository as-is (history rewriting via force-push, etc.)**: Since the existing history has no value, building on the development repository's actual history as the foundation is ultimately simpler. While there is an advantage in retaining the existing repository's settings (Pages, CNAME, etc.), the cost of reconfiguring these after delete-and-recreate is small, so this advantage was judged not decisive.
- **Squashing the new repository's history into a single initial commit**: Rejected because retaining the development history better serves OSS transparency. That said, if the early stages of the development repository's history contain a lot of trial-and-error (e.g., before/after applying the template) that makes the history hard to read, there is room to use `git filter-repo` to clean up only the meaningless intermediate commits.
