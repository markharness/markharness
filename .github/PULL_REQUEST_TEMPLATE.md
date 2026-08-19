## What

## Why

## Pre-PR checklist

- [ ] `cargo test` passes
- [ ] `cargo clippy --all-targets -- -D warnings` passes
- [ ] `cargo fmt --check` passes
- [ ] `cargo audit` reports no vulnerabilities
- [ ] `cargo deny check licenses` passes (no new dependency outside the allow-list in `deny.toml`)
- [ ] `.markharness/generated/testcases/*.yml` matches `.markharness/knowledge/` (`cargo run -- generate` produces no diff)
- [ ] `src/` changes were developed test-first (Red-Green-Refactor)
- [ ] No secrets in code, logs, or this PR description
