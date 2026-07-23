# Date module expansion (date.* functions + flat date_* filters/tests)

Status: implemented

## Parent

`.scratch/template-service/spec.md`

## What to build

Extend the `date` namespace beyond `date.now(format)` (issue 05). Add generator functions on the `date` namespace object, flat-named pipeline filters for date string transformations, and minijinja tests for date introspection.

All use `chrono` (already a dependency per spec). Date/time strings are parsed via `chrono::NaiveDateTime` first, falling back to `chrono::NaiveDate`.

### Functions (on the `date` namespace object, alongside `now()`)

- **`date.today(format="%Y-%m-%d")`** — Returns the current date (no time component).
- **`date.tomorrow(format="%Y-%m-%d")`** — Returns tomorrow's date.
- **`date.yesterday(format="%Y-%m-%d")`** — Returns yesterday's date.
- **`date.from_timestamp(unix_ts, format="%Y-%m-%d")`** — Converts a Unix timestamp (integer) to a formatted date string.

### Filters (flat names)

Each takes a date/time string as pipeline input (parsed as datetime first, fallback to date).

- **`date_format(format_string)`** — Re-formats the date string using chrono `strftime` specifiers. Prefixed to avoid collision with minijinja's built-in `format` filter.
- **`timestamp`** — Converts a date string to a Unix timestamp (integer seconds).
- **`add_days(n)`** — Adds n days.
- **`sub_days(n)`** — Subtracts n days.
- **`add_months(n)`** — Adds n months.
- **`sub_months(n)`** — Subtracts n months.
- **`add_years(n)`** — Adds n years.
- **`sub_years(n)`** — Subtracts n years.
- **`start_of_month`** — Returns the first day of the date's month.
- **`end_of_month`** — Returns the last day of the date's month.
- **`weekday`** — Returns the day of the week as integer (0=Monday .. 6=Sunday).
- **`date_diff(other, unit="days")`** — Returns the duration between the piped date and `other`, expressed in `unit` (e.g., `"days"`, `"hours"`, `"minutes"`, `"seconds"`). Both dates are parsed as datetimes for sub-day precision.

Usage:
```jinja
{{ date.today() }}
{{ date.tomorrow("%A %d %B %Y") }}
{{ "2026-07-23" | add_days(7) }}
{{ "2026-07-23 14:30" | date_diff("2026-07-24", unit="hours") }}
```

### Tests (flat names, registered via `add_test`)

- **`is_past`** — Returns `true` if the date/datetime string is before `chrono::Utc::now()` (or the local equivalent).
- **`is_future`** — Returns `true` if the date/datetime string is after now.
- **`is_leap_year`** — Returns `true` if the date or integer year represents a leap year. Accepts a date string or an integer.

Usage:
```jinja
{% if note_date is is_past %}Archived{% endif %}
{% if 2024 is is_leap_year %}Leap!{% endif %}
```

## Acceptance criteria

- [x] 4 date generator functions on `date` namespace, alongside existing `now()`
- [x] 12 flat filters (`date_format`, `timestamp`, `add_days`, `sub_days`, `add_months`, `sub_months`, `add_years`, `sub_years`, `start_of_month`, `end_of_month`, `weekday`, `date_diff`)
- [x] 3 flat tests (`is_past`, `is_future`, `is_leap_year`)
- [x] `date_diff` returns sub-day precision (hours/minutes/seconds) when both inputs include time
- [x] Parse errors for invalid date strings produce `minijinja::Error` (not panics)
- [x] Tests cover leap years, month boundaries, UTC edge cases, and sub-day diff

## Rust guidance

