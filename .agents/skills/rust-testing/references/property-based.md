# Property-Based Testing

> Use `proptest` when the property should hold for a class of inputs, not just hand-picked examples.

## When to Use

Reach for `proptest` when you can state a property, such as:

| Property | Example |
|---|---|
| Roundtrip | `decode(encode(x)) == x` |
| Idempotence | `f(f(x)) == f(x)` |
| Commutativity | `f(a, b) == f(b, a)` |
| Associativity | `f(f(a, b), c) == f(a, f(b, c))` |
| Identity | `f(x, identity) == x` |
| Invariant | `len(push(v, x)) == len(v) + 1` |

Do not use it when the expected output is one fixed value, the input space is small and enumerable, or the problem is thread interleaving correctness; use `fixtures.md` table-driven cases or `concurrency.md` instead.

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
}
```

Use `prop_assert!` and `prop_assert_eq!` inside `proptest!` blocks so failures can shrink instead of escaping as raw panics.

## Shrinking

Read the shrunk failure, not the original random case. The minimized input is usually the useful reproduction.

## Configuration

```rust
proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1000,
        max_shrink_iters: 10000,
        ..ProptestConfig::default()
    })]

    #[test]
    fn extensive_property_check(x in any::<i32>()) { }
}
```
