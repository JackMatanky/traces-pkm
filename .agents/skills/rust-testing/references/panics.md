# Testing Panics

> Use `#[should_panic]` to verify that code panics as designed — and only for that.

## Why It Matters

Some code should panic on invariant violations by design (an assertion in a private helper, a `NonEmpty` constructor rejecting an empty `Vec`). `#[should_panic]` documents and verifies that panic occurs, optionally checking the message, without the boilerplate of `std::panic::catch_unwind`.

## When to Use

Use `#[should_panic]` only when panicking is the **intended, documented behavior** of an invariant — not a stand-in for error handling you haven't written yet.

```rust
// Bad: recoverable failure, tested as a panic
#[test]
#[should_panic] // wrong — this should return Err, not panic
fn invalid_input_panics() {
    parse_config("invalid");
}

// Good: return Result and test the error variant instead
#[test]
fn invalid_input_returns_error() {
    let result = parse_config("invalid");
    assert!(result.is_err());
}
```

If you find yourself reaching for `#[should_panic]` on something a caller could plausibly trigger with bad-but-not-buggy input, that's a sign the function should return `Result`/`Option` instead of panicking — see `err-result-over-panic` in `rust-skills`.

## Basic Usage

```rust
#[test]
#[should_panic]
fn divide_by_zero_panics() {
    divide(1, 0);
}

// Prefer an expected message — verifies *why* it panicked, not just *that* it did
#[test]
#[should_panic(expected = "division by zero")]
fn divide_by_zero_panics_with_message() {
    divide(1, 0);
}

// Partial match — the message only needs to contain this substring
#[test]
#[should_panic(expected = "index out of bounds")]
fn indexing_past_the_end_panics() {
    let v = vec![1, 2, 3];
    let _ = v[100];
}
```

Always prefer `expected = "..."` over a bare `#[should_panic]`. A bare version passes for *any* panic, including one from an unrelated bug earlier in the function — the message check makes sure you're testing the panic you think you are.

## Testing Invariants

```rust
struct NonEmpty<T>(Vec<T>);

impl<T> NonEmpty<T> {
    fn new(items: Vec<T>) -> Self {
        assert!(!items.is_empty(), "NonEmpty cannot be empty");
        NonEmpty(items)
    }
}

#[test]
#[should_panic(expected = "NonEmpty cannot be empty")]
fn new_rejects_empty_vec() {
    NonEmpty::new(Vec::<i32>::new());
}

#[test]
fn new_accepts_non_empty_vec() {
    let ne = NonEmpty::new(vec![1, 2, 3]);
    assert_eq!(ne.0.len(), 3);
}
```

## With `.expect()` Messages

```rust
fn get_config_value(key: &str) -> String {
    CONFIG.get(key)
        .unwrap_or_else(|| panic!("missing required config: {key}"))
        .to_string()
}

#[test]
#[should_panic(expected = "missing required config: DATABASE_URL")]
fn get_config_value_panics_on_missing_required_key() {
    get_config_value("DATABASE_URL");
}
```

## Combining with `Result` Setup

```rust
#[test]
#[should_panic]
fn processing_invalid_data_panics() -> Result<(), Error> {
    let data = setup_test_data()?; // `?` still works for Arrange failures
    process_invalid(&data);        // this call is what's expected to panic
    Ok(())                          // never reached
}
```

## See Also

- [`assertions.md`](assertions.md) — `assert!`/`assert_eq!` for the non-panic case
- `rust-skills` `err-result-over-panic` — deciding panic vs. `Result` in the first place
