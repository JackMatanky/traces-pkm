//! ISO-8601 and RFC-3339 date and date-time parsing, formatting, and
//! arithmetic.
//!
//! Single owner of date/time recognition, parsing, and formatting across the
//! crate. Every date-shaped string funnels through [`DateValue::parse_iso`];
//! every date-time-shaped string through [`DateTimeValue::parse_iso`].
//!
//! # Key types
//!
//! - [`DateValue`] - Parsed calendar date with no time-of-day component.
//! - [`DateTimeValue`] - Parsed UTC date-time instant.
//! - [`DateFormat`] - Format grammar for calendar date recognition.
//! - [`DateTimeFormat`] - Format grammar for date-time recognition.
//! - [`DateError`] - Error type for parse and formatting failures.

use std::{fmt, str::FromStr, time::SystemTime};

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use crate::duration::DurationValue;

/// [`DateValue`]'s canonical output format: `2026-07-29`.
pub(crate) const DEFAULT_DATE_FORMAT: &str = "%Y-%m-%d";

/// [`DateTimeValue`]'s canonical output format: `2026-07-29T14:30:00`.
pub(crate) const DEFAULT_DATETIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

/// Recognized date input format shapes tried in order by
/// [`DateValue::parse_iso`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DateFormat {
    /// Full ISO date: `2026-07-29`.
    Full,
    /// Year-month reduced precision: `2026-07`. Day defaults to 1.
    YearMonth,
}

impl DateFormat {
    /// Formats tried in order by [`DateValue::parse_iso`].
    pub(crate) const ALL: [Self; 2] = [Self::Full, Self::YearMonth];

    /// Returns the strftime pattern for this format, or `None` for
    /// [`Self::YearMonth`] which parses year and month components directly.
    #[must_use]
    pub(crate) const fn pattern(self) -> Option<&'static str> {
        match self {
            Self::Full => Some(DEFAULT_DATE_FORMAT),
            Self::YearMonth => None,
        }
    }

    /// Attempts to parse `s` according to this format.
    ///
    /// # Errors
    ///
    /// - [`chrono::ParseError`] if `s` does not match this format's expected
    ///   shape.
    pub(crate) fn parse(
        self,
        s: &str,
    ) -> Result<NaiveDate, chrono::ParseError> {
        match self.pattern() {
            Some(pat) => NaiveDate::parse_from_str(s, pat),
            None => Self::parse_year_month(s)
                .map_or_else(|| NaiveDate::parse_from_str(s, "%Y-%m"), Ok),
        }
    }

    /// Parses `"YYYY-MM"`, defaulting day to 1.
    ///
    /// chrono's `%Y-%m` format string alone fails with `NotEnough`, so this
    /// splits and parses the year and month integers directly.
    fn parse_year_month(s: &str) -> Option<NaiveDate> {
        let (year_str, month_str) = s.split_once('-')?;
        if year_str.len() != 4 || month_str.len() != 2 {
            return None;
        }
        let year: i32 = year_str.parse().ok()?;
        let month: u32 = month_str.parse().ok()?;
        NaiveDate::from_ymd_opt(year, month, 1)
    }
}

/// Recognized date-time input format shapes tried in order by
/// [`DateTimeValue::parse_iso`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DateTimeFormat {
    /// RFC 3339 format with offset: `2026-07-29T14:30:00Z`.
    Rfc3339,
    /// `T`-separated with fractional seconds: `2026-07-29T14:30:00.123`.
    IsoTFractional,
    /// `T`-separated with whole seconds: `2026-07-29T14:30:00`.
    IsoTSeconds,
    /// `T`-separated minute-only precision: `2026-07-29T14:30`.
    IsoTMinute,
    /// Space-separated with whole seconds: `2026-07-29 14:30:00`.
    IsoSpaceSeconds,
    /// Space-separated minute-only precision: `2026-07-29 14:30`.
    IsoSpaceMinute,
}

impl DateTimeFormat {
    /// Formats tried in order by [`DateTimeValue::parse_iso`] (most
    /// specific/unambiguous first).
    pub(crate) const ALL: [Self; 6] = [
        Self::Rfc3339,
        Self::IsoTFractional,
        Self::IsoTSeconds,
        Self::IsoTMinute,
        Self::IsoSpaceSeconds,
        Self::IsoSpaceMinute,
    ];

