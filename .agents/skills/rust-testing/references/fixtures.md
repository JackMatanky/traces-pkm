# Fixtures

## RAII Cleanup

Use `Drop`, `tempfile`, or `scopeguard` when cleanup must run on panic.

```rust
#[test]
fn process_file_returns_ok_for_valid_input() {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), "test data").unwrap();

    let result = process_file(file.path());

    assert!(result.is_ok());
} // file is removed even if the assertion panics
```

For global state such as environment variables, use a named guard that restores the original value in `Drop`, and run those tests single-threaded.

## Table-Driven Cases

Use `rstest` when one behavior has many named literal cases. Do not use it when each case is a separate behavior.

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

Use property-based tests instead when the input space is large or unbounded.

## Completion

Every fixture either builds Arrange data only or is explicitly named as a cleanup guard; hidden assertions in helpers belong in `rust-unit-testing/references/code-quality.md` findings.
