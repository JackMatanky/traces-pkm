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
//! [`parse_date`]. A full datetime is tried first, falling back to a bare ISO
//! date (`YYYY-MM-DD` or reduced-precision `YYYY-MM`) at midnight. Arithmetic
//! filters re-serialize at the input's original precision via
//! [`format_precise`].
//!
//! # Clock doctrine
//!
//! A naive datetime input means the reader's local wall clock: it parses to a
//! UTC instant through the core date module's DST resolver, while a date-only
//! input stays a zone-free civil date. Human-facing filters render the local
//! wall clock; `timestamp`, `is_past`, `is_future`, and fixed-unit
//! [`date_diff`] measure the stored UTC instant. `date.now()`,
//! `date.today()`, `date.tomorrow()`, `date.yesterday()`, and
//! `date.from_timestamp()` read or render the local clock for display.

use std::{fmt::Write as _, sync::Arc};

use chrono::{
    DateTime, Datelike as _, Days, Local, Months, NaiveDate, NaiveDateTime, Utc,
};
use minijinja::{
    Environment, Error, ErrorKind,
    value::{Enumerator, Kwargs, Object, Value},
};
use num_traits::ToPrimitive as _;

use super::error::{TemplateEngineResult, invalid_operation};
use crate::{
    DEFAULT_DATE_FORMAT, DEFAULT_DATETIME_FORMAT, DateTimeValue, DateValue,
    DurationUnit,
};

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
                |kwargs: Kwargs| -> TemplateEngineResult<String> {
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
                |kwargs: Kwargs| -> TemplateEngineResult<String> {
                    let format = format_kwarg(&kwargs)?;
                    format_with(
                        Local::now().date_naive().format(format),
                        format,
                    )
                },
            )),
            "tomorrow" => Some(Value::from_function(
                |kwargs: Kwargs| -> TemplateEngineResult<String> {
                    let format = format_kwarg(&kwargs)?;
                    let date = Local::now()
                        .date_naive()
                        .succ_opt()
                        .ok_or_else(date_out_of_range_error)?;
                    format_with(date.format(format), format)
                },
            )),
            "yesterday" => Some(Value::from_function(
                |kwargs: Kwargs| -> TemplateEngineResult<String> {
                    let format = format_kwarg(&kwargs)?;
                    let date = Local::now()
                        .date_naive()
                        .pred_opt()
                        .ok_or_else(date_out_of_range_error)?;
                    format_with(date.format(format), format)
                },
            )),
            "from_timestamp" => Some(Value::from_function(
                |unix_ts: i64,
                 kwargs: Kwargs|
                 -> TemplateEngineResult<String> {
                    let format = format_kwarg(&kwargs)?;
                    let utc = chrono::DateTime::from_timestamp(unix_ts, 0)
                        .ok_or_else(|| {
                            Error::new(
                                ErrorKind::InvalidOperation,
                                format!("timestamp {unix_ts} is out of range"),
                            )
                        })?;
                    // Display is human-facing: the local wall clock of the
                    // instant, via the crate's canonical conversion
                    // (`.naive_utc()` would show the UTC clock instead).
                    let local = DateTimeValue::from(utc).wall_or_utc();
                    format_with(local.format(format), format)
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
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum DatePrecision {
    Date,
    DateTime,
}

impl DatePrecision {
    const fn format(self) -> &'static str {
        match self {
            Self::Date => DEFAULT_DATE_FORMAT,
            Self::DateTime => DEFAULT_DATETIME_FORMAT,
        }
    }
}

/// A successfully parsed date/time string.
///
/// `wall` is the human-facing civil datetime: the local wall clock of
/// `instant` for a datetime input, or the zone-free civil date at midnight
/// for a date-only input. `instant` is the stored UTC instant used for
/// storage, comparison, and fixed-duration arithmetic. Every arithmetic
/// filter re-serializes `wall` at the original precision via
/// [`format_precise`].
struct ParsedDate {
    wall: NaiveDateTime,
    instant: DateTime<Utc>,
    precision: DatePrecision,
}

impl ParsedDate {
    /// Parses `s` as a date/time string via [`DateTimeValue::parse_iso`],
    /// falling back to [`DateValue::parse_iso`] at midnight.
    ///
    /// A naive datetime resolves through the core module's local-zone DST
    /// resolver; an explicit-offset input converts directly to its instant; a
    /// date-only input stays civil, its instant being local-zone midnight.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::InvalidOperation`] if `s` matches neither parser. The
    ///   underlying [`DateError`] is attached as the error's source so render
    ///   diagnostics keep the full parse-failure chain.
    /// - [`ErrorKind::InvalidOperation`] if the local wall clock of a parsed
    ///   instant overflows [`NaiveDateTime`]'s range.
    fn parse(s: &str) -> TemplateEngineResult<Self> {
        match DateTimeValue::parse_iso(s) {
            Ok(value) => {
                let instant = value.into_inner();
                let wall = DateTimeValue::from(instant)
                    .local_wall()
                    .ok_or_else(date_out_of_range_error)?;
                Ok(Self {
                    wall,
                    instant,
                    precision: DatePrecision::DateTime,
                })
            }
            // `s` didn't parse as a date-time; try it as a bare date. If that
            // also fails, `datetime_source` (from the first, more specific
            // attempt) is the more useful diagnostic to surface.
            Err(datetime_source) => match DateValue::parse_iso(s) {
                Ok(value) => {
                    let Some(wall) = value.into_inner().and_hms_opt(0, 0, 0)
                    else {
                        return Err(date_out_of_range_error());
                    };
                    let instant = DateTimeValue::from(value).into_inner();
                    Ok(Self {
                        wall,
                        instant,
                        precision: DatePrecision::Date,
                    })
                }
                Err(_date_source) => Err(invalid_operation(
                    format!("invalid date {s:?}"),
                    datetime_source,
                )),
            },
        }
    }
}

/// Extracts the shared `format="..."` kwarg every `date.*` namespace method
/// takes, defaulting to [`DEFAULT_DATE_FORMAT`], and rejects any other kwarg
/// via [`Kwargs::assert_all_used`].
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
fn format_kwarg(kwargs: &Kwargs) -> TemplateEngineResult<&str> {
    let format =
        kwargs.get::<Option<&str>>("format")?.unwrap_or(DEFAULT_DATE_FORMAT);
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
///   [`DurationUnit::parse`]'s accepted unit names or abbreviations.
/// - [`ErrorKind::TooManyArguments`] if `kwargs` carries any key besides
///   `unit`.
fn unit_kwarg(kwargs: &Kwargs) -> TemplateEngineResult<DurationUnit> {
    let unit_str = kwargs.get::<Option<&str>>("unit")?.unwrap_or("days");
    kwargs.assert_all_used()?;
    DurationUnit::parse(unit_str).ok_or_else(|| unknown_unit_error(unit_str))
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
) -> TemplateEngineResult<String> {
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
/// component, [`DEFAULT_DATE_FORMAT`] otherwise. Every arithmetic filter uses
/// this for its output, so a date-only string never grows a fabricated
/// `00:00:00`, and a datetime string never silently loses its time-of-day.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if formatting unexpectedly fails. This is
///   unreachable in practice because the format is always
///   [`DEFAULT_DATE_FORMAT`] or [`DEFAULT_DATETIME_FORMAT`], both valid
///   strftime specifiers.
fn format_precise(
    dt: NaiveDateTime,
    precision: DatePrecision,
) -> TemplateEngineResult<String> {
    format_with(dt.format(precision.format()), precision.format())
}

/// The shared date/time string parser for filters that work on the human wall
/// clock (`date_format`, `weekday`, `is_leap_year`); instant-facing filters
/// parse via [`ParsedDate`] directly. See [`ParsedDate::parse`] for the
/// accepted formats.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `s` is not a parseable date/time
///   string.
fn parse_date(s: &str) -> Result<NaiveDateTime, Error> {
    ParsedDate::parse(s).map(|parsed| parsed.wall)
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
fn date_format(value: &str, format: &str) -> TemplateEngineResult<String> {
    let datetime = parse_date(value)?;
    format_with(datetime.format(format), format)
}

/// `{{ value | timestamp }}` converts a piped date/time string to Unix seconds.
///
/// A naive input means the reader's local zone, so it compares correctly
/// against file timestamps; an explicit-offset input converts directly; a
/// date-only input resolves at local-zone midnight.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not a parseable date/time
///   string; see [`ParsedDate::parse`].
fn timestamp(value: &str) -> TemplateEngineResult<i64> {
    Ok(ParsedDate::parse(value)?.instant.timestamp())
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
) -> TemplateEngineResult<String> {
    let parsed = ParsedDate::parse(value)?;
    let shifted = op(parsed.wall).ok_or_else(date_out_of_range_error)?;
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
fn date_add(
    value: &str,
    n: i64,
    kwargs: Kwargs,
) -> TemplateEngineResult<String> {
    let unit = unit_kwarg(&kwargs)?;
    date_shift_unit(value, n, unit)
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
fn date_sub(
    value: &str,
    n: i64,
    kwargs: Kwargs,
) -> TemplateEngineResult<String> {
    let unit = unit_kwarg(&kwargs)?;
    date_shift_unit(
        value,
        n.checked_neg().ok_or_else(date_out_of_range_error)?,
        unit,
    )
}
fn date_shift_unit(
    value: &str,
    n: i64,
    unit: DurationUnit,
) -> TemplateEngineResult<String> {
    let parsed = ParsedDate::parse(value)?;
    // Day, month, and year units shift the civil wall clock (calendar
    // application); sub-day units shift the stored instant exactly, which a DST
    // transition then exposes in the local wall clock. A date-only input stays
    // civil for every unit: a zone-free date has no instant to shift.
    let wall = match (parsed.precision, unit) {
        (DatePrecision::Date, _)
        | (
            DatePrecision::DateTime,
            DurationUnit::Year | DurationUnit::Month | DurationUnit::Day,
        ) => shift_wall(parsed.wall, n, unit)
            .ok_or_else(date_out_of_range_error)?,
        (DatePrecision::DateTime, _) => {
            let delta = match unit {
                DurationUnit::Millisecond => {
                    chrono::Duration::try_milliseconds(n)
                }
                u => chrono::Duration::try_seconds(
                    u.seconds_i64()
                        .ok_or_else(date_out_of_range_error)?
                        .checked_mul(n)
                        .ok_or_else(date_out_of_range_error)?,
                ),
            }
            .ok_or_else(date_out_of_range_error)?;
            let instant = parsed
                .instant
                .checked_add_signed(delta)
                .ok_or_else(date_out_of_range_error)?;
            DateTimeValue::from(instant)
                .local_wall()
                .ok_or_else(date_out_of_range_error)?
        }
    };
    format_precise(wall, parsed.precision)
}

/// Shifts a civil wall-clock datetime by `n` `unit`s.
///
/// Returns `None` when the unit has no whole-second value or the arithmetic
/// overflows chrono's representable range.
fn shift_wall(
    wall: NaiveDateTime,
    n: i64,
    unit: DurationUnit,
) -> Option<NaiveDateTime> {
    match unit {
        DurationUnit::Year => {
            let months = n.checked_mul(12)?;
            let months_u32 = u32::try_from(months.abs()).ok()?;
            if months >= 0 {
                wall.checked_add_months(Months::new(months_u32))
            } else {
                wall.checked_sub_months(Months::new(months_u32))
            }
        }
        DurationUnit::Month => {
            let months_u32 = u32::try_from(n.abs()).ok()?;
            if n >= 0 {
                wall.checked_add_months(Months::new(months_u32))
            } else {
                wall.checked_sub_months(Months::new(months_u32))
            }
        }
        DurationUnit::Day => {
            let days_u64 = u64::try_from(n.abs()).ok()?;
            if n >= 0 {
                wall.checked_add_days(Days::new(days_u64))
            } else {
                wall.checked_sub_days(Days::new(days_u64))
            }
        }
        DurationUnit::Millisecond => {
            wall.checked_add_signed(chrono::Duration::try_milliseconds(n)?)
        }
        u => wall.checked_add_signed(chrono::Duration::try_seconds(
            u.seconds_i64()?.checked_mul(n)?,
        )?),
    }
}

/// `{{ value | add_days(n) }}` is a convenience shortcut for
/// `{{ value | date_add(n, unit="days") }}`.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not parseable or arithmetic
///   overflows chrono's representable range.
fn add_days(value: &str, n: u64) -> TemplateEngineResult<String> {
    let n_i64 = i64::try_from(n).map_err(|_| date_out_of_range_error())?;
    date_shift_unit(value, n_i64, DurationUnit::Day)
}

/// `{{ value | sub_days(n) }}` is a convenience shortcut for
/// `{{ value | date_sub(n, unit="days") }}`.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not parseable or arithmetic
///   overflows chrono's representable range.
fn sub_days(value: &str, n: u64) -> TemplateEngineResult<String> {
    let n_i64 = i64::try_from(n).map_err(|_| date_out_of_range_error())?;
    let n_i64 = n_i64.checked_neg().ok_or_else(date_out_of_range_error)?;
    date_shift_unit(value, n_i64, DurationUnit::Day)
}

/// `{{ value | add_months(n) }}` is a convenience shortcut for
/// `{{ value | date_add(n, unit="months") }}`.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not parseable or arithmetic
///   overflows chrono's representable range.
fn add_months(value: &str, n: u32) -> TemplateEngineResult<String> {
    date_shift_unit(value, i64::from(n), DurationUnit::Month)
}

/// `{{ value | sub_months(n) }}` is a convenience shortcut for
/// `{{ value | date_sub(n, unit="months") }}`.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not parseable or arithmetic
///   overflows chrono's representable range.
fn sub_months(value: &str, n: u32) -> TemplateEngineResult<String> {
    let n_i64 =
        i64::from(n).checked_neg().ok_or_else(date_out_of_range_error)?;
    date_shift_unit(value, n_i64, DurationUnit::Month)
}

/// `{{ value | add_years(n) }}` is a convenience shortcut for
/// `{{ value | date_add(n, unit="years") }}`.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not parseable or arithmetic
///   overflows chrono's representable range.
fn add_years(value: &str, n: u32) -> TemplateEngineResult<String> {
    date_shift_unit(value, i64::from(n), DurationUnit::Year)
}

/// `{{ value | sub_years(n) }}` is a convenience shortcut for
/// `{{ value | date_sub(n, unit="years") }}`.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not parseable or arithmetic
///   overflows chrono's representable range.
fn sub_years(value: &str, n: u32) -> TemplateEngineResult<String> {
    let n_i64 =
        i64::from(n).checked_neg().ok_or_else(date_out_of_range_error)?;
    date_shift_unit(value, n_i64, DurationUnit::Year)
}

/// `{{ value | start_of_month }}` returns the first day of the input month.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not a parseable date/time
///   string; see [`ParsedDate::parse`].
/// - [`ErrorKind::InvalidOperation`] if the first day of the month is outside
///   chrono's representable range; see [`date_out_of_range_error`].
fn start_of_month(value: &str) -> TemplateEngineResult<String> {
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
fn end_of_month(value: &str) -> TemplateEngineResult<String> {
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
fn weekday(value: &str) -> TemplateEngineResult<u32> {
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
///   [`DurationUnit::parse`]'s accepted names (see [`unit_kwarg`]).
/// - [`ErrorKind::TooManyArguments`] if `kwargs` carries any key besides
///   `unit`.
#[expect(
    clippy::needless_pass_by_value,
    reason = "minijinja's Function trait extracts a filter's trailing Kwargs \
              argument by value; only `&self` methods on it are needed here"
)]
fn date_diff(
    value: &str,
    other: &str,
    kwargs: Kwargs,
) -> TemplateEngineResult<Value> {
    let unit = unit_kwarg(&kwargs)?;
    let from = ParsedDate::parse(value)?;
    let to = ParsedDate::parse(other)?;

    match unit {
        DurationUnit::Year => Ok(Value::from(signed_years_since(
            from.wall.date(),
            to.wall.date(),
        ))),
        DurationUnit::Month => Ok(Value::from(signed_months_since(
            from.wall.date(),
            to.wall.date(),
        ))),
        u => {
            let unit_secs = u.seconds();
            // Fixed units measure elapsed time between the stored instants when
            // both inputs carry a time component; a date-only input stays
            // zone-free, so any such pair subtracts civil wall clocks.
            let both_datetimes = from.precision == DatePrecision::DateTime
                && to.precision == DatePrecision::DateTime;
            let delta = if both_datetimes {
                to.instant.signed_duration_since(from.instant)
            } else {
                to.wall.signed_duration_since(from.wall)
            };

            if both_datetimes {
                let whole_seconds =
                    delta.num_seconds().to_f64().ok_or_else(|| {
                        Error::new(
                            ErrorKind::InvalidOperation,
                            "date difference is too large to represent as \
                             seconds",
                        )
                    })?;
                let result = (whole_seconds
                    + f64::from(delta.subsec_nanos()) / 1e9)
                    / unit_secs;
                Ok(Value::from(result))
            } else if u == DurationUnit::Millisecond {
                Ok(Value::from(delta.num_milliseconds()))
            } else {
                let unit_secs_i64 = u.seconds_i64().ok_or_else(|| {
                    Error::new(
                        ErrorKind::InvalidOperation,
                        "duration unit has no whole-second value",
                    )
                })?;
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "unit_secs_i64 is 86_400, 3_600, 60, or 1 (fixed \
                              units only), never zero, so this division never \
                              panics"
                )]
                let result = delta.num_seconds() / unit_secs_i64;
                Ok(Value::from(result))
            }
        }
    }
}

/// `{% if value is is_past %}` returns `true` when the piped date/time string
/// is before now.
///
/// A naive input is interpreted in the reader's local zone (see
/// [`ParsedDate::parse`]), so it compares correctly against file timestamps.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not a parseable date/time
///   string; see [`ParsedDate::parse`].
fn is_past(value: &str) -> TemplateEngineResult<bool> {
    Ok(ParsedDate::parse(value)?.instant < Utc::now())
}

/// `{% if value is is_future %}` mirrors [`is_past`] for future instants.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not a parseable date/time
///   string; see [`ParsedDate::parse`].
fn is_future(value: &str) -> TemplateEngineResult<bool> {
    Ok(ParsedDate::parse(value)?.instant > Utc::now())
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
fn is_leap_year(value: &Value) -> TemplateEngineResult<bool> {
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
/// [`DurationUnit::parse`]'s accepted unit names.
///
/// Shared by [`date_add`], [`date_sub`], and [`date_diff`] via [`unit_kwarg`].
fn unknown_unit_error(unit: &str) -> Error {
    Error::new(
        ErrorKind::InvalidOperation,
        format!(
            "unknown unit {unit:?} (expected \"years\"/\"y\", \
             \"months\"/\"mo\", \"days\"/\"d\", \"hours\"/\"h\", \
             \"minutes\"/\"m\", or \"seconds\"/\"s\")"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TzGuard;

    fn env() -> Environment<'static> {
        // Every date filter reads the local zone through the core parser, so
        // the render below must own `TZ` against concurrent tests.
        TzGuard::keep();
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
        /// impl panics on that `Err`. Writing through `fmt::Write` directly
        /// instead must surface it as a normal render error.
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
            TzGuard::set("UTC");

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
            TzGuard::set("UTC");

            let rendered = env()
                .render_str(
                    r#"{{ date.from_timestamp(0, format="%H:%M") }}"#,
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "00:00");
        }

        #[test]
        fn formats_in_the_local_wall_clock() {
            TzGuard::set("Etc/GMT-2"); // UTC+02:00, no DST

            // Midnight UTC reads as 02:00 local on the same calendar day.
            let rendered = env()
                .render_str(
                    r#"{{ date.from_timestamp(0, format="%H:%M") }}"#,
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "02:00");
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
        use crate::DateError;

        #[rstest]
        #[case::date_only("2026-07-23", "%d/%m/%Y", "23/07/2026")]
        #[case::datetime_input("2026-07-23 14:30", "%H:%M", "14:30")]
        #[case::rfc3339_with_z_offset("2026-07-29T14:30:00Z", "%Y", "2026")]
        #[case::rfc3339_with_numeric_offset(
            "2026-07-29T14:30:00+02:00",
            "%H:%M",
            "12:30"
        )]
        #[case::fractional_seconds(
            "2026-07-29T14:30:00.123",
            "%H:%M:%S%.3f",
            "14:30:00.123"
        )]
        #[case::year_month_reduced_precision("2026-08", "%Y-%m", "2026-08")]
        fn reformats_a_piped_date_string(
            #[case] input: &str,
            #[case] format: &str,
            #[case] expected: &str,
        ) {
            TzGuard::set("UTC");

            let rendered = env()
                .render_str(
                    &format!(r#"{{{{ value | date_format("{format}") }}}}"#),
                    minijinja::context! { value => input },
                )
                .expect("render succeeds");

            assert_eq!(rendered, expected);
        }

        #[test]
        fn renders_an_explicit_offset_input_in_the_local_wall_clock() {
            TzGuard::set("Etc/GMT-2"); // UTC+02:00, no DST

            // The everyday spelling is the reader's wall clock: 14:30Z is 16:30
            // local, not the UTC 12:30 the old behavior printed.
            let rendered = env()
                .render_str(
                    r#"{{ value | date_format("%H:%M") }}"#,
                    minijinja::context! { value => "2026-07-29T14:30:00Z" },
                )
                .expect("render succeeds");

            assert_eq!(rendered, "16:30");
        }

        #[test]
        fn reformats_a_naive_input_to_its_own_wall_clock() {
            TzGuard::set("Etc/GMT-2"); // UTC+02:00, no DST

            // A naive datetime means the reader's wall clock, so displaying it
            // back yields the written time in any zone.
            let rendered = env()
                .render_str(
                    r#"{{ value | date_format("%H:%M") }}"#,
                    minijinja::context! { value => "2026-07-29 14:30" },
                )
                .expect("render succeeds");

            assert_eq!(rendered, "14:30");
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

        /// Regression: `ParsedDate::parse` discarded the underlying
        /// [`DateError`], leaving template render errors with an empty source
        /// chain.
        #[test]
        fn rejects_an_unparseable_date_preserving_the_parse_error_source() {
            use std::error::Error as _;

            let error = env()
                .render_str(
                    r#"{{ "not a date" | date_format("%Y") }}"#,
                    minijinja::context!(),
                )
                .expect_err("unparseable date fails cleanly");

            let source =
                error.source().expect("render error carries its source");
            assert!(
                source.downcast_ref::<DateError>().is_some(),
                "source chain must retain the core DateError, got {source:?}"
            );
        }

        /// Regression: for a datetime-shaped input that fails for a
        /// datetime-specific reason, `ParsedDate::parse` used to try
        /// [`DateTimeValue::parse_iso`] first, discard its error
        /// unconditionally, then surface [`DateValue::parse_iso`]'s unrelated
        /// shape-mismatch error instead, hiding the more specific cause (spec
        /// N4).
        #[test]
        fn surfaces_the_datetime_parsers_error_not_the_date_only_fallbacks() {
            use std::error::Error as _;

            let input = "2026-07-29T99:99:99"; // datetime-shaped, invalid time
            let datetime_error = DateTimeValue::parse_iso(input)
                .expect_err("datetime parser rejects an invalid time");

            let error = env()
                .render_str(
                    r#"{{ value | date_format("%Y") }}"#,
                    minijinja::context! { value => input },
                )
                .expect_err("unparseable datetime fails cleanly");

            let source =
                error.source().expect("render error carries its source");
            assert_eq!(
                source.to_string(),
                datetime_error.to_string(),
                "the surfaced source must be the datetime parser's own error, \
                 not the date-only fallback's unrelated shape-mismatch error"
            );
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
            TzGuard::set("UTC");

            let rendered = env()
                .render_str(
                    "{{ value | timestamp }}",
                    minijinja::context! { value => input },
                )
                .expect("render succeeds");

            assert_eq!(rendered, expected.to_string());
        }

        #[test]
        fn interprets_a_naive_datetime_in_the_local_zone() {
            TzGuard::set("Etc/GMT-2"); // UTC+02:00, no DST

            let rendered = env()
                .render_str(
                    r#"{{ "2026-07-29 14:30:00" | timestamp
                        - "2026-07-29T12:30:00Z" | timestamp }}"#,
                    minijinja::context!(),
                )
                .expect("render succeeds");

            // Local 14:30 in UTC+2 is the same instant as 12:30Z.
            assert_eq!(rendered, "0");
        }

        #[test]
        fn resolves_a_date_only_input_at_local_midnight() {
            TzGuard::set("Etc/GMT-2"); // UTC+02:00, no DST

            let rendered = env()
                .render_str(
                    r#"{{ "2026-07-29" | timestamp
                        - "2026-07-29T00:00:00" | timestamp }}"#,
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "0");
        }

        #[test]
        fn resolves_a_spring_forward_gap_input_instead_of_failing() {
            TzGuard::set("America/New_York");

            // 02:30 does not exist on 2026-03-08; the adopted 'compatible'
            // policy shifts it forward by the gap to 03:30 EDT (07:30Z).
            let rendered = env()
                .render_str(
                    r#"{{ "2026-03-08 02:30:00" | timestamp
                        - "2026-03-08T07:30:00Z" | timestamp }}"#,
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "0");
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
            "2026-07-24T14:30:00"
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
            "2024-02-29T10:00:00"
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

            assert_eq!(rendered, "2026-08-26-2028-07-26-2026-07-26T15:00:00");
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

        /// Regression: `1e16` seconds fits `i64` (so the `checked_mul` guard
        /// passes) but overflows chrono's `TimeDelta` millisecond range, where
        /// `chrono::Duration::seconds` panics instead of returning `None`.
        #[test]
        fn date_add_returns_an_error_for_an_out_of_range_second_magnitude() {
            let error = env()
                .render_str(
                    r"{{ '2026-07-26' | date_add(10000000000000000, unit='seconds') }}",
                    minijinja::context!(),
                )
                .expect_err("out-of-range shift magnitude fails cleanly");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }

        /// Pins the declared split between calendar and exact units: a day unit
        /// shifts the civil wall clock across the 2026-03-08 gap, while 24
        /// hours shifts the instant and lands an hour later on the clock.
        #[test]
        fn a_day_unit_preserves_the_wall_clock_across_a_dst_gap_while_hours_do_not()
         {
            TzGuard::set("America/New_York");

            let rendered = env()
                .render_str(
                    r"{{ '2026-03-07 17:00:00' | date_add(1, unit='days') }}-{{ '2026-03-07 17:00:00' | date_add(24, unit='hours') }}",
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "2026-03-08T17:00:00-2026-03-08T18:00:00");
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

            assert_eq!(rendered, "TrueFalse");
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

            assert_eq!(rendered, "FalseTrue");
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

        #[test]
        fn current_timestamp_is_neither_past_nor_future() {
            let now = Utc::now();
            let formatted = now.format("%Y-%m-%dT%H:%M:%S").to_string();

            let past = is_past(&formatted).expect("parse succeeds");
            let future = is_future(&formatted).expect("parse succeeds");

            assert!(
                !past || !future,
                "is_past and is_future must be mutually exclusive, got \
                 past={past}, future={future}"
            );
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

    mod is_leap_year {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::divisible_by_4("2024 is is_leap_year", "True")]
        #[case::not_divisible_by_4("2023 is is_leap_year", "False")]
        #[case::divisible_by_100_not_400("1900 is is_leap_year", "False")]
        #[case::divisible_by_400("2000 is is_leap_year", "True")]
        #[case::leap_date_string("'2024-02-15' is is_leap_year", "True")]
        #[case::non_leap_date_string("'2023-02-15' is is_leap_year", "False")]
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
