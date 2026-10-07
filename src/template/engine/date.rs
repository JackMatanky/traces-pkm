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
//! Date/time string parsing funnels through [`DateValue::classify`] via
//! [`parse_date`]. A full datetime is tried first, falling back to a bare ISO
//! date (`YYYY-MM-DD` or reduced-precision `YYYY-MM`) at midnight. Arithmetic
//! filters re-serialize at the input's original precision via the calendar
//! owner.
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

use chrono::{Datelike as _, NaiveDate, NaiveDateTime, Utc};
use minijinja::{
    Environment, Error, ErrorKind,
    value::{Enumerator, Kwargs, Object, Value},
};

use super::error::{TemplateEngineResult, invalid_operation};
use crate::{
    DEFAULT_DATE_FORMAT, DateTimeValue, DateValue, DurationSeconds,
    DurationUnit, DurationValue, UNIT_HINT,
    date::{
        DateDiff, DateError, DateFormat, DatePoint, DateTimeFormat, Precision,
        RecognizedDate,
    },
};

const METHODS: &[&str] = &[
    "now",
    "today",
    "tomorrow",
    "yesterday",
    "from_timestamp",
    "start_of_week",
    "sow",
    "end_of_week",
    "eow",
    "start_of_month",
    "som",
    "end_of_month",
    "eom",
    "start_of_year",
    "soy",
    "end_of_year",
    "eoy",
    "weekday",
];
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
        env.add_filter("durationformat", durationformat);
        env.add_filter("timestamp", timestamp);
        env.add_filter("date_add", date_add);
        env.add_filter("date_sub", date_sub);
        env.add_filter("add_days", add_days);
        env.add_filter("sub_days", sub_days);
        env.add_filter("add_months", add_months);
        env.add_filter("sub_months", sub_months);
        env.add_filter("add_years", add_years);
        env.add_filter("sub_years", sub_years);
        env.add_filter("start_of_week", start_of_week);
        env.add_filter("sow", start_of_week);
        env.add_filter("end_of_week", end_of_week);
        env.add_filter("eow", end_of_week);
        env.add_filter("start_of_month", start_of_month);
        env.add_filter("som", start_of_month);
        env.add_filter("end_of_month", end_of_month);
        env.add_filter("eom", end_of_month);
        env.add_filter("start_of_year", start_of_year);
        env.add_filter("soy", start_of_year);
        env.add_filter("end_of_year", end_of_year);
        env.add_filter("eoy", end_of_year);
        env.add_filter("weekday", weekday);
        env.add_filter("date_diff", date_diff);
        env.add_test("is_past", is_past);
        env.add_test("is_future", is_future);
        env.add_test("is_leap_year", is_leap_year);
        env.add_global("date", Value::from_object(self));
    }
}

fn local_point() -> DatePoint {
    let utc_now = Utc::now();
    let wall = DateTimeValue::from(utc_now).wall_or_utc();
    DatePoint::new(wall, utc_now, Precision::DateTime)
}

fn today_point() -> DatePoint {
    let local = local_point();
    DatePoint::new(
        local.wall.date().and_hms_opt(0, 0, 0).unwrap_or(local.wall),
        DateTimeValue::from(DateValue::from(local.wall.date())).into_inner(),
        Precision::Date,
    )
}

#[expect(
    clippy::excessive_nesting,
    reason = "character parsing loop builds ISO component offsets"
)]
fn translate_iso_offset(input: &str) -> String {
    let trimmed = input.trim();
    let (lead_neg, rest) = if let Some(s) = trimmed.strip_prefix('-') {
        (true, s)
    } else {
        (false, trimmed)
    };
    if let Some(rest) =
        rest.strip_prefix('P').or_else(|| rest.strip_prefix('p'))
    {
        let (int_neg, rest) = if let Some(s) = rest.strip_prefix('-') {
            (true, s)
        } else {
            (false, rest)
        };
        let is_neg = lead_neg ^ int_neg;
        let prefix = if is_neg {
            "-"
        } else {
            ""
        };
        let mut out = String::new();
        let (date_part, time_part) =
            match rest.split_once('T').or_else(|| rest.split_once('t')) {
                Some((d, t)) => (d, Some(t)),
                None => (rest, None),
            };
        let mut parse_parts = |part: &str, is_time: bool| {
            let mut num_buf = String::new();
            for ch in part.chars() {
                if ch.is_ascii_digit() || ch == '.' || ch == '-' {
                    num_buf.push(ch);
                } else if ch.is_ascii_alphabetic() {
                    let unit_str = match (ch.to_ascii_uppercase(), is_time) {
                        ('Y', false) => "y",
                        ('M', false) => "mo",
                        ('W', false) => "w",
                        ('D', false) => "d",
                        ('H', true) => "h",
                        ('M', true) => "m",
                        ('S', true) => "s",
                        _ => "",
                    };
                    if !unit_str.is_empty() && !num_buf.is_empty() {
                        if !out.is_empty() {
                            out.push(' ');
                        }
                        out.push_str(prefix);
                        out.push_str(&num_buf);
                        out.push_str(unit_str);
                    }
                    num_buf.clear();
                } else {
                    num_buf.clear();
                }
            }
        };
        parse_parts(date_part, false);
        if let Some(t) = time_part {
            parse_parts(t, true);
        }
        if !out.is_empty() {
            return out;
        }
    }
    input.to_owned()
}

