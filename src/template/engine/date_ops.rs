//! [`DateOps`]: the `date` namespace object registered as a minijinja
//! global by [`super::TemplateEngine`], plus the flat `date_*` pipeline
//! filters and `is_*` tests this module registers alongside it.
//!
//! **Namespace functions** (`date.now()`, `date.today()`,
//! `date.tomorrow()`, `date.yesterday()`, `date.from_timestamp(ts)`) are
//! *generators* — they take no date argument, producing a formatted
//! string from the current instant (or a timestamp). They live on
//! [`DateOps`] via [`Object::get_value`], same pattern as `now`.
//!
//! **Filters** (`date_format`, `timestamp`, `add_days`, `sub_days`,
//! `add_months`, `sub_months`, `add_years`, `sub_years`,
//! `start_of_month`, `end_of_month`, `weekday`, `date_diff`) and
//! **tests** (`is_past`, `is_future`, `is_leap_year`) are
//! *transformations* — they take a piped date/time string, so they're
//! plain functions registered via [`Environment::add_filter`]/
//! [`Environment::add_test`], same reasoning as
//! [`StrOps`](super::str_ops::StrOps): no shared state, no namespace method
//! dispatch needed. `date_format` is prefixed to avoid colliding with
//! minijinja's built-in `format` filter (printf-style, unrelated to dates).
//!
//! All date/time string parsing funnels through [`parse_date`] (or
//! [`parse_date_precise`], which additionally reports whether a time
//! component was present) — every filter and test shares the same
//! accepted formats, so there's exactly one place that decides what
//! counts as a valid date string. Per [`parse_date_precise`]'s docs: a
//! full datetime is tried first, falling back to a bare `%Y-%m-%d` date
//! at midnight — never a panic, always a [`minijinja::Error`] on
//! failure.
//!
//! Every arithmetic filter (`add_days`, `end_of_month`, etc.) re-serializes
//! its result at the same precision its input had — see
//! [`format_precise`] — so piping a date-only string through a chain of
//! filters never silently grows a `00:00:00` suffix, and piping a
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
/// omitted — matches the spec's own example, an ISO-8601-style date.
/// Also the default output shape [`format_precise`] uses for a
/// date-only (no time component) input.
const DEFAULT_FORMAT: &str = "%Y-%m-%d";

/// [`format_precise`]'s output shape for an input that carried a time
/// component.
const DEFAULT_DATETIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

/// Formats [`parse_date_precise`] tries, in order, before falling back
/// to a bare date. Covers both shapes the issue's usage examples
/// exercise — space-separated (`2026-07-23 14:30[:00]`) and
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
        env.add_filter("date_format", date_format_filter);
        env.add_filter("timestamp", timestamp_filter);
        env.add_filter("add_days", add_days_filter);
        env.add_filter("sub_days", sub_days_filter);
        env.add_filter("add_months", add_months_filter);
        env.add_filter("sub_months", sub_months_filter);
        env.add_filter("add_years", add_years_filter);
        env.add_filter("sub_years", sub_years_filter);
        env.add_filter("start_of_month", start_of_month_filter);
        env.add_filter("end_of_month", end_of_month_filter);
        env.add_filter("weekday", weekday_filter);
        env.add_filter("date_diff", date_diff_filter);
        env.add_test("is_past", is_past_test);
        env.add_test("is_future", is_future_test);
        env.add_test("is_leap_year", is_leap_year_test);
        env.add_global("date", Value::from_object(self));
    }
}

