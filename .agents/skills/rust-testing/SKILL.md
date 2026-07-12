---
name: rust-testing
description: >
  Comprehensive guide to testing in Rust: unit tests, integration tests,
  doctests, naming, assertions, table-driven cases, property-based testing,
  mocking, fixtures/RAII cleanup, async tests, lock-free/concurrency testing,
  snapshot testing, and benchmarking. Use when writing, reviewing, or
  planning tests for Rust code — #[test], #[cfg(test)], #[tokio::test],
  mockall, proptest, rstest, insta, criterion, loom, should_panic, test
  naming, test coverage, TDD, nextest. Invoke with /rust-testing.
license: MIT
metadata:
  version: "1.0.0"
  companion_to: rust-skills
---

# Rust Testing

Comprehensive guide to testing Rust code: what kind of test to write, what tools to reach for, how to name and structure tests, and how to run them. Companion to [`rust-skills`](../rust-skills/SKILL.md) — that skill covers idiomatic Rust in general; this one owns everything under the `test-` prefix in depth.

## When to Apply

- Writing any `#[test]` function, new or modified
- Deciding unit vs. integration vs. doctest for a piece of behavior
- Choosing a testing tool (rstest? proptest? mockall? insta? criterion? loom?)
- Naming tests or organizing test modules
- Reviewing a PR's test coverage
- Setting up a new crate's test suite from scratch

## Which Test Do I Need?

```
Testing pure logic / a single function / private internals?
├─ Yes → Unit test → references/unit-testing.md
│         Same input tested many ways with different data?
│         ├─ Yes → Table-driven with rstest → references/table-driven-testing.md
│         └─ Testing a broad invariant across generated inputs?
│             └─ Yes → proptest → references/property-based-testing.md
│
Testing the crate's public API as an external consumer would?
├─ Yes → Integration test in tests/ → references/integration-testing.md
│
Verifying a doc example compiles and behaves as documented?
├─ Yes → Doctest → references/doctests.md
│
Code under test calls a DB / HTTP / filesystem / other dependency?
├─ Yes → Extract a trait, inject a fake or mockall mock → references/mocking.md
│
Test needs temp files, env vars, servers, or other setup that must
always clean up, even on panic?
├─ Yes → RAII guard → references/fixtures-and-cleanup.md
│
Code under test is `async fn`?
├─ Yes → #[tokio::test] → references/async-testing.md
│
Code under test uses atomics / locks / lock-free structures and you
need to prove correctness across thread interleavings?
├─ Yes → loom → references/concurrency-testing.md
│
Expected output is large/structured (rendered errors, JSON, generated
code, CLI output) and painful to hand-write as assert_eq!?
├─ Yes → insta snapshot → references/snapshot-testing.md
│
Need to prove/track that something is fast, not just correct?
├─ Yes → criterion → references/benchmarking.md
│
Code should panic on invalid input by design (not a recoverable error)?
└─ Yes → #[should_panic] → references/panics.md
```

Naming and module structure for whatever you write: [`references/test-naming.md`](references/test-naming.md). Assertion style (`assert!` vs `assert_eq!` vs `matches!`, `pretty_assertions`): [`references/assertions.md`](references/assertions.md). Commands to run any of this: [`references/running-tests.md`](references/running-tests.md).

## Default Toolchain

Reach for these unless you have a specific reason not to — they are the default stack, not situational add-ons.

```toml
[dev-dependencies]
pretty_assertions = "1"
rstest = "0.23"

# situational — add only when the test actually needs it
proptest = "1"          # property-based testing
mockall = "0.13"        # trait mocking
insta = "1"             # snapshot testing
criterion = "0.5"       # benchmarking
tokio = { version = "1", features = ["test-util", "macros", "rt-multi-thread"] }
```

```bash
cargo install cargo-nextest --locked   # once per machine
cargo nextest run                      # default test runner
cargo test --doc                       # doctests separately — nextest does not run them
```

See [`references/running-tests.md`](references/running-tests.md) for the full nextest/CI setup and why doctests need a separate invocation.

## Reference Files

