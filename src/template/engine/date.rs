//! Register date and time helpers for templates.
//!
//! [`DateOps`] provides the `date` namespace object registered as a minijinja
//! global by [`super::TemplateEngine`]. It also registers the flat `date_*`
//! filters and `is_*` tests used in pipelines.
//!
//! Template-facing operations split into three groups:
//!
//! - Namespace generators: `date.now()`, `date.today()`, `date.tomorrow()`,
//!   `date.yesterday()`, and `date.from_timestamp(ts)` produce formatted
//!   strings from the current instant, current date, or a Unix timestamp.
//! - Filters: `date_format`, `timestamp`, `date_add`, `date_sub`, `add_days`,
//!   `sub_days`, `add_months`, `sub_months`, `add_years`, `sub_years`,
//!   `start_of_month`, `end_of_month`, `weekday`, and `date_diff` transform a
//!   piped date/time string. `date_format` is prefixed to avoid minijinja's
//!   built-in `format` filter.
//! - Tests: `is_past`, `is_future`, and `is_leap_year` inspect a piped value.
//!
//! Date/time string parsing funnels through [`ParsedDate::parse`] and
//! [`parse_date`]. A full datetime is tried first, falling back to a bare
//! `%Y-%m-%d` date at midnight. Arithmetic filters re-serialize at the input's
//! original precision via [`format_precise`].

use std::{fmt::Write as _, sync::Arc};

use chrono::{
    Datelike as _, Days, Local, Months, NaiveDate, NaiveDateTime, Utc,
};
use minijinja::{
    Environment, Error, ErrorKind,
    value::{Enumerator, Kwargs, Object, Value},
};

/// `date.now(format=...)`'s default format when the `format` kwarg is omitted.
///
/// This is an ISO-8601-style date (`YYYY-MM-DD`) and the default output shape
/// [`format_precise`] uses for a date-only input.
const DEFAULT_FORMAT: &str = "%Y-%m-%d";

/// [`format_precise`]'s output shape for an input that carried a time
/// component.
const DEFAULT_DATETIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

/// Formats [`ParsedDate::parse`] tries, in order, before falling back to a bare
/// date.
///
/// Covers space-separated (`2026-07-23 14:30[:00]`) and `T`-separated ISO 8601
/// inputs, with or without seconds/fractional seconds.
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

/// Backs the `date` namespace object. Stateless; see the module docs.
#[derive(Debug)]
pub(super) struct DateOps;

impl DateOps {
    /// Registers the `date` global plus every flat `date_*` filter and `is_*`
    /// test this module owns.
    ///
    /// Filters/tests are added first because they are zero-capture free
    /// functions and need no `self`; then `self` is consumed registering the
    /// `date` namespace object last.
    #[inline]
    pub(super) fn register(self, env: &mut Environment<'static>) {
        env.add_filter("date_format", date_format);
        env.add_filter("timestamp", timestamp);
        env.add_filter("date_add", date_add);
        env.add_filter("date_sub", date_sub);
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
                    // `write!` into a `String` propagates a formatting failure
                    // as `Err`; `.to_string()` would instead panic on the same
                    // input, since its blanket impl `.expect()`s a successful
                    // `Display::fmt`, and Chrono's `DelayedFormat` returns
                    // `Err`, not a panic of its own, for an invalid specifier
                    // such as `%Q`.
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

/// Whether the input carried only a date or a date plus time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DatePrecision {
    Date,
    DateTime,
}

impl DatePrecision {
    const fn format(self) -> &'static str {
        match self {
            Self::Date => DEFAULT_FORMAT,
            Self::DateTime => DEFAULT_DATETIME_FORMAT,
        }
    }
}

/// A successfully parsed date/time string.
///
/// Stores the [`NaiveDateTime`] plus the original input precision. Every
/// arithmetic filter re-serializes at the same precision via
/// [`format_precise`].
struct ParsedDate {
    datetime: NaiveDateTime,
    precision: DatePrecision,
}

impl ParsedDate {
    /// Parses `s` as a date/time string. Tries each of [`DATETIME_FORMATS`] in
    /// turn; on no match, falls back to a bare `%Y-%m-%d` date at midnight.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::InvalidOperation`] if `s` matches neither a
    ///   [`DATETIME_FORMATS`] entry nor the bare `%Y-%m-%d` fallback.
    fn parse(s: &str) -> Result<Self, Error> {
        if let Some(datetime) = try_parse_datetime(s) {
            return Ok(Self {
                datetime,
                precision: DatePrecision::DateTime,
            });
        }
        NaiveDate::parse_from_str(s, DEFAULT_FORMAT)
            .ok()
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .map(|datetime| Self {
                datetime,
                precision: DatePrecision::Date,
            })
            .ok_or_else(|| invalid_date_error(s))
    }
}