fn apply_offset_to_point(
    point: DatePoint,
    offset_val: &Value,
) -> TemplateEngineResult<DatePoint> {
    if let Some(n) = offset_val.as_i64() {
        return point.shift(n, DurationUnit::Day).map_err(date_error);
    }
    if let Some(s) = offset_val.as_str() {
        let translated = translate_iso_offset(s);
        let duration = DurationValue::parse(&translated).map_err(|_| {
            invalid_operation(
                format!("invalid duration offset {s:?}"),
                DateError::OutOfRange,
            )
        })?;
        let shifted = if point.has_time() {
            let dt = DateTimeValue::from(point.instant)
                .apply(&duration)
                .map_err(date_error)?;
            let wall = dt.local_wall().ok_or_else(date_out_of_range_error)?;
            DatePoint {
                wall,
                instant: dt.into_inner(),
                precision: point.precision,
            }
        } else {
            let date = DateValue::from(point.wall.date())
                .apply(&duration)
                .map_err(date_error)?;
            let wall = date
                .into_inner()
                .and_hms_opt(0, 0, 0)
                .ok_or_else(date_out_of_range_error)?;
            let instant = DateTimeValue::from(date).into_inner();
            DatePoint {
                wall,
                instant,
                precision: point.precision,
            }
        };
        return Ok(shifted);
    }
    Err(invalid_operation(
        format!(
            "expected integer days or duration string for offset, got \
             {offset_val:?}"
        ),
        DateError::OutOfRange,
    ))
}

#[expect(
    clippy::too_many_arguments,
    reason = "accepts format, offset, reference, and reference_format options"
)]
fn resolve_shorthand(
    base_point: DatePoint,
    transform: impl FnOnce(DatePoint) -> Result<DatePoint, DateError>,
    format: Option<&str>,
    offset: Option<Value>,
    reference: Option<&str>,
    reference_format: Option<&str>,
    kwargs: &Kwargs,
) -> TemplateEngineResult<String> {
    let format = format
        .or(kwargs.get::<Option<&str>>("format")?)
        .unwrap_or(DEFAULT_DATE_FORMAT);
    let offset = offset.or(kwargs.get::<Option<Value>>("offset")?);
    let reference = reference.or(kwargs.get::<Option<&str>>("reference")?);
    let reference_format =
        reference_format.or(kwargs.get::<Option<&str>>("reference_format")?);
    kwargs.assert_all_used()?;

    let mut point = if let Some(ref_text) = reference {
        if let Some(ref_fmt) = reference_format {
            let recognized =
                DateValue::parse_with(ref_text, ref_fmt).map_err(date_error)?;
            recognized.point()
        } else {
            let recognized = parse_recognized(ref_text)?;
            recognized.point()
        }
    } else {
        base_point
    };
    point = transform(point).map_err(date_error)?;

    if let Some(off_val) = offset {
        point = apply_offset_to_point(point, &off_val)?;
    }

    let pat = translate_pattern(format, point.wall)?;
    format_with(point.wall.format(&pat), format)
}

fn parse_weekday_num(n: i64) -> TemplateEngineResult<chrono::Weekday> {
    match n {
        0 | 1 => Ok(chrono::Weekday::Mon),
        2 => Ok(chrono::Weekday::Tue),
        3 => Ok(chrono::Weekday::Wed),
        4 => Ok(chrono::Weekday::Thu),
        5 => Ok(chrono::Weekday::Fri),
        6 => Ok(chrono::Weekday::Sat),
        7 => Ok(chrono::Weekday::Sun),
        _ => Err(invalid_operation(
            format!(
                "weekday index {n} is out of range (expected 0..=7 under ISO \
                 Monday convention)"
            ),
            DateError::OutOfRange,
        )),
    }
}