impl Object for DateOps {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        match key.as_str()? {
            "now" => Some(Value::from_function(
                |kwargs: Kwargs| -> Result<String, Error> {
                    let format = kwargs
                        .get::<Option<&str>>("format")?
                        .unwrap_or(DEFAULT_FORMAT);
                    kwargs.assert_all_used()?;
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
                    let format = kwargs
                        .get::<Option<&str>>("format")?
                        .unwrap_or(DEFAULT_FORMAT);
                    kwargs.assert_all_used()?;
                    format_with(
                        Local::now().date_naive().format(format),
                        format,
                    )
                },
            )),
            "tomorrow" => Some(Value::from_function(
                |kwargs: Kwargs| -> Result<String, Error> {
                    let format = kwargs
                        .get::<Option<&str>>("format")?
                        .unwrap_or(DEFAULT_FORMAT);
                    kwargs.assert_all_used()?;
                    let date = Local::now()
                        .date_naive()
                        .succ_opt()
                        .ok_or_else(date_out_of_range_error)?;
                    format_with(date.format(format), format)
                },
            )),
            "yesterday" => Some(Value::from_function(
                |kwargs: Kwargs| -> Result<String, Error> {
                    let format = kwargs
                        .get::<Option<&str>>("format")?
                        .unwrap_or(DEFAULT_FORMAT);
                    kwargs.assert_all_used()?;
                    let date = Local::now()
                        .date_naive()
                        .pred_opt()
                        .ok_or_else(date_out_of_range_error)?;
                    format_with(date.format(format), format)
                },
            )),
            "from_timestamp" => Some(Value::from_function(
                |unix_ts: i64, kwargs: Kwargs| -> Result<String, Error> {
                    let format = kwargs
                        .get::<Option<&str>>("format")?
                        .unwrap_or(DEFAULT_FORMAT);
                    kwargs.assert_all_used()?;
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

/// Formats `formattable` — anything chrono's `.format(fmt)` produces
/// (a `DelayedFormat`, from a `NaiveDate`/`NaiveDateTime`/`DateTime<_>`)
/// — writing through [`std::fmt::Write`] rather than `.to_string()`: the
/// latter's blanket impl `.expect()`s a successful `Display::fmt`, but
/// `DelayedFormat` returns `Err` (not a panic of its own) for an invalid
/// strftime specifier such as `%Q`, so writing directly is what turns
/// that into a normal [`minijinja::Error`] instead of a panic.
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
/// date at midnight. [`date_diff_filter`] uses the `bool` to decide
/// between integer-unit and sub-day-precision (`f64`) output — every
/// other caller goes through [`parse_date`], which discards it.
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
/// [`date_diff_filter`] uses — see [`parse_date_precise`] for the
/// accepted formats and fallback behavior.
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
fn date_format_filter(value: &str, format: &str) -> Result<String, Error> {
    let datetime = parse_date(value)?;
    format_with(datetime.format(format), format)
}

/// `{{ value | timestamp }}` — converts a piped date/time string to
/// Unix seconds, treating a naive (timezone-less) input as UTC.
fn timestamp_filter(value: &str) -> Result<i64, Error> {
    Ok(parse_date(value)?.and_utc().timestamp())
}

/// `{{ value | add_days(n) }}`
fn add_days_filter(value: &str, n: u64) -> Result<String, Error> {
    let (datetime, has_time) = parse_date_precise(value)?;
    let shifted = datetime
        .checked_add_days(Days::new(n))
        .ok_or_else(date_out_of_range_error)?;
    format_precise(shifted, has_time)
}

/// `{{ value | sub_days(n) }}`
fn sub_days_filter(value: &str, n: u64) -> Result<String, Error> {
    let (datetime, has_time) = parse_date_precise(value)?;
    let shifted = datetime
        .checked_sub_days(Days::new(n))
        .ok_or_else(date_out_of_range_error)?;
    format_precise(shifted, has_time)
}

/// `{{ value | add_months(n) }}` — clamps to the last day of the
/// resulting month when the original day doesn't exist there (e.g.
/// `2023-01-31` + 1 month -> `2023-02-28`), per
/// [`NaiveDateTime::checked_add_months`]'s documented behavior.
fn add_months_filter(value: &str, n: u32) -> Result<String, Error> {
    let (datetime, has_time) = parse_date_precise(value)?;
    let shifted = datetime
        .checked_add_months(Months::new(n))
        .ok_or_else(date_out_of_range_error)?;
    format_precise(shifted, has_time)
}

/// `{{ value | sub_months(n) }}` — same end-of-month clamping as
/// [`add_months_filter`], in reverse.
fn sub_months_filter(value: &str, n: u32) -> Result<String, Error> {
    let (datetime, has_time) = parse_date_precise(value)?;
    let shifted = datetime
        .checked_sub_months(Months::new(n))
        .ok_or_else(date_out_of_range_error)?;
    format_precise(shifted, has_time)
}

/// `{{ value | add_years(n) }}` — implemented as `n * 12` months (via
/// [`add_months_filter`]'s underlying call), so a Feb 29 input clamps to
/// Feb 28 in a non-leap target year exactly like `add_months` clamps
/// across any other month-length mismatch.
fn add_years_filter(value: &str, n: u32) -> Result<String, Error> {
    let (datetime, has_time) = parse_date_precise(value)?;
    let months = n.checked_mul(12).ok_or_else(date_out_of_range_error)?;
    let shifted = datetime
        .checked_add_months(Months::new(months))
        .ok_or_else(date_out_of_range_error)?;
    format_precise(shifted, has_time)
}

/// `{{ value | sub_years(n) }}` — see [`add_years_filter`].
fn sub_years_filter(value: &str, n: u32) -> Result<String, Error> {
    let (datetime, has_time) = parse_date_precise(value)?;
    let months = n.checked_mul(12).ok_or_else(date_out_of_range_error)?;
    let shifted = datetime
        .checked_sub_months(Months::new(months))
        .ok_or_else(date_out_of_range_error)?;
    format_precise(shifted, has_time)
}

/// `{{ value | start_of_month }}`
fn start_of_month_filter(value: &str) -> Result<String, Error> {
    let (datetime, has_time) = parse_date_precise(value)?;
    let shifted = datetime.with_day(1).ok_or_else(date_out_of_range_error)?;
    format_precise(shifted, has_time)
}

/// `{{ value | end_of_month }}`
fn end_of_month_filter(value: &str) -> Result<String, Error> {
    let (datetime, has_time) = parse_date_precise(value)?;
    let last_day = datetime.num_days_in_month();
    let shifted = datetime
        .with_day(u32::from(last_day))
        .ok_or_else(date_out_of_range_error)?;
    format_precise(shifted, has_time)
}

/// `{{ value | weekday }}` — `0` for Monday through `6` for Sunday, per
/// the issue's spec (not chrono's own Sunday-first
/// [`Weekday::number_from_sunday`](chrono::Weekday::number_from_sunday)).
fn weekday_filter(value: &str) -> Result<u32, Error> {
    Ok(parse_date(value)?.weekday().num_days_from_monday())
}

/// `{{ value | date_diff(other, unit="days") }}` — the signed duration
/// from the piped value to `other` (positive when `other` is later),
/// expressed in `unit` (`"days"` default, `"hours"`, `"minutes"`, or
/// `"seconds"`). Returns `f64` when both `value` and `other` carry a
/// time component (sub-day precision is meaningful); otherwise an `i64`
/// whole-unit count.
#[allow(
    clippy::needless_pass_by_value,
    reason = "minijinja's Function trait extracts a filter's trailing Kwargs \
              argument by value; only `&self` methods on it are needed here"
)]
fn date_diff_filter(
    value: &str,
    other: &str,
    kwargs: Kwargs,
) -> Result<Value, Error> {
    let unit = kwargs.get::<Option<&str>>("unit")?.unwrap_or("days");
    kwargs.assert_all_used()?;
    let (from, from_has_time) = parse_date_precise(value)?;
    let (to, to_has_time) = parse_date_precise(other)?;
    let delta = to.signed_duration_since(from);

    if from_has_time && to_has_time {
        #[allow(
            clippy::as_conversions,
            clippy::cast_precision_loss,
            reason = "TimeDelta::num_seconds() is bounded by chrono's \
                      NaiveDateTime range (~262,000 years, well under 2^52 \
                      seconds), so this cast never loses precision in practice"
        )]
        let seconds =
            delta.num_seconds() as f64 + f64::from(delta.subsec_nanos()) / 1e9;
        let result = match unit {
            "days" => seconds / 86_400.0,
            "hours" => seconds / 3_600.0,
            "minutes" => seconds / 60.0,
            "seconds" => seconds,
            unknown => return Err(unknown_unit_error(unknown)),
        };
        Ok(Value::from(result))
    } else {
        let result = match unit {
            "days" => delta.num_days(),
            "hours" => delta.num_hours(),
            "minutes" => delta.num_minutes(),
            "seconds" => delta.num_seconds(),
            unknown => return Err(unknown_unit_error(unknown)),
        };
        Ok(Value::from(result))
    }
}

/// `{% if value is is_past %}` — `true` when the piped date/time string
/// is before now (UTC; a naive input is treated as UTC, matching
/// [`timestamp_filter`]).
fn is_past_test(value: &str) -> Result<bool, Error> {
    Ok(parse_date(value)?.and_utc() < Utc::now())
}

/// `{% if value is is_future %}` — see [`is_past_test`].
fn is_future_test(value: &str) -> Result<bool, Error> {
    Ok(parse_date(value)?.and_utc() > Utc::now())
}

/// `{% if value is is_leap_year %}` — accepts either an integer year
/// (`2024 is is_leap_year`) or a date/time string, checked via
/// [`parse_date`].
fn is_leap_year_test(value: &Value) -> Result<bool, Error> {
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
