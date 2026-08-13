# Contributing to markharness

markharness is a Git-native test knowledge management CLI written in Rust. Contributions are welcome via pull request.

## Building

```sh
cargo build
```

> **Windows note**: if linking fails with the default `msvc` toolchain, run `rustup override set stable-x86_64-pc-windows-gnu` and add WinLibs (mingw64) `bin` to `PATH`.

## Development workflow

This project develops `src/` code with Red-Green-Refactor TDD: write a failing test first, make it pass with the minimal change, then refactor. Do not add production code without a covering test.

## Before opening a pull request

All of the following must pass locally (they are also enforced in CI):

- [ ] `cargo test` — all tests pass
- [ ] `cargo clippy --all-targets -- -D warnings` — zero lint errors
- [ ] `cargo fmt --check` — formatted
- [ ] `cargo audit` — no known vulnerabilities
- [ ] `cargo deny check licenses` — no dependency license outside the allow-list in `deny.toml` (see [Adding a dependency](#adding-a-dependency))
- [ ] `generated/testcases/*.yml` matches `knowledge/` (`cargo run -- generate` produces no diff)
- [ ] No secrets in code, logs, or commit messages

## Adding a dependency

Before adding a new crate, check its license. Only licenses listed in `deny.toml`'s `[licenses] allow` (MIT, Apache-2.0, BSD-2-Clause, BSL-1.0, Unicode-3.0, Unlicense, Zlib, MIT-0, and compatible dual/multi-license combinations) are acceptable, because markharness is distributed under MIT. GPL/LGPL/AGPL-licensed dependencies are not acceptable, even transitively — this was the cause of a real license-compliance incident (see [docs/en/decisions/](./docs/en/decisions/) for the kakasi → wana_kana replacement). If a needed crate has an incompatible license, do not add it silently; open an issue to discuss alternatives.

## Versioning

`Cargo.toml`'s `version` field is the single source of truth (SemVer). Release tags are always `vX.Y.Z` matching that value exactly; CI fails a release build if they diverge. There is no separate CalVer or date-based tagging scheme.

## Documentation layout

- `docs/en/decisions/` (Japanese source: `docs/ja/decisions/`) — Architecture Decision Records (ADRs), one sequential number space, one directory per language (Michael Nygard's ADR convention / MADR). Each file starts with a `## Status`/`## ステータス` section (Proposed / Accepted / Rejected / Deprecated / Superseded, or a project-specific note like "Accepted, partially executed"). In-progress or not-yet-finalized decisions stay here too — status changes are edits to that section, not moves to another directory.
- `docs/en/design/` (Japanese source: `docs/ja/design/`) — implementation-level design docs aimed at contributors.
- English and Japanese docs under `docs/en/` and `docs/ja/` are updated together — a change to one language's docs should be mirrored in the other in the same PR.
- Transient documents (investigation notes, superseded drafts) should be deleted once they've served their purpose rather than left to accumulate.

## Commit style

Use [Conventional Commits](https://www.conventionalcommits.org/) prefixes (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`) — the release changelog is generated from them automatically.