impl Object for DateOps {
    #[expect(
        clippy::too_many_lines,
        reason = "dispatches all date shorthands and aliases"
    )]
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        match key.as_str()? {
            "now" => Some(Value::from_function(
                |format: Option<&str>,
                 offset: Option<Value>,
                 reference: Option<&str>,
                 reference_format: Option<&str>,
                 kwargs: Kwargs|
                 -> TemplateEngineResult<String> {
                    resolve_shorthand(
                        local_point(),
                        Ok,
                        format,
                        offset,
                        reference,
                        reference_format,
                        &kwargs,
                    )
                },
            )),
            "today" => Some(Value::from_function(
                |format: Option<&str>,
                 offset: Option<Value>,
                 reference: Option<&str>,
                 reference_format: Option<&str>,
                 kwargs: Kwargs|
                 -> TemplateEngineResult<String> {
                    resolve_shorthand(
                        today_point(),
                        Ok,
                        format,
                        offset,
                        reference,
                        reference_format,
                        &kwargs,
                    )
                },
            )),
            "tomorrow" => Some(Value::from_function(
                |format: Option<&str>,
                 offset: Option<Value>,
                 reference: Option<&str>,
                 reference_format: Option<&str>,
                 kwargs: Kwargs|
                 -> TemplateEngineResult<String> {
                    resolve_shorthand(
                        today_point(),
                        |pt| pt.shift(1, DurationUnit::Day),
                        format,
                        offset,
                        reference,
                        reference_format,
                        &kwargs,
                    )
                },
            )),
            "yesterday" => Some(Value::from_function(
                |format: Option<&str>,
                 offset: Option<Value>,
                 reference: Option<&str>,
                 reference_format: Option<&str>,
                 kwargs: Kwargs|
                 -> TemplateEngineResult<String> {
                    resolve_shorthand(
                        today_point(),
                        |pt| pt.shift(-1, DurationUnit::Day),
                        format,
                        offset,
                        reference,
                        reference_format,
                        &kwargs,
                    )
                },
            )),
            "start_of_week" | "sow" => Some(Value::from_function(
                |format: Option<&str>,
                 offset: Option<Value>,
                 reference: Option<&str>,
                 reference_format: Option<&str>,
                 kwargs: Kwargs|
                 -> TemplateEngineResult<String> {
                    resolve_shorthand(
                        today_point(),
                        |pt| pt.week_boundary(false),
                        format,
                        offset,
                        reference,
                        reference_format,
                        &kwargs,
                    )
                },
            )),
            "end_of_week" | "eow" => Some(Value::from_function(
                |format: Option<&str>,
                 offset: Option<Value>,
                 reference: Option<&str>,
                 reference_format: Option<&str>,
                 kwargs: Kwargs|
                 -> TemplateEngineResult<String> {
                    resolve_shorthand(
                        today_point(),
                        |pt| pt.week_boundary(true),
                        format,
                        offset,
                        reference,
                        reference_format,
                        &kwargs,
                    )
                },
            )),
            "start_of_month" | "som" => Some(Value::from_function(
                |format: Option<&str>,
                 offset: Option<Value>,
                 reference: Option<&str>,
                 reference_format: Option<&str>,
                 kwargs: Kwargs|
                 -> TemplateEngineResult<String> {
                    resolve_shorthand(
                        today_point(),
                        |pt| pt.month_boundary(false),
                        format,
                        offset,
                        reference,
                        reference_format,
                        &kwargs,
                    )
                },
            )),
            "end_of_month" | "eom" => Some(Value::from_function(
                |format: Option<&str>,
                 offset: Option<Value>,
                 reference: Option<&str>,
                 reference_format: Option<&str>,
                 kwargs: Kwargs|
                 -> TemplateEngineResult<String> {
                    resolve_shorthand(
                        today_point(),
                        |pt| pt.month_boundary(true),
                        format,
                        offset,
                        reference,
                        reference_format,
                        &kwargs,
                    )
                },
            )),
            "start_of_year" | "soy" => Some(Value::from_function(
                |format: Option<&str>,
                 offset: Option<Value>,
                 reference: Option<&str>,
                 reference_format: Option<&str>,
                 kwargs: Kwargs|
                 -> TemplateEngineResult<String> {
                    resolve_shorthand(
                        today_point(),
                        |pt| pt.year_boundary(false),
                        format,
                        offset,
                        reference,
                        reference_format,
                        &kwargs,
                    )
                },
            )),
            "end_of_year" | "eoy" => Some(Value::from_function(
                |format: Option<&str>,
                 offset: Option<Value>,
                 reference: Option<&str>,
                 reference_format: Option<&str>,
                 kwargs: Kwargs|
                 -> TemplateEngineResult<String> {
                    resolve_shorthand(
                        today_point(),
                        |pt| pt.year_boundary(true),
                        format,
                        offset,
                        reference,
                        reference_format,
                        &kwargs,
                    )
                },
            )),
            "weekday" => Some(Value::from_function(
                |n: i64,
                 format: Option<&str>,
                 reference: Option<&str>,
                 reference_format: Option<&str>,
                 kwargs: Kwargs|
                 -> TemplateEngineResult<String> {
                    let target = parse_weekday_num(n)?;
                    resolve_shorthand(
                        today_point(),
                        move |pt| pt.weekday_point(target),
                        format,
                        None,
                        reference,
                        reference_format,
                        &kwargs,
                    )
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

/// Parses `s` as a date or date-time expression via [`DateValue::classify`].
///
/// Attaches [`DateError`] as the underlying source so render diagnostics keep
/// the full parse-failure chain.
fn parse_recognized(s: &str) -> TemplateEngineResult<RecognizedDate> {
    match DateValue::classify(s) {
        Some(Ok(rec)) => Ok(rec),
        Some(Err(err)) => {
            Err(invalid_operation(format!("invalid date {s:?}"), err))
        }
        None => {
            let trimmed = s.trim();
            let source = match DateFormat::parse_any(trimmed) {
                Err(err) => err,
                Ok(_) => match DateTimeFormat::parse_any(trimmed) {
                    Err(err) => err,
                    Ok(_) => {
                        return Err(invalid_operation(
                            format!("invalid date {s:?}"),
                            DateError::OutOfRange,
                        ));
                    }
                },
            };
            Err(invalid_operation(
                format!("invalid date {s:?}"),
                DateError::Unparseable {
                    input: trimmed.into(),
                    source,
                },
            ))
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

/// The shared date/time string parser for filters that work on the human wall
/// clock (`date_format`, `weekday`, `is_leap_year`); instant-facing filters
/// parse via [`parse_recognized`] directly.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `s` is not a parseable date/time
///   string.
fn parse_date(s: &str) -> Result<NaiveDateTime, Error> {
    parse_recognized(s).map(|rec| rec.wall_or_utc())
}

/// `{{ value | date_format(format_string) }}` formats a piped date/time string
/// according to either a strftime pattern (containing `%`) or common
/// moment-dialect tokens (`YYYY MM DD HH mm ss`, `Do`/`S`,
/// `dddd`/`ddd`/`MMM`/`MMMM`, bracket literals like `[Daily]`).
///
/// Documented strftime grammar is [`chrono::format::strftime`]. `%Z` prints
/// only a UTC offset, `%S` may render `60` for a leap second, and week numbers
/// use `%V`/`%G` (ISO), never `%U`/`%W`. Chrono advises against `%+`, which
/// is never emitted.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not a parseable date/time
///   string; see [`parse_date`].
/// - [`ErrorKind::InvalidOperation`] if `format` is an invalid strftime pattern
///   or contains an unsupported moment token.
fn date_format(value: &str, format: &str) -> TemplateEngineResult<String> {
    let datetime = parse_date(value)?;
    let pattern = translate_pattern(format, datetime)?;
    format_with(datetime.format(&pattern), format)
}

/// Translates `format` to a strftime pattern.
///
/// If `format` contains `%`, it is treated as a strftime pattern and passed
/// through. Otherwise, it is translated from common moment-dialect tokens:
/// bracket literals (`[Daily]`), `YYYY MM DD HH mm ss` family, `Do`/`S`
/// ordinals, and `dddd`/`ddd`/`MMM`/`MMMM`.
fn translate_pattern(
    format: &str,
    datetime: NaiveDateTime,
) -> TemplateEngineResult<String> {
    if format.contains('%') {
        return Ok(format.to_owned());
    }
    translate_moment_dialect(format, datetime)
}

const MOMENT_TOKENS: &[(&str, &str)] = &[
    ("YYYY", "%Y"),
    ("YY", "%y"),
    ("MMMM", "%B"),
    ("MMM", "%b"),
    ("MM", "%m"),
    ("M", "%-m"),
    ("dddd", "%A"),
    ("ddd", "%a"),
    ("DD", "%d"),
    ("D", "%-d"),
    ("HH", "%H"),
    ("H", "%-H"),
    ("hh", "%I"),
    ("h", "%-I"),
    ("mm", "%M"),
    ("m", "%-M"),
    ("ss", "%S"),
    ("s", "%-S"),
    ("SSS", "%3f"),
    ("SS", "%2f"),
    ("S", "%1f"),
];

const DURATION_HUMAN_UNITS: &[(&str, &str, f64)] = &[
    ("year", "years", 31_536_000.0),
    ("month", "months", 2_592_000.0),
    ("week", "weeks", 604_800.0),
    ("day", "days", 86_400.0),
    ("hour", "hours", 3_600.0),
    ("minute", "minutes", 60.0),
    ("second", "seconds", 1.0),
];

/// Returns the English ordinal suffix (`"st"`, `"nd"`, `"rd"`, `"th"`) for
/// `day`.
fn ordinal_suffix(day: u32) -> &'static str {
    match day {
        11..=13 => "th",
        _ => match day % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        },
    }
}

/// Translates a moment-dialect pattern into a strftime format string.
fn translate_moment_dialect(
    pattern: &str,
    datetime: NaiveDateTime,
) -> TemplateEngineResult<String> {
    use std::fmt::Write as _;

    use chrono::Datelike as _;

    let mut result = String::with_capacity(pattern.len().saturating_mul(2));
    let bytes = pattern.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let Some(&current_byte) = bytes.get(i) else {
            break;
        };
        if current_byte == b'[' {
            i = i.saturating_add(1);
            let start = i;
            while i < len && bytes.get(i) != Some(&b']') {
                i = i.saturating_add(1);
            }
            let literal = pattern.get(start..i).unwrap_or_default();
            if bytes.get(i) == Some(&b']') {
                i = i.saturating_add(1);
            }
            for ch in literal.chars() {
                if ch == '%' {
                    result.push_str("%%");
                } else {
                    result.push(ch);
                }
            }
        } else if pattern.get(i..).is_some_and(|tail| tail.starts_with("Do")) {
            i = i.saturating_add(2);
            let day = datetime.day();
            let _ = write!(result, "{day}{}", ordinal_suffix(day));
        } else if current_byte.is_ascii_alphabetic() {
            let mut matched = false;
            let tail = pattern.get(i..).unwrap_or_default();
            for &(tok, spec) in MOMENT_TOKENS {
                if tail.starts_with(tok) {
                    result.push_str(spec);
                    i = i.saturating_add(tok.len());
                    matched = true;
                    break;
                }
            }
            if !matched {
                let start = i;
                let ch = current_byte;
                while i < len && bytes.get(i) == Some(&ch) {
                    i = i.saturating_add(1);
                }
                let token = pattern.get(start..i).unwrap_or_default();
                return Err(Error::new(
                    ErrorKind::InvalidOperation,
                    format!(
                        "unsupported moment-dialect token `{token}` in \
                         pattern `{pattern}`; supported tokens are YYYY, YY, \
                         MMMM, MMM, MM, M, dddd, ddd, Do, DD, D, HH, H, hh, \
                         h, mm, m, ss, s, SSS, SS, S, and [bracketed literals]"
                    ),
                ));
            }
        } else {
            let ch = pattern
                .get(i..)
                .and_then(|tail| tail.chars().next())
                .unwrap_or(' ');
            i = i.saturating_add(ch.len_utf8());
            if ch == '%' {
                result.push_str("%%");
            } else {
                result.push(ch);
            }
        }
    }

    Ok(result)
}

/// `{{ value | durationformat }}` renders a duration in human-readable compound
/// format (e.g. `"3 days, 4 hours"`).
fn durationformat(value: Value) -> TemplateEngineResult<String> {
    let dv = if let Some(s) = value.as_str() {
        DurationValue::parse(s).map_err(|source| {
            invalid_operation(format!("invalid duration {s:?}"), source)
        })?
    } else if let Ok(f) = f64::try_from(value) {
        DurationSeconds::try_from(f).map(DurationValue::from_seconds).map_err(
            |source| {
                invalid_operation(
                    format!("out-of-range duration seconds {f}"),
                    source,
                )
            },
        )?
    } else {
        return Err(Error::new(
            ErrorKind::InvalidOperation,
            "durationformat expects a duration string or number",
        ));
    };
    Ok(format_human_duration(&dv))
}

/// Renders a [`DurationValue`] in human-readable compound format.
fn format_human_duration(dv: &DurationValue) -> String {
    let total_secs = dv.to_seconds().as_f64();
    if total_secs == 0.0 {
        return "0 seconds".to_owned();
    }
    let is_negative = total_secs < 0.0;
    let mut rem = total_secs.abs();

    let mut parts = Vec::new();
    for &(singular, plural, unit_secs) in DURATION_HUMAN_UNITS {
        if rem >= unit_secs {
            let count = (rem / unit_secs).floor();
            rem = count.mul_add(-unit_secs, rem);
            #[expect(
                clippy::as_conversions,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "unit count is non-negative and fits u64"
            )]
            let count_u64 = count as u64;
            if count_u64 == 1 {
                parts.push(format!("1 {singular}"));
            } else {
                parts.push(format!("{count_u64} {plural}"));
            }
        }
    }

    if parts.is_empty() {
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "sub-second residual fits u64"
        )]
        let ms = (rem * 1000.0).round() as u64;
        match ms.cmp(&1) {
            std::cmp::Ordering::Equal => parts.push("1 millisecond".to_owned()),
            std::cmp::Ordering::Greater => {
                parts.push(format!("{ms} milliseconds"));
            }
            std::cmp::Ordering::Less => return "0 seconds".to_owned(),
        }
    }

    let joined = parts.join(", ");
    if is_negative {
        format!("-{joined}")
    } else {
        joined
    }
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
///   string; see [`parse_recognized`].
fn timestamp(value: &str) -> TemplateEngineResult<i64> {
    Ok(parse_recognized(value)?.instant().timestamp())
}

/// Returns the first or last day of the input's month at its original
/// precision.
fn month_boundary(value: &str, end: bool) -> TemplateEngineResult<String> {
    let recognized = parse_recognized(value)?;
    let shifted = recognized.point().month_boundary(end).map_err(date_error)?;
    let pat = shifted.precision.format_pattern(shifted.wall);
    format_with(shifted.wall.format(pat), pat)
}

/// Returns the first or last day of the input's week at its original precision.
fn week_boundary(value: &str, end: bool) -> TemplateEngineResult<String> {
    let recognized = parse_recognized(value)?;
    let shifted = recognized.point().week_boundary(end).map_err(date_error)?;
    let pat = shifted.precision.format_pattern(shifted.wall);
    format_with(shifted.wall.format(pat), pat)
}

/// Returns the first or last day of the input's year at its original precision.
fn year_boundary(value: &str, end: bool) -> TemplateEngineResult<String> {
    let recognized = parse_recognized(value)?;
    let shifted = recognized.point().year_boundary(end).map_err(date_error)?;
    let pat = shifted.precision.format_pattern(shifted.wall);
    format_with(shifted.wall.format(pat), pat)
}

fn start_of_week(value: &str) -> TemplateEngineResult<String> {
    week_boundary(value, false)
}

fn end_of_week(value: &str) -> TemplateEngineResult<String> {
    week_boundary(value, true)
}

fn start_of_year(value: &str) -> TemplateEngineResult<String> {
    year_boundary(value, false)
}

fn end_of_year(value: &str) -> TemplateEngineResult<String> {
    year_boundary(value, true)
}

/// `{{ value | date_add(n, unit="days") }}` adds `n` `unit`s to a piped
/// date/time string.
///
/// `unit` defaults to `"days"` and accepts any [`DurationUnit`] spelling
/// (e.g. `"years"`/`"y"`, `"months"`/`"mo"`, `"weeks"`/`"w"`, `"days"`/`"d"`,
/// `"hours"`/`"h"`, `"minutes"`/`"m"`, `"seconds"`/`"s"`, or `"ms"`).
/// Sub-day shifts on date-only values are civil and do not change the date.
///
/// `"years"`, `"months"`, `"weeks"`, and `"days"` preserve the civil wall
/// clock across a DST transition; the remaining units shift the exact
/// instant, so the wall clock can land earlier or later than a naive `n`-unit
/// shift (see [`date_shift_unit`]).
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
/// `unit` defaults to `"days"` and accepts any [`DurationUnit`] spelling
/// (e.g. `"years"`/`"y"`, `"months"`/`"mo"`, `"weeks"`/`"w"`, `"days"`/`"d"`,
/// `"hours"`/`"h"`, `"minutes"`/`"m"`, `"seconds"`/`"s"`, or `"ms"`).
/// Sub-day shifts on date-only values are civil and do not change the date.
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

/// Shifts `value` by `n` `unit`s, applying calendar or fixed-duration
/// semantics per `unit`.
///
/// - Inputs with a time component ([`Precision::DateTime`]) shift calendar
///   units on the local wall clock (preserving wall-clock hour across DST) and
///   sub-day units exactly on the instant.
/// - Civil inputs ([`Precision::Date`], [`Precision::YearMonth`]) shift
///   entirely on the civil wall clock. Sub-day shifts on date-only inputs leave
///   the calendar day unchanged.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not parseable or the shift
///   overflows chrono's representable range (see [`date_error`]).
fn date_shift_unit(
    value: &str,
    n: i64,
    unit: DurationUnit,
) -> TemplateEngineResult<String> {
    let recognized = parse_recognized(value)?;
    #[expect(
        clippy::match_same_arms,
        reason = "pat-exhaustive-enum: explicit match per variant forces a \
                  shift decision when adding precision"
    )]
    let shifted_point = match recognized.precision {
        Precision::YearMonth => {
            recognized.point().shift(n, unit).map_err(date_error)?
        }
        Precision::Date => {
            recognized.point().shift(n, unit).map_err(date_error)?
        }
        Precision::DateTime => {
            recognized.point().shift(n, unit).map_err(date_error)?
        }
    };
    let pat = shifted_point.precision.format_pattern(shifted_point.wall);
    format_with(shifted_point.wall.format(pat), pat)
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
    month_boundary(value, false)
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
    month_boundary(value, true)
}