/// Date/time unit parsed from a `unit="..."` kwarg.
///
/// Used by [`date_add`], [`date_sub`], and [`date_diff`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DateTimeUnit {
    Years,
    Months,
    Days,
    Hours,
    Minutes,
    Seconds,
}

impl DateTimeUnit {
    /// Parses `unit`'s `unit="..."` kwarg value, accepting both plural
    /// (`"days"`) and singular (`"day"`) forms. `None` for unrecognized unit
    /// names.
    fn parse(unit: &str) -> Option<Self> {
        match unit {
            "years" | "year" => Some(Self::Years),
            "months" | "month" => Some(Self::Months),
            "days" | "day" => Some(Self::Days),
            "hours" | "hour" => Some(Self::Hours),
            "minutes" | "minute" => Some(Self::Minutes),
            "seconds" | "second" => Some(Self::Seconds),
            _ => None,
        }
    }

    /// This unit's length in whole seconds for [`date_diff`].
    ///
    /// `None` for [`Self::Years`] and [`Self::Months`], which vary in length
    /// (28-31 days, 365-366 days) and are not fixed numbers of seconds.
    const fn diff_seconds(self) -> Option<i64> {
        match self {
            Self::Days => Some(86_400),
            Self::Hours => Some(3_600),
            Self::Minutes => Some(60),
            Self::Seconds => Some(1),
            Self::Years | Self::Months => None,
        }
    }
}

/// Extracts the shared `format="..."` kwarg every `date.*` namespace method
/// takes, defaulting to [`DEFAULT_FORMAT`], and rejects any other kwarg via
/// [`Kwargs::assert_all_used`].
///
/// This is the one place all five
/// `now`/`today`/`tomorrow`/`yesterday`/`from_timestamp` closures decide how
/// their optional `format=` argument is read.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `format` is present but is not a
///   string.
/// - [`ErrorKind::TooManyArguments`] if `kwargs` carries any key besides
///   `format`.
fn format_kwarg(kwargs: &Kwargs) -> Result<&str, Error> {
    let format =
        kwargs.get::<Option<&str>>("format")?.unwrap_or(DEFAULT_FORMAT);
    kwargs.assert_all_used()?;
    Ok(format)
}

/// Extracts the shared `unit="..."` kwarg [`date_add`], [`date_sub`], and
/// [`date_diff`] all take, defaulting to `"days"`, and rejects any other kwarg
/// via [`Kwargs::assert_all_used`], mirroring [`format_kwarg`] for the `date.*`
/// namespace methods' `format=` kwarg.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `unit` is not one of
///   [`DateTimeUnit::parse`]'s six accepted units.
/// - [`ErrorKind::TooManyArguments`] if `kwargs` carries any key besides
///   `unit`.
fn unit_kwarg(kwargs: &Kwargs) -> Result<DateTimeUnit, Error> {
    let unit_str = kwargs.get::<Option<&str>>("unit")?.unwrap_or("days");
    kwargs.assert_all_used()?;
    DateTimeUnit::parse(unit_str).ok_or_else(|| unknown_unit_error(unit_str))
}

/// Formats `formattable`, anything chrono's `.format(fmt)` produces
/// (`DelayedFormat` from a `NaiveDate`, `NaiveDateTime`, or `DateTime<_>`).
///
/// Writes through [`std::fmt::Write`] rather than `.to_string()`: the latter's
/// blanket impl `.expect()`s a successful `Display::fmt`, but `DelayedFormat`
/// returns `Err` for an invalid strftime specifier such as `%Q`. Writing
/// directly turns that into a normal [`minijinja::Error`] instead of a panic.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `format` is not a strftime specifier
///   `formattable` can render, for example `%Q`.
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

