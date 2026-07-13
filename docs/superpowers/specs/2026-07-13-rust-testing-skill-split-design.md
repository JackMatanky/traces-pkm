# Rust Testing Skill Split Design

## Context

The current `rust-testing` skill is useful as a reference catalog, but it has grown too broad for reliable execution. It mixes unit-suite writing, suite review, integration testing, doctests, mocking, snapshots, concurrency testing, and benchmarking under one model-invoked description. That creates sprawl: the agent sees many possible branches at once and can stop at reference lookup instead of following a focused workflow.

The goal is to preserve the useful reference material while splitting the operational workflows into smaller skills with clearer triggers and completion criteria.

## Decision

Use a hybrid split:

- `rust-testing`: thin model-invoked router for broad Rust testing prompts and skill selection.
- `rust-unit-testing`: model-invoked workflow skill for writing, reviewing, and refactoring unit test suites from source understanding.
- `rust-integration-testing`: model-invoked workflow skill for public API, cross-module, binary/CLI, and external-boundary test suites.
- `rust-benchmarking`: model-invoked workflow skill for writing and reviewing Criterion benchmarks.

All child skills are directly model-invoked and also reachable through `rust-testing`.

## Skill Responsibilities

### `rust-testing`

Purpose: route broad testing requests to the right focused skill.

It should not contain long workflows or tool-specific essays. It should identify the user's testing intent and point to:

- `rust-unit-testing` for `#[cfg(test)]`, `#[test]`, unit suite writing/review/refactor, test naming, and coverage-gap work.
- `rust-integration-testing` for `tests/`, public API workflows, CLI/binary tests, cross-boundary behavior, and real/fake dependency choices.
- `rust-benchmarking` for Criterion benchmarks, `cargo bench`, baselines, and benchmark validity review.

### `rust-unit-testing`

Purpose: make unit test suite work deterministic.

Primary workflows:

- Write a suite by mechanically enumerating the source case surface: every error variant, `match` arm, `None` path, panic site, numeric/collection boundary, and state transition.
- Review/refactor an existing suite by rebuilding that case surface from the implementation, mapping existing tests to it, reporting concrete gaps, and auditing naming, AAA structure, assertions, fixtures, determinism, and anti-patterns.

Completion criteria:

- New suite: every non-`n/a` case-surface row maps to a named test.
- Review: output includes a gap list, even if empty, plus a per-test audit finding list, even if empty.

### `rust-integration-testing`

Purpose: focus on tests that exercise the crate like an external consumer.

Primary workflows:

- Derive integration cases from public API workflows and externally visible behavior, not private implementation branches.
- Decide when dependencies should be real, fake, mocked, temp-local, or containerized.
- Review integration suites for public-contract coverage, isolation, determinism, fixture cleanup, and over-duplication of unit tests.

Completion criteria:

- Public workflows and boundary failures are explicitly mapped to integration tests or marked out of scope.
- External state strategy is named for each boundary.

### `rust-benchmarking`

Purpose: make performance measurement reliable rather than decorative.

Primary workflows:

- Require a benchmark reason: known hot path, regression guard, or implementation comparison.
- Define the measured unit, inputs, baseline, and success criterion before writing Criterion code.
- Review benchmark validity: `black_box`, setup excluded from measurement, representative inputs, stable baseline, and no benchmarking of optimized-away work.

Completion criteria:

- Benchmark includes a stated question, representative input set, and baseline/comparison plan.
- Review output states whether the benchmark measures the intended work.

## Reference Layout

Keep shared reference files under `rust-testing/references/` unless a file becomes specific enough to belong to one child skill.

Move or adapt workflow-owned files:

- Move `writing-a-test-suite.md`, `reviewing-a-test-suite.md`, and `unit-testing.md` under `rust-unit-testing/references/`.
- Move or adapt `integration-testing.md` under `rust-integration-testing/references/`.
- Move or adapt `benchmarking.md` under `rust-benchmarking/references/`.

Keep shared references in `rust-testing/references/`:

- `test-naming.md`
- `assertions.md`
- `fixtures-and-cleanup.md`
- `running-tests.md`
- `property-based-testing.md`
- `mocking.md`
- `snapshot-testing.md`
- `doctests.md`
- `async-testing.md`
- `concurrency-testing.md`
- `panics.md`
- `table-driven-testing.md`

Child skills should link to shared references instead of duplicating their content.

## Invocation Design

Use direct model invocation plus router invocation.

Descriptions should be focused:

- `rust-testing`: use for broad Rust testing questions or choosing which Rust testing skill applies.
- `rust-unit-testing`: use for writing/reviewing/refactoring Rust unit tests, `#[cfg(test)]`, `#[test]`, test naming, coverage gaps, and source-derived test suites.
- `rust-integration-testing`: use for writing/reviewing integration tests, `tests/`, public API tests, CLI/binary tests, and external boundary tests.
- `rust-benchmarking`: use for writing/reviewing Criterion benchmarks, performance tests, baselines, and `cargo bench`.

## Non-Goals

- Do not modify `rust-skills`.
- Do not delete the original `rust-skills/rules/test-*.md` files.
- Do not create separate model-invoked skills for every testing tool (`proptest`, `mockall`, `insta`, etc.) unless a future workflow proves they need their own invocation.
- Do not duplicate large reference content across child skills.

## Validation

After implementation:

- `rust-testing/SKILL.md` is a router, not a catalog.
- Each child skill has a focused description and a workflow with checkable completion criteria.
- Shared references have no broken relative links.
- `rust-skills` has no diff.