    /// Returns the strftime pattern for this format, or `None` for
    /// [`Self::Rfc3339`] which uses dedicated RFC 3339 parsing.
    #[must_use]
    pub(crate) const fn pattern(self) -> Option<&'static str> {
        match self {
            Self::Rfc3339 => None,
            Self::IsoTFractional => Some("%Y-%m-%dT%H:%M:%S%.f"),
            Self::IsoTSeconds => Some("%Y-%m-%dT%H:%M:%S"),
            Self::IsoTMinute => Some("%Y-%m-%dT%H:%M"),
            Self::IsoSpaceSeconds => Some("%Y-%m-%d %H:%M:%S"),
            Self::IsoSpaceMinute => Some("%Y-%m-%d %H:%M"),
        }
    }

    /// Attempts to parse `s` according to this format.
    ///
    /// # Errors
    ///
    /// - [`chrono::ParseError`] if `s` does not match this format's expected
    ///   shape.
    pub(crate) fn parse(
        self,
        s: &str,
    ) -> Result<DateTime<Utc>, chrono::ParseError> {
        match self.pattern() {
            None => {
                DateTime::parse_from_rfc3339(s).map(|dt| dt.with_timezone(&Utc))
            }
            Some(pat) => NaiveDateTime::parse_from_str(s, pat)
                .map(|naive| naive.and_utc()),
        }
    }
}

/// Parsed calendar date with no time-of-day component.
///
/// Wraps [`NaiveDate`] as a newtype, enforcing ISO-8601 recognition through
/// [`DateValue::parse_iso`]. All four-digit years are accepted; two-digit years
/// are rejected to prevent chrono's silent century misinterpretation.
#[repr(transparent)]
#[derive(
    Copy,
    Clone,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Deserialize,
    Serialize,
)]
pub struct DateValue(NaiveDate);

impl DateValue {
    /// Returns `true` if `s` begins with exactly 4 ASCII digits.
    ///
    /// Guards against chrono's lenient `%Y` specifier silently accepting a
    /// short year (`"26-08-22"` parses as year 26 CE, not rejected).
    /// Deliberately does not check what follows the digits: a well-formed
    /// 4-digit year with an unrecognized separator (`"2026/08/22"`) should
    /// reach the format cascade and fail as [`DateError::Unparseable`], not be
    /// misclassified as [`DateError::InvalidYearDigits`].
    #[must_use]
    pub(crate) fn has_four_digit_year(s: &str) -> bool {
        let bytes = s.as_bytes();
        bytes.get(0..4).is_some_and(|b| b.iter().all(u8::is_ascii_digit))
    }

    /// Returns `true` if `s`'s first 10 bytes have the shape `YYYY-MM-DD`
    /// (ASCII digits and hyphens in the right positions).
    ///
    /// Fast non-allocating pre-check for note-parser call sites before
    /// committing to [`DateValue::parse_iso`]. Does not validate calendar
    /// values (`"9999-99-99"` passes this check but fails the real parse); the
    /// real parse is always the authoritative decision.
    #[must_use]
    pub(crate) fn is_iso_shape(s: &str) -> bool {
        let bytes = s.as_bytes();
        bytes.len() >= 10
            && bytes.get(0..4).is_some_and(|b| b.iter().all(u8::is_ascii_digit))
            && bytes.get(4) == Some(&b'-')
            && bytes.get(5..7).is_some_and(|b| b.iter().all(u8::is_ascii_digit))
            && bytes.get(7) == Some(&b'-')
            && bytes
                .get(8..10)
                .is_some_and(|b| b.iter().all(u8::is_ascii_digit))
    }

    /// Parses an ISO-8601 date string (`YYYY-MM-DD` or `YYYY-MM`) using
    /// [`DateFormat::ALL`].
    ///
    /// # Errors
    ///
    /// - [`DateError::InvalidYearDigits`] if the year segment is not exactly 4
    ///   ASCII digits.
    /// - [`DateError::Unparseable`] if no accepted shape matches.
    pub(crate) fn parse_iso(s: &str) -> Result<Self, DateError> {
        let trimmed = s.trim();
        if !Self::has_four_digit_year(trimmed) {
            return Err(DateError::InvalidYearDigits {
                input: trimmed.into(),
            });
        }
        let [first, rest @ ..] = DateFormat::ALL;
        let mut result = first.parse(trimmed);
        for format in rest {
            if result.is_ok() {
                break;
            }
            result = format.parse(trimmed);
        }
        result.map(Self).map_err(|source| DateError::Unparseable {
            input: trimmed.into(),
            source,
        })
    }

