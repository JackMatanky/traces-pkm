//! [`DateOps`]: the `date` namespace object registered as a minijinja
//! global by [`super::TemplateEngine`], plus the flat `date_*`/`is_*`
//! pipeline filters and tests it registers alongside it.
//!
//! **Namespace functions** (`date.now()`, `date.today()`,
//! `date.tomorrow()`, `date.yesterday()`, `date.from_timestamp(ts)`) are
//! *generators*: no date argument, producing a formatted string from the
//! current instant (or a timestamp). **Filters** (`date_format`,
//! `timestamp`, `add_days`, `sub_days`, `add_months`, `sub_months`,
//! `add_years`, `sub_years`, `start_of_month`, `end_of_month`, `weekday`,
//! `date_diff`) and **tests** (`is_past`, `is_future`, `is_leap_year`) are
//! *transformations*: they take a piped date/time string. `date_format` is
//! prefixed to avoid colliding with minijinja's built-in `format` filter.
//!
//! All date/time string parsing funnels through [`parse_date`]/
//! [`parse_date_precise`], so every filter and test shares the same
//! accepted formats: a full datetime is tried first, falling back to a
//! bare `%Y-%m-%d` date at midnight.
//!
//! Every arithmetic filter re-serializes its result at the same precision
//! its input had (see [`format_precise`]), so piping a date-only string
//! through a chain of filters never grows a fabricated `00:00:00`, and a
//! datetime string never silently loses its time-of-day.

use std::{fmt::Write as _, sync::Arc};

use chrono::{
    Datelike as _, Days, Local, Months, NaiveDate, NaiveDateTime, Utc,
};
use minijinja::{
    Environment, Error, ErrorKind,
    value::{Enumerator, Kwargs, Object, Value},
};

/// `date.now(format=...)`'s default format when the `format` kwarg is
/// omitted — an ISO-8601-style date (`YYYY-MM-DD`).
/// Also the default output shape [`format_precise`] uses for a
/// date-only (no time component) input.
const DEFAULT_FORMAT: &str = "%Y-%m-%d";

/// [`format_precise`]'s output shape for an input that carried a time
/// component.
const DEFAULT_DATETIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

/// Formats [`parse_date_precise`] tries, in order, before falling back
/// to a bare date — space-separated (`2026-07-23 14:30[:00]`) and
/// `T`-separated ISO 8601 (with or without seconds/fractional seconds).
const DATETIME_FORMATS: &[&str] = &[
    "%Y-%m-%d %H:%M:%S",
    "%Y-%m-%d %H:%M",
    "%Y-%m-%dT%H:%M:%S%.f",
    "%Y-%m-%dT%H:%M:%S",
    "%Y-%m-%dT%H:%M",
];

/// Method names `date` exposes, for [`DateOps::enumerate`].
const METHODS: &[&str] =
    &["now", "today", "tomorrow", "yesterday", "from_timestamp"];

/// Backs the `date` namespace object. Stateless — see the module docs.
#[derive(Debug)]
pub(super) struct DateOps;

impl DateOps {
    /// Registers the `date` global plus every flat `date_*` filter and
    /// `is_*` test this module owns. Filters/tests are added first —
    /// they're zero-capture free functions, needing no `self` — then
    /// `self` is consumed registering the `date` namespace object last.
    #[inline]
    pub(super) fn register(self, env: &mut Environment<'static>) {
        env.add_filter("date_format", date_format);
        env.add_filter("timestamp", timestamp);
        env.add_filter("add_days", add_days);
        env.add_filter("sub_days", sub_days);
        env.add_filter("add_months", add_months);
        env.add_filter("sub_months", sub_months);
        env.add_filter("add_years", add_years);
        env.add_filter("sub_years", sub_years);
        env.add_filter("start_of_month", start_of_month);
        env.add_filter("end_of_month", end_of_month);
        env.add_filter("weekday", weekday);
        env.add_filter("date_diff", date_diff);
        env.add_test("is_past", is_past);
        env.add_test("is_future", is_future);
        env.add_test("is_leap_year", is_leap_year);
        env.add_global("date", Value::from_object(self));
    }
}

