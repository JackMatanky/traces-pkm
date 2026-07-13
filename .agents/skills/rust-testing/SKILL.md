---
name: rust-testing
description: >
  Rust testing router. Use when the user asks broadly about Rust testing,
  asks which Rust testing approach or skill applies, or mentions multiple
  testing concerns at once. Routes to rust-unit-testing, rust-integration-testing,
  or rust-benchmarking.
license: MIT
metadata:
  version: "2.0.0"
  companion_to: rust-skills
---

# Rust Testing Router

Use this skill to choose the smallest focused Rust testing workflow. Do not run a full testing workflow from this router.

## Route

| User intent | Use |
|---|---|
| Write, review, or refactor `#[cfg(test)]` / `#[test]` unit suites from source understanding | `rust-unit-testing` |
| Maintain rustdoc executable examples / doctests | `rust-unit-testing` |
| Review unit suite lint suppressions or suite-code quality | `rust-unit-testing` |
| Write or review `tests/` integration suites, public API workflows, CLI/binary behavior, or external boundaries | `rust-integration-testing` |
| Write or review Criterion benchmarks, baselines, representative inputs, or `cargo bench` output | `rust-benchmarking` |
| Unsure which level applies | Read `references/boundaries.md`, then route to exactly one child skill |

## Shared References

| Reference | Use when |
|---|---|
| `references/boundaries.md` | Choosing unit vs integration vs doctest vs property vs snapshot vs benchmark vs concurrency |
| `references/commands.md` | Choosing cargo/nextest/doctest/clippy commands |
| `references/assertions.md` | Assertions, `matches!`, panic-contract checks |
| `references/fixtures.md` | Fixtures, cleanup, RAII, table-driven cases |
| `references/property-based.md` | Proptest properties, shrinking, generated input caveats |
| `references/mocks.md` | Fakes, trait seams, mock expectations |
| `references/snapshots.md` | Insta snapshots, redactions, review flow |
| `references/async.md` | Tokio async tests, paused time, async I/O fakes |
| `references/concurrency.md` | Loom model checking and limits |

## Completion

Stop after selecting a child workflow or explicitly stating that no Rust testing skill applies.