    /// Formats this date as `YYYY-MM-DD`.
    #[inline]
    #[must_use]
    pub(crate) fn to_date_string(self) -> String {
        self.to_string()
    }

    /// Formats this date with an arbitrary strftime `pattern`.
    ///
    /// # Errors
    ///
    /// - [`DateError::InvalidPattern`] if `pattern` is not a strftime specifier
    ///   this value can render.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; documented deliberate \
                      API for future arbitrary-pattern template formatting, \
                      mirrors DateTimeValue::format_with"
        )
    )]
    pub(crate) fn format_with(
        self,
        pattern: &str,
    ) -> Result<String, DateError> {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(pattern.len().max(32));
        write!(out, "{}", self.0.format(pattern)).map_err(|_fmt_error| {
            DateError::InvalidPattern {
                pattern: pattern.into(),
            }
        })?;
        Ok(out)
    }

    /// Returns the wrapped [`NaiveDate`].
    #[must_use]
    pub(crate) const fn into_inner(self) -> NaiveDate {
        self.0
    }
}

impl fmt::Display for DateValue {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.format(DEFAULT_DATE_FORMAT))
    }
}

impl From<DateValue> for NaiveDate {
    #[inline]
    fn from(value: DateValue) -> Self {
        value.into_inner()
    }
}

impl FromStr for DateValue {
    type Err = DateError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_iso(s)
    }
}

/// Parsed UTC date-time instant.
///
/// Wraps [`DateTime<Utc>`] as a newtype, enforcing ISO-8601/RFC-3339
/// recognition through [`DateTimeValue::parse_iso`]. All values are
/// UTC-normalized; offset-bearing input is converted to UTC at parse time.
#[repr(transparent)]
#[derive(
    Copy,
    Clone,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Deserialize,
    Serialize,
)]
pub struct DateTimeValue(DateTime<Utc>);

