# Date module expansion (date.* functions + flat date_* filters/tests)

Status: ready-for-agent

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

- [ ] 4 date generator functions on `date` namespace, alongside existing `now()`
- [ ] 12 flat filters (`date_format`, `timestamp`, `add_days`, `sub_days`, `add_months`, `sub_months`, `add_years`, `sub_years`, `start_of_month`, `end_of_month`, `weekday`, `date_diff`)
- [ ] 3 flat tests (`is_past`, `is_future`, `is_leap_year`)
- [ ] `date_diff` returns sub-day precision (hours/minutes/seconds) when both inputs include time
- [ ] Parse errors for invalid date strings produce `minijinja::Error` (not panics)
- [ ] Tests cover leap years, month boundaries, UTC edge cases, and sub-day diff

## Rust guidance

- **File layout:** `src/template/date_ops.rs` — `DateOps` holds the namespace functions (via `Object` trait). Filters and tests registered via a `register(&self, env)` method calling `env.add_filter()`/`env.add_test()`.
- **Date generators** (`today`/`tomorrow`/`yesterday`/`from_timestamp`): methods on the `DateOps` struct returned via `get_value` (same pattern as `FileOps.write_to` from issue 02). These live alongside the existing `now` method.
- **Date parsing:** For filters/tests, parse piped value as `chrono::NaiveDateTime` first (with format parsing), fall back to `chrono::NaiveDate`. This allows `date_diff` to return sub-day precision.
- **`date_format` filter:** Prefixed because minijinja has a built-in `format` filter (printf-style). Our `date_format` wraps chrono's `strftime`.
- **Tests (`is_past`, etc.):** Registered via `env.add_test()`. Accept `Value` and return `bool`. `is_leap_year` checks if the value is an integer (treat as year) or a date string (extract year).
- **`date_diff`:** Takes `other: &str` and optional `unit: Option<&str>`. Default unit `"days"`. Uses `chrono::Duration` (or `TimeDelta` in newer chrono) for the difference, converts to requested unit.
- **`duration` return type:** `date_diff` returns the numeric value as f64 (to support fractional hours/minutes), or integer if `unit="days"` and both inputs are dates (no time component).

## Blocked by

- `.scratch/template-service/issues/05-includes-and-utility-functions.md` (establishes `DateOps`, `date.now`, and the struct-based registration pattern)