impl Object for DateOps {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        match key.as_str()? {
            "now" => Some(Value::from_function(
                |kwargs: Kwargs| -> Result<String, Error> {
                    let format = format_kwarg(&kwargs)?;
                    // `write!` into a `String` propagates a formatting
                    // failure as `Err`; `.to_string()` would instead
                    // panic on the same input, since its blanket impl
                    // `.expect()`s a successful `Display::fmt`, and
                    // Chrono's `DelayedFormat` returns `Err` — not a
                    // panic of its own — for an invalid specifier such
                    // as `%Q`.
                    format_with(Local::now().format(format), format)
                },
            )),
            "today" => Some(Value::from_function(
                |kwargs: Kwargs| -> Result<String, Error> {
                    let format = format_kwarg(&kwargs)?;
                    format_with(
                        Local::now().date_naive().format(format),
                        format,
                    )
                },
            )),
            "tomorrow" => Some(Value::from_function(
                |kwargs: Kwargs| -> Result<String, Error> {
                    let format = format_kwarg(&kwargs)?;
                    let date = Local::now()
                        .date_naive()
                        .succ_opt()
                        .ok_or_else(date_out_of_range_error)?;
                    format_with(date.format(format), format)
                },
            )),
            "yesterday" => Some(Value::from_function(
                |kwargs: Kwargs| -> Result<String, Error> {
                    let format = format_kwarg(&kwargs)?;
                    let date = Local::now()
                        .date_naive()
                        .pred_opt()
                        .ok_or_else(date_out_of_range_error)?;
                    format_with(date.format(format), format)
                },
            )),
            "from_timestamp" => Some(Value::from_function(
                |unix_ts: i64, kwargs: Kwargs| -> Result<String, Error> {
                    let format = format_kwarg(&kwargs)?;
                    let datetime = chrono::DateTime::from_timestamp(unix_ts, 0)
                        .ok_or_else(|| invalid_timestamp_error(unix_ts))?
                        .naive_utc();
                    format_with(datetime.format(format), format)
                },
            )),
            _ => None,
        }
    }

    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Str(METHODS)
    }
}

/// Extracts the shared `format="..."` kwarg every `date.*` namespace
/// method takes, defaulting to [`DEFAULT_FORMAT`], and rejects any
/// other kwarg via [`Kwargs::assert_all_used`] — the one place all
/// five `now`/`today`/`tomorrow`/`yesterday`/`from_timestamp` closures
/// decide how their optional `format=` argument is read.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `format` is present but isn't a string.
/// - [`ErrorKind::TooManyArguments`] if `kwargs` carries any key besides
///   `format`.
fn format_kwarg(kwargs: &Kwargs) -> Result<&str, Error> {
    let format =
        kwargs.get::<Option<&str>>("format")?.unwrap_or(DEFAULT_FORMAT);
    kwargs.assert_all_used()?;
    Ok(format)
}

/// Formats `formattable` — anything chrono's `.format(fmt)` produces
/// (a `DelayedFormat`, from a `NaiveDate`/`NaiveDateTime`/`DateTime<_>`)
/// — writing through [`std::fmt::Write`] rather than `.to_string()`: the
/// latter's blanket impl `.expect()`s a successful `Display::fmt`, but
/// `DelayedFormat` returns `Err` (not a panic of its own) for an invalid
/// strftime specifier such as `%Q`, so writing directly is what turns
/// that into a normal [`minijinja::Error`] instead of a panic.
///
/// # Errors
///
/// [`ErrorKind::InvalidOperation`] if `format` isn't a strftime
/// specifier `formattable` can render (e.g. `%Q`).
fn format_with(
    formattable: impl std::fmt::Display,
    format: &str,
) -> Result<String, Error> {
    let mut rendered = String::new();
    write!(rendered, "{formattable}").map_err(|_fmt_error| {
        Error::new(
            ErrorKind::InvalidOperation,
            format!("invalid date format {format:?}"),
        )
    })?;
    Ok(rendered)
}

/// Re-serializes `dt` at the precision `has_time` reports —
/// [`DEFAULT_DATETIME_FORMAT`] when the original input carried a time
/// component, [`DEFAULT_FORMAT`] (bare date) otherwise. Every arithmetic
/// filter (`add_days`, `end_of_month`, etc.) uses this for its output,
/// so the output shape mirrors the input shape: a date-only string
/// piped through never grows a fabricated `00:00:00`, and a datetime
/// string piped through never silently loses its time-of-day.
///
/// # Errors
///
/// Propagates [`format_with`]'s [`ErrorKind::InvalidOperation`]; in
/// practice unreachable here, since `format` is always
/// [`DEFAULT_FORMAT`] or [`DEFAULT_DATETIME_FORMAT`], both valid
/// strftime specifiers.
fn format_precise(dt: NaiveDateTime, has_time: bool) -> Result<String, Error> {
    let format = if has_time {
        DEFAULT_DATETIME_FORMAT
    } else {
        DEFAULT_FORMAT
    };
    format_with(dt.format(format), format)
}

/// Tries each of [`DATETIME_FORMATS`] in turn; `None` if none match.
fn try_parse_datetime(s: &str) -> Option<NaiveDateTime> {
    DATETIME_FORMATS
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(s, format).ok())
}

/// Parses `s` as a date/time string, reporting alongside it whether a
/// genuine time component was found. Tries a full datetime first (see
/// [`DATETIME_FORMATS`]); on no match, falls back to a bare `%Y-%m-%d`
/// date at midnight. [`date_diff`] uses the `bool` to decide
/// between integer-unit and sub-day-precision (`f64`) output — every
/// other caller goes through [`parse_date`], which discards it.
///
/// # Errors
///
/// [`ErrorKind::InvalidOperation`] if `s` matches neither a
/// [`DATETIME_FORMATS`] entry nor the bare `%Y-%m-%d` fallback.
fn parse_date_precise(s: &str) -> Result<(NaiveDateTime, bool), Error> {
    if let Some(datetime) = try_parse_datetime(s) {
        return Ok((datetime, true));
    }
    NaiveDate::parse_from_str(s, DEFAULT_FORMAT)
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|datetime| (datetime, false))
        .ok_or_else(|| invalid_date_error(s))
}

