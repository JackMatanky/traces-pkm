# Numeric filters (ceil, floor, sqrt, num_format)

Status: ready-for-agent

## Parent

`.scratch/template-service/spec.md`

## What to build

Register flat-named numeric filters. All operate as pipeline filters on numeric values.

### Filters

- **`ceil`** — Rounds a float up to the nearest integer. Returns f64.
- **`floor`** — Rounds a float down to the nearest integer. Returns f64.
- **`sqrt`** — Returns the square root. Works on integers and floats; returns f64. Errors on negative input.
- **`num_format(decimals)`** — Formats a float to exactly `n` decimal places. Returns a string. Prefixed to avoid collision with minijinja's built-in `format`.

Usage:
```jinja
{{ 3.14 | ceil }}             {# -> 4.0 #}
{{ 42 | sqrt }}               {# -> 6.48... #}
{{ 3.14159 | num_format(2) }} {# -> "3.14" #}
```

## Acceptance criteria

- [ ] All 4 filters callable from templates
- [ ] `ceil` and `floor` work for positive and negative numbers
- [ ] `sqrt` returns correct value; produces `minijinja::Error` on negative input
- [ ] `num_format` rounds correctly and returns a string
- [ ] Tests cover zero, negatives, large numbers

## Rust guidance

- **File:** `src/template/engine/num_ops.rs` — new file in the `engine/` submodule. Unit struct `NumOps` with associated fn `pub(super) fn register(env: &mut Environment<'static>)`, same pattern as `StrOps::register` (no `&self`, clippy denies unused self, and there's no state to hold).
- **Implementation:** All are 1–3 line wrappers around `f64` methods (`ceil`, `floor`, `sqrt`). `num_format` uses `format!("{:.n$}", value)`. Error on negative sqrt via `minijinja::Error`.
- **Type handling:** Accept `Value`, convert via `as_f64()`. This handles both integer and float inputs transparently.
- **No new dependencies.**
- **Wiring in `engine.rs`:** add `mod num_ops;` and `NumOps::register(&mut env);` after the existing registrations.

## Blocked by

- `.scratch/template-service/issues/05-includes-and-utility-functions.md` (establishes the `register` associated fn pattern)