| File | Use when |
|------|----------|
| [`unit-testing.md`](references/unit-testing.md) | Placing, structuring, and scoping `#[cfg(test)]` unit tests; suite planning; determinism |
| [`test-naming.md`](references/test-naming.md) | Naming a test function or choosing a test module name |
| [`assertions.md`](references/assertions.md) | Choosing `assert!`/`assert_eq!`/`matches!`, writing diagnostic messages, `pretty_assertions` |
| [`table-driven-testing.md`](references/table-driven-testing.md) | Same behavior, many inputs — `rstest` cases and fixtures |
| [`property-based-testing.md`](references/property-based-testing.md) | Verifying an invariant holds across generated inputs — `proptest` |
| [`mocking.md`](references/mocking.md) | Isolating code from a DB/HTTP/filesystem dependency — trait design + `mockall` |
| [`fixtures-and-cleanup.md`](references/fixtures-and-cleanup.md) | Setup/teardown that must run even on panic — RAII guards |
| [`async-testing.md`](references/async-testing.md) | Testing `async fn` — `#[tokio::test]`, timeouts, channels |
| [`concurrency-testing.md`](references/concurrency-testing.md) | Proving correctness of atomics/locks across thread interleavings — `loom` |
| [`integration-testing.md`](references/integration-testing.md) | Testing the public API from outside the crate — `tests/` directory |
| [`doctests.md`](references/doctests.md) | Keeping doc examples executable and correct |
| [`snapshot-testing.md`](references/snapshot-testing.md) | Asserting large/structured output — `insta` |
| [`benchmarking.md`](references/benchmarking.md) | Measuring and tracking performance — `criterion` |
| [`panics.md`](references/panics.md) | Testing that code panics by design — `#[should_panic]` |
| [`running-tests.md`](references/running-tests.md) | `cargo test` / `cargo nextest` commands, CI wiring, doctest gotcha |

## Definition of Done for a Test Suite

- [ ] Names follow [`test-naming.md`](references/test-naming.md) — no `test_foo`, `it_works`, or bundled `_and_` behaviors.
- [ ] Happy path, boundary conditions, and failure/error variants are all covered — not just the happy path.
- [ ] Equality assertions use `pretty_assertions::assert_eq!`; enum-shaped checks use `matches!`.
- [ ] No `unwrap`/`expect` in the Act or Assert phase (Arrange is fine — see [`unit-testing.md`](references/unit-testing.md)).
- [ ] No shared mutable state, uncontrolled time, or unseeded randomness — see [`unit-testing.md`](references/unit-testing.md#determinism-and-speed).
- [ ] External dependencies (DB, HTTP, FS) are behind a trait and faked/mocked in unit tests — see [`mocking.md`](references/mocking.md).
- [ ] Every bugfix that changes behavior in a file has a regression test.
- [ ] Suite passes under `cargo nextest run` and `cargo test --doc`.

## Anti-Patterns At a Glance

| Anti-Pattern | Why It's Bad | Fix |
|---|---|---|
| `test_foo`, `test1`, `it_works` | Failure output tells you nothing | [`test-naming.md`](references/test-naming.md) |
| Multiple behaviors in one `#[test]` | Can't tell what broke, or why | One behavior per test — [`test-naming.md`](references/test-naming.md) |
| `unwrap()` in the Act/Assert phase | Panic message obscures which assertion failed | [`unit-testing.md`](references/unit-testing.md) |
| Hidden assertions in helper functions | Failure points at the helper, not the behavior | [`unit-testing.md`](references/unit-testing.md) |
| Concrete `PostgresConnection` etc. as a field | Can't test error paths without a real dependency | [`mocking.md`](references/mocking.md) |
| Manual `remove_file`/`remove_var` cleanup | Skipped when the test panics, pollutes later tests | [`fixtures-and-cleanup.md`](references/fixtures-and-cleanup.md) |
| `assert_eq!` on a whole large struct | Unreadable diff, brittle to unrelated field changes | [`snapshot-testing.md`](references/snapshot-testing.md) or assert the one field that matters |
| `#[should_panic]` for a recoverable error | Should return `Result`/`Err`, not panic | [`panics.md`](references/panics.md) |
| Non-deterministic time/random in a unit test | Flaky, unreproducible failures | [`unit-testing.md`](references/unit-testing.md#determinism-and-speed) |

## Related Skills

| When | See |
|------|-----|
| General idiomatic Rust (ownership, errors, API design) | `rust-skills` |
| Error type design | `m06-error-handling` / `m13-domain-error` |
| Concurrency primitives beyond testing them | `m07-concurrency` |