impl DateTimeValue {
    /// Returns the current UTC date-time.
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "no current caller outside tests; used only by \
                      FileBase::for_test"
        )
    )]
    #[inline]
    #[must_use]
    #[allow(dead_code, reason = "utility constructor used only in tests")]
    pub(crate) fn now() -> Self {
        Self(Utc::now())
    }

    /// Parses an RFC-3339 or ISO-8601 date-time string using
    /// [`DateTimeFormat::ALL`].
    ///
    /// # Errors
    ///
    /// - [`DateError::InvalidYearDigits`] if the year segment is not exactly 4
    ///   ASCII digits.
    /// - [`DateError::Unparseable`] if no accepted shape matches.
    pub(crate) fn parse_iso(s: &str) -> Result<Self, DateError> {
        let trimmed = s.trim();
        if !DateValue::has_four_digit_year(trimmed) {
            return Err(DateError::InvalidYearDigits {
                input: trimmed.into(),
            });
        }
        let [first, rest @ ..] = DateTimeFormat::ALL;
        let mut result = first.parse(trimmed);
        for format in rest {
            if result.is_ok() {
                break;
            }
            result = format.parse(trimmed);
        }
        result.map(Self).map_err(|source| DateError::Unparseable {
            input: trimmed.into(),
            source,
        })
    }

    /// Formats this date-time as an RFC 3339 date and time with a UTC offset
    /// (always `+00:00`).
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; documented deliberate \
                      API in index-query#05; to_datetime_string (used by \
                      field resolution's DateTime variant) and \
                      DateValue::to_date_string (used by field resolution's \
                      Date variant) cover the shipping formatting paths"
        )
    )]
    #[inline]
    #[must_use]
    pub(crate) fn to_offset_string(self) -> String {
        self.0.to_rfc3339()
    }

    /// Formats this date-time without a UTC offset, including fractional
    /// seconds only when this value carries a nonzero nanosecond component
    /// (round-trips [`DateTimeFormat::IsoTFractional`] input instead of
    /// silently truncating it).
    #[inline]
    #[must_use]
    pub(crate) fn to_datetime_string(self) -> String {
        self.to_string()
    }

    /// Formats this date-time as a bare date without time or offset.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; DateValue's own \
                      to_date_string covers field resolution's Date-variant \
                      formatting; kept for API symmetry with \
                      to_datetime_string"
        )
    )]
    #[inline]
    #[must_use]
    pub(crate) fn to_date_string(self) -> String {
        self.0.format(DEFAULT_DATE_FORMAT).to_string()
    }

    /// Formats this date-time as a bare time-of-day component.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; documented deliberate \
                      API in index-query#05; to_datetime_string (used by \
                      field resolution's DateTime variant) and \
                      DateValue::to_date_string (used by field resolution's \
                      Date variant) cover the shipping formatting paths"
        )
    )]
    #[inline]
    #[must_use]
    pub(crate) fn to_time_string(self) -> String {
        self.0.format("%H:%M:%S").to_string()
    }

    /// Returns a new date-time truncated to the start of the UTC day.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; field resolution \
                      truncates via DateTimeValue::date() instead"
        )
    )]
    #[inline]
    #[must_use]
    pub(crate) fn start_of_day(self) -> Self {
        Self::from(self.date())
    }

    /// Returns the calendar date component, discarding time-of-day.
    #[inline]
    #[must_use]
    pub(crate) fn date(self) -> DateValue {
        DateValue(self.0.date_naive())
    }

    /// Formats this date-time with an arbitrary strftime `pattern`.
    ///
    /// # Errors
    ///
    /// - [`DateError::InvalidPattern`] if `pattern` is not a strftime specifier
    ///   this value can render.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; documented deliberate \
                      API for future arbitrary-pattern template formatting"
        )
    )]
    pub(crate) fn format_with(
        self,
        pattern: &str,
    ) -> Result<String, DateError> {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(pattern.len().max(32));
        write!(out, "{}", self.0.format(pattern)).map_err(|_fmt_error| {
            DateError::InvalidPattern {
                pattern: pattern.into(),
            }
        })?;
        Ok(out)
    }

    /// Adds `duration` to this date-time.
    ///
    /// Converts `duration` via [`TryFrom<DurationValue>`] for [`TimeDelta`].
    /// Returns [`None`] on arithmetic overflow or a non-finite duration.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; template engine's \
                      date_shift_unit keeps its own calendar arithmetic \
                      rather than delegating here (see Step 11's design \
                      note); kept for future direct-arithmetic callers"
        )
    )]
    #[must_use]
    pub(crate) fn checked_add(self, duration: DurationValue) -> Option<Self> {
        let delta = TimeDelta::try_from(duration).ok()?;
        self.0.checked_add_signed(delta).map(Self)
    }

    /// Subtracts `duration` from this date-time.
    ///
    /// Converts `duration` via [`TryFrom<DurationValue>`] for [`TimeDelta`].
    /// Returns [`None`] on arithmetic overflow or a non-finite duration.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; see checked_add"
        )
    )]
    #[must_use]
    pub(crate) fn checked_sub(self, duration: DurationValue) -> Option<Self> {
        let delta = TimeDelta::try_from(duration).ok()?;
        self.0.checked_sub_signed(delta).map(Self)
    }

    /// Compares this date-time against `date`, coercing `date` to midnight UTC.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; sort/filter \
                      comparisons currently compare within one already- \
                      normalized SortKey::DateTime variant, not across \
                      Date/DateTime directly"
        )
    )]
    #[must_use]
    pub(crate) fn cmp_date(self, date: DateValue) -> std::cmp::Ordering {
        self.0.cmp(&Self::from(date).0)
    }

    /// Returns `true` if this date-time is exactly midnight UTC on `date`.
    #[must_use]
    pub(crate) fn is_equal_to_date(self, date: DateValue) -> bool {
        self.0 == Self::from(date).0
    }

    /// Returns the wrapped [`DateTime<Utc>`].
    #[must_use]
    pub(crate) const fn into_inner(self) -> DateTime<Utc> {
        self.0
    }
}

impl fmt::Display for DateTimeValue {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use chrono::Timelike as _;
        let format = if self.0.nanosecond() == 0 {
            DEFAULT_DATETIME_FORMAT
        } else {
            "%Y-%m-%dT%H:%M:%S%.f"
        };
        write!(f, "{}", self.0.format(format))
    }
}

impl From<SystemTime> for DateTimeValue {
    #[inline]
    fn from(time: SystemTime) -> Self {
        Self(DateTime::<Utc>::from(time))
    }
}

impl From<SystemTime> for DateValue {
    #[inline]
    fn from(time: SystemTime) -> Self {
        let dt: DateTime<Utc> = time.into();
        Self(dt.date_naive())
    }
}

impl From<DateTime<Utc>> for DateTimeValue {
    #[inline]
    fn from(dt: DateTime<Utc>) -> Self {
        Self(dt)
    }
}

impl From<DateTimeValue> for DateTime<Utc> {
    #[inline]
    fn from(value: DateTimeValue) -> Self {
        value.into_inner()
    }
}