/// The shared date/time string parser every filter and test besides
/// [`date_diff`] uses — see [`parse_date_precise`] for the
/// accepted formats and fallback behavior.
///
/// # Errors
///
/// Propagates [`parse_date_precise`]'s [`ErrorKind::InvalidOperation`].
fn parse_date(s: &str) -> Result<NaiveDateTime, Error> {
    parse_date_precise(s).map(|(datetime, _has_time)| datetime)
}

/// Builds the error for a date/time string matching none of
/// [`parse_date_precise`]'s accepted formats.
fn invalid_date_error(s: &str) -> Error {
    Error::new(ErrorKind::InvalidOperation, format!("invalid date {s:?}"))
}

/// Builds the error for a Unix timestamp chrono can't represent as a
/// `NaiveDateTime` (`date.from_timestamp`'s `unix_ts` argument, roughly
/// ±262,000 years from the epoch).
fn invalid_timestamp_error(unix_ts: i64) -> Error {
    Error::new(
        ErrorKind::InvalidOperation,
        format!("timestamp {unix_ts} is out of range"),
    )
}

/// Builds the error for date arithmetic (`add_days`, `add_years`,
/// `date.tomorrow()`, etc.) that overflows chrono's representable date
/// range — reached only at the extremes (multi-millennia offsets), but
/// every `checked_*` chrono call this module makes can return `None`,
/// and this module never `.unwrap()`s one.
fn date_out_of_range_error() -> Error {
    Error::new(
        ErrorKind::InvalidOperation,
        "date arithmetic overflowed the supported range",
    )
}

/// Builds the error for `date_diff`'s `unit` kwarg when it isn't one of
/// the four accepted unit names.
fn unknown_unit_error(unit: &str) -> Error {
    Error::new(
        ErrorKind::InvalidOperation,
        format!(
            "unknown date_diff unit {unit:?} (expected \"days\", \"hours\", \
             \"minutes\", or \"seconds\")"
        ),
    )
}

/// `{{ value | date_format(format_string) }}` — re-formats a piped
/// date/time string with an arbitrary strftime specifier. Prefixed
/// (not just `format`) to avoid colliding with minijinja's built-in
/// `format` filter, which is printf-style and unrelated to dates.
///
/// # Errors
///
/// [`ErrorKind::InvalidOperation`] if `value` isn't a parseable
/// date/time string (see [`parse_date`]) or `format` isn't a valid
/// strftime specifier (see [`format_with`]).
fn date_format(value: &str, format: &str) -> Result<String, Error> {
    let datetime = parse_date(value)?;
    format_with(datetime.format(format), format)
}

/// `{{ value | timestamp }}` — converts a piped date/time string to
/// Unix seconds, treating a naive (timezone-less) input as UTC.
///
/// # Errors
///
/// [`ErrorKind::InvalidOperation`] if `value` isn't a parseable
/// date/time string — see [`parse_date`].
fn timestamp(value: &str) -> Result<i64, Error> {
    Ok(parse_date(value)?.and_utc().timestamp())
}

/// Parses `value` as a date/time string, transforms `datetime` via `op`,
/// and re-serializes the result at `value`'s original precision.
///
/// # Errors
///
/// [`ErrorKind::InvalidOperation`] if `value` isn't parseable or `op`
/// returns `None` (arithmetic overflow).
fn shift_date(
    value: &str,
    op: impl FnOnce(NaiveDateTime) -> Option<NaiveDateTime>,
) -> Result<String, Error> {
    let (datetime, has_time) = parse_date_precise(value)?;
    let shifted = op(datetime).ok_or_else(date_out_of_range_error)?;
    format_precise(shifted, has_time)
}
/// `{{ value | add_days(n) }}`
///
/// # Errors
///
/// [`ErrorKind::InvalidOperation`] if `value` isn't a parseable
/// date/time string (see [`parse_date_precise`]) or the shift
/// overflows chrono's representable range (see
/// [`date_out_of_range_error`]).
fn add_days(value: &str, n: u64) -> Result<String, Error> {
    shift_date(value, |dt| dt.checked_add_days(Days::new(n)))
}

/// `{{ value | sub_days(n) }}`
///
/// # Errors
///
/// [`ErrorKind::InvalidOperation`] if `value` isn't a parseable
/// date/time string (see [`parse_date_precise`]) or the shift
/// overflows chrono's representable range (see
/// [`date_out_of_range_error`]).
fn sub_days(value: &str, n: u64) -> Result<String, Error> {
    shift_date(value, |dt| dt.checked_sub_days(Days::new(n)))
}

/// `{{ value | add_months(n) }}` — clamps to the last day of the
/// resulting month when the original day doesn't exist there (e.g.
/// `2023-01-31` + 1 month -> `2023-02-28`), per
/// [`NaiveDateTime::checked_add_months`]'s documented behavior.
///
/// # Errors
///
/// [`ErrorKind::InvalidOperation`] if `value` isn't a parseable
/// date/time string (see [`parse_date_precise`]) or the shift
/// overflows chrono's representable range (see
/// [`date_out_of_range_error`]).
fn add_months(value: &str, n: u32) -> Result<String, Error> {
    shift_date(value, |dt| dt.checked_add_months(Months::new(n)))
}