/// Re-serializes `dt` at the given `precision`.
///
/// Uses [`DEFAULT_DATETIME_FORMAT`] when the original input carried a time
/// component, [`DEFAULT_FORMAT`] otherwise. Every arithmetic filter uses this
/// for its output, so a date-only string never grows a fabricated `00:00:00`,
/// and a datetime string never silently loses its time-of-day.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if formatting unexpectedly fails. This is
///   unreachable in practice because the format is always [`DEFAULT_FORMAT`] or
///   [`DEFAULT_DATETIME_FORMAT`], both valid strftime specifiers.
fn format_precise(
    dt: NaiveDateTime,
    precision: DatePrecision,
) -> Result<String, Error> {
    format_with(dt.format(precision.format()), precision.format())
}

/// Tries each of [`DATETIME_FORMATS`] in turn; `None` if none match.
fn try_parse_datetime(s: &str) -> Option<NaiveDateTime> {
    DATETIME_FORMATS
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(s, format).ok())
}

/// The shared date/time string parser every filter and test besides
/// [`date_diff`] uses. See [`ParsedDate::parse`] for the accepted formats and
/// fallback behavior.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `s` is not a parseable date/time
///   string.
fn parse_date(s: &str) -> Result<NaiveDateTime, Error> {
    ParsedDate::parse(s).map(|parsed| parsed.datetime)
}

/// `{{ value | date_format(format_string) }}` re-formats a piped date/time
/// string with an arbitrary strftime specifier.
///
/// Prefixed as `date_format`, not just `format`, to avoid colliding with
/// minijinja's built-in printf-style `format` filter.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not a parseable date/time
///   string; see [`parse_date`].
/// - [`ErrorKind::InvalidOperation`] if `format` is not a valid strftime
///   specifier; see [`format_with`].
fn date_format(value: &str, format: &str) -> Result<String, Error> {
    let datetime = parse_date(value)?;
    format_with(datetime.format(format), format)
}

/// `{{ value | timestamp }}` converts a piped date/time string to Unix seconds,
/// treating a naive (timezone-less) input as UTC.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not a parseable date/time
///   string; see [`parse_date`].
fn timestamp(value: &str) -> Result<i64, Error> {
    Ok(parse_date(value)?.and_utc().timestamp())
}

/// Parses `value` as a date/time string, transforms `datetime` via `op`, and
/// re-serializes the result at `value`'s original precision.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not parseable.
/// - [`ErrorKind::InvalidOperation`] if `op` returns `None`, indicating
///   arithmetic overflow.
fn shift_date(
    value: &str,
    op: impl FnOnce(NaiveDateTime) -> Option<NaiveDateTime>,
) -> Result<String, Error> {
    let parsed = ParsedDate::parse(value)?;
    let shifted = op(parsed.datetime).ok_or_else(date_out_of_range_error)?;
    format_precise(shifted, parsed.precision)
}

/// `{{ value | date_add(n, unit="days") }}` adds `n` `unit`s to a piped
/// date/time string.
///
/// `unit` defaults to `"days"` and accepts `"years"`, `"months"`, `"days"`,
/// `"hours"`, `"minutes"`, and `"seconds"`.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not a parseable date/time
///   string, `unit` is not an accepted unit name, or the shift overflows
///   chrono's representable range.
#[expect(
    clippy::needless_pass_by_value,
    reason = "minijinja's Function trait extracts a filter's trailing Kwargs \
              argument by value; only `&self` methods on it are needed here"
)]
fn date_add(value: &str, n: i64, kwargs: Kwargs) -> Result<String, Error> {
    date_shift_unit(value, n, unit_kwarg(&kwargs)?)
}

/// `{{ value | date_sub(n, unit="days") }}` subtracts `n` `unit`s from a piped
/// date/time string.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not a parseable date/time
///   string, `unit` is not an accepted unit name, `n` is [`i64::MIN`], or the
///   shift overflows chrono's representable range.
#[expect(
    clippy::needless_pass_by_value,
    reason = "minijinja's Function trait extracts a filter's trailing Kwargs \
              argument by value; only `&self` methods on it are needed here"
)]
fn date_sub(value: &str, n: i64, kwargs: Kwargs) -> Result<String, Error> {
    date_shift_unit(
        value,
        n.checked_neg().ok_or_else(date_out_of_range_error)?,
        unit_kwarg(&kwargs)?,
    )
}

