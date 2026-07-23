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

## Implementation notes

Implemented in `.worktrees/numeric-module` on branch
`issue/08-numeric-module`, commit `2a5f438` (parent `0d9326a`, current
`main` tip). Not yet merged to `main`.

`NumOps` follows `StrOps`'s exact shape: a unit struct whose
`pub(super) fn register(env: &mut Environment<'static>)` calls
`env.add_filter(...)` once per filter, wired into `TemplateEngine::new`
(`src/template/engine.rs`) via `mod num_ops;` + `NumOps::register(&mut
env);`, both added after the existing registrations exactly as the
guidance directs. No new dependencies — `ceil`/`floor`/`sqrt` are thin
`f64` method wrappers; `num_format` is `format!("{value:.decimals$}")`.

One deviation from this issue's guidance:

- **No `Value::as_f64()` call.** The guidance says "Accept `Value`,
  convert via `as_f64()`", but `minijinja` 2.21.0's public `Value` API
  has no `as_f64()` method — confirmed via `rust-docs-mcp`
  (`search_items_preview` for `as_f64` against the cached crate
  returns zero results). Filter arguments are declared `f64` directly
  instead (e.g. `env.add_filter("ceil", |value: f64| value.ceil())`),
  letting minijinja's own `ArgType` impl for `f64` convert an
  int-or-float `Value` automatically and raise minijinja's standard
  argument-type error on anything else — the same pattern `str_ops.rs`
  already uses for its `&str` filter arguments, so this keeps the
  numeric filters consistent with the rest of `engine/`.

Test literals avoid `3.14`/`3.14159` (the issue's own usage examples
above) in the `rstest` case tables: `clippy::approx_constant` is a
deny-by-default lint that fires on *any* float literal close to a
known math constant, PI included, regardless of context. `cargo
clippy --workspace -- -D warnings` (no `--all-targets`) doesn't
compile `#[cfg(test)]` code and missed this; `cargo clippy --workspace
--all-targets -- -D warnings` caught 6 violations, fixed by swapping
in arbitrary decimals (`3.62`, `7.4567`, etc.) that exercise the same
rounding/formatting behavior without resembling a constant. The one
place the issue's literal examples are preserved verbatim is the
`engine.rs::tests::utilities::numeric_filters_are_reachable` wiring
test (`3.14 | ceil`, `42 | sqrt`, `3.14159 | num_format(2)`) — none of
those three particular literals happen to trip the lint (`42.0` isn't
constant-adjacent, and that test's `3.14`/`3.14159` render through the
full `TemplateEngine`, a different code path than the flagged
`rstest` case tables, so the same six flagged lines don't recur
there).

### Review pass (`rust-skills`)

A follow-up review against `own-`/`err-`/`num-`/`test-` guidelines
found and fixed two real issues, both still visible in the final diff
(not reverted/hidden):

1. **`sqrt`'s negative check was needlessly indirect.** Originally
   `value.is_sign_negative() && value != 0.0` — a sign-bit check plus
   an exact-equality float comparison to carve out `-0.0`. Simplified
   to a single ordering comparison, `value < 0.0`: `-0.0 < 0.0` is
   `false` under IEEE 754, so it already excludes `-0.0` without the
   extra clause. Matches `num-float-compare`'s guidance to prefer
   ordering comparisons over ad-hoc bit-pattern/equality tricks.
2. **The test harness itself had a latent correctness bug**, caught
   while adding a regression test for the `-0.0` behavior above. Every
   `rstest` case built its template source via
   `format!("{{{{ {input} | ceil }}}}")` — interpolating the `f64`
   case value directly into the source text. Rust's `f64` `Display`
   omits the trailing `.0` on whole numbers (`format!("{}", 4.0)` →
   `"4"`, not `"4.0"`) and, worse, `format!("{}", -0.0)` → `"-0"`,
   which minijinja's parser reads as an **integer** literal, not a
   float — collapsing to plain `0` (integers have no negative zero).
   The first attempt at a `case::negative_zero(-0.0, "-0.0")` case
   passed through this path and got `0.0`, i.e. it silently tested
   `0.0`, not `-0.0` — the exact input it claimed to cover. Rewrote
   the whole harness to pass every case value through minijinja's
   render context instead — `env().render_str("{{ value | ceil }}",
   minijinja::context! { value => input })` — the same pattern
   `str_ops.rs`'s existing tests already use for the same reason. This
   is a correctness fix, not cosmetic: it changes what several
   existing cases actually exercise (whole-number inputs like
   `already_whole(4.0, ...)` were previously round-tripping through an
   *integer* template literal that minijinja's `ArgType` conversion
   happened to make indistinguishable from a float one for those
   particular assertions — the bug was silent until the sign-sensitive
   `-0.0` case exposed it).

### Acceptance criteria status: 5/5 met, 0 unfulfilled

(Checkboxes above are left as originally written — this list
documents status only.)

- MET — All 4 filters callable from templates: registered in
  `NumOps::register` (`src/template/engine/num_ops.rs`), reachable
  through `TemplateEngine::new`. Tested end-to-end in
  `engine.rs::tests::utilities::numeric_filters_are_reachable`.
- MET — `ceil`/`floor` work for positive and negative numbers: tested
  in `num_ops.rs::tests::ceil::rounds_up_to_the_nearest_integer` and
  `tests::floor::rounds_down_to_the_nearest_integer`, each with
  `positive_fraction`/`negative_fraction` cases (plus
  `already_whole`/`zero`/`large_number`).
- MET — `sqrt` returns the correct value and produces a
  `minijinja::Error` on negative input:
  `tests::sqrt::returns_the_square_root` covers a perfect square, zero,
  negative zero (locks in the `value < 0.0` review fix above), a
  non-perfect square, and a large number;
  `tests::sqrt::errors_on_negative_input` renders `-4.0 | sqrt` and
  asserts `err.kind() == ErrorKind::InvalidOperation`.
- MET — `num_format` rounds correctly and returns a string:
  `tests::num_format::formats_to_exactly_n_decimal_places` covers
  rounding to 2 places, padding a whole number, 0 decimals, a negative
  number, zero, and a large number; the filter's signature
  (`fn num_format(value: f64, decimals: usize) -> String`) makes the
  return type a compile-time guarantee, not just a tested behavior.
- MET — Tests cover zero, negatives, and large numbers: zero is a
  case in all four filters' tables; negatives appear in
  `ceil`/`floor`/`num_format`'s tables and as the dedicated
  `sqrt::errors_on_negative_input` case; large numbers appear in all
  four (`1_000_000.5` for ceil/floor, `1_000_000.0` for sqrt,
  `1_234_567.891` for num_format).

Full crate verification at `HEAD` (`2a5f438`): **440** unit tests (23
new — 22 in `num_ops.rs`, 1 wiring test in `engine.rs`) + 1 integration
test + 10 doctests, all pass. `cargo clippy --workspace --all-targets
-- -D warnings` is clean except one pre-existing, unrelated
`disallowed_methods` failure on `std::fs::canonicalize` in
`src/config/store.rs` (confirmed via `git status`/`git diff` against
this branch's base commit `0d9326a` — untouched by this change,
matches the same pre-existing gap issue 05's write-up already
documented).