/// Promotes a [`DateValue`] to a [`DateTimeValue`] at midnight UTC.
impl From<DateValue> for DateTimeValue {
    #[inline]
    fn from(date: DateValue) -> Self {
        Self(date.0.and_time(NaiveTime::MIN).and_utc())
    }
}

impl FromStr for DateTimeValue {
    type Err = DateError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_iso(s)
    }
}

/// Error type for date/date-time parse and formatting failures.
///
/// Returned by [`DateValue::parse_iso`], [`DateTimeValue::parse_iso`],
/// [`DateValue::format_with`], and [`DateTimeValue::format_with`].
#[derive(Debug, Clone, Eq, PartialEq, thiserror::Error)]
pub enum DateError {
    /// No accepted date/time shape matched `input`.
    ///
    /// Wraps the last-attempted format's [`chrono::ParseError`].
    #[error("`{input}` is not a recognized date/time: {source}")]
    Unparseable {
        input: Box<str>,
        #[source]
        source: chrono::ParseError,
    },
    /// `input`'s year segment is not exactly 4 ASCII digits.
    ///
    /// chrono's `%Y` accepts fewer digits, silently misreading the year.
    #[error("`{input}` does not have a 4-digit year")]
    InvalidYearDigits {
        input: Box<str>,
    },
    /// `pattern` is not a valid strftime specifier.
    #[error("`{pattern}` is not a valid format pattern")]
    InvalidPattern {
        pattern: Box<str>,
    },
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone as _;

    use super::*;

    fn fixed_datetime() -> DateTimeValue {
        DateTimeValue(
            Utc.with_ymd_and_hms(2026, 7, 29, 14, 30, 5).single().expect(
                "2026-07-29 14:30:05 UTC is a valid, unambiguous instant",
            ),
        )
    }

    mod constructor {
        use super::*;

        #[test]
        fn now_produces_an_instant_close_to_the_system_clock() {
            let before = Utc::now();
            let produced = DateTimeValue::now();
            let after = Utc::now();
            assert!(produced.into_inner() >= before);
            assert!(produced.into_inner() <= after);
        }
    }

    mod formatting {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn renders_to_offset_string_with_the_utc_offset() {
            assert_eq!(
                fixed_datetime().to_offset_string(),
                "2026-07-29T14:30:05+00:00"
            );
        }

        #[test]
        fn renders_to_datetime_string_without_the_offset() {
            assert_eq!(
                fixed_datetime().to_datetime_string(),
                "2026-07-29T14:30:05"
            );
        }

        #[test]
        fn round_trips_fractional_seconds_in_to_datetime_string() {
            let with_nanos = DateTimeValue(
                fixed_datetime().into_inner()
                    + chrono::TimeDelta::milliseconds(123),
            );
            assert_eq!(
                with_nanos.to_datetime_string(),
                "2026-07-29T14:30:05.123"
            );
        }

        #[test]
        fn renders_to_date_string_without_the_time() {
            assert_eq!(fixed_datetime().to_date_string(), "2026-07-29");
        }

        #[test]
        fn renders_to_time_string_without_the_date() {
            assert_eq!(fixed_datetime().to_time_string(), "14:30:05");
        }

        #[test]
        fn truncates_to_midnight_utc_in_start_of_day() {
            assert_eq!(
                fixed_datetime().start_of_day(),
                DateTimeValue::from(
                    DateValue::parse_iso("2026-07-29").expect("valid date")
                )
            );
        }

        #[test]
        fn extracts_the_calendar_date_discarding_time_of_day() {
            let extracted = fixed_datetime().date();
            let expected =
                DateValue::parse_iso("2026-07-29").expect("valid date");
            assert_eq!(extracted, expected);
        }

        #[test]
        fn round_trips_a_date_value_through_to_date_string_and_parse_iso() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let reparsed = DateValue::parse_iso(&date.to_date_string())
                .expect("formatted output re-parses");
            assert_eq!(reparsed, date);
        }