fn date_shift_unit(
    value: &str,
    n: i64,
    unit: DateTimeUnit,
) -> Result<String, Error> {
    shift_date(value, |dt| match unit {
        DateTimeUnit::Years => {
            let months = n.checked_mul(12)?;
            let months_u32 = u32::try_from(months.abs()).ok()?;
            if months >= 0 {
                dt.checked_add_months(Months::new(months_u32))
            } else {
                dt.checked_sub_months(Months::new(months_u32))
            }
        }
        DateTimeUnit::Months => {
            let months_u32 = u32::try_from(n.abs()).ok()?;
            if n >= 0 {
                dt.checked_add_months(Months::new(months_u32))
            } else {
                dt.checked_sub_months(Months::new(months_u32))
            }
        }
        DateTimeUnit::Days => {
            let days_u64 = u64::try_from(n.abs()).ok()?;
            if n >= 0 {
                dt.checked_add_days(Days::new(days_u64))
            } else {
                dt.checked_sub_days(Days::new(days_u64))
            }
        }
        DateTimeUnit::Hours => {
            dt.checked_add_signed(chrono::Duration::hours(n))
        }
        DateTimeUnit::Minutes => {
            dt.checked_add_signed(chrono::Duration::minutes(n))
        }
        DateTimeUnit::Seconds => {
            dt.checked_add_signed(chrono::Duration::seconds(n))
        }
    })
}

/// `{{ value | add_days(n) }}` is a convenience shortcut for
/// `{{ value | date_add(n, unit="days") }}`.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not parseable or arithmetic
///   overflows chrono's representable range.
fn add_days(value: &str, n: u64) -> Result<String, Error> {
    let n_i64 = i64::try_from(n).map_err(|_| date_out_of_range_error())?;
    date_shift_unit(value, n_i64, DateTimeUnit::Days)
}

/// `{{ value | sub_days(n) }}` is a convenience shortcut for
/// `{{ value | date_sub(n, unit="days") }}`.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not parseable or arithmetic
///   overflows chrono's representable range.
fn sub_days(value: &str, n: u64) -> Result<String, Error> {
    let n_i64 = i64::try_from(n).map_err(|_| date_out_of_range_error())?;
    let n_i64 = n_i64.checked_neg().ok_or_else(date_out_of_range_error)?;
    date_shift_unit(value, n_i64, DateTimeUnit::Days)
}

/// `{{ value | add_months(n) }}` is a convenience shortcut for
/// `{{ value | date_add(n, unit="months") }}`.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not parseable or arithmetic
///   overflows chrono's representable range.
fn add_months(value: &str, n: u32) -> Result<String, Error> {
    date_shift_unit(value, i64::from(n), DateTimeUnit::Months)
}

/// `{{ value | sub_months(n) }}` is a convenience shortcut for
/// `{{ value | date_sub(n, unit="months") }}`.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not parseable or arithmetic
///   overflows chrono's representable range.
fn sub_months(value: &str, n: u32) -> Result<String, Error> {
    let n_i64 =
        i64::from(n).checked_neg().ok_or_else(date_out_of_range_error)?;
    date_shift_unit(value, n_i64, DateTimeUnit::Months)
}

/// `{{ value | add_years(n) }}` is a convenience shortcut for
/// `{{ value | date_add(n, unit="years") }}`.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not parseable or arithmetic
///   overflows chrono's representable range.
fn add_years(value: &str, n: u32) -> Result<String, Error> {
    date_shift_unit(value, i64::from(n), DateTimeUnit::Years)
}

/// `{{ value | sub_years(n) }}` is a convenience shortcut for
/// `{{ value | date_sub(n, unit="years") }}`.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not parseable or arithmetic
///   overflows chrono's representable range.
fn sub_years(value: &str, n: u32) -> Result<String, Error> {
    let n_i64 =
        i64::from(n).checked_neg().ok_or_else(date_out_of_range_error)?;
    date_shift_unit(value, n_i64, DateTimeUnit::Years)
}

/// `{{ value | start_of_month }}` returns the first day of the input month.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not a parseable date/time
///   string; see [`ParsedDate::parse`].
/// - [`ErrorKind::InvalidOperation`] if the first day of the month is outside
///   chrono's representable range; see [`date_out_of_range_error`].
fn start_of_month(value: &str) -> Result<String, Error> {
    shift_date(value, |dt| dt.with_day(1))
}

/// `{{ value | end_of_month }}` returns the last day of the input month.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not a parseable date/time
///   string; see [`ParsedDate::parse`].
/// - [`ErrorKind::InvalidOperation`] if the last day of the month is outside
///   chrono's representable range; see [`date_out_of_range_error`].
fn end_of_month(value: &str) -> Result<String, Error> {
    shift_date(value, |dt| dt.with_day(u32::from(dt.num_days_in_month())))
}