/// `{{ value | weekday }}` returns `0` for Monday through `6` for Sunday.
/// When `n` is supplied, returns the formatted date of that weekday in
/// `value`'s week.
#[expect(
    clippy::needless_pass_by_value,
    reason = "minijinja Function trait extracts trailing Kwargs argument by \
              value"
)]
fn weekday(
    value: &str,
    n: Option<i64>,
    kwargs: Kwargs,
) -> TemplateEngineResult<Value> {
    let recognized = parse_recognized(value)?;
    if let Some(target_num) = n {
        let format = format_kwarg(&kwargs)?;
        let target_weekday = parse_weekday_num(target_num)?;
        let shifted = recognized
            .point()
            .weekday_point(target_weekday)
            .map_err(date_error)?;
        let pat = translate_pattern(format, shifted.wall)?;
        Ok(Value::from(format_with(shifted.wall.format(&pat), format)?))
    } else {
        kwargs.assert_all_used()?;
        Ok(Value::from(
            recognized.wall_or_utc().weekday().num_days_from_monday(),
        ))
    }
}

/// `{{ value | date_diff(other, unit="days") }}` returns the signed difference
/// from the piped value to `other`, positive when `other` is later.
///
/// The `unit` kwarg defaults to `"days"` and accepts `"years"`, `"months"`,
/// `"weeks"`, `"hours"`, `"minutes"`, `"seconds"`, or `"ms"`.
/// `"years"`/`"months"` are calendar counts: whole units elapsed, day-of-month
/// aware (see the date module's `signed_years_since`/`signed_months_since`),
/// always an `i64` regardless of input precision. The remaining units,
/// including `"weeks"`, `"ms"`, and the default `"days"`, are fixed-duration:
/// `f64` when both inputs carry a time component, otherwise an `i64` whole-unit
/// count.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` or `other` is not a parseable
///   date/time string (see [`ParsedDate::parse`]) or `unit` is not one of
///   [`DurationUnit::parse`]'s accepted names (see [`unit_kwarg`]).
/// - [`ErrorKind::InvalidOperation`] if the difference overflows chrono's
///   representable range (see [`date_error`]).
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
    let from = parse_recognized(value)?;
    let to = parse_recognized(other)?;

    let diff = from.point().diff(to.point(), unit).map_err(date_error)?;

    match diff {
        DateDiff::Whole(n) => Ok(Value::from(n)),
        DateDiff::Exact(f) => Ok(Value::from(f)),
    }
}

