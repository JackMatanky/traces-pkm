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
| Review/refactor an existing unit suite | `references/reviewing-suites.md` |
| Maintain doctests | `references/doctests.md` |
| Review lint suppressions | `references/linting.md` |
| Review suite code clarity | `references/code-quality.md` |

## Shared References

Use `../rust-testing/references/assertions.md`, `../rust-testing/references/fixtures.md`, `../rust-testing/references/property-based.md`, `../rust-testing/references/mocks.md`, `../rust-testing/references/snapshots.md`, `../rust-testing/references/async.md`, `../rust-testing/references/concurrency.md`, and `../rust-testing/references/commands.md` when a workflow reaches that branch.

## Completion

- New suite: every non-`n/a` case-surface row maps to a named test.
- Review: output includes a gap list, a per-test audit, a code-quality finding list, and a lint suppression inventory, each explicitly empty if no findings.
- Doctest work: every changed public example is run with `cargo test --doc`, marked `no_run`/`compile_fail`/`ignore-*` with reason, or explicitly out of scope.
