# Snapshots

> Use `insta` when correctness is visual or structural and a reviewable diff is clearer than a handwritten assertion.

Snapshots make output changes deliberate: the approved `.snap` file is committed and reviewed with the code change.

## When to Use

| Situation | Prefer |
|---|---|
| Short, simple values | `assert_eq!` |
| Multi-line or structured output | `assert_debug_snapshot!` |
| JSON/YAML serialization | `assert_json_snapshot!` / `assert_yaml_snapshot!` |
| Rendered errors or CLI output | `assert_snapshot!` |
| Compiler-error-style output | `assert_snapshot!` |

Do not snapshot randomness, timestamps, UUIDs, or external-resource output unless unstable fields are redacted or faked.

## Example

```rust
use insta::{assert_debug_snapshot, assert_json_snapshot};

#[test]
fn render_error_produces_expected_message() {
    let err = AppError::NotFound { id: 42 };
    assert_debug_snapshot!(err);
}

#[test]
fn config_serializes_to_expected_shape() {
    assert_json_snapshot!(Config::default());
}
```

## Review Flow

1. Run tests; new or changed snapshots are written as `.snap.new`.
2. Run `cargo insta review` and accept only intentional output changes.
3. Commit accepted `.snap` files with the code change.
4. In CI, use `INSTA_UPDATE=no cargo test` so unapproved snapshots fail.

## Redactions

```rust
assert_json_snapshot!(
    "endpoints/get_user_data",
    data,
    { ".created_at" => "[timestamp]", ".id" => "[uuid]" }
);
```

Keep snapshots small and explicitly named when the default test name is not clear.

## Avoid

- Snapshotting short scalar values; use `assert_eq!`.
- Snapshotting unstable timestamps, UUIDs, random ordering, or external output without redactions/fakes.
- Accepting `.snap.new` files without reading the diff.