/// `{% if value is is_past %}` returns `true` when the piped date/time string
/// is before now.
///
/// A naive input is interpreted in the reader's local zone, so it compares
/// correctly against file timestamps.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not a parseable date/time
///   string; see [`parse_recognized`].
fn is_past(value: &str) -> TemplateEngineResult<bool> {
    Ok(parse_recognized(value)?.instant() < Utc::now())
}

/// `{% if value is is_future %}` mirrors [`is_past`] for future instants.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `value` is not a parseable date/time
///   string; see [`parse_recognized`].
fn is_future(value: &str) -> TemplateEngineResult<bool> {
    Ok(parse_recognized(value)?.instant() > Utc::now())
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

/// Translates a core [`DateError`] from the calendar owner into a render
/// error.
///
/// An overflow keeps the shared range message; a local-zone lookup failure
/// keeps its distinct diagnosis (a broken tz-data/OS environment, never a DST
/// ambiguity, which the resolver resolves deterministically) with the source
/// attached. Parse-shape failures cannot occur here: shift and diff receive
/// already-parsed values, but the match stays exhaustive per
/// `pat-exhaustive-enum`.
fn date_error(error: DateError) -> Error {
    match error {
        DateError::LocalZoneLookup {
            input,
        } => invalid_operation(
            format!("local timezone lookup failed for {input:?}"),
            DateError::LocalZoneLookup {
                input,
            },
        ),
        DateError::OutOfRange
        | DateError::Unparseable {
            ..
        }
        | DateError::InvalidYearDigits {
            ..
        } => date_out_of_range_error(),
    }
}

/// Builds the error for a `unit="..."` kwarg naming anything outside
/// [`DurationUnit::parse`]'s accepted unit names.
///
/// The expected-spellings hint comes from [`UNIT_HINT`], kept beside the
/// unit registry so the message cannot drift from what [`unit_kwarg`]
/// actually accepts.
///
/// Shared by [`date_add`], [`date_sub`], and [`date_diff`] via [`unit_kwarg`].
fn unknown_unit_error(unit: &str) -> Error {
    Error::new(
        ErrorKind::InvalidOperation,
        format!("unknown unit {unit:?} (expected {UNIT_HINT})"),
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

    /// Renders `{{ value | <filter_call> }}` under the registered
    /// environment and asserts the exact output.
    ///
    /// Shared by the per-filter `shifts_a_piped_date` tables.
    fn assert_filter_render(input: &str, filter_call: &str, expected: &str) {
        let rendered = env()
            .render_str(
                &format!("{{{{ value | {filter_call} }}}}"),
                minijinja::context! { value => input },
            )
            .expect("render succeeds");
        pretty_assertions::assert_eq!(rendered, expected);
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
    /// All three render inside one template so they read the clock in a
    /// single evaluation; a local-midnight crossing between separate renders
    /// would otherwise flake the one-day-apart assertions.
    mod today_tomorrow_yesterday {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[test]
        fn tomorrow_and_yesterday_are_one_day_from_today() {
            let rendered = env()
                .render_str(
                    "{{ date.yesterday() }}/{{ date.today() }}/{{ \
                     date.tomorrow() }}",
                    minijinja::context!(),
                )
                .expect("render succeeds");
            let mut parts = rendered.split('/');
            let yesterday: NaiveDate = parts
                .next()
                .expect("yesterday segment")
                .parse()
                .expect("valid ISO date");
            let today: NaiveDate = parts
                .next()
                .expect("today segment")
                .parse()
                .expect("valid ISO date");
            let tomorrow: NaiveDate = parts
                .next()
                .expect("tomorrow segment")
                .parse()
                .expect("valid ISO date");

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
        #[rstest]
        #[case::moment_iso_date("2026-07-29", "YYYY-MM-DD", "2026-07-29")]
        #[case::bracket_literal(
            "2026-07-29",
            "[Daily] YYYY-MM-DD",
            "Daily 2026-07-29"
        )]
        #[case::ordinal_day("2026-07-29", "Do MMMM YYYY", "29th July 2026")]
        #[case::weekday_and_month(
            "2026-07-29",
            "dddd, MMMM Do, YYYY",
            "Wednesday, July 29th, 2026"
        )]
        #[case::short_weekday_month(
            "2026-07-29",
            "ddd, MMM D, YY",
            "Wed, Jul 29, 26"
        )]
        #[case::time_tokens("2026-07-29T14:30:05", "HH:mm:ss", "14:30:05")]
        #[case::time_fractional(
            "2026-07-29T14:30:05.123",
            "HH:mm:ss.SSS",
            "14:30:05.123"
        )]
        fn formats_moment_tokens(
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
        fn rejects_unsupported_moment_token_with_token_and_dialect_name() {
            let error = env()
                .render_str(
                    r#"{{ "2026-07-29" | date_format("YYYY Q") }}"#,
                    minijinja::context!(),
                )
                .expect_err("unsupported moment token fails cleanly");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
            let msg = error.to_string();
            assert!(msg.contains("unsupported moment-dialect token `Q`"));
            assert!(msg.contains("supported tokens are"));
        }

        #[test]
        fn format_bindings_never_emit_percent_plus() {
            let sample_pattern = "YYYY-MM-DD HH:mm:ss [Daily]";
            let dt = chrono::NaiveDate::from_ymd_opt(2026, 7, 29)
                .unwrap()
                .and_hms_opt(14, 30, 0)
                .unwrap();
            let translated = translate_pattern(sample_pattern, dt).unwrap();
            assert!(!translated.contains("%+"));
        }
    }

    mod durationformat {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::compound_days_hours("3d 4h", "3 days, 4 hours")]
        #[case::single_day("1d", "1 day")]
        #[case::single_hour("1h", "1 hour")]
        #[case::minutes_seconds("1m 30s", "1 minute, 30 seconds")]
        #[case::zero_duration("0s", "0 seconds")]
        #[case::negative_duration("-3d 4h", "-3 days, 4 hours")]
        fn formats_compound_durations(
            #[case] input: &str,
            #[case] expected: &str,
        ) {
            let rendered = env()
                .render_str(
                    &format!(r#"{{{{ "{input}" | durationformat }}}}"#),
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, expected);
        }

        #[test]
        fn formats_duration_from_numeric_seconds() {
            let rendered = env()
                .render_str(
                    r"{{ 86400 | durationformat }}",
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "1 day");
        }

        #[test]
        fn formats_sub_second_duration_as_milliseconds() {
            let rendered = env()
                .render_str(
                    r#"{{ "500ms" | durationformat }}"#,
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "500 milliseconds");
        }

        #[test]
        fn rejects_invalid_duration_value() {
            let error = env()
                .render_str(
                    r#"{{ "not a duration" | durationformat }}"#,
                    minijinja::context!(),
                )
                .expect_err("invalid duration fails");

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
            assert_filter_render(input, filter_call, expected);
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
            assert_filter_render(input, filter_call, expected);
        }
    }

    mod add_and_sub_years {
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
            assert_filter_render(input, filter_call, expected);
        }
    }

    mod start_and_end_of_month {
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
            assert_filter_render(input, filter_call, expected);
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
        use crate::TzGuard;

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

        #[test]
        fn milliseconds_are_a_whole_count_for_date_only_inputs() {
            let rendered = env()
                .render_str(
                    r#"{{ "2026-07-23" | date_diff("2026-07-24", unit="ms") }}"#,
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "86400000");
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
        #[case::weeks_as_a_fixed_unit(
            "2026-07-23 00:00:00",
            "2026-08-06 00:00:00",
            "weeks",
            "2.0"
        )]
        #[case::milliseconds_as_a_fixed_unit(
            "2026-07-23 00:00:00",
            "2026-07-23T00:00:01.5",
            "ms",
            "1500.0"
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

        #[rstest]
        #[case::naive_input("2026-07-29T14:30:00")]
        #[case::explicit_offset_input("2026-07-29T12:30:00Z")]
        fn truncates_to_a_whole_hour_count_for_a_mixed_date_and_datetime_pair(
            #[case] other: &str,
        ) {
            TzGuard::set("Etc/GMT-2"); // UTC+02:00, no DST

            // Regression: the mixed-precision branch previously subtracted
            // whichever naive representations the two parsers happened to
            // produce; it must subtract civil wall clocks under the pinned
            // zone instead. A naive 14:30 local input and its 12:30Z
            // explicit-offset equivalent both resolve to the same 14:30
            // local wall clock, so both land on the same 14-hour truncated
            // difference (the mixed branch is i64, not f64).
            let rendered = env()
                .render_str(
                    r#"{{ "2026-07-29" | date_diff(other, unit="hours") }}"#,
                    minijinja::context! { other },
                )
                .expect("render succeeds");

            assert_eq!(rendered, "14");
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

    mod error_translation {
        use super::*;

        #[test]
        fn preserves_the_local_zone_lookup_diagnosis() {
            let error = date_error(DateError::LocalZoneLookup {
                input: "2026-03-08 02:30:00".into(),
            });
            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
            assert!(
                error.to_string().contains("local timezone lookup failed"),
                "expected the zone diagnosis, got {error}"
            );

            // The typed DateError survives in the source chain for callers
            // that inspect it, instead of being flattened into the generic
            // overflow message.
            let source = std::error::Error::source(&error)
                .expect("source chain present")
                .downcast_ref::<DateError>()
                .expect("DateError preserved as source");
            assert!(matches!(source, DateError::LocalZoneLookup { .. }));
        }

        #[test]
        fn keeps_the_range_message_for_an_overflow() {
            let error = date_error(DateError::OutOfRange);
            assert!(
                error
                    .to_string()
                    .contains("date arithmetic overflowed the supported range"),
                "unexpected detail: {error}"
            );
        }
    }
}