/// `{{ value | sub_months(n) }}` — same end-of-month clamping as
/// [`add_months`], in reverse.
///
/// # Errors
///
/// [`ErrorKind::InvalidOperation`] if `value` isn't a parseable
/// date/time string (see [`parse_date_precise`]) or the shift
/// overflows chrono's representable range (see
/// [`date_out_of_range_error`]).
fn sub_months(value: &str, n: u32) -> Result<String, Error> {
    shift_date(value, |dt| dt.checked_sub_months(Months::new(n)))
}

/// `{{ value | add_years(n) }}` — implemented as `n * 12` months (via
/// [`add_months`]'s underlying call), so a Feb 29 input clamps to
/// Feb 28 in a non-leap target year exactly like `add_months` clamps
/// across any other month-length mismatch.
///
/// # Errors
///
/// [`ErrorKind::InvalidOperation`] if `value` isn't a parseable
/// date/time string (see [`parse_date_precise`]), `n * 12` overflows
/// [`u32`], or the shift overflows chrono's representable range (see
/// [`date_out_of_range_error`]).
fn add_years(value: &str, n: u32) -> Result<String, Error> {
    shift_date(value, |dt| {
        let months = n.checked_mul(12)?;
        dt.checked_add_months(Months::new(months))
    })
}

/// `{{ value | sub_years(n) }}` — see [`add_years`].
///
/// # Errors
///
/// [`ErrorKind::InvalidOperation`] if `value` isn't a parseable
/// date/time string (see [`parse_date_precise`]), `n * 12` overflows
/// [`u32`], or the shift overflows chrono's representable range (see
/// [`date_out_of_range_error`]).
fn sub_years(value: &str, n: u32) -> Result<String, Error> {
    shift_date(value, |dt| {
        let months = n.checked_mul(12)?;
        dt.checked_sub_months(Months::new(months))
    })
}

/// `{{ value | start_of_month }}`
///
/// # Errors
///
/// [`ErrorKind::InvalidOperation`] if `value` isn't a parseable
/// date/time string — see [`parse_date_precise`]. The `with_day(1)`
/// call underneath can't itself fail (day 1 exists in every month), but
/// stays behind [`date_out_of_range_error`] since it's a fallible API.
fn start_of_month(value: &str) -> Result<String, Error> {
    shift_date(value, |dt| dt.with_day(1))
}

/// `{{ value | end_of_month }}`
///
/// # Errors
///
/// [`ErrorKind::InvalidOperation`] if `value` isn't a parseable
/// date/time string (see [`parse_date_precise`]) or the resulting
/// last-of-month day is out of chrono's representable range (see
/// [`date_out_of_range_error`]).
fn end_of_month(value: &str) -> Result<String, Error> {
    shift_date(value, |dt| dt.with_day(u32::from(dt.num_days_in_month())))
}

/// `{{ value | weekday }}` — `0` for Monday through `6` for Sunday
/// (chrono's own
/// [`Weekday::number_from_sunday`](chrono::Weekday::number_from_sunday)
/// is Sunday-first, so this filter remaps to Monday-first order).
///
/// # Errors
///
/// [`ErrorKind::InvalidOperation`] if `value` isn't a parseable
/// date/time string — see [`parse_date`].
fn weekday(value: &str) -> Result<u32, Error> {
    Ok(parse_date(value)?.weekday().num_days_from_monday())
}

/// The `unit=` kwarg [`date_diff`] accepts, parsed once via
/// [`Self::parse`] and matched exhaustively in each precision branch
/// below — the same "small enum over a piped string" pattern
/// [`path_ops::PathQuery`](super::path_ops) uses for its I/O tests.
#[derive(Clone, Copy)]
enum DiffUnit {
    Days,
    Hours,
    Minutes,
    Seconds,
}

impl DiffUnit {
    /// Parses `unit`'s `unit=` kwarg value; `None` for anything but
    /// the four accepted names.
    fn parse(unit: &str) -> Option<Self> {
        match unit {
            "days" => Some(Self::Days),
            "hours" => Some(Self::Hours),
            "minutes" => Some(Self::Minutes),
            "seconds" => Some(Self::Seconds),
            _ => None,
        }
    }
}

