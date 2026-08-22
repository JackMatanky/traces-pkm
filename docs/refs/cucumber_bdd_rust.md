# Cucumber-style BDD in Rust

## Conclusion

Do not add Cucumber to `traces-pkm` now. The existing `tests/integration.rs` and
`tests/e2e.rs` already exercise the public library and real CLI in isolated
temporary sandboxes; the repository's normal `mise test` route uses
`cargo nextest`. Cucumber would add Gherkin-to-Rust step glue and a separate
test execution path without a current stakeholder or specification problem
that needs plain-language scenarios.

Reconsider only when a real reader of executable specifications will maintain
or approve a small, stable set of user-facing CLI journeys.

## What Cucumber-Rust provides

[`cucumber`][cucumber] 0.23.0 is the native Rust implementation of Cucumber.
It parses `.feature` files written in Gherkin, creates a fresh `World` for
each scenario, and maps `Given`/`When`/`Then` text to Rust functions marked
with `#[given]`, `#[when]`, and `#[then]`.

Its default `macros` feature supplies `#[derive(cucumber::World)]` and
auto-registers step definitions through `inventory`. Step modules must
therefore be included by the runner binary with `mod`; unlinked modules do not
register their steps. Step definitions may be synchronous or asynchronous and
may fail by returning `Result` or by panicking on an assertion.

The crate is async internally but does not mandate Tokio. Its quickstart uses
`futures::executor::block_on`; Tokio is appropriate only if steps need Tokio
services. This synchronous CLI can use `futures` as the smallest executor
dependency. The repository's nightly Rust 1.99 toolchain exceeds Cucumber
0.23.0's declared Rust 1.88 minimum.

## If adoption becomes justified

Keep Cucumber as one optional, focused acceptance suite. Do not migrate
existing unit, integration, or end-to-end coverage.

1. Add `cucumber = "0.23"` and `futures = "0.3"` to
   `[dev-dependencies]`.
2. Add a dedicated `[[test]]` target such as `bdd`, with `harness = false`;
   its `main` drives `World::run("tests/features")` through
   `futures::executor::block_on`.
3. Put `.feature` files in `tests/features/`, organized by externally visible
   command or journey. Keep the runner thin; include a `steps` module and put
   step functions there.
4. Make the `World` own each scenario's fixture state. For this repository,
   that means a fresh temporary project and isolated `TRACES_STATE_DIR` and
   `XDG_CONFIG_HOME`, following the existing `tests/e2e/support.rs` contract.
   Use `#[world(init = ...)]` if `Default` is insufficient.
5. Implement steps in terms of public APIs or the compiled CLI, not private
   internals. A scenario should describe one durable, user-observable
   contract—not a translation of a unit test.

Example manifest shape, intentionally not applied:

```toml
[dev-dependencies]
cucumber = "0.23"
futures = "0.3"

[[test]]
name = "bdd"
harness = false
```

Run that target directly:

```sh
cargo test --test bdd
cargo test --test bdd -- --tags '@smoke'
cargo test --test bdd -- --concurrency 1
```

The custom runner accepts Cucumber's CLI filters, including feature input
globs, scenario-name regular expressions, tag expressions, fail-fast, retry,
and concurrency controls.

## Important fit constraint: Nextest

The project's `mise test` invokes `cargo nextest run`. A `harness = false`
target is **not automatically Nextest-compatible**: Nextest requires custom
harnesses to support `--list --format terse`,
`--list --format terse --ignored`, and per-test
`<name> --nocapture --exact`. Cucumber's documented `libtest` feature only
provides `--format=json` output for IDE integration; it does not document the
required Nextest listing/execution protocol.

If Cucumber is adopted, expose it through a separate task that runs
`cargo test --test bdd`; do not silently include it in the existing Nextest
task. A wrapper implementing Nextest's protocol would be additional bespoke
test infrastructure and is not justified for a small BDD suite.

## Isolation, concurrency, and reporting

Scenarios run concurrently by default. A new `World` isolates in-memory
scenario state, but it does not make global process state safe. The current
end-to-end harness documents that its `CwdGuard` cannot serialize concurrent
current-working-directory mutation. Consequently, BDD steps must prefer a
spawned CLI in an isolated sandbox; scenarios that must mutate the process
working directory need `@serial` or `--concurrency 1`.

Default terminal output is sufficient initially. Enable one of these
dependency features only when a concrete consumer exists:

| Need | Cucumber feature | Writer |
| --- | --- | --- |
| IDE-oriented libtest JSON | `libtest` | `writer::Libtest::or_basic()` |
| Cucumber JSON artifact | `output-json` | `writer::Json` |
| CI test-report artifact | `output-junit` | `writer::JUnit` |
| Structured tracing events | `tracing` | tracing integration |

These are **dependency features** in `Cargo.toml`, not flags passed through
the package's `cargo test --features` command.

## Recommendation

Keep the existing Rust integration and E2E suites as the source of executable
behavior. They already provide isolated real-binary coverage, fit Nextest, and
avoid duplicated English/Rust maintenance.

Use Cucumber only for a deliberately bounded acceptance layer—approximately a
few cross-command journeys whose `.feature` wording is reviewed by a
non-implementer. Otherwise, direct Rust tests are the smaller and more
maintainable specification.

## Primary sources

- [Cucumber-Rust 0.23.0 manifest][manifest] — version, MSRV, features, and
  dependencies.
- [Cucumber-Rust quickstart][quickstart] — custom harness, `World`, steps,
  executor choice, scenario lifecycle, and concurrency.
- [Cucumber-Rust CLI guide][cli] — filtering, retries, and concurrency flags.
- [Cucumber-Rust output guide][output] — `libtest`, JSON, and JUnit writers.
- [Cargo Nextest custom-harness requirements][nextest] — required listing and
  single-test invocation protocol.

[cucumber]: https://github.com/cucumber-rs/cucumber
[manifest]: https://github.com/cucumber-rs/cucumber/blob/v0.23.0/Cargo.toml
[quickstart]: https://github.com/cucumber-rs/cucumber/blob/v0.23.0/book/src/quickstart.md
[cli]: https://github.com/cucumber-rs/cucumber/blob/v0.23.0/book/src/cli.md
[output]: https://github.com/cucumber-rs/cucumber/tree/v0.23.0/book/src/output
[nextest]: https://nexte.st/docs/design/custom-test-harnesses/
