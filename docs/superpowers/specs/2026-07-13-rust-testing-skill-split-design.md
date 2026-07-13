# Rust Testing Skill Split Design

## Context

The current `rust-testing` skill is useful as a reference catalog, but it has grown too broad for reliable execution. It mixes unit-suite writing, suite review, integration testing, doctests, mocking, snapshots, concurrency testing, and benchmarking under one model-invoked description. That creates sprawl: the agent sees many possible branches at once and can stop at reference lookup instead of following a focused workflow.

The goal is to preserve the useful reference material while splitting the operational workflows into smaller skills with clearer triggers and completion criteria.

## Decision

Use a hybrid split. Reference filenames should avoid `test` and `testing`; skill names may keep clear testing-oriented names.

- `rust-testing`: thin model-invoked router for broad Rust testing prompts and skill selection.
- `rust-unit-testing`: model-invoked workflow skill for writing, reviewing, and refactoring unit suites from source understanding.
- `rust-integration-testing`: model-invoked workflow skill for public API, cross-module, binary/CLI, and external-boundary suites.
- `rust-benchmarking`: model-invoked workflow skill for writing and reviewing Criterion benchmarks.

All child skills are directly model-invoked and also reachable through `rust-testing`.

## Skill Responsibilities

### `rust-testing`

Purpose: route broad testing requests to the right focused skill.

It should not contain long workflows or tool-specific essays. It should identify the user's testing intent and point to:

- `rust-unit-testing` for `#[cfg(test)]`, `#[test]`, unit suite writing/review/refactor, naming, coverage-gap work, doctest examples, lint review, and unit code-quality review.
- `rust-integration-testing` for `tests/`, public API workflows, CLI/binary tests, cross-boundary behavior, and real/fake dependency choices.
- `rust-benchmarking` for Criterion benchmarks, `cargo bench`, baselines, and benchmark validity review.

### `rust-unit-testing`

Purpose: make unit test suite work deterministic.

Primary workflows:

- Write a suite by mechanically enumerating the source case surface: every error variant, `match` arm, `None` path, panic site, numeric/collection boundary, and state transition.
- Review/refactor an existing suite by rebuilding that case surface from the implementation, mapping existing tests to it, reporting concrete gaps, and auditing naming, AAA structure, assertions, fixtures, determinism, and anti-patterns.
- Review code quality: flag unclear helpers, hidden assertions, overbroad fixtures, excessive setup, overuse of `unwrap`/`expect`, and production-like complexity in unit suites.
- Review lint suppressions: classify every `#[expect(...)]` and `#[allow(...)]` in unit suite code as acceptable or suspicious.
- Maintain doctests as part of unit verification. Doctest guidance belongs here because doctests validate item-level examples and API contracts close to the unit being documented.

Completion criteria:

- New suite: every non-`n/a` case-surface row maps to a named test.
- Review: output includes a gap list, even if empty, plus a per-test audit finding list, even if empty.
- Code-quality review: output lists concrete readability, fixture, assertion, and complexity issues in unit suite code.
- Lint review: output lists every lint `expect`/`allow` found in unit suite code and classifies it as acceptable or suspicious.
- Doctest review: every changed public example is run with `cargo test --doc`, marked `no_run`/`compile_fail`/`ignore-*` with reason, or explicitly out of scope.

### `rust-integration-testing`

Purpose: focus on tests that exercise the crate like an external consumer.

Primary workflows:

- Derive integration cases from public API workflows and externally visible behavior, not private implementation branches.
- Decide when dependencies should be real, fake, mocked, temp-local, or containerized.
- Review integration suites for public-contract coverage, isolation, determinism, fixture cleanup, and over-duplication of unit tests.

Completion criteria:

- Public workflows and boundary failures are explicitly mapped to integration tests or marked out of scope.
- External state strategy is named for each boundary.
- CLI cases state how the binary is invoked, preferably with Cargo-provided binary path behavior when applicable.

### `rust-benchmarking`

Purpose: make performance measurement reliable rather than decorative.

Primary workflows:

- Require a benchmark reason: known hot path, regression guard, or implementation comparison.
- Define the measured unit, inputs, baseline, and success criterion before writing Criterion code.
- Review benchmark validity: `black_box`, setup excluded from measurement, representative inputs, stable baseline, and no benchmarking of optimized-away work.

Completion criteria:

- Benchmark includes a stated question, measured unit, representative input set, baseline/comparison plan, and success criterion.
- Review output states whether setup is excluded from timing, inputs are protected from optimization, and Criterion defaults are sufficient.

