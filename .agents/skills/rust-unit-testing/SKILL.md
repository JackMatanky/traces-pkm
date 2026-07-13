---
name: rust-unit-testing
description: >
  Use when writing, reviewing, or refactoring Rust unit suites: #[cfg(test)],
  #[test], source-derived coverage gaps, doctests, lint suppression review,
  and unit suite code-quality review.
license: MIT
metadata:
  version: "1.0.0"
  companion_to: rust-testing
---

# Rust Unit Testing

Unit work starts from source understanding, not from guessed edge cases.

## Choose Workflow

| Task | Use |
|---|---|
| Write a new unit suite | `references/writing-suites.md` |
| Review/refactor an existing unit suite | `references/review.md` |
| Maintain doctests | `references/doctests.md` |
| Review suite code clarity or lint suppressions | `references/code-quality.md` |

## Unit Suite Basics

Put unit tests in `#[cfg(test)] mod tests { ... }` next to the implementation so private items are reachable and the suite moves with the code during refactors.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_error_when_name_is_empty() {
        // Arrange
        let name = "";

        // Act
        let result = User::new(name);

        // Assert
        assert!(matches!(result, Err(UserError::EmptyName)));
    }
}
```

Rules:
- Test the behavior contract and local invariants, not implementation line-by-line.
- Keep Arrange, Act, Assert visually separate.
- `unwrap()`/`expect()` is acceptable in Arrange; in Act/Assert, capture the result and assert explicitly.
- Prefer Arrange helpers that build data; avoid hidden assertions in helpers.
- Use fresh per-test fixtures; avoid shared mutable state, uncontrolled time, and randomness.
- Keep unit tests local, deterministic, and fast. Use integration tests for cross-boundary workflows.

For non-trivial units, enumerate happy paths, boundary conditions, failure paths, invariants, and state transitions before writing tests. Use submodules per behavior group when a file has multiple meaningful units.

## Shared References

Use `../rust-testing/references/assertions.md`, `../rust-testing/references/raii-cleanup.md`, `../rust-testing/references/table-driven.md`, `../rust-testing/references/property-based.md`, `../rust-testing/references/mocks.md`, `../rust-testing/references/snapshots.md`, `../rust-testing/references/async.md`, `../rust-testing/references/concurrency.md`, and `../rust-testing/references/commands.md` when a workflow reaches that branch.

## Completion

- New suite: every non-`n/a` case-surface row maps to a named test.
- Review: output includes a gap list, a per-test audit, a code-quality finding list, and a lint suppression inventory, each explicitly empty if no findings.
- Doctest work: every changed public example is run with `cargo test --doc`, marked `no_run`/`compile_fail`/`ignore-*` with reason, or explicitly out of scope.