/// `{{ value | date_diff(other, unit="days") }}` — the signed duration
/// from the piped value to `other` (positive when `other` is later),
/// expressed in `unit` (`"days"` default, `"hours"`, `"minutes"`, or
/// `"seconds"`). Returns `f64` when both `value` and `other` carry a
/// time component (sub-day precision is meaningful); otherwise an `i64`
/// whole-unit count.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` or `other` isn't a parseable
///   date/time string (see [`parse_date_precise`]), or `unit` isn't `"days"`,
///   `"hours"`, `"minutes"`, or `"seconds"` (see [`unknown_unit_error`]).
/// - [`ErrorKind::TooManyArguments`] if `kwargs` carries any key besides
///   `unit`.
#[expect(
    clippy::needless_pass_by_value,
    reason = "minijinja's Function trait extracts a filter's trailing Kwargs \
              argument by value; only `&self` methods on it are needed here"
)]
fn date_diff(value: &str, other: &str, kwargs: Kwargs) -> Result<Value, Error> {
    let unit = kwargs.get::<Option<&str>>("unit")?.unwrap_or("days");
    kwargs.assert_all_used()?;
    let unit = DiffUnit::parse(unit).ok_or_else(|| unknown_unit_error(unit))?;
    let (from, from_has_time) = parse_date_precise(value)?;
    let (to, to_has_time) = parse_date_precise(other)?;
    let delta = to.signed_duration_since(from);

    if from_has_time && to_has_time {
        #[expect(
            clippy::as_conversions,
            clippy::cast_precision_loss,
            reason = "TimeDelta::num_seconds() is bounded by chrono's \
                      NaiveDateTime range (~262,000 years, well under 2^52 \
                      seconds), so this cast never loses precision in practice"
        )]
        let seconds =
            delta.num_seconds() as f64 + f64::from(delta.subsec_nanos()) / 1e9;
        let result = match unit {
            DiffUnit::Days => seconds / 86_400.0,
            DiffUnit::Hours => seconds / 3_600.0,
            DiffUnit::Minutes => seconds / 60.0,
            DiffUnit::Seconds => seconds,
        };
        Ok(Value::from(result))
    } else {
        let result = match unit {
            DiffUnit::Days => delta.num_days(),
            DiffUnit::Hours => delta.num_hours(),
            DiffUnit::Minutes => delta.num_minutes(),
            DiffUnit::Seconds => delta.num_seconds(),
        };
        Ok(Value::from(result))
    }
}

/// `{% if value is is_past %}` — `true` when the piped date/time string
/// is before now (UTC; a naive input is treated as UTC, matching
/// [`timestamp`]).
///
/// # Errors
///
/// [`ErrorKind::InvalidOperation`] if `value` isn't a parseable
/// date/time string — see [`parse_date`].
fn is_past(value: &str) -> Result<bool, Error> {
    Ok(parse_date(value)?.and_utc() < Utc::now())
}

/// `{% if value is is_future %}` — see [`is_past`].
///
/// # Errors
///
/// [`ErrorKind::InvalidOperation`] if `value` isn't a parseable
/// date/time string — see [`parse_date`].
fn is_future(value: &str) -> Result<bool, Error> {
    Ok(parse_date(value)?.and_utc() > Utc::now())
}

/// `{% if value is is_leap_year %}` — accepts either an integer year
/// (`2024 is is_leap_year`) or a date/time string, checked via
/// [`parse_date`].
///
/// # Errors
///
/// [`ErrorKind::InvalidOperation`] (via [`leap_year_input_error`]) if
/// `value` is neither an integer year representable as [`i32`] nor a
/// parseable date/time string (see [`parse_date`]), or the year is out
/// of [`NaiveDate`]'s representable range.
fn is_leap_year(value: &Value) -> Result<bool, Error> {
    let year = if let Some(year) = value.as_i64() {
        i32::try_from(year)
            .map_err(|_out_of_range| leap_year_input_error(value))?
    } else if let Some(s) = value.as_str() {
        parse_date(s)?.year()
    } else {
        return Err(leap_year_input_error(value));
    };
    NaiveDate::from_ymd_opt(year, 1, 1)
        .map(|date| date.leap_year())
        .ok_or_else(|| leap_year_input_error(value))
}

