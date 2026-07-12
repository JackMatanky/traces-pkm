# Property-Based Testing

> Use `proptest` when the property you care about should hold for a whole class of inputs, not just the ones you thought to hand-pick.

## Why It Matters

Hand-picked test cases only cover inputs you thought of — proptest generates hundreds of random inputs per run and, when one fails, automatically shrinks it to the smallest input that still reproduces the failure. It routinely finds edge cases (empty strings, integer overflow, unicode boundaries, deeply nested structures) that never occur to a human writing examples by hand.

## When to Use

Reach for `proptest` when you can state a **property** — something that should be true for every valid input — rather than a fixed expected output for a fixed input.

| Property | Example |
|---|---|
| Roundtrip | `decode(encode(x)) == x` |
| Idempotence | `f(f(x)) == f(x)` |
| Commutativity | `f(a, b) == f(b, a)` |
| Associativity | `f(f(a, b), c) == f(a, f(b, c))` |
| Identity | `f(x, identity) == x` |
| Invariant | `len(push(v, x)) == len(v) + 1` |

Do **not** reach for `proptest` when:
- The expected output for a given input is a specific fixed value, not a general property — that's a regular test or an `rstest` case (see [`table-driven-testing.md`](table-driven-testing.md)).
- The input space is small and enumerable — just list the cases.
- You need to prove correctness across *thread interleavings*, not input values — that's `loom` (see [`concurrency-testing.md`](concurrency-testing.md)).

## Setup

```toml
[dev-dependencies]
proptest = "1"
```

## Basic Usage

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn reverse_reverse_is_identity(s in ".*") {
        let reversed: String = s.chars().rev().collect();
        let double_reversed: String = reversed.chars().rev().collect();
        prop_assert_eq!(s, double_reversed);
    }

    #[test]
    fn sort_is_idempotent(mut v in prop::collection::vec(any::<i32>(), 0..100)) {
        v.sort();
        let sorted_once = v.clone();
        v.sort();
        prop_assert_eq!(v, sorted_once);
    }
}
```

Use `prop_assert!`/`prop_assert_eq!` inside `proptest! { ... }` blocks — they return a `TestCaseError` instead of panicking, which is what lets proptest catch a failure and shrink the input, rather than propagating a raw panic.

## Common Strategies

```rust
proptest! {
    #[test]
    fn test_i32(x in any::<i32>()) { }

    #[test]
    fn test_range(x in 0..100i32) { }

    #[test]
    fn test_email(email in "[a-z]+@[a-z]+\\.[a-z]{2,3}") { } // regex-based generation

    #[test]
    fn test_vec(v in prop::collection::vec(any::<i32>(), 0..10)) { }

    #[test]
    fn test_option(opt in prop::option::of(any::<i32>())) { }
}
```

## Custom Strategies

```rust
use proptest::prelude::*;

#[derive(Debug, Clone)]
struct User {
    name: String,
    age: u8,
}

fn user_strategy() -> impl Strategy<Value = User> {
    ("[a-zA-Z]{1,20}", 0..120u8)
        .prop_map(|(name, age)| User { name, age })
}

proptest! {
    #[test]
    fn user_age_is_always_under_150(user in user_strategy()) {
        prop_assert!(user.age < 150);
    }
}

// Or derive Arbitrary for straightforward structs
use proptest_derive::Arbitrary;

#[derive(Debug, Arbitrary)]
struct Point {
    x: i32,
    y: i32,
}
```

## Example: Parser Roundtrip

```rust
proptest! {
    #[test]
    fn config_parse_roundtrip(config in valid_config_strategy()) {
        let serialized = config.to_string();
        let parsed = Config::parse(&serialized).unwrap();
        prop_assert_eq!(config, parsed);
    }
}
```

## Shrinking

When a case fails, proptest automatically shrinks the input to a minimal reproduction — a failure on `vec![100, 50, 75, 25, 0]` typically shrinks down to something like `vec![1, 0]`, the smallest input that still fails. Read the shrunk failure, not the original random one; it's almost always more informative.

## Configuration

```rust
proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1000,             // more test cases per run
        max_shrink_iters: 10000, // more shrinking effort
        ..ProptestConfig::default()
    })]

    #[test]
    fn extensive_property_check(x in any::<i32>()) { }
}
```

## See Also

- [`table-driven-testing.md`](table-driven-testing.md) — when inputs are enumerable, use `rstest` instead
- [`concurrency-testing.md`](concurrency-testing.md) — for thread-interleaving correctness, use `loom` instead
- [`unit-testing.md`](unit-testing.md) — put proptest suites in their own `mod proptests` submodule
