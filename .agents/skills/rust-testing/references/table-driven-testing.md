# Table-Driven Testing

> Use `rstest` when the same behavior needs to be verified against many inputs.

## Why It Matters

Hand-writing N nearly-identical tests for N inputs is boilerplate that's easy to under-maintain — a new case gets skipped because copy-pasting a whole test function feels expensive. `rstest`'s `#[case]` attribute turns one parameterized test body into N named test cases, each of which shows up as its own line in `cargo nextest run` output. It's part of the default toolchain — reach for it before hand-rolling a loop over a `Vec` of inputs inside a single `#[test]`.

## When to Use

Use `rstest` cases when you're verifying **one behavior** across **several inputs** that would otherwise be copy-pasted tests differing only in the literal values.

Do **not** reach for `rstest` when:
- Each "case" actually exercises a different behavior — write separate, distinctly-named tests instead (see [`test-naming.md`](test-naming.md)).
- The input space is large/unbounded and you're really looking for edge cases you haven't thought of — use `proptest` instead (see [`property-based-testing.md`](property-based-testing.md)).

## Bad

```rust
// Four tests, only the string literal differs, and it's easy to
// stop adding cases here without anyone noticing.
#[test]
fn accepts_a() { assert!(the_function("a").is_ok()); }
#[test]
fn accepts_ab() { assert!(the_function("ab").is_ok()); }
#[test]
fn accepts_ba() { assert!(the_function("ba").is_ok()); }

// Or worse: one test, multiple assertions, no way to know which
// input failed without reading the panic line number.
#[test]
fn accepts_all_valid_strings() {
    assert!(the_function("a").is_ok());
    assert!(the_function("ab").is_ok());
    assert!(the_function("ba").is_ok());
    assert!(the_function("bab").is_ok());
}
```

## Good

```rust
use rstest::rstest;

#[rstest]
#[case::single("a")]
#[case::first_letter("ab")]
#[case::last_letter("ba")]
#[case::in_the_middle("bab")]
fn the_function_accepts_all_strings_with_a(#[case] input: &str) {
    assert!(the_function(input).is_ok(), "expected {input:?} to be accepted");
}
```

`cargo nextest run` shows each case individually: `the_function_accepts_all_strings_with_a::case_1_single`, `::case_2_first_letter`, etc. — the `::name` suffix on `#[case::name(...)]` gives each one a readable label instead of a bare index.

## Multiple Parameters and Expected Values

Name the expectation first, since `rstest` puts the case's values before the test body reads them — this is the one place the naming convention in [`test-naming.md`](test-naming.md) inverts (expectation before condition, because the case list reads top-down as data):

```rust
#[rstest]
#[case::valid_email("user@example.com", true)]
#[case::missing_at("userexample.com", false)]
#[case::missing_domain("user@", false)]
fn validates_email_format(#[case] input: &str, #[case] expected: bool) {
    assert_eq!(is_valid_email(input), expected, "input: {input:?}");
}
```

## Fixtures

`rstest` fixtures replace hand-written `fn setup() -> Thing` Arrange helpers with an injectable, composable dependency:

```rust
use rstest::{fixture, rstest};

#[fixture]
fn empty_cart() -> Cart {
    Cart::new()
}

#[rstest]
fn empty_cart_has_zero_total(empty_cart: Cart) {
    assert_eq!(empty_cart.total(), 0.0);
}

// Fixtures can depend on other fixtures
#[fixture]
fn cart_with_item(empty_cart: Cart) -> Cart {
    let mut cart = empty_cart;
    cart.add_item(Item::new("Widget", 10.0));
    cart
}

#[rstest]
fn cart_with_item_has_nonzero_total(cart_with_item: Cart) {
    assert!(cart_with_item.total() > 0.0);
}
```

## Combining Cases and Fixtures with Async

```rust
#[rstest]
#[case::timeout_exceeded(Duration::from_millis(1))]
#[case::timeout_generous(Duration::from_secs(5))]
#[tokio::test]
async fn fetch_respects_timeout(#[case] timeout: Duration) {
    let result = fetch_with_timeout(timeout).await;
    // ...
}
```

## Considerations

- Harder for both IDEs and humans to run/locate one specific case than a hand-named test — if you find yourself wanting to debug a single case constantly, that case may deserve to be its own named test.
- Expectation-then-condition ordering in the case list is visually inverted from the usual `action_expected_condition` naming formula — that's expected and fine for `rstest`, not a naming violation.

## See Also

- [`test-naming.md`](test-naming.md) — naming individual `#[test]` functions outside of `rstest`
- [`property-based-testing.md`](property-based-testing.md) — when the input space is too large to enumerate by hand
- [`unit-testing.md`](unit-testing.md) — Arrange/Act/Assert structure inside each case
