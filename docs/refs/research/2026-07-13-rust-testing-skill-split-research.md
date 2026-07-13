# Rust Testing Skill Split Research

Date: 2026-07-13

## Executive Recommendation

Keep the hybrid split, but make it leaner: one router, three workflow skills, and fewer shared references.

The proposed top-level split is sound because Rust itself draws a hard workflow boundary between unit tests near implementation and integration tests that exercise the public API as an external consumer (https://doc.rust-lang.org/book/ch11-03-test-organization.html). Criterion benchmarking also deserves a separate workflow because benchmark validity has a different completion criterion than correctness testing: stated performance question, representative inputs, measurement hygiene, and baseline/comparison (https://bheisler.github.io/criterion.rs/book/user_guide/benchmarking_with_inputs.html, https://bheisler.github.io/criterion.rs/book/user_guide/comparing_functions.html).

Do not create model-invoked skills for `proptest`, `mockall`, `insta`, `rstest`, Tokio async testing, or Loom yet. They are tools or sub-branches, not independently invoked workflows. Keep them as references reached by unit or integration workflows. The local skill-writing guidance says to split by invocation only when a distinct leading word should trigger independently, and otherwise use progressive disclosure to keep the top-level skill small (`.agents/skills/writing-great-skills/SKILL.md`).

Doctests should stay inside `rust-unit-testing` as item-level executable examples, with a shared `doctests.md` reference if integration/API guidance also needs to point at it. They are closer to unit/API example verification than to full external integration suites, but the skill must remember they run separately from nextest: Cargo runs documentation tests by default via rustdoc, while nextest currently does not support doctests and recommends `cargo test --doc` as a separate step (https://doc.rust-lang.org/cargo/commands/cargo-test.html, https://nexte.st/docs/running/).

## Source-Backed Findings

### Rust Test Boundaries

Rust's official split is two main categories: unit tests are small, focused, in `src`, can test private interfaces, and are normally under a `#[cfg(test)] mod tests`; integration tests live under top-level `tests/`, compile as separate crates, and can only use the public API (https://doc.rust-lang.org/book/ch11-03-test-organization.html). This directly supports separate `rust-unit-testing` and `rust-integration-testing` workflows.

Integration helper layout matters: a top-level `tests/common.rs` is treated as its own integration-test crate and appears as a zero-test binary, while `tests/common/mod.rs` avoids that behavior (https://doc.rust-lang.org/book/ch11-03-test-organization.html). This belongs in integration/boundary reference, not the router.

Binary crates should put important logic in `src/lib.rs` so integration tests can import it, while `src/main.rs` stays thin (https://doc.rust-lang.org/book/ch11-03-test-organization.html). CLI testing belongs in integration testing because it exercises binary/public behavior, not private branches.

Cargo builds and runs unit, integration, and documentation tests; test binaries use libtest, run test functions in multiple threads, and Cargo runs multiple test targets serially (https://doc.rust-lang.org/cargo/commands/cargo-test.html). Skill completion should include the smallest relevant command, not one universal command for all branches.

Cargo target selection affects verification: `--lib`, `--test`, `--tests`, `--doc`, and `--all-targets` select different surfaces; binary integration tests can locate binaries through `CARGO_BIN_EXE_<name>` (https://doc.rust-lang.org/cargo/commands/cargo-test.html). This supports a `commands.md` reference, but it should be short and branch-indexed.

### Doctests

Rustdoc executes documentation examples to keep examples up to date; doctests pass if they compile and run without panicking, and ordinary assertions work inside examples (https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html). That makes doctests executable examples, not merely documentation style.

Doctests have special mechanics that need their own reference: hidden `#` lines, `?` handling with `Ok::<(), E>(())` or hidden `fn main() -> Result`, `should_panic`, `no_run`, `compile_fail`, edition attributes, `standalone_crate`, target-specific ignores, and `#[cfg(doctest)]` for hidden doctest-only items (https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html).

Doctests only link against public items; rustdoc says private items need unit tests (https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html). That argues against a separate model-invoked doctest skill: doctests are item/API examples that supplement unit and public API checks.

### Runners And Completion Commands

`cargo test` runs doctests by default, but section failures stop later sections; `--no-fail-fast` keeps running test executables after a target fails (https://doc.rust-lang.org/book/ch11-03-test-organization.html, https://doc.rust-lang.org/cargo/commands/cargo-test.html). Review workflows should avoid claiming a whole suite is clean from a partial fail-fast run.

Nextest improves unit/integration workflow by building test binaries, listing tests, then running individual tests in separate processes in parallel, collecting per-test results and supporting retries/timeouts (https://nexte.st/docs/design/how-it-works/, https://nexte.st/docs/running/). It is a command/run reference, not a separate skill.

Nextest does not currently support doctests because of stable Rust limitations; its running docs footnote says to run doctests separately with `cargo test --doc` (https://nexte.st/docs/running/). Any skill definition of done that says only `cargo nextest run` is incomplete when doctests are relevant.

### Lint Suppressions And Code Quality

Rust lint attributes include `allow`, `expect`, `warn`, `deny`, and `forbid`; `allow` suppresses reporting, while `expect` suppresses an expected lint and warns if the expectation is unfulfilled (https://doc.rust-lang.org/reference/attributes/diagnostics.html). This supports preferring narrow `#[expect(...)]` over broad `#[allow(...)]` when a test intentionally triggers a lint.

All lint attributes support `reason = "..."`, and the Reference explicitly frames the reason as context/documentation for the reader (https://doc.rust-lang.org/reference/attributes/diagnostics.html). The unit review workflow should require every `allow`/`expect` in test code to be listed and classified, with missing reasons treated as suspicious.

Clippy's own docs say `#[allow(..)]` can be appropriate when you disagree with a lint, but CI commonly elevates warnings to errors with `cargo clippy -- -Dwarnings`, and `clippy::restriction` lints should be cherry-picked rather than enabled wholesale because some contradict other Clippy lints (https://doc.rust-lang.org/clippy/usage.html). This means the skill should not ban suppressions; it should review scope, reason, and whether the suppression hides unclear test code.

### Property, Table, Mock, Snapshot, Async, And Concurrency Branches

Proptest is best used to complement traditional unit testing: hand-written tests cover known edge cases and regression inputs, while property tests search arbitrary generated inputs and shrink failures to a minimal case (https://proptest-rs.github.io/proptest/intro.html). `property-based.md` should be a shared/unit reference for invariant surfaces, not its own skill.

Proptest performance can require explicit test-profile optimization for `proptest` and `rand_chacha`, and shared mutable resources in closure-style `proptest!` need interior mutability such as `RefCell` (https://proptest-rs.github.io/proptest/proptest/tips-and-best-practices.html). This belongs in the reference, not the main workflow.

`rstest` is a fixture and table/case macro. It helps inject fixtures, create one generated test per `#[case]`, generate value combinations, and use string conversion for `FromStr` values (https://docs.rs/rstest/latest/rstest/). It supports a table-driven reference, but should not be the default for every table unless the crate already uses it or repetition justifies it.

Mockall creates mock structs, sets expectations with matchers, call counts, sequence constraints, and required return values; unexpected access panics (https://docs.rs/mockall/latest/mockall/). Its static methods and module mocks use global expectations that require synchronization when used across tests (https://docs.rs/mockall/latest/mockall/). Mocking belongs in boundary/dependency guidance, with a bias toward fakes or trait seams before generated mocks.

Insta is a Rust snapshot tool for large/structured output, with docs for serializers, redactions, filters, CLI testing, settings, and `cargo insta` review (https://insta.rs/docs/). Snapshot guidance should stay a reference for structured output and review discipline, not a separate workflow skill.

Tokio's unit testing guidance emphasizes `#[tokio::test(start_paused = true)]` for time-dependent async tests and `tokio_test::io::Builder` for mocking `AsyncRead`/`AsyncWrite` I/O (https://tokio.rs/tokio/topics/testing). Async testing should be a branch reference used by unit and integration workflows.

Loom deterministically explores possible concurrent executions using loom replacement types, requires deterministic tests, often uses `RUSTFLAGS="--cfg loom"`, is usually run separately and in release mode, and can hit combinatorial explosion mitigated with preemption bounds (https://docs.rs/loom/latest/loom/). Loom is specialized enough for a reference, not a model-invoked skill unless the repo does a lot of lock-free/concurrency work.

### Benchmarking

Criterion benchmark groups and `bench_with_input` are the right default for input-sensitive code; `bench_with_input` automatically black-boxes the input and records input in the benchmark ID (https://bheisler.github.io/criterion.rs/book/user_guide/benchmarking_with_inputs.html). Benchmark workflows should require an input set before code is written.

Criterion comparison benchmarks put multiple implementations in the same group and generate per-input and cross-input summaries (https://bheisler.github.io/criterion.rs/book/user_guide/comparing_functions.html). This supports a separate `rust-benchmarking` workflow because benchmark review asks whether the comparison answers the intended question.

Criterion advanced configuration covers sample size, significance, throughput, log-scale charts, and flat sampling for long-running benchmarks, but warns flat sampling is not recommended except where necessary (https://bheisler.github.io/criterion.rs/book/user_guide/advanced_configuration.html). The skill should avoid premature tuning; default Criterion settings are enough unless the benchmark question demands otherwise.

### Skill Design Principles

The local `writing-great-skills` guidance says model-invoked skills cost context load through their descriptions, so they should exist only when the agent must reach them autonomously or another skill must invoke them (`.agents/skills/writing-great-skills/SKILL.md`). This argues against many tool-specific model-invoked skills.

The same guidance says to split by sequence when later steps tempt premature completion, and to use checkable, exhaustive completion criteria for each workflow step (`.agents/skills/writing-great-skills/SKILL.md`). This supports splitting `rust-unit-testing`, `rust-integration-testing`, and `rust-benchmarking`, because their completion artifacts differ.

Progressive disclosure should push branch-specific reference into linked files and keep inline only what every branch needs (`.agents/skills/writing-great-skills/SKILL.md`). That supports references for doctests, property-based checks, mocks, snapshots, async, concurrency, fixtures, assertions, and commands.

Single source of truth matters: duplicated guidance across router, child skills, and references creates maintenance drift (`.agents/skills/writing-great-skills/SKILL.md`). The router should route, not summarize the entire catalog.

## Gap Analysis Against Current Plan

What is right:

- Keep `rust-testing` as a thin router.
- Keep `rust-unit-testing` for source-derived unit suites, doctests, lint/code-quality review, naming, assertions, fixtures, and case-surface mapping.
- Keep `rust-integration-testing` for public API, CLI/binary, cross-module, and external-boundary suites.
- Keep `rust-benchmarking` separate for Criterion benchmark writing/review.
- Keep tool-specific guidance in references rather than model-invoked skills.

What should be tightened:

- `rust-unit-testing` is carrying too many branches in its model-invoked description. Move `naming`, `assertions`, `fixtures`, `doctests`, `lint`, and `code-quality` into body pointers, not all description triggers. The description should trigger on writing/reviewing/refactoring unit suites and source-derived coverage gaps.
- The router plus directly model-invoked children is acceptable, but child descriptions should be short. Otherwise the split pays context load four times.
- `commands.md` should explicitly say `cargo nextest run` does not cover doctests and `cargo test --doc` remains required when doctests are relevant.
- Unit-suite review completion criteria need an explicit “verification command attempted or reason not run” line. A coverage/gap table without a runnable check invites premature completion.
- Integration completion criteria should require one public workflow/boundary matrix row per external behavior, with the dependency strategy named: real, fake, mock, temp-local, container, or out-of-scope.
- Benchmark completion criteria should require the benchmark question, input set, baseline/comparison, and a statement that setup is excluded from timing where applicable.

What is missing:

- `verification.md` or a tighter `commands.md` section covering `cargo test`, `cargo nextest run`, `cargo test --doc`, `cargo clippy --all-targets`, target selectors, and fail-fast caveats.
- `boundaries.md` for the decision matrix: private implementation branch, public API workflow, CLI binary, external service/filesystem, async time/I/O, concurrency interleaving, invariant/property, snapshot-sized output, performance question.
- `linting.md` should cite `reason` and `expect` semantics, and require file/line, scope, classification, and smallest fix.
- `code-quality.md` should be explicitly part of unit-suite review, not a generic “nice to have.” It should require findings for hidden assertions, overbroad fixtures, Arrange/Act/Assert phase clarity, production-like branching, excessive setup, broad object assertions, and unexplained suppressions.
- `doctests.md` should include rustdoc-specific mechanics and the nextest gap.

What is unnecessary or too granular:

- Separate shared `tables-driven.md` and `fixtures-and-cleanup.md` are reasonable only if short. If both become small, merge into `suite-shape.md` or `fixtures.md` to reduce reference sprawl.
- `panics.md` can probably merge into `assertions.md` unless it contains enough Rust-specific panic contract guidance to justify a separate file.
- `async.md` and `concurrency.md` should stay separate because Tokio time/I/O testing and Loom model checking have different commands and failure modes.
- Do not create `rust-doctesting`, `rust-property-testing`, `rust-mocking`, `rust-snapshot-testing`, or `rust-async-testing` model-invoked skills now.

## Proposed Final Layout

```text
.agents/skills/
  rust-testing/
    SKILL.md                         # thin router only
    references/
      boundaries.md                   # choose unit/integration/doctest/property/snapshot/bench/concurrency
      commands.md                     # cargo/nextest/doc/clippy commands and caveats
      assertions.md                   # assert_eq, matches, panic contracts if small
      fixtures.md                     # fixtures, cleanup, RAII, rstest if not large
      property-based.md               # proptest invariants, shrinking, persistence, performance notes
      mocks.md                        # fakes first, mockall expectations, static/global caveat
      snapshots.md                    # insta, redactions, review flow
      async.md                        # tokio::test, paused time, async I/O fakes
      concurrency.md                  # loom model, cfg, release command, limits

  rust-unit-testing/
    SKILL.md                         # write/review/refactor source-derived unit suites
    references/
      writing-suites.md               # case-surface enumeration workflow
      reviewing-suites.md             # gap table + per-test audit workflow
      unit-suites.md                  # placement, cfg(test), private items, determinism
      doctests.md                     # rustdoc executable examples and cargo test --doc
      code-quality.md                 # readability/fixture/assertion/helper audit
      linting.md                      # #[expect]/#[allow] review policy
      naming.md                       # can be here if unit-specific; otherwise shared

  rust-integration-testing/
    SKILL.md                         # public API, CLI, external boundaries
    references/
      boundary-suites.md              # tests/ layout, common/mod.rs, public workflow matrix
      cli.md                          # optional; only if CLI guidance is substantial

  rust-benchmarking/
    SKILL.md                         # Criterion benchmark workflow and review
    references/
      criterion.md                    # inputs, black_box, baselines, setup exclusion, config
```

Reasons:

- The router stays small and pays for broad invocation once.
- Unit and integration stay separate because Rust's official organization and visibility rules differ.
- Benchmarking stays separate because “passing” means “answers a performance question,” not “assertions pass.”
- Tool files remain references because they are branch-specific and should be loaded only when the workflow reaches that branch.
- Doctests stay under unit because they verify item-level examples and API snippets close to source, but `doctests.md` can be linked by integration when public API example coverage matters.

## Stronger Completion Criteria

For `rust-unit-testing`:

- New suite: every source-derived case-surface row is mapped to a named test, marked `covered`, `doctest`, `property`, `integration`, or `n/a` with reason.
- Review: output includes a case-surface gap table, a per-test code-quality audit, and a lint suppression inventory, each explicitly empty if no findings.
- Doctests: every changed public example is either run with `cargo test --doc`, marked `no_run`/`compile_fail`/`ignore-*` with reason, or explicitly out of scope.
- Lint review: every `#[allow]` and `#[expect]` in touched suite code is listed with file/line, scope, reason presence, classification, and smallest fix.

For `rust-integration-testing`:

- Every public workflow and boundary failure is mapped to an integration test or out-of-scope reason.
- Every external dependency has a named strategy: real, fake, mock, temp-local, container, or skipped with reason.
- CLI cases state how the binary is invoked, preferably through Cargo-provided binary path behavior when applicable.

For `rust-benchmarking`:

- Benchmark has a stated question, measured unit, representative inputs, baseline/comparison, and success criterion.
- Review states whether setup is excluded from timing, whether inputs are protected from optimization, and whether Criterion defaults are sufficient.

For `rust-testing` router:

- It finishes only after selecting a child workflow or explicitly saying no Rust testing skill applies.

## Risks And Open Questions

- The “avoid `test` or `testing` in reference filenames” rule may make references less discoverable. Keep it only where it does not obscure meaning; `doctests.md` is already explicitly exempt.
- `naming.md` placement is ambiguous. If naming is mostly unit-suite naming, put it under `rust-unit-testing`; if integration and benchmarks need it too, keep it shared.
- `panics.md` may not earn its own file. Merge into `assertions.md` unless it contains substantial panic-contract guidance.
- `fixtures.md` may become too broad if it holds RAII cleanup, rstest fixtures, temp dirs, env vars, and integration resources. Split only if the file grows enough that agents miss the relevant branch.
- If this repo frequently tests concurrent primitives, `rust-concurrency-testing` could become a future child skill. Today Loom is too specialized to justify model-invoked context load.
