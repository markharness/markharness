# 0009: Layer the CLI into Domain / Application / Infrastructure

## Status

Accepted (Phases 1–2 were completed on 2026-08-18. In addition to typed change options and Git-operation consolidation, Application Use Cases for `generate`, `changes compute`, and `verify pending`, `CommandOutcome`, and Human/JSON Presenters are implemented. Phases 3–5 have not started).

## Context

The current implementation (a single Rust crate) is organized as a flat set of feature-named `.rs` files. As the codebase has grown, the following concrete problems have been confirmed.

- `src/cli.rs` is 2248 lines and handles argument parsing, Use Case execution, human-readable/JSON output, and exit-code decisions all in one place (32 `process::exit` calls, 92 `println!`/`eprintln!` calls).
- `src/changes.rs`'s `compute_changes(root, from_milestone, to_milestone, use_cache: bool, use_current_tree: bool)` takes two low-signal boolean parameters, making caller intent hard to read.
- Five direct `Command::new("git")` calls are scattered inside `changes.rs`, not consolidated into `src/git.rs`.
- `src/verify.rs`'s `trace`/`pending` functions call `fs::read_to_string` directly; the judgment logic (the branching that corresponds to Current/Pending/Stale/Unknown) is not separated from file I/O.
- `src/knowledge.rs` only provides YAML parse/serialize and has no normalized Snapshot abstraction. As a result, `src/generate.rs` and `src/validate.rs` each independently walk `knowledge/` via `fs::read_dir`, duplicating traversal logic.
- `changes.rs`'s `historical_testcases_by_feature` runs `git worktree add` → `generate_testcases` → `git worktree remove` for every milestone. `markharness backfill run` (UC6, priority-ordered backfill for large existing repositories, per PROJECT.md) is designed to process many milestone pairs, so this worktree-creation cost directly affects backfill's scalability.

A design proposal supplied by the user, "markharness Architecture Design Proposal" (dated 2026-08-18), presented a layering into Domain/Application/Infrastructure that addresses these. Review confirmed that its analysis of the current state matches the implementation (each item above was verified against the code), and that its Chapter 13 ("designs not adopted") judgments align with CLAUDE.md's operating rules ("reject compatibility-oriented design without assuming backward compatibility; always aim for the best product" and "do not count effort among a design's downsides"). However, the proposal's `ChangeAnalyzer` interface conflicted with the roadmap in [decisions/0008](./0008-verification-plan-product-roadmap.md) on one point, so this ADR corrects that and decides adoption.

## Decision

### 1. Adopt five Domain Modules

`KnowledgeWorkspace`, `TestcaseCompiler`, `ChangeAnalyzer`, `VerificationEngine`, and `BackfillCoordinator` are the core of the Domain layer. The interface each Module exposes to callers, and the processing it hides internally, are governed by the design document.

### 2. Adopt a one-way dependency: CLI → Application → Domain → Infrastructure

Introduce a `CommandOutcome` type and a `Presenter` trait, and eliminate `println!`/`eprintln!`/`std::process::exit` from the Domain and Application layers. Human-readable output and JSON output are both generated from the same `CommandOutcome`.

### 3. Generalize `ChangeAnalyzer`'s version reference to `CommitRef` instead of a fixed `MilestoneRef` (a correction to the original proposal)

The original proposal specified `ChangeAnalyzer::compute(from: MilestoneRef, to: MilestoneRef, options: ChangeOptions)`. However, [decisions/0008](./0008-verification-plan-product-roadmap.md) Stage 2 has already decided to "generalize the milestone-only UX into a common version range that adds PR base/head as first-class." Fixing the type to `MilestoneRef` through Phase 4 would force a redesign of the core interface when Stage 2 starts, creating rework. This ADR therefore adopts the following instead.

```rust
enum CommitRef {
    Milestone(MilestoneId),  // a tag name; resolved to a commit internally via git tag resolution
    Commit(CommitId),        // an arbitrary commit (e.g. a PR's base/head SHA)
}

impl ChangeAnalyzer {
    fn compute(
        &self,
        from: CommitRef,
        to: CommitRef,
        options: ChangeOptions,
    ) -> Result<ChangeSet>;
}
```

The existing `markharness changes compute` and `backfill run` continue to use `CommitRef::Milestone` (behavior unchanged). The PR Verification Plan feature added in [decisions/0008](./0008-verification-plan-product-roadmap.md) Stage 2 can pass `CommitRef::Commit` to the same `ChangeAnalyzer` without any interface redesign.