/// `{{ value | weekday }}` returns `0` for Monday through `6` for Sunday.
///
/// Chrono's own [`Weekday::number_from_sunday`] is Sunday-first, so this filter
/// remaps to Monday-first order.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not a parseable date/time
///   string; see [`parse_date`].
///
/// [`Weekday::number_from_sunday`]: chrono::Weekday::number_from_sunday
fn weekday(value: &str) -> Result<u32, Error> {
    Ok(parse_date(value)?.weekday().num_days_from_monday())
}

/// Whole calendar years from `from` to `to`, signed.
///
/// Delegates to chrono's [`NaiveDate::years_since`], which is day-of-year
/// aware: a year is not "up" until `to`'s month/day reaches `from`'s. This
/// wrapper just accepts either ordering.
fn signed_years_since(from: NaiveDate, to: NaiveDate) -> i64 {
    let (earlier, later, sign) = if to >= from {
        (from, to, 1)
    } else {
        (to, from, -1)
    };
    #[expect(
        clippy::expect_used,
        reason = "earlier/later are ordered by construction just above, so \
                  years_since's None case (base > self) is unreachable here"
    )]
    let years = later.years_since(earlier).expect(
        "later >= earlier by construction, so years_since can't return None",
    );
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "sign is always ±1 and years is bounded by NaiveDate's \
                  representable range (~±262,000), so this multiply can't \
                  overflow i64"
    )]
    let result = sign * i64::from(years);
    result
}

/// Whole calendar months from `from` to `to`, signed. See
/// [`signed_years_since`].
///
/// Chrono has no `months_since` equivalent, so this mirrors
/// [`NaiveDate::years_since`]'s algorithm at month granularity: total calendar
/// months between the dates, decremented by one when the day-of-month has not
/// yet been reached.
fn signed_months_since(from: NaiveDate, to: NaiveDate) -> i64 {
    let (earlier, later, sign) = if to >= from {
        (from, to, 1)
    } else {
        (to, from, -1)
    };
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "year/month/day are all bounded by NaiveDate's representable \
                  range (~±262,000 years), so the year subtraction, ×12 month \
                  conversion, day comparison, and sign multiply can't \
                  overflow i64"
    )]
    let result = sign
        * (i64::from(later.year() - earlier.year()) * 12
            + i64::from(later.month())
            - i64::from(earlier.month())
            - i64::from(later.day() < earlier.day()));
    result
}

/// `{{ value | date_diff(other, unit="days") }}` returns the signed difference
/// from the piped value to `other`, positive when `other` is later.
///
/// The `unit` kwarg defaults to `"days"` and accepts `"years"`, `"months"`,
/// `"hours"`, `"minutes"`, or `"seconds"`. `"years"`/`"months"` are calendar
/// counts: whole units elapsed, day-of-month aware (see
/// [`signed_years_since`]/[`signed_months_since`]), always an `i64` regardless
/// of input precision. The remaining units are fixed-duration: `f64` when both
/// inputs carry a time component, otherwise an `i64` whole-unit count.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` or `other` is not a parseable
///   date/time string (see [`ParsedDate::parse`]) or `unit` is not one of
///   [`DateTimeUnit::parse`]'s six accepted names (see [`unit_kwarg`]).
/// - [`ErrorKind::TooManyArguments`] if `kwargs` carries any key besides
///   `unit`.
#[expect(
    clippy::needless_pass_by_value,
    reason = "minijinja's Function trait extracts a filter's trailing Kwargs \
              argument by value; only `&self` methods on it are needed here"
)]
fn date_diff(value: &str, other: &str, kwargs: Kwargs) -> Result<Value, Error> {
    let unit = unit_kwarg(&kwargs)?;
    let from = ParsedDate::parse(value)?;
    let to = ParsedDate::parse(other)?;

    match unit {
        DateTimeUnit::Years => Ok(Value::from(signed_years_since(
            from.datetime.date(),
            to.datetime.date(),
        ))),
        DateTimeUnit::Months => Ok(Value::from(signed_months_since(
            from.datetime.date(),
            to.datetime.date(),
        ))),
        DateTimeUnit::Days
        | DateTimeUnit::Hours
        | DateTimeUnit::Minutes
        | DateTimeUnit::Seconds => {
            #[expect(
                clippy::expect_used,
                reason = "this arm is reached only for Days/Hours/Minutes/ \
                          Seconds, all of which DateTimeUnit::diff_seconds \
                          maps to Some; None is only ever Years/Months, \
                          handled by the arms above"
            )]
            let unit_seconds = unit.diff_seconds().expect(
                "Days/Hours/Minutes/Seconds always have a fixed length",
            );
            let delta = to.datetime.signed_duration_since(from.datetime);

            if from.precision == DatePrecision::DateTime
                && to.precision == DatePrecision::DateTime
            {
                #[expect(
                    clippy::as_conversions,
                    clippy::cast_precision_loss,
                    reason = "TimeDelta::num_seconds() is bounded by chrono's \
                              NaiveDateTime range (~262,000 years, well under \
                              2^52 seconds), and unit_seconds is at most \
                              86,400, so neither cast loses precision in \
                              practice"
                )]
                let result = (delta.num_seconds() as f64
                    + f64::from(delta.subsec_nanos()) / 1e9)
                    / unit_seconds as f64;
                Ok(Value::from(result))
            } else {
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "unit_seconds is 86_400, 3_600, 60, or 1 \
                              (DateTimeUnit::diff_seconds), never zero, so \
                              this division never panics"
                )]
                let result = delta.num_seconds() / unit_seconds;
                Ok(Value::from(result))
            }
        }
    }
}

