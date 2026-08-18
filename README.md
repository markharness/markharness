# markharness

日本語版 / Japanese version: [README.ja.md](./README.ja.md)

A Git-native management CLI (Rust) for test knowledge (Feature / Condition / ExpectedResult) that uses Git itself as the backend. It deterministically generates `TestCase`s from test knowledge hand-written as YAML under `knowledge/`, and automatically computes `ChangeEvent`s (a diff log of each Feature's version history — not a persistently queryable graph) by comparing Git tree SHAs between milestone tags. This main-lineage computation (`changes compute`) only looks at the tree diff between two milestones, so it does not depend on branching workflow (merge/squash/rebase); however, the secondary feature that audits the branching itself (`changes lineage`, `true_divergences`) assumes merge commits are preserved, so it does not work under a squash/rebase workflow (see [docs/en/cli-manual.md](./docs/en/cli-manual.md) §1.11/1.16 for details).

For the design background, see [docs/en/git-native-model-for-test-knowledge-management.md](./docs/en/git-native-model-for-test-knowledge-management.md); for product details, see [docs/en/product-operation.md](./docs/en/product-operation.md). For how to contribute, see [CONTRIBUTING.md](./CONTRIBUTING.md).

## Minimal tutorial

All commands in this section refer to `target/release/markharness` (`.exe` on Windows) after `cargo build --release`. It is written as `markharness` below.

A complete set of sample knowledge data is at [examples/todo-minimal/](./examples/todo-minimal/). It has no dependency on any external repository and is fully self-contained within this repository.

```bash
# 1. Prepare an empty repository for a new project
mkdir my-todo-project && cd my-todo-project
git init

# 2. markharness init — creates knowledge/ / axes/ / generated/ / executions/ / changes/ / schema/
markharness init

# 3. Register knowledge — use the axis registry and draft YAML from examples/todo-minimal/
cp -r <path to your markharness clone>/examples/todo-minimal/axes .
markharness knowledge apply <path to your markharness clone>/examples/todo-minimal/draft-v1.yml

# 4. Generate — deterministically generate TestCase from knowledge/
markharness generate

# 5. Milestone (git tag) — tag the first release point
git add -A && git commit -m "add todo-management/add-todo knowledge"
git tag v1
markharness milestone init v1

# --- Now suppose the spec changes (examples/todo-minimal/draft-v2.yml is
#     a draft that adds one new Condition to the same Feature) ---
markharness knowledge apply <path to your markharness clone>/examples/todo-minimal/draft-v2.yml
markharness generate
git add -A && git commit -m "add max-length condition"
git tag v2
markharness milestone init v2

# 6. changes compute — automatically compute the ChangeEvent between v1..v2
markharness changes compute v1 v2
cat changes/v2.yaml

# Build the same range as a reviewable, versioned Verification Plan
markharness plan --base v1 --head v2 --format json

# View the same plan and Feature History in a localhost-only read-only dashboard
markharness serve --base v1 --head v2

# 7. Record execution results, then check for TestCases still pending re-verification
markharness execution record tc-todo-management-add-todo-add-task-empty-title --milestone v2 --result pass --executor <your-name>
markharness execution record tc-todo-management-add-todo-add-task-max-length --milestone v2 --result pass --executor <your-name>
markharness verify pending --from v1 --to v2
```

The `case_id`s above follow the generator's `tc-{requirement.id}-{feature.id}-{behavior.id}-{condition.id}` rule; if you're unsure of the exact id after `generate`, read it from the generated file directly (e.g. `generated/testcases/todo-management/add-todo/add-task/empty-title.yml`).

The final `verify pending` detects that both TestCases affected by `v1..v2` already have execution records as of `v2`, and reports 0 `pending` (not-yet-re-executed) items. If you skip both recording steps and run `verify pending` directly, these same two TestCases are instead reported as pending (verified hands-on with the command sequence above).

See [docs/en/cli-manual.md](./docs/en/cli-manual.md) for the detailed options and output format of each command.

## Operational constraints

- **Git tags are a prerequisite for milestones**: `changes compute` / `backfill run` can only treat points that have been `git tag`ged as milestones. Release boundaries cannot be recognized unless a tag is created (the act of tagging itself, per UC4, remains a human decision point that `markharness` does not perform on your behalf).
- **`git notes` are not automatically synced by push/fetch**: Backfill progress records ([§4.3](./docs/en/git-native-model-for-test-knowledge-management.md)) are stored under `refs/notes/markharness-backfill`, which is outside the scope of ordinary `git push`/`git fetch`. When operating as a team on a shared repository, add `git push origin refs/notes/*` and a corresponding fetch configuration (e.g. `git config --add remote.origin.fetch '+refs/notes/*:refs/notes/*'`) for each member and CI environment.
- **Canonical import currently supports native knowledge and JUnit XML**: `markharness import --source native|junit --format json` emits a versioned canonical snapshot. TestRail/Xray migration into `knowledge/` remains outside the current scope; use `knowledge apply`/`add` for authoring settled knowledge.

## Unaddressed items

See [docs/en/git-native-model-for-test-knowledge-management.md §3.6 Summary of Implementation Status](./docs/en/git-native-model-for-test-knowledge-management.md#36-summary-of-implementation-status). Highlights:

- An importer from an existing TMS (TestRail/Xray, etc.) (UC8) — not implemented.
- The id-resolution cache's `canonicalization_rule_version` / `id_index_schema_version` — currently fixed values; an actual revision workflow is unverified.
- Rewriting the `id:` field in `feature.yml` breaks tracking as the same Feature (unlike a directory rename), severing version history. There is currently no migration procedure or alias mechanism (see [decisions/0004](./docs/en/decisions/0004-feature-id-change-migration.md) for the discussion).
- A general-purpose, independent id↔path index layer (e.g. tracking an id change that doesn't change the path) — not implemented.
- `verify trace` / `verify pending` are not applied retroactively to existing execution records predating their introduction (those without `verified_feature_tree_shas`) — treated as "unknown". `executions/*/results.yml` is JSON-Schema-validated (`schema/execution_result.schema.json`).
- `markharness backfill run` — not a resident daemon; designed to process one pass of unprocessed pairs per invocation and then exit (intended to be invoked repeatedly, e.g. from CI).

## Development

Implemented in Rust (edition 2024). See [CONTRIBUTING.md](./CONTRIBUTING.md) for the build/test/lint process and the pre-PR checklist.

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## Document index

- [CONTRIBUTING.md](./CONTRIBUTING.md) — build/development workflow, pre-PR checklist
- [docs/README.md](./docs/README.md) — document language index (Japanese/English)
- [docs/en/product-operation.md](./docs/en/product-operation.md) — product overview, use cases, directory structure (日本語: [docs/ja/product-operation.md](./docs/ja/product-operation.md))
- [docs/en/README.md](./docs/en/README.md) — index of design documents (paper, product operation picture, CLI manual, individual command specs) (日本語: [docs/ja/README.md](./docs/ja/README.md))
- [docs/en/cli-manual.md](./docs/en/cli-manual.md) — list of implemented/unimplemented CLI commands (日本語: [docs/ja/cli-manual.md](./docs/ja/cli-manual.md))
- [docs/en/git-native-model-for-test-knowledge-management.md](./docs/en/git-native-model-for-test-knowledge-management.md) — the research (paper draft) behind the design (日本語: [docs/ja/テスト知識管理のGit-nativeモデル_統合版.md](./docs/ja/テスト知識管理のGit-nativeモデル_統合版.md))