### 4. Do not introduce a `GitRepository` trait yet; consolidate into `git.rs` first (a correction to the original proposal)

The original proposal's Section 7.1 presented a complete `GitRepository` trait definition up front, which does not sit well with the proposal's own Chapter 13 principle of "not abstracting a dependency that has only one implementation." This ADR decides only as far as consolidating the direct git calls in `changes.rs` into `git.rs`, in Phase 1. Introducing a trait is deferred to the point where a concrete need arises — e.g. a fake implementation is needed for tests, or multiple Adapters become a requirement — and the trait's shape is not fixed at this time.

### 5. Adopt a `KnowledgeSource` trait

Unlike item 4, two Adapters (`WorkingTreeKnowledgeSource` / `GitTreeKnowledgeSource`) are clearly needed from the start, so trait abstraction is warranted here. `GitTreeKnowledgeSource` replaces `historical_testcases_by_feature`'s `git worktree add`/`remove` with direct reads of the tree/blobs under a commit, reducing backfill's scaling cost.

### 6. Adopt atomicity for generated-artifact updates

Make `generate`'s update of TestCases and the traceability index transactional across the whole directory (generate everything into a temp directory → verify → switch `generated/` over). On mid-way failure, keep the existing generated artifacts.

### 7. Keep full generation as the canonical behavior; add incremental generation only as an optimization

Do not run on incremental generation alone from the start. Even after incremental generation is added, periodic full generation in CI remains the basis for verification (retaining the Chapter 13 judgment).

### 8. Adopt the staged migration plan

| Phase | Content |
|---|---|
| Phase 1 | Replace `compute_changes`'s boolean parameters with `ChangeOptions`; consolidate direct git calls in `changes.rs` into `git.rs`. Pin existing behavior and the CLI contract with characterization tests. Directory layout unchanged. |
| Phase 2 | Introduce `CommandOutcome`. Move `std::process::exit` from the CLI to the Presenter. Split human-readable and JSON Presenters. Extract Application Use Cases from `generate`, `changes compute`, and `verify pending`. |
| Phase 3 | Add atomicity: generate TestCases and the traceability index into a temp area, then reflect into `generated/` only on success. |
| Phase 4 | Introduce `KnowledgeSnapshot`. Separate `TestcaseCompiler` from the filesystem. Separate the Data Loader from the Engine in Verification. Finalize `ChangeAnalyzer` on the `CommitRef` basis from Decision 3. |
| Phase 5 | Introduce `KnowledgeSource`. Replace worktrees with `GitTreeKnowledgeSource`. Add reconstructible indexes for Feature/ChangeEvent/Execution. Add throughput limits to Backfill (`--max-pairs`, `--time-budget`, etc.). |

See [domain-application-infrastructure-layering-design.md](../design/domain-application-infrastructure-layering-design.md) for detailed interface definitions, the Mermaid diagram, code layout, and test strategy.

## Consequences

- Removes the concentration of changes in `cli.rs`, making the blast radius of feature additions more predictable.
- `ChangeAnalyzer` can accommodate [decisions/0008](./0008-verification-plan-product-roadmap.md) Stage 2's PR Verification Plan feature without a backward-incompatible redesign.
- Deferring the `GitRepository` trait avoids a needless interface for a dependency that has only one implementation.
- Phases 1–3 do not change the existing CLI contract (exit codes, JSON output shape), so the refactor can proceed without user impact.
- Because full generation remains canonical even after incremental generation is added, cache inconsistencies stay easy to detect.

## Options considered but not adopted

- **Moving to microservices**: For local knowledge management inside a Git repository, the complexity of networking, authentication, distributed transactions, and operational infrastructure would be excessive. Not adopted.
- **An RDB as canonical data**: Would create a second source of truth alongside Git's history, review, and branch/tag workflow. Not adopted; SQLite and similar are limited to reconstructible index use.
- **Trait-abstracting every dependency (including `GitRepository`)**: Abstracting a dependency with only one implementation adds interfaces and reduces maintainability. Only abstract seams where multiple Adapters are concretely needed, as with `KnowledgeSource` (Decisions 4 and 5).
- **Running on incremental generation alone from the start**: Cache corruption, deletion detection, and inconsistencies from canonicalization-rule changes would be hard to detect. Not adopted.
- **Adopting `ChangeAnalyzer` with a fixed `MilestoneRef` as originally proposed**: Rejected because it conflicts with [decisions/0008](./0008-verification-plan-product-roadmap.md) Stage 2 and would cause rework when the PR Verification Plan is started; generalized to `CommitRef` instead (Decision 3).
