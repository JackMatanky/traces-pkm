# Unit Testing

> Test pure logic and local invariants next to the code that implements them.

## Scope

- Test pure logic and local invariants — the contract of the function/type, not its implementation details.
- Avoid external I/O by default. Allow it only when I/O behavior is the actual unit under test, and keep it deterministic and local (a temp file, not a network call).
- Keep tests in the same file/module as the implementation, so private items are reachable and the test moves with the code during refactors.
- Protect behavior contracts and edge cases. Don't write tests that just restate the implementation line-by-line — those break on every refactor and catch nothing.

## Placement

Put unit tests in `#[cfg(test)] mod tests { }` within the module that owns the implementation. `#[cfg(test)]` ensures the test code is compiled only for `cargo test`, never into release binaries.

```rust
// src/my_module.rs
fn public_api() -> i32 {
    private_helper() * 2
}

fn private_helper() -> i32 {
    21
}

#[cfg(test)]
mod tests {
    use super::*; // pulls in public_api, private_helper, and everything else in scope

    #[test]
    fn public_api_doubles_private_helper() {
        assert_eq!(public_api(), 42);
    }

    #[test]
    fn private_helper_returns_twenty_one() {
        assert_eq!(private_helper(), 21); // private items are reachable
    }
}
```

`use super::*;` is the standard way in: it imports everything from the parent module — public and private — instead of listing items one by one. Only switch to explicit imports (`use super::{parse, ParseError};`) when you deliberately want to keep the test module's namespace small.

## Structure: Arrange, Act, Assert

Every test has three phases. Keep them visually separate — with a blank line or a comment — so a reader (and you, six months later) can tell what's being set up versus what's being verified.

```rust
#[test]
fn user_creation_fails_with_empty_name() {
    // Arrange
    let name = "";
    let email = "email@example.com";

    // Act
    let result = User::new(name, email);

    // Assert
    assert!(matches!(result, Err(UserError::EmptyName)));
}
```

Phase-specific rules:

- **Arrange**: `unwrap()`/`expect()` is fine here. Setup failing is a test-infra bug, not the behavior under test.
- **Act**: capture the `Result`/`Option` in a variable. Do not immediately `.unwrap()` it — that collapses the Act and Assert phases and makes failures report "called unwrap on an Err" instead of the actual mismatch.
- **Assert**: verify outcomes explicitly. No hidden pass/fail helper that swallows the actual comparison — see the anti-pattern below.

```rust
// Bad: Act and Assert collapsed, `?` masks what actually was expected
#[test]
fn order_total_includes_tax() {
    assert_eq!(Order::new().calculate_total(0.10).unwrap(), 110.0);
}

// Good: Act produces a value, Assert checks it
#[test]
fn order_total_includes_tax() {
    // Arrange
    let mut order = Order::new();
    order.add_item(Item::new("Widget", 100.00));

    // Act
    let total = order.calculate_total(0.10);

    // Assert
    assert_eq!(total, 110.0);
}
```

For test helpers that build inputs, prefer plain Arrange helpers over Assert helpers that hide the comparison:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Good: an Arrange helper — builds a value, asserts nothing
    fn order_with_items(items: &[(&str, f64)]) -> Order {
        let mut order = Order::new();
        for (name, price) in items {
            order.add_item(Item::new(name, *price));
        }
        order
    }

    // Bad: an Assert helper — hides the comparison. A failure just says
    // "assertion failed" pointing at this helper, not at the calling test.
    fn assert_order_total(order: &Order, expected: f64) {
        assert!((order.calculate_total(0.0) - expected).abs() < 0.01);
    }

    #[test]
    fn order_total_sums_items() {
        let order = order_with_items(&[("A", 10.0), ("B", 20.0)]);
        assert_eq!(order.calculate_total(0.0), 30.0);
    }
}
```

## Suite Planning (Before Writing Tests)

For any non-trivial unit:

1. Identify the unit(s) of work in the file — usually one per public function or one per meaningfully distinct behavior group.
2. Enumerate the happy path, boundary conditions, and failure paths.
3. Define invariants that must always hold (candidates for `proptest` — see [`property-based-testing.md`](property-based-testing.md)).
4. Choose structure:
   - Simple file, 1–2 behaviors → flat tests are fine.
   - Multi-unit file → submodules per unit of work (see [`test-naming.md`](test-naming.md)).
5. Decide whether `rstest` (repeated cases) or `proptest` (generated inputs) adds value over hand-written tests. Neither is mandatory — plain tests win when there's no repeated shape.

## Recommended Suite Shape

```rust
#[cfg(test)]
mod tests {
    use super::*;

    mod fixtures {
        use super::*;
        // setup helpers only — no assertions
    }

    mod constructor {
        use super::*;

        #[test]
        fn returns_error_when_input_is_invalid() {}
    }

    mod validation {
        use super::*;

        #[test]
        fn rejects_value_when_rule_is_violated() {}
    }

    mod proptests {
        use super::*;
        // property-based suites only
    }
}
```

## Fixture Guidance

- Keep fixtures local to the module under test — don't reach across modules for setup.
- Prefer small helper functions over builder abstractions unless the construction complexity is unavoidable.
- Avoid sharing mutable fixture state across tests; each test builds its own.
- Give helpers concrete names (`valid_schema`, `temp_vault`), not generic ones (`setup`, `helper`).
- For setup/teardown that must run even if the test panics, use RAII — see [`fixtures-and-cleanup.md`](fixtures-and-cleanup.md).

## Determinism and Speed

- Most unit tests should run in under 10ms; a module's whole suite should usually finish in under 1s. If it doesn't, you're probably doing I/O or sleeping — reconsider scope.
- Avoid time and randomness unless seeded/controlled (`tokio::time::pause`, a fixed RNG seed).
- Use per-test isolated fixtures instead of shared mutable global state (a `static` counter, a shared temp directory).
- Keep assertions local and specific, so a failure points at exactly one behavior.

## Coverage Intent

- Verify domain invariants and validation failures first — these are where bugs hide, not the happy path.
- Cover error variants and boundary values, not only the success case.
- Add a regression test for every bugfix that changes behavior in this file.
- Use integration/e2e tests for cross-boundary workflows (see [`integration-testing.md`](integration-testing.md)); don't stretch unit tests to cover system behavior — that's slow and gives worse failure locality.

## Anti-Patterns

| Anti-Pattern | Fix |
|---|---|
| Hidden assertions in helpers | Split into an Arrange helper (builds, no asserts) and an explicit Assert in the test body |
| Shared mutable state across tests | Build fresh fixtures per test |
| Non-deterministic random/time behavior | Seed the RNG; pause/control the clock |
| `unwrap`/`expect` in Act or Assert | Capture the `Result`, assert on it explicitly |
| Test names that bundle multiple behaviors with `and` | Split into separate tests — see [`test-naming.md`](test-naming.md) |
| Assertion-only smoke tests that don't check domain behavior | Assert the actual invariant, not just "it didn't panic" |

## See Also

- [`test-naming.md`](test-naming.md) — naming functions and organizing submodules
- [`assertions.md`](assertions.md) — `assert!` vs `assert_eq!` vs `matches!`
- [`integration-testing.md`](integration-testing.md) — testing the public API instead
- [`table-driven-testing.md`](table-driven-testing.md) — many inputs, one behavior
