# Running Tests

> `cargo nextest run` is the default test runner; doctests need a separate invocation.

## Why `nextest`

`cargo nextest run` runs each test in its own process (better isolation from shared statics/env vars than `cargo test`'s thread-based model), gives faster wall-clock time on multi-core machines, and produces output that's easier to scan than default `cargo test`. It's part of the default toolchain.

```bash
cargo install cargo-nextest --locked   # once per machine
```

## The Doctest Gotcha

**`cargo nextest run` does not run doctests.** This is nextest's most commonly-missed gap — a doctest can silently rot with no CI signal if `cargo test --doc` isn't wired in separately.

```bash
cargo nextest run       # unit + integration tests
cargo test --doc        # doctests, separately, always
```

Wire both into CI as separate steps, not just the first one. See [`doctests.md`](doctests.md) for why doctests exist and how to write them.

## Common Commands

```bash
# Run everything
cargo nextest run
cargo test --doc

# Run one crate in a workspace
cargo nextest run -p my-crate

# Run a specific module/path
cargo nextest run schema::property

# Filter by name substring
cargo nextest run -E 'test(returns_error)'

# Only unit tests (skip tests/ integration binaries)
cargo nextest run --lib

# Only integration tests
cargo nextest run --test '*'

# Single-threaded (needed for env-var-mutating tests — see fixtures-and-cleanup.md)
cargo nextest run --test-threads=1

# Show output even for passing tests
cargo nextest run --no-capture
```

## Iterating Locally

Run the narrowest scope that covers what you're changing while iterating, then the full suite before committing:

```bash
cargo nextest run -p my-crate schema::property   # tight loop while iterating
cargo nextest run && cargo test --doc            # full verification before commit
```

## Ignoring Unfinished Tests

`#[ignore]` skips a test by default without deleting it — use sparingly, and always with a reason:

```rust
#[test]
#[ignore = "requires a live network connection, run manually"]
fn fetches_from_real_api() { /* ... */ }
```

```bash
cargo nextest run --run-ignored=only   # run only ignored tests
cargo nextest run --run-ignored=all    # run everything including ignored
```

An `#[ignore]`d test with no reason string, or one that's stayed ignored for months, is a smell — either fix it, delete it, or move it to a slower/manual test tier deliberately.

## CI Wiring

A minimal CI test stage:

```bash
cargo nextest run --workspace
cargo test --doc --workspace
```

If the project defines a task runner (e.g. a `mise`/`just`/`make` task), prefer invoking that over raw `cargo` commands so local and CI runs stay in sync — check for an existing `test`/`test:unit` task before adding a new invocation pattern.

## See Also

- [`doctests.md`](doctests.md) — why the separate `cargo test --doc` step exists
- [`integration-testing.md`](integration-testing.md) — what `--test '*'` runs
- [`unit-testing.md`](unit-testing.md) — what `--lib` runs