/// `{% if value is is_past %}` returns `true` when the piped date/time string
/// is before now.
///
/// A naive input is treated as UTC, matching [`timestamp`].
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not a parseable date/time
///   string; see [`parse_date`].
fn is_past(value: &str) -> Result<bool, Error> {
    Ok(parse_date(value)?.and_utc() < Utc::now())
}

/// `{% if value is is_future %}` mirrors [`is_past`] for future instants.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not a parseable date/time
///   string; see [`parse_date`].
fn is_future(value: &str) -> Result<bool, Error> {
    Ok(parse_date(value)?.and_utc() > Utc::now())
}

/// `{% if value is is_leap_year %}` accepts either an integer year (`2024 is
/// is_leap_year`) or a date/time string checked through [`parse_date`].
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] (via [`leap_year_input_error`]) if `value`
///   is neither an integer year representable as [`i32`] nor a parseable
///   date/time string; see [`parse_date`].
/// - [`ErrorKind::InvalidOperation`] if the year is outside [`NaiveDate`]'s
///   representable range.
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

/// Builds the error for a date/time string matching none of
/// [`ParsedDate::parse`]'s accepted formats.
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

/// Builds the error for date arithmetic that overflows chrono's representable
/// date range.
///
/// This is reached only at the extremes, such as multi-millennia offsets, but
/// every `checked_*` chrono call this module makes can return `None`, and this
/// module never `.unwrap()`s one.
fn date_out_of_range_error() -> Error {
    Error::new(
        ErrorKind::InvalidOperation,
        "date arithmetic overflowed the supported range",
    )
}