        #[test]
        fn round_trips_a_datetime_value_through_to_datetime_string_and_parse_iso()
         {
            let value = fixed_datetime();
            let reparsed =
                DateTimeValue::parse_iso(&value.to_datetime_string())
                    .expect("formatted output re-parses");
            assert_eq!(reparsed, value);
        }
    }

    mod parsing {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[test]
        fn covers_all_six_datetime_format_variants() {
            assert_eq!(DateTimeFormat::ALL.len(), 6);
        }

        #[test]
        fn covers_both_date_format_variants() {
            assert_eq!(DateFormat::ALL.len(), 2);
        }

        #[test]
        fn detects_the_iso_shape_byte_pattern() {
            assert!(DateValue::is_iso_shape("2026-07-29"));
            assert!(!DateValue::is_iso_shape("2026-7-29"));
            assert!(!DateValue::is_iso_shape("2026/07/29"));
        }

        #[rstest]
        #[case::has_offset_z("2026-07-29T14:30:00Z")]
        #[case::has_numeric_offset("2026-07-29T14:30:00+00:00")]
        #[case::has_fractional_seconds("2026-07-29T14:30:00.123")]
        #[case::t_separated_with_seconds("2026-07-29T14:30:00")]
        #[case::t_separated_minute_only("2026-07-29T14:30")]
        #[case::space_separated_with_seconds("2026-07-29 14:30:00")]
        #[case::space_separated_minute_only("2026-07-29 14:30")]
        fn accepts_every_datetime_shape(#[case] input: &str) {
            assert!(
                DateTimeValue::parse_iso(input).is_ok(),
                "{input} should parse"
            );
        }

        #[test]
        fn accepts_full_iso_date_shape() {
            assert!(DateValue::parse_iso("2026-07-29").is_ok());
        }

        #[test]
        fn accepts_year_month_reduced_precision_date_shape() {
            let year_month =
                DateValue::parse_iso("2026-08").expect("year-month parses");
            let expected = DateValue(
                NaiveDate::from_ymd_opt(2026, 8, 1).expect("valid date"),
            );
            assert_eq!(year_month, expected);
        }

        #[test]
        fn accepts_missing_leading_zeros() {
            assert!(DateValue::parse_iso("2026-8-22").is_ok());
        }

        #[rstest]
        #[case::too_short("202")]
        #[case::empty("")]
        #[case::non_digit_year("abcd-01-01")]
        fn rejects_a_missing_four_digit_year(#[case] input: &str) {
            assert!(!DateValue::has_four_digit_year(input));
        }

        #[test]
        fn accepts_a_five_digit_year_prefix_as_having_four_digits() {
            // has_four_digit_year only checks the first 4 bytes; a longer
            // numeric prefix still passes this guard and is rejected later, by
            // the format cascade, as Unparseable rather than InvalidYearDigits.
            assert!(DateValue::has_four_digit_year("20265-01-01"));
            assert!(matches!(
                DateValue::parse_iso("20265-01-01"),
                Err(DateError::Unparseable { .. })
            ));
        }

        #[test]
        fn date_value_rejects_a_short_year_with_invalid_year_digits() {
            let result = DateValue::parse_iso("26-08-22");
            assert!(matches!(result, Err(DateError::InvalidYearDigits { .. })));
        }

        #[test]
        fn datetime_value_rejects_a_short_year_with_invalid_year_digits() {
            let result = DateTimeValue::parse_iso("26-08-22T14:30:00");
            assert!(matches!(result, Err(DateError::InvalidYearDigits { .. })));
        }

        #[rstest]
        #[case::invalid_month_number("2026-13")]
        #[case::non_numeric_month("2026-ab")]
        #[case::single_digit_month("2026-7")]
        fn rejects_a_malformed_year_month_as_unparseable(#[case] input: &str) {
            assert!(matches!(
                DateValue::parse_iso(input),
                Err(DateError::Unparseable { .. })
            ));
        }

        #[test]
        fn rejects_an_unrecognized_date_shape_as_unparseable() {
            let result = DateValue::parse_iso("2026/08/22");
            assert!(matches!(result, Err(DateError::Unparseable { .. })));
        }

        #[test]
        fn rejects_an_unrecognized_datetime_shape_as_unparseable() {
            let result = DateTimeValue::parse_iso("2026-07-29Tinvalid");
            assert!(matches!(result, Err(DateError::Unparseable { .. })));
        }

        #[test]
        fn rejects_an_invalid_pattern_in_datetime_value_format_with() {
            let result = fixed_datetime().format_with("%Q");
            assert!(matches!(result, Err(DateError::InvalidPattern { .. })));
        }

        #[test]
        fn renders_a_custom_pattern_in_date_value_format_with() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let rendered = date.format_with("%d/%m/%Y").expect("valid pattern");
            assert_eq!(rendered, "29/07/2026");
        }

        #[test]
        fn renders_a_custom_pattern_in_datetime_value_format_with() {
            let rendered = fixed_datetime()
                .format_with("%d/%m/%Y %H:%M")
                .expect("valid pattern");
            assert_eq!(rendered, "29/07/2026 14:30");
        }

        #[test]
        fn rejects_an_invalid_pattern_in_date_value_format_with() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let result = date.format_with("%Q");
            assert!(matches!(result, Err(DateError::InvalidPattern { .. })));
        }

        #[test]
        fn date_value_parses_via_from_str() {
            let date: DateValue = "2026-07-29".parse().expect("valid date");
            let expected =
                DateValue::parse_iso("2026-07-29").expect("valid date");
            assert_eq!(date, expected);
        }

        #[test]
        fn datetime_value_parses_via_from_str() {
            let datetime: DateTimeValue =
                "2026-07-29T14:30:00".parse().expect("valid datetime");
            let expected = DateTimeValue::parse_iso("2026-07-29T14:30:00")
                .expect("valid datetime");
            assert_eq!(datetime, expected);
        }

        #[test]
        fn datetime_format_pattern_maps_each_variant_to_its_strftime_spec() {
            assert_eq!(DateTimeFormat::Rfc3339.pattern(), None);
            assert_eq!(
                DateTimeFormat::IsoTSeconds.pattern(),
                Some("%Y-%m-%dT%H:%M:%S")
            );
            assert_eq!(
                DateTimeFormat::IsoSpaceMinute.pattern(),
                Some("%Y-%m-%d %H:%M")
            );
        }

        #[test]
        fn date_format_pattern_maps_each_variant_to_its_strftime_spec() {
            assert_eq!(DateFormat::Full.pattern(), Some(DEFAULT_DATE_FORMAT));
            assert_eq!(DateFormat::YearMonth.pattern(), None);
        }
    }

    mod arithmetic {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn checked_add_advances_the_instant_by_the_duration() {
            let base = fixed_datetime();
            let one_hour = DurationValue::parse("1h").expect("valid duration");
            let forward = base.checked_add(one_hour).expect("no overflow");
            assert_eq!(
                forward,
                DateTimeValue(base.into_inner() + TimeDelta::hours(1))
            );
        }

        #[test]
        fn checked_sub_rewinds_the_instant_by_the_duration() {
            let base = fixed_datetime();
            let one_hour = DurationValue::parse("1h").expect("valid duration");
            let back = base.checked_sub(one_hour).expect("no overflow");
            assert_eq!(
                back,
                DateTimeValue(base.into_inner() - TimeDelta::hours(1))
            );
        }

        #[test]
        fn overflows_near_the_representable_range_in_checked_add() {
            let near_max = DateTimeValue(DateTime::<Utc>::MAX_UTC);
            let one_second =
                DurationValue::parse("1s").expect("valid duration");
            assert_eq!(near_max.checked_add(one_second), None);
        }

        #[test]
        fn underflows_near_the_representable_range_in_checked_sub() {
            let near_min = DateTimeValue(DateTime::<Utc>::MIN_UTC);
            let one_second =
                DurationValue::parse("1s").expect("valid duration");
            assert_eq!(near_min.checked_sub(one_second), None);
        }
    }

    mod comparison {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn requires_exact_midnight_in_is_equal_to_date() {
            let midnight = DateTimeValue::from(
                DateValue::parse_iso("2026-07-29").expect("valid date"),
            );
            assert!(midnight.is_equal_to_date(
                DateValue::parse_iso("2026-07-29").expect("valid date")
            ));
            assert!(!fixed_datetime().is_equal_to_date(
                DateValue::parse_iso("2026-07-29").expect("valid date")
            ));
        }

        #[test]
        fn cmp_date_reports_greater_when_the_instant_is_after_midnight() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            assert_eq!(
                fixed_datetime().cmp_date(date),
                std::cmp::Ordering::Greater
            );
        }

        #[test]
        fn cmp_date_reports_equal_at_exact_midnight() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let midnight = DateTimeValue::from(date);
            assert_eq!(midnight.cmp_date(date), std::cmp::Ordering::Equal);
        }

        #[test]
        fn cmp_date_reports_less_when_the_instant_is_before_midnight() {
            let date = DateValue::parse_iso("2026-07-30").expect("valid date");
            assert_eq!(
                fixed_datetime().cmp_date(date),
                std::cmp::Ordering::Less
            );
        }
    }

    mod conversions {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn converts_from_system_time() {
            let system_time = std::time::SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(1_000);
            let converted = DateTimeValue::from(system_time);
            assert_eq!(
                converted,
                DateTimeValue::parse_iso("1970-01-01T00:16:40")
                    .expect("valid datetime")
            );
        }

        #[test]
        fn converts_from_system_time_to_date_value() {
            let system_time = std::time::SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(1_000);
            let converted = DateValue::from(system_time);
            assert_eq!(
                converted,
                DateValue::parse_iso("1970-01-01").expect("valid date")
            );
        }

        #[test]
        fn round_trips_through_chrono_date_time_utc() {
            let chrono_dt = fixed_datetime().into_inner();
            let converted = DateTimeValue::from(chrono_dt);
            assert_eq!(converted, fixed_datetime());
            let back: DateTime<Utc> = converted.into();
            assert_eq!(back, chrono_dt);
        }

        #[test]
        fn promotes_a_date_to_midnight_utc_via_from_trait() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let promoted = DateTimeValue::from(date);
            assert_eq!(promoted, fixed_datetime().start_of_day());
        }

        #[test]
        fn converts_date_value_into_naive_date_via_from_trait() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let naive: NaiveDate = date.into();
            assert_eq!(
                naive,
                NaiveDate::from_ymd_opt(2026, 7, 29).expect("valid date")
            );
        }

        #[test]
        fn extracts_wrapped_naive_date_via_into_inner() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let naive: NaiveDate = date.into();
            assert_eq!(date.into_inner(), naive);
        }
    }

    mod ordering {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn compares_dates_chronologically() {
            let earlier =
                DateValue::parse_iso("2026-01-01").expect("valid date");
            let later = DateValue::parse_iso("2026-12-31").expect("valid date");
            assert!(earlier < later);
            assert!(later > earlier);
            assert_eq!(earlier.cmp(&earlier), std::cmp::Ordering::Equal);
        }

        #[test]
        fn compares_datetimes_chronologically() {
            let earlier = DateTimeValue::parse_iso("2026-01-01T00:00:00")
                .expect("valid datetime");
            let later = fixed_datetime();
            assert!(earlier < later);
            assert!(later > earlier);
            assert_eq!(earlier.cmp(&earlier), std::cmp::Ordering::Equal);
        }
    }

    mod display_and_traits {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn displays_a_date_value_as_its_canonical_string() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            assert_eq!(date.to_string(), "2026-07-29");
        }

        #[test]
        fn displays_a_datetime_value_as_its_canonical_string() {
            assert_eq!(fixed_datetime().to_string(), "2026-07-29T14:30:05");
        }

        #[test]
        fn displays_unparseable_error_naming_the_input_and_source() {
            let err = DateValue::parse_iso("2026/08/22")
                .expect_err("unrecognized shape");
            assert_eq!(
                err.to_string(),
                "`2026/08/22` is not a recognized date/time: input contains \
                 invalid characters"
            );
        }

        #[test]
        fn displays_invalid_year_digits_error_naming_the_input() {
            let err = DateValue::parse_iso("26-08-22")
                .expect_err("short year rejected");
            assert_eq!(
                err.to_string(),
                "`26-08-22` does not have a 4-digit year"
            );
        }

        #[test]
        fn displays_invalid_pattern_error_naming_the_pattern() {
            let err = fixed_datetime()
                .format_with("%Q")
                .expect_err("invalid strftime specifier");
            assert_eq!(err.to_string(), "`%Q` is not a valid format pattern");
        }

        #[test]
        fn round_trips_a_date_value_through_json() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let json = serde_json::to_string(&date).expect("serializable");
            let restored: DateValue =
                serde_json::from_str(&json).expect("deserializable");
            assert_eq!(restored, date);
        }

        #[test]
        fn round_trips_a_datetime_value_through_json() {
            let json =
                serde_json::to_string(&fixed_datetime()).expect("serializable");
            let restored: DateTimeValue =
                serde_json::from_str(&json).expect("deserializable");
            assert_eq!(restored, fixed_datetime());
        }

        #[test]
        fn is_usable_as_a_hash_set_key() {
            let mut set = std::collections::HashSet::new();
            set.insert(DateValue::parse_iso("2026-07-29").expect("valid date"));
            set.insert(DateValue::parse_iso("2026-07-29").expect("valid date"));
            set.insert(DateValue::parse_iso("2026-07-30").expect("valid date"));
            assert_eq!(set.len(), 2);
        }
    }
}
