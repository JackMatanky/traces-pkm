# Snapshot Testing

> Use `insta` when correctness is visual or structural — snapshots tell the story better than a hand-written `assert_eq!`.

## Why It Matters

Hand-asserting large structured output — pretty-printed structs, rendered error messages, JSON responses, generated code, CLI output — with `assert_eq!` is verbose, brittle, and painful to update on legitimate changes. `insta` records an approved snapshot on first run and diffs against it on every run after; when the output changes on purpose, `cargo insta review` shows the diff and lets you accept it in one keystroke. Snapshots are committed to the repo and reviewed in PRs, so output changes are visible and deliberate instead of silently baked into a big `assert_eq!` string.

## When to Use

| Situation | Prefer |
|---|---|
| Short, simple values (`true`, `42`, `"ok"`) | `assert_eq!` — see [`assertions.md`](assertions.md) |
| Multi-line or structured output | `assert_debug_snapshot!` |
| JSON/YAML serialization | `assert_json_snapshot!` / `assert_yaml_snapshot!` |
| Rendered error messages, compiler-style output | `assert_snapshot!` |

Do **not** use snapshots for:
- Very stable, small, numeric-only data — `assert_eq!` is more direct.
- Critical-path logic where you want the exact expected value visible in the test itself, not in a separate `.snap` file.
- Output containing randomness/timestamps/UUIDs you haven't redacted — that snapshot will fail every run.
- External-resource output — mock/stub the resource, don't snapshot its response (see [`mocking.md`](mocking.md)).

## Setup

```toml
[dev-dependencies]
insta = { version = "1", features = ["json", "yaml"] }
```

```bash
cargo install cargo-insta  # for the review workflow
```

## Bad

```rust
#[test]
fn render_error_produces_expected_message() {
    let err = AppError::NotFound { id: 42 };
    // Fragile: this string must be maintained by hand forever
    assert_eq!(
        format!("{err}"),
        "resource with id 42 was not found in the database and could not be retrieved"
    );
}
```

## Good

```rust
use insta::{assert_debug_snapshot, assert_json_snapshot};

#[test]
fn render_error_produces_expected_message() {
    let err = AppError::NotFound { id: 42 };
    assert_debug_snapshot!(err); // creates/diffs snapshots/render_error_produces_expected_message.snap
}

#[test]
fn config_serializes_to_expected_shape() {
    let config = Config::default();
    assert_json_snapshot!(config); // stored as pretty-printed JSON for easy review
}

#[test]
fn cli_help_output_matches_snapshot() {
    let output = run_cli(&["--help"]);
    assert_debug_snapshot!("cli_help_output", output); // explicit name for clarity
}
```

## Workflow

1. Run tests: `cargo test` (or `cargo insta test`) — insta writes `.snap.new` files for anything new or changed.
2. Review: `cargo insta review` — interactive diff, press `a` to accept, `r` to reject.
3. Commit the accepted `.snap` files alongside the code change.
4. In CI, fail the build on any unapproved snapshot:

```bash
INSTA_UPDATE=no cargo test
```

## Best Practices

**Name snapshots explicitly** so the `.snap` filename is meaningful:

```rust
assert_snapshot!("this_is_a_named_snapshot", output);
```

**Keep snapshots small and focused** — snapshot the field that matters, not the whole object:

```rust
// Good
assert_snapshot!("app_config/http", whole_app_config.http);

// Bad: huge object, hard to review, breaks on unrelated field changes
assert_snapshot!("app_config", whole_app_config);
```

**Don't snapshot simple types** — plain `assert_eq!` is more direct and doesn't need a `.snap` file:

```rust
// Good
assert_eq!(meaning_of_life, 42);

// Overkill
assert_snapshot!("the_meaning_of_life", meaning_of_life);
```

**Redact unstable fields** (timestamps, UUIDs, random IDs) instead of letting the snapshot fail every run:

```rust
use insta::assert_json_snapshot;

#[test]
fn get_user_data_matches_shape() {
    let data = http::client.get_user_data();
    assert_json_snapshot!(
        "endpoints/get_user_data",
        data,
        { ".created_at" => "[timestamp]", ".id" => "[uuid]" }
    );
}
```

**Commit `.snap` files to git** — they live in `snapshots/` next to the tests. Review changes to them exactly as carefully as code changes; an accepted snapshot is a claim that the new output is correct, not just "the test passes now."

## See Also

- [`assertions.md`](assertions.md) — when a plain `assert_eq!` is the better choice
- [`unit-testing.md`](unit-testing.md) — snapshot tests still follow Arrange/Act/Assert