/// Builds the error for a `unit="..."` kwarg naming anything outside
/// [`DateTimeUnit::parse`]'s six accepted unit names.
///
/// Shared by [`date_add`], [`date_sub`], and [`date_diff`] via [`unit_kwarg`].
fn unknown_unit_error(unit: &str) -> Error {
    Error::new(
        ErrorKind::InvalidOperation,
        format!(
            "unknown unit {unit:?} (expected \"years\", \"months\", \"days\", \
             \"hours\", \"minutes\", or \"seconds\")"
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

        /// Asserts the rendered shape, not the current date.
        ///
        /// The clock makes the literal value nondeterministic, but the default
        /// format still has a stable `YYYY-MM-DD` shape.
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
        /// impl panics on that `Err`. Writing through `fmt::Write`
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

    /// `today`/`tomorrow`/`yesterday` share the same nondeterministic clock,
    /// but their relative dates are deterministic within one render window.
    ///
    /// Whatever `today()` returns, `tomorrow()` and `yesterday()` should be one
    /// calendar day ahead and behind it, except for a midnight rollover between
    /// calls.
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

        #[rstest]
        #[case::exact_anniversary("2020-06-15", "2025-06-15", "5")]
        #[case::before_anniversary_rounds_down("2020-06-15", "2025-06-14", "4")]
        #[case::after_anniversary_rounds_up_to_the_next_whole_year(
            "2020-06-15",
            "2025-06-16",
            "5"
        )]
        #[case::negative_when_other_precedes_the_piped_value(
            "2025-06-15",
            "2020-06-15",
            "-5"
        )]
        fn computes_whole_calendar_years_between_dates(
            #[case] value: &str,
            #[case] other: &str,
            #[case] expected: &str,
        ) {
            let rendered = env()
                .render_str(
                    r#"{{ value | date_diff(other, unit="years") }}"#,
                    minijinja::context! { value, other },
                )
                .expect("render succeeds");

            assert_eq!(rendered, expected);
        }

        #[rstest]
        #[case::exact_month_boundary("2026-01-15", "2026-04-15", "3")]
        #[case::before_day_of_month_rounds_down(
            "2026-01-15",
            "2026-04-14",
            "2"
        )]
        #[case::after_day_of_month_rounds_up_to_the_next_whole_month(
            "2026-01-15",
            "2026-04-16",
            "3"
        )]
        #[case::spans_a_year_boundary("2025-11-15", "2026-02-15", "3")]
        #[case::negative_when_other_precedes_the_piped_value(
            "2026-04-15",
            "2026-01-15",
            "-3"
        )]
        fn computes_whole_calendar_months_between_dates(
            #[case] value: &str,
            #[case] other: &str,
            #[case] expected: &str,
        ) {
            let rendered = env()
                .render_str(
                    r#"{{ value | date_diff(other, unit="months") }}"#,
                    minijinja::context! { value, other },
                )
                .expect("render succeeds");

            assert_eq!(rendered, expected);
        }

        #[test]
        fn years_and_months_ignore_the_time_of_day_component() {
            let rendered = env()
                .render_str(
                    r#"{{ "2020-06-15 23:59:59" | date_diff("2025-06-15 00:00:00", unit="years") }}"#,
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "5");
        }
    }

    mod date_add_and_date_sub {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn date_add_defaults_to_days() {
            let rendered = env()
                .render_str(
                    r"{{ '2026-07-26' | date_add(5) }}",
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "2026-07-31");
        }

        #[test]
        fn date_add_accepts_units_and_singular_plural_forms() {
            let rendered = env()
                .render_str(
                    r"{{ '2026-07-26' | date_add(1, unit='month') }}-{{ '2026-07-26' | date_add(2, unit='years') }}-{{ '2026-07-26 12:00:00' | date_add(3, unit='hours') }}",
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "2026-08-26-2028-07-26-2026-07-26 15:00:00");
        }

        #[test]
        fn date_sub_subtracts_units() {
            let rendered = env()
                .render_str(
                    r"{{ '2026-07-26' | date_sub(10, unit='days') }}-{{ '2026-07-26' | date_sub(1, unit='year') }}",
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "2026-07-16-2025-07-26");
        }

        #[test]
        fn date_add_rejects_unknown_unit() {
            let error = env()
                .render_str(
                    r"{{ '2026-07-26' | date_add(1, unit='fortnight') }}",
                    minijinja::context!(),
                )
                .expect_err("unknown unit fails");

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

    mod diff_precision {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn uses_float_precision_for_datetime_values() {
            let rendered = env()
                .render_str(
                    r#"{{ "2026-01-01T00:00:00" | date_diff("2026-01-01T00:00:01.500", unit="seconds") }}"#,
                    minijinja::context!(),
                )
                .expect("render succeeds");

            let value: f64 = rendered.parse().expect("numeric result");
            assert!(
                (value - 1.5).abs() < 0.01,
                "datetime diff must use float precision, got: {value}"
            );
        }

        #[test]
        fn uses_integer_division_for_date_only_values() {
            let rendered = env()
                .render_str(
                    r#"{{ "2026-01-01" | date_diff("2026-01-02", unit="days") }}"#,
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "1");
        }
    }

    mod comparison {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn is_past_returns_true_for_past_date() {
            let rendered = env()
                .render_str(
                    "{{ '2000-01-01' is is_past }}",
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "true");
        }

        #[test]
        fn is_future_returns_true_for_far_future_date() {
            let rendered = env()
                .render_str(
                    "{{ '2099-12-31' is is_future }}",
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "true");
        }

        #[test]
        fn is_past_returns_false_for_far_future_date() {
            let rendered = env()
                .render_str(
                    "{{ '2099-12-31' is is_past }}",
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "false");
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
