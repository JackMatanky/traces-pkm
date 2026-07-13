# Table-Driven Cases

> Use table-driven cases when one behavior has many named literal examples.

## When to Use

Use `rstest` when the test body is identical and only the input/output literals differ. Do not use it when each case is a separate behavior.

Use property-based tests instead when the input space is large or unbounded.

## Example

```rust
use rstest::rstest;

#[rstest]
#[case::single("a")]
#[case::first_letter("ab")]
#[case::last_letter("ba")]
fn accepts_strings_with_a(#[case] input: &str) {
    assert!(the_function(input).is_ok(), "expected {input:?} to be accepted");
}
```

## Naming

Name each case after the scenario, not the input value. `#[case::empty_string("")]` is useful; `#[case::case_1("")]` is not.

## Completion

The table has one behavior, named cases, and no per-case branching inside the test body.