/// Builds the `is_leap_year` error for an argument that's neither a
/// representable year nor a valid date string.
fn leap_year_input_error(value: &Value) -> Error {
    Error::new(
        ErrorKind::InvalidOperation,
        format!(
            "is_leap_year expects an integer year or a date string, got \
             {value:?}"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Environment<'static> {
        let mut env = Environment::new();
        DateOps.register(&mut env);
        env
    }

    mod get_value {
        use super::*;

        #[test]
        fn get_value_returns_none_for_an_unknown_key() {
            let ops = Arc::new(DateOps);

            assert!(ops.get_value(&Value::from("later")).is_none());
        }

        #[test]
        fn get_value_returns_none_for_a_non_string_key() {
            let ops = Arc::new(DateOps);

            assert!(ops.get_value(&Value::from(1)).is_none());
        }
    }

    mod now {
        use pretty_assertions::assert_eq;

        use super::*;

        /// Asserts the shape (fixed length, all-ASCII-digit-or-hyphen), not
        /// a literal value — see the issue's determinism guidance for
        /// `date.now()`.
        #[test]
        fn now_formats_with_the_default_format_when_no_kwarg_is_given() {
            let rendered = env()
                .render_str("{{ date.now() }}", minijinja::context!())
                .expect("render succeeds");

            assert_eq!(rendered.len(), "YYYY-MM-DD".len());
            assert!(
                rendered.chars().all(|c| c.is_ascii_digit() || c == '-'),
                "expected an all-digit-or-hyphen date, got {rendered:?}"
            );
            assert_eq!(
                rendered.as_bytes().get(4),
                Some(&b'-'),
                "expected a hyphen at index 4 of {rendered:?}"
            );
            assert_eq!(
                rendered.as_bytes().get(7),
                Some(&b'-'),
                "expected a hyphen at index 7 of {rendered:?}"
            );
        }

        #[test]
        fn now_formats_using_an_explicit_format_kwarg() {
            let rendered = env()
                .render_str(
                    r#"{{ date.now(format="%Y") }}"#,
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered.len(), 4);
            assert!(
                rendered.chars().all(|c| c.is_ascii_digit()),
                "expected an all-digit year, got {rendered:?}"
            );
        }

        /// Regression: Chrono's `DelayedFormat::fmt` returns `Err` for an
        /// invalid specifier like `%Q`, and `String::to_string()`'s blanket
        /// impl panics on that `Err` — writing through `fmt::Write`
        /// directly instead must surface it as a normal render error.
        #[test]
        fn now_returns_an_error_instead_of_panicking_on_an_invalid_format() {
            let error = env()
                .render_str(
                    r#"{{ date.now(format="%Q") }}"#,
                    minijinja::context!(),
                )
                .expect_err("invalid format specifier fails cleanly");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }

        #[test]
        fn now_rejects_a_non_string_format_kwarg() {
            let error = env()
                .render_str("{{ date.now(format=1) }}", minijinja::context!())
                .expect_err("non-string format kwarg fails");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }

        #[test]
        fn now_rejects_an_unknown_kwarg() {
            let error = env()
                .render_str("{{ date.now(bogus=1) }}", minijinja::context!())
                .expect_err("unknown kwarg fails");

            assert_eq!(error.kind(), ErrorKind::TooManyArguments);
        }
    }

    /// `today`/`tomorrow`/`yesterday` can't assert a literal value
    /// without mocking the clock (same determinism constraint as `now`),
    /// but their relationship to each other IS deterministic: whatever
    /// `today()` renders, `tomorrow()`/`yesterday()` must be exactly one
    /// calendar day ahead/behind it, modulo the vanishingly rare case of
    /// a midnight rollover between the two calls.
    mod today_tomorrow_yesterday {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[test]
        fn tomorrow_and_yesterday_are_one_day_from_today() {
            let rendered_env = env();
            let today: NaiveDate = rendered_env
                .render_str("{{ date.today() }}", minijinja::context!())
                .expect("render succeeds")
                .parse()
                .expect("today() renders a valid ISO date");
            let tomorrow: NaiveDate = rendered_env
                .render_str("{{ date.tomorrow() }}", minijinja::context!())
                .expect("render succeeds")
                .parse()
                .expect("tomorrow() renders a valid ISO date");
            let yesterday: NaiveDate = rendered_env
                .render_str("{{ date.yesterday() }}", minijinja::context!())
                .expect("render succeeds")
                .parse()
                .expect("yesterday() renders a valid ISO date");

            assert_eq!(tomorrow, today.succ_opt().unwrap());
            assert_eq!(yesterday, today.pred_opt().unwrap());
        }

        #[rstest]
        #[case::today("today")]
        #[case::tomorrow("tomorrow")]
        #[case::yesterday("yesterday")]
        fn accepts_an_explicit_format_kwarg(#[case] function: &str) {
            let rendered = env()
                .render_str(
                    &format!(r#"{{{{ date.{function}(format="%Y") }}}}"#),
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered.len(), 4);
            assert!(rendered.chars().all(|c| c.is_ascii_digit()));
        }

        #[rstest]
        #[case::today("today")]
        #[case::tomorrow("tomorrow")]
        #[case::yesterday("yesterday")]
        fn returns_an_error_instead_of_panicking_on_an_invalid_format(
            #[case] function: &str,
        ) {
            let error = env()
                .render_str(
                    &format!(r#"{{{{ date.{function}(format="%Q") }}}}"#),
                    minijinja::context!(),
                )
                .expect_err("invalid format specifier fails cleanly");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }
    }

    mod from_timestamp {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn formats_the_unix_epoch_with_the_default_format() {
            let rendered = env()
                .render_str(
                    "{{ date.from_timestamp(0) }}",
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "1970-01-01");
        }

        #[test]
        fn accepts_an_explicit_format_kwarg() {
            let rendered = env()
                .render_str(
                    r#"{{ date.from_timestamp(0, format="%H:%M") }}"#,
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "00:00");
        }

        #[test]
        fn rejects_a_timestamp_out_of_range_instead_of_panicking() {
            let error = env()
                .render_str(
                    "{{ date.from_timestamp(9999999999999999) }}",
                    minijinja::context!(),
                )
                .expect_err("out-of-range timestamp fails cleanly");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }
    }

    mod enumerate {
        use super::*;

        #[test]
        fn enumerate_lists_every_method() {
            let ops = Arc::new(DateOps);

            assert!(matches!(ops.enumerate(), Enumerator::Str(METHODS)));
        }

        #[test]
        fn every_enumerated_method_resolves_via_get_value() {
            let ops = Arc::new(DateOps);

            for method in METHODS {
                assert!(
                    ops.get_value(&Value::from(*method)).is_some(),
                    "{method:?} is enumerated but get_value has no matching \
                     arm"
                );
            }
        }
    }

    mod date_format {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::date_only("2026-07-23", "%d/%m/%Y", "23/07/2026")]
        #[case::datetime_input("2026-07-23 14:30", "%H:%M", "14:30")]
        fn reformats_a_piped_date_string(
            #[case] input: &str,
            #[case] format: &str,
            #[case] expected: &str,
        ) {
            let rendered = env()
                .render_str(
                    &format!(r#"{{{{ value | date_format("{format}") }}}}"#),
                    minijinja::context! { value => input },
                )
                .expect("render succeeds");

            assert_eq!(rendered, expected);
        }

        #[test]
        fn rejects_an_unparseable_date_instead_of_panicking() {
            let error = env()
                .render_str(
                    r#"{{ "not a date" | date_format("%Y") }}"#,
                    minijinja::context!(),
                )
                .expect_err("unparseable date fails cleanly");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }

        #[test]
        fn rejects_an_invalid_format_specifier_instead_of_panicking() {
            let error = env()
                .render_str(
                    r#"{{ "2026-07-23" | date_format("%Q") }}"#,
                    minijinja::context!(),
                )
                .expect_err("invalid format specifier fails cleanly");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }
    }

    mod timestamp {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::epoch("1970-01-01", 0)]
        #[case::one_second_after_epoch("1970-01-01 00:00:01", 1)]
        fn converts_a_piped_date_to_unix_seconds(
            #[case] input: &str,
            #[case] expected: i64,
        ) {
            let rendered = env()
                .render_str(
                    "{{ value | timestamp }}",
                    minijinja::context! { value => input },
                )
                .expect("render succeeds");

            assert_eq!(rendered, expected.to_string());
        }

        #[test]
        fn rejects_an_unparseable_date_instead_of_panicking() {
            let error = env()
                .render_str(
                    r#"{{ "not a date" | timestamp }}"#,
                    minijinja::context!(),
                )
                .expect_err("unparseable date fails cleanly");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }
    }

    mod add_and_sub_days {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::crosses_into_a_leap_day(
            "2024-02-28",
            "add_days(1)",
            "2024-02-29"
        )]
        #[case::crosses_a_non_leap_month_end(
            "2023-02-28",
            "add_days(1)",
            "2023-03-01"
        )]
        #[case::sub_days_symmetric("2024-02-29", "sub_days(1)", "2024-02-28")]
        #[case::preserves_the_time_component(
            "2026-07-23 14:30",
            "add_days(1)",
            "2026-07-24 14:30:00"
        )]
        fn shifts_a_piped_date(
            #[case] input: &str,
            #[case] filter_call: &str,
            #[case] expected: &str,
        ) {
            let rendered = env()
                .render_str(
                    &format!("{{{{ value | {filter_call} }}}}"),
                    minijinja::context! { value => input },
                )
                .expect("render succeeds");

            assert_eq!(rendered, expected);
        }

        #[test]
        fn overflow_returns_an_error_instead_of_panicking() {
            let error = env()
                .render_str(
                    "{{ \"2026-07-23\" | add_days(100000000000000) }}",
                    minijinja::context!(),
                )
                .expect_err("date-range overflow fails cleanly");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }
    }

    mod add_and_sub_months {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::clamps_to_a_shorter_month(
            "2023-01-31",
            "add_months(1)",
            "2023-02-28"
        )]
        #[case::clamps_into_a_leap_february(
            "2024-01-31",
            "add_months(1)",
            "2024-02-29"
        )]
        #[case::sub_months_symmetric(
            "2023-03-31",
            "sub_months(1)",
            "2023-02-28"
        )]
        fn shifts_a_piped_date(
            #[case] input: &str,
            #[case] filter_call: &str,
            #[case] expected: &str,
        ) {
            let rendered = env()
                .render_str(
                    &format!("{{{{ value | {filter_call} }}}}"),
                    minijinja::context! { value => input },
                )
                .expect("render succeeds");

            assert_eq!(rendered, expected);
        }
    }

    mod add_and_sub_years {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::clamps_a_leap_day_into_a_non_leap_year(
            "2024-02-29",
            "add_years(1)",
            "2025-02-28"
        )]
        #[case::lands_on_a_leap_day_again(
            "2024-02-29",
            "add_years(4)",
            "2028-02-29"
        )]
        #[case::sub_years_symmetric("2028-02-29", "sub_years(4)", "2024-02-29")]
        fn shifts_a_piped_date(
            #[case] input: &str,
            #[case] filter_call: &str,
            #[case] expected: &str,
        ) {
            let rendered = env()
                .render_str(
                    &format!("{{{{ value | {filter_call} }}}}"),
                    minijinja::context! { value => input },
                )
                .expect("render succeeds");

            assert_eq!(rendered, expected);
        }
    }

    mod start_and_end_of_month {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::start_of_month("2024-02-15", "start_of_month", "2024-02-01")]
        #[case::end_of_a_leap_february(
            "2024-02-15",
            "end_of_month",
            "2024-02-29"
        )]
        #[case::end_of_a_non_leap_february(
            "2023-02-15",
            "end_of_month",
            "2023-02-28"
        )]
        #[case::end_of_month_preserves_the_time_component(
            "2024-02-15 10:00",
            "end_of_month",
            "2024-02-29 10:00:00"
        )]
        fn shifts_a_piped_date(
            #[case] input: &str,
            #[case] filter_call: &str,
            #[case] expected: &str,
        ) {
            let rendered = env()
                .render_str(
                    &format!("{{{{ value | {filter_call} }}}}"),
                    minijinja::context! { value => input },
                )
                .expect("render succeeds");

            assert_eq!(rendered, expected);
        }
    }

    mod weekday {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::monday("2026-07-20", 0)]
        #[case::tuesday("2026-07-21", 1)]
        #[case::wednesday("2026-07-22", 2)]
        #[case::thursday("2026-07-23", 3)]
        #[case::friday("2026-07-24", 4)]
        #[case::saturday("2026-07-25", 5)]
        #[case::sunday("2026-07-26", 6)]
        fn returns_zero_indexed_from_monday(
            #[case] input: &str,
            #[case] expected: u32,
        ) {
            let rendered = env()
                .render_str(
                    "{{ value | weekday }}",
                    minijinja::context! { value => input },
                )
                .expect("render succeeds");

            assert_eq!(rendered, expected.to_string());
        }
    }

    mod date_diff {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[test]
        fn returns_an_integer_day_count_when_neither_input_has_time() {
            let rendered = env()
                .render_str(
                    r#"{{ "2026-07-23" | date_diff("2026-07-30") }}"#,
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "7");
        }

        #[rstest]
        #[case::hours(
            "2026-07-23 00:00:00",
            "2026-07-24 12:00:00",
            "hours",
            "36.0"
        )]
        #[case::minutes(
            "2026-07-23 00:00:00",
            "2026-07-23 01:30:00",
            "minutes",
            "90.0"
        )]
        #[case::seconds(
            "2026-07-23 00:00:00",
            "2026-07-23 00:01:00",
            "seconds",
            "60.0"
        )]
        #[case::default_unit_is_days(
            "2026-07-23 00:00:00",
            "2026-07-25 00:00:00",
            "days",
            "2.0"
        )]
        #[case::fractional_days_from_a_sub_day_remainder(
            "2026-07-23 00:00:00",
            "2026-07-23 12:00:00",
            "days",
            "0.5"
        )]
        fn returns_sub_day_precision_when_both_inputs_have_time(
            #[case] value: &str,
            #[case] other: &str,
            #[case] unit: &str,
            #[case] expected: &str,
        ) {
            let rendered = env()
                .render_str(
                    "{{ value | date_diff(other, unit=unit) }}",
                    minijinja::context! { value, other, unit },
                )
                .expect("render succeeds");

            assert_eq!(rendered, expected);
        }

        #[test]
        fn negative_when_other_precedes_the_piped_value() {
            let rendered = env()
                .render_str(
                    r#"{{ "2026-07-24" | date_diff("2026-07-23") }}"#,
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "-1");
        }

        #[test]
        fn rejects_an_unknown_unit_instead_of_panicking() {
            let error = env()
                .render_str(
                    r#"{{ "2026-07-23" | date_diff("2026-07-24", unit="fortnights") }}"#,
                    minijinja::context!(),
                )
                .expect_err("unknown unit fails cleanly");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }
    }

    mod is_past_and_is_future {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn a_far_past_date_is_past_but_not_future() {
            let rendered = env()
                .render_str(
                    "{{ '2000-01-01' is is_past }}{{ '2000-01-01' is \
                     is_future }}",
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "truefalse");
        }

        #[test]
        fn a_far_future_date_is_future_but_not_past() {
            let rendered = env()
                .render_str(
                    "{{ '2999-01-01' is is_past }}{{ '2999-01-01' is \
                     is_future }}",
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "falsetrue");
        }

        #[test]
        fn rejects_an_unparseable_date_instead_of_panicking() {
            let error = env()
                .render_str(
                    "{{ 'not a date' is is_past }}",
                    minijinja::context!(),
                )
                .expect_err("unparseable date fails cleanly");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }
    }

    mod is_leap_year {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::divisible_by_4("2024 is is_leap_year", "true")]
        #[case::not_divisible_by_4("2023 is is_leap_year", "false")]
        #[case::divisible_by_100_not_400("1900 is is_leap_year", "false")]
        #[case::divisible_by_400("2000 is is_leap_year", "true")]
        #[case::leap_date_string("'2024-02-15' is is_leap_year", "true")]
        #[case::non_leap_date_string("'2023-02-15' is is_leap_year", "false")]
        fn checks_a_year_or_date_string(
            #[case] expr: &str,
            #[case] expected: &str,
        ) {
            let rendered = env()
                .render_str(&format!("{{{{ {expr} }}}}"), minijinja::context!())
                .expect("render succeeds");

            assert_eq!(rendered, expected);
        }

        #[test]
        fn rejects_a_non_integer_non_string_argument() {
            let error = env()
                .render_str("{{ [] is is_leap_year }}", minijinja::context!())
                .expect_err("a list argument fails cleanly");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }
    }
}