## Reference Layout

Keep shared reference files under `rust-testing/references/` unless a file becomes specific enough to belong to one child skill. Reference filenames should avoid `test` and `testing`.

Move or adapt workflow-owned files:

- Move the current unit suite writing workflow to `rust-unit-testing/references/writing-suites.md`.
- Move the current unit suite review workflow to `rust-unit-testing/references/reviewing-suites.md`.
- Move the current unit suite reference to `rust-unit-testing/references/unit-suites.md`.
- Move the current doctest/example reference to `rust-unit-testing/references/doctests.md`; this is the dedicated doctest reference.
- Add `rust-unit-testing/references/code-quality.md` for reviewing unit suite readability, fixture shape, assertion clarity, and avoidable complexity.
- Add `rust-unit-testing/references/linting.md` for lint `expect`/`allow` policy and review.
- Move or adapt the current naming reference to `rust-unit-testing/references/naming.md` unless integration or benchmark workflows prove they need it shared.
- Move or adapt the current integration reference to `rust-integration-testing/references/boundary-suites.md`.
- Move or adapt `benchmarking.md` to `rust-benchmarking/references/criterion.md`.

Keep shared references in `rust-testing/references/`:

- `boundaries.md`
- `assertions.md`
- `fixtures.md`
- `commands.md`
- `property-based.md`
- `mocks.md`
- `snapshots.md`
- `async.md`
- `concurrency.md`

Potentially merge if small:

- Panic-contract guidance can live in `assertions.md` unless it grows into a substantial standalone topic.
- Table-driven cases can live in `fixtures.md` unless the reference becomes large enough to hide either fixture cleanup or table-driven case guidance.

Child skills should link to shared references instead of duplicating their content.

## Invocation Design

Use direct model invocation plus router invocation.

Descriptions should be focused:

- `rust-testing`: use for broad Rust testing questions or choosing which Rust testing skill applies.
- `rust-unit-testing`: use for writing/reviewing/refactoring Rust unit tests, `#[cfg(test)]`, `#[test]`, naming, coverage gaps, source-derived unit suites, doctest examples, lint review, and unit code-quality review.
- `rust-integration-testing`: use for writing/reviewing integration tests, `tests/`, public API tests, CLI/binary tests, and external boundary tests.
- `rust-benchmarking`: use for writing/reviewing Criterion benchmarks, performance tests, baselines, and `cargo bench`.

## Lint and Code Quality References

`rust-unit-testing` must include separate references for linting and code quality:

- `references/linting.md`: lint suppressions in unit suite code are reviewable design decisions, not harmless noise.
- `references/code-quality.md`: unit suite code should be reviewed for readability and maintainability, not only behavior coverage.

### Lint `expect` / `allow` Policy

Acceptable examples:

- `#[expect(clippy::panic, reason = "verifies documented panic contract")]` on a small test that intentionally exercises a panic.
- `#[allow(clippy::too_many_lines)]` on a generated snapshot fixture module where splitting would reduce readability.
- `#[expect(clippy::unwrap_used, reason = "fixture construction; failure means invalid test setup")]` in Arrange-only helper code.

Suspicious examples:

- `#[allow(clippy::unwrap_used)]` over an entire `mod tests`.
- Any lint suppression without a reason.
- Suppressions that hide unclear control flow, excessive setup, repeated assertions, or production-like complexity in test code.
- `expect`/`allow` used to silence a lint instead of making the test smaller or clearer.

The review workflow should report each suppression with file/line, classification, and the smallest fix: keep with reason, narrow the scope, rewrite the suite code, or delete the suppression.

### Code Quality Policy

The code-quality review should flag:

- Hidden assertions in helpers.
- Overbroad fixtures or builders that hide the case being arranged.
- Excessive setup relative to the behavior being verified.
- Repeated assertion blocks that should become table-driven cases.
- Assertions that check large objects when one field or a snapshot would be clearer.
- Production-like branching, loops, or abstractions inside suite code.

The review workflow should report each code-quality finding with file/line, why it reduces clarity, and the smallest refactor that improves it.

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
- `rust-testing/references/commands.md` covers `cargo test`, `cargo nextest run`, `cargo test --doc`, `cargo clippy --all-targets`, target selectors, and fail-fast caveats.
- `rust-testing/references/boundaries.md` routes private implementation branches, public API workflows, CLI binaries, external service/filesystem boundaries, async time/I/O, concurrency interleavings, invariants/properties, snapshot-sized output, and performance questions.
- `rust-skills` has no diff.
