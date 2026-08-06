---
description: "Use when developing code with Test-Driven Development. Covers the Red-Green-Refactor cycle, test structure, and TDD discipline for Rust with cargo test."
applyTo: src/**/*.rs, tests/**/*.rs
---

# TDD Workflow

> Respond in the language the user is using in the chat.

This instruction defines the standard TDD (Test-Driven Development) process for this project.
It is referenced by the `/dev-tdd` prompt and applies to all Rust source and test files.

> **Stack note**: Commands below reflect this project's stack (Rust + `cargo test` + `cargo clippy`) as defined in [PROJECT.md](../../PROJECT.md). If the stack changes again, update the commands here to match PROJECT.md's 標準コマンド table — the Red-Green-Refactor discipline itself is stack-independent.

## The Red-Green-Refactor Cycle

Every piece of functionality is built through this cycle. No exceptions.

### 1. Red — Write a Failing Test

- Write exactly ONE test that describes the next small piece of behavior you need.
- Run the test and confirm it **fails** for the expected reason.
- The failure message should clearly describe what is missing.
- Do NOT write any production code yet.

```bash
cargo test <test-name>
```

### 2. Green — Make It Pass

- Write the **minimum** production code needed to make the failing test pass.
- Resist the urge to write more than necessary — no "while I'm here" additions.
- Run the test again and confirm it **passes**.
- Run the full test suite to ensure nothing else broke:

```bash
cargo test
```

### 3. Refactor — Clean Up

- Improve the code's structure, naming, or clarity without changing behavior.
- Run all tests after each refactoring change to confirm everything still passes.
- Apply lint and formatting:

```bash
cargo clippy --all-targets -- -D warnings
cargo fmt
```

### Then Repeat

Pick the next behavior and start a new Red-Green-Refactor cycle.

## Test File Conventions

- Unit tests live alongside source files in a `#[cfg(test)] mod tests` block within the same file (e.g. `src/parser.rs`).
- Integration tests that exercise the CLI as a whole live in `tests/*.rs`.
- Use descriptive test function names that read like specifications:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_single_valid_entry_with_all_fields() { /* ... */ }

    #[test]
    fn returns_empty_when_input_has_no_entries() { /* ... */ }

    #[test]
    fn handles_multiple_entries_in_one_input() { /* ... */ }
}
```

## Guiding Principles

- **Small steps**: Each cycle should take minutes, not hours. If you are stuck, the step is too big — break it down.
- **Trust the tests**: If all tests pass, the code works. If a test is missing, write it before adding code.
- **One behavior per test**: Each test should verify exactly one thing. Multiple assertions are fine if they all describe the same behavior.
- **No production code without a test**: Every line of production code exists because a test required it.
- **Determinism matters here specifically**: markharness's core value proposition (`generated/testcases.yaml`, `changes/<milestone>.yaml`) depends on deterministic output. Tests for generation/derivation logic should assert exact output, not just "no error".

## Vulnerability Check

After completing a feature (a group of TDD cycles), run:

```bash
cargo audit
cargo clippy --all-targets -- -D warnings
```

Address any reported vulnerabilities or lint warnings before moving on.
