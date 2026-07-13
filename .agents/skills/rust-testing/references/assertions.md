# Assertions

> An assertion that fails without context is a puzzle. An assertion that fails with context is a diagnosis.

## Use `pretty_assertions` by Default

`std::assert_eq!`/`assert_ne!` print an unreadable single-line diff for anything beyond a primitive. `pretty_assertions` overrides both macros with a colorized, line-by-line diff — same call sites, better failure output.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::{assert_eq, assert_ne}; // shadows std's macros in this module

    #[test]
    fn parses_config_from_toml() {
        let parsed = Config::parse(TOML_INPUT).unwrap();
        assert_eq!(parsed, Config::default_for_test());
    }
}
```

## Choosing the Right Macro

| Situation | Use |
|---|---|
| Boolean condition (`is_ok`, `is_empty`, custom predicate) | `assert!(cond, "context: {value:?}")` |
| Two values must be equal/unequal | `pretty_assertions::assert_eq!` / `assert_ne!` |
| Checking an enum variant, ignoring payload details | `assert!(matches!(value, Variant::Foo(_)))` |
| Code should panic by design | `#[should_panic(expected = "...")]` |
| Large/structured output (JSON, rendered errors, CLI output) | snapshot — see [`snapshots.md`](snapshots.md) |

## Always Include Context

Rust asserts accept format arguments exactly like `println!`. A bare `assert!(result.is_ok())` tells you nothing when it fails at 2am; the message tells you what actually went wrong.

```rust
// Bad: failure says only "assertion failed: result.is_ok()"
assert!(result.is_ok());

// Good: failure includes the actual error
assert!(
    result.is_ok(),
    "expected success, got: {:?}",
    result.err()
);
```

`assert_eq!`/`assert_ne!` already print both sides on failure — extra context is optional there, but still worth adding when the "why" isn't obvious from the values alone:

```rust
assert_eq!(
    result, expected,
    "order total should include the 10% tax rate"
);
```

## `matches!` for Enum Variants

When you care about the variant but not the full payload, `matches!` avoids requiring `PartialEq` on the payload and reads more directly than a manual `match`:

```rust
let error = process(bad_input).unwrap_err();
assert!(
    matches!(error, MyError::BadInput(_)),
    "expected BadInput, got {error:?}"
);
```

Prefer this over `assert_eq!(error, MyError::BadInput("...".into()))` when the payload's exact contents aren't the thing under test — asserting on it couples the test to details it doesn't care about.

## Panic Contracts

Use `#[should_panic(expected = "...")]` only when panic is intentional API behavior. Prefer the `expected` message so an unrelated panic does not accidentally pass the check. For recoverable invalid input, assert on `Result` or `Option` instead.

## Anti-Patterns

| Anti-Pattern | Fix |
|---|---|
| Bare `assert!(x.is_ok())` with no message | Add `"context: {x:?}"` |
| `assert_eq!` on a whole large struct just to check one field | Assert the one field, or use a snapshot — see [`snapshots.md`](snapshots.md) |
| Manual `match` + `panic!("unexpected")` to check a variant | `assert!(matches!(...))` |
| `std::assert_eq!` on multi-line output | Import `pretty_assertions::assert_eq` in the test module |

## See Also

- [`snapshots.md`](snapshots.md) — when a value is too large for `assert_eq!` to be readable