- **File:** `src/template/engine/date_ops.rs` — extends the existing file in the `engine/` submodule. `DateOps` already exists as a unit struct with `register(self, env: &mut Environment<'static>)` and `Object` impl via `get_value`.
- **Date generators** (`today`/`tomorrow`/`yesterday`/`from_timestamp`): add match arms in the existing `get_value` method, same pattern as `"now"`. Each returns `Some(Value::from_function(move |...| ...))`. `from_timestamp` takes a positional `unix_ts: i64` arg plus an optional `format` kwarg.
- **Registering filters & tests:** Add `env.add_filter(...)` and `env.add_test(...)` calls at the TOP of the existing `register` method, before `env.add_global("date", ...)` — `self` is consumed by `add_global` but filters/tests don't need it (zero-capture closures). Update the `METHODS` const to include the 4 new function names so `enumerate` stays coherent.
- **Date parsing helpers:** Extract a shared helper `parse_date(s: &str) -> Result<NaiveDateTime, Error>` that tries `NaiveDateTime::parse_from_str` first (with common format attempts), then falls back to `NaiveDate::parse_from_str`. Both filters and tests need this same logic — don't duplicate it.
- **`date_format` filter:** Prefixed because minijinja has a built-in `format` filter (printf-style). Our `date_format` wraps chrono's `strftime` on the parsed piped input.
- **Tests (`is_past`, etc.):** Use `env.add_test()`. Accept `Value` and return `bool`. `is_leap_year` checks if the value is an integer (treat as year first, then try date string).
- **`date_diff`:** Takes `other: &str` and optional `unit: Option<&str>`. Default unit `"days"`. Uses `chrono::TimeDelta` for the difference, converts to requested unit. Returns `f64` for sub-day precision when both inputs include time; integer otherwise.

## Blocked by

- `.scratch/template-service/issues/05-includes-and-utility-functions.md` (establishes `DateOps`, `date.now`, and the struct-based registration pattern)

## Implementation notes

Delivered in `.worktrees/date-expansion` (branch `issue/07-date-expansion`,
not yet merged to `main`), a single-file extension of
`src/template/engine/date_ops.rs` plus doc-comment updates in
`src/template/engine.rs` and `src/template/mod.rs` referencing the
expanded `date` namespace. 473/473 lib tests (65 new in `date_ops`),
1/1 `tests/init_cli.rs`, 10/10 doctests passing; `cargo clippy --lib
--tests` clean against this module (the one remaining clippy error —
`std::fs::canonicalize` in `src/config/store.rs` — pre-exists on `main`,
confirmed via `cargo clippy` there too, unrelated to this ticket).

- **Parsing:** `parse_date_precise(s) -> Result<(NaiveDateTime, bool),
  Error>` is the single shared parser — tries a fixed list of full
  datetime formats (space- and `T`-separated, with/without
  seconds/fractional seconds) first, falls back to a bare `%Y-%m-%d`
  date at midnight. The `bool` (whether a real time component was
  found) drives two behaviors: `date_diff`'s integer-vs-`f64` choice,
  and every arithmetic filter's output precision via `format_precise`
  (re-serializes at the same shape the input had, so a date-only string
  piped through `add_days`/`end_of_month`/etc. never grows a fabricated
  `00:00:00`, and a datetime string never silently loses its
  time-of-day). `parse_date` is `parse_date_precise` with the bool
  discarded — the entrypoint every filter/test besides `date_diff` uses.
- **No panics:** every `checked_*` chrono call (`checked_add_days`,
  `checked_add_months`, `with_day`, `succ_opt`/`pred_opt`, ...) routes
  its `None` through `date_out_of_range_error()`; `from_timestamp` uses
  the non-deprecated `DateTime::from_timestamp` (not
  `NaiveDateTime::from_timestamp_opt`, which chrono 0.4.45 deprecates).
  `add_years`/`sub_years` reuse `add_months`/`sub_months`'s
  end-of-month clamping via `n.checked_mul(12)` months, rather than a
  separate year-arithmetic path.
- **`weekday`** uses `Weekday::num_days_from_monday()` (0=Monday) per
  spec — not chrono's own Sunday-first `number_from_sunday`.
- **`is_leap_year`** takes `&Value` (not owned `Value`) — confirmed via
  a build probe that minijinja's `add_test` accepts a reference
  argument here (unlike `add_filter`'s trailing `Kwargs`, which must be
  owned to satisfy the `Function` trait; `date_diff_filter` carries a
  documented `#[allow(clippy::needless_pass_by_value, reason = ...)]`
  for that one).
- **Timezone convention:** `date.now`/`today`/`tomorrow`/`yesterday`
  stay `Local` (existing convention from `now`, issue 05). Every filter
  parsing an arbitrary date *string* (`timestamp`, `date_diff`,
  `is_past`/`is_future`) treats a naive input as UTC — deterministic
  and testable, and consistent with `date.from_timestamp` also being
  UTC-based.
- Verified against real `chrono`/`minijinja` 0.4.45/2.21.0 API via
  `rust-docs-mcp` before writing (`checked_add_months`/
  `checked_sub_months` take `Months::new(u32)`; `Datelike::
  num_days_in_month` returns `u8`, not `u32` — caught by `cargo build`,
  not assumed; `NaiveDate::leap_year` takes `&self`, not part of the
  `Datelike` trait).
