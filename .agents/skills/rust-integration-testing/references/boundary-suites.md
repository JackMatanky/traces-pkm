# Boundary Suites

> Put integration tests in `tests/` -- they exercise your crate's public API the same way an external consumer would.

## Why It Matters

Every file in `tests/` compiles as its own separate crate that depends on your library through its public API only -- no access to private items. That constraint is the point: it forces you to test what you actually ship, not implementation details that unit tests already cover from the inside.

## Unit vs. Integration

| Unit Tests | Integration Tests |
|---|---|
| In `src/`, `#[cfg(test)] mod tests` | In `tests/` directory |
| Access private items | Public API only |
| Test individual functions/invariants | Test module/crate interactions |
| Fast, isolated, no I/O by default | May involve real I/O, slower |
| `cargo nextest run --lib` | `cargo nextest run --tests` |

## Structure

```
my_project/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   └── internal.rs
└── tests/
    ├── integration_test.rs    # Each file compiles as a separate test binary
    ├── api_tests.rs
    └── common/                # Shared test utilities
        └── mod.rs
```

## Bad

```rust
// src/lib.rs -- this is a unit test location, not an integration test,
// even though it's testing a full workflow
#[test]
fn integration_test_full_workflow() { /* ... */ }
```

## Good

```rust
// tests/integration_test.rs
use my_crate::{Client, Config}; // public API only

#[test]
fn client_processes_valid_input() {
    let client = Client::new(Config::default());

    let result = client.process("input");

    assert!(result.is_ok());
}

#[test]
fn client_rejects_invalid_input_with_typed_error() {
    let client = Client::new(Config::strict());

    let result = client.process("invalid");

    assert!(matches!(result, Err(Error::InvalidInput { .. })));
}
```

## Shared Test Utilities

`tests/common/mod.rs` is preferred for shared integration helpers because `tests/common.rs` is compiled as its own integration target.

A `tests/common/mod.rs` file is not itself compiled as a test binary, so it's the right place for shared setup:

```rust
// tests/common/mod.rs
use my_crate::Config;

pub fn test_config() -> Config {
    Config { timeout: Duration::from_secs(5), retries: 3, debug: true }
}

pub fn setup_test_environment() { /* ... */ }
```

```rust
// tests/api_tests.rs
mod common;
use my_crate::Client;

#[test]
fn client_works_with_shared_config() {
    common::setup_test_environment();
    let client = Client::new(common::test_config());
    // ...
}
```

## Organizing Many Integration Tests

```
tests/
├── Cargo.toml
└── api/
    ├── mod.rs      # mod auth; mod users; mod orders;
    ├── auth.rs
    ├── users.rs
    └── orders.rs
```

```rust
// tests/api/auth.rs
use my_crate::auth::{login, logout};

#[test]
fn login_succeeds_with_valid_credentials() { /* ... */ }

#[test]
fn login_fails_with_invalid_credentials() { /* ... */ }
```

## Boundary Strategy

For each external dependency, name the strategy before writing the case: real, fake, mock, temp-local, container, or out-of-scope with reason.

## When to Use

- The behavior under test spans multiple public API calls or modules working together.
- You're verifying the crate's public contract, not an internal helper.
- Testing a binary crate's CLI surface, driven through `src/main.rs`'s thin wrapper around `src/lib.rs` logic.

Do **not** put here what a unit test already covers -- integration tests are slower to compile and run (each file is a separate crate); don't duplicate a fast, focused unit test at this layer just because it "feels more real."

## See Also

- [`../../rust-testing/references/boundaries.md`](../../rust-testing/references/boundaries.md) -- choosing unit vs integration boundaries
- [`../../rust-testing/references/commands.md`](../../rust-testing/references/commands.md) -- commands and CI wiring
