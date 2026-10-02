//! ISO-8601 and RFC-3339 date and date-time parsing, formatting, and
//! arithmetic.
//!
//! Single owner of date/time recognition, parsing, and formatting across the
//! crate. Every date-shaped string funnels through [`DateValue::parse_iso`];
//! every date-time-shaped string through [`DateTimeValue::parse_iso`].
//!
//! # Clock doctrine
//!
//! A naive datetime means what a human means: it is interpreted in the
//! process's local zone and stored as UTC (see [`local_naive_to_utc`]), while
//! a date-only value stays a zone-free civil date. Instants render as the
//! local wall clock for humans and travel as UTC for storage and comparison.
//!
//! # Key types
//!
//! - [`DateValue`] - Parsed calendar date with no time-of-day component.
//! - [`DateTimeValue`] - Parsed UTC date-time instant.
//! - [`DateFormat`] - Format grammar for calendar date recognition.
//! - [`DateTimeFormat`] - Format grammar for date-time recognition.
//! - [`DateError`] - Error type for parse and formatting failures.

use std::{borrow::Cow, fmt, str::FromStr, time::SystemTime};

use chrono::{
    DateTime, Datelike as _, Days, FixedOffset, Local, MappedLocalTime, Months,
    NaiveDate, NaiveDateTime, NaiveTime, Offset as _, SecondsFormat, TimeDelta,
    TimeZone as _, Utc,
};
use num_traits::ToPrimitive as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::duration::{DurationUnit, DurationValue};

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

    /// Attempts to parse `s` according to this format's shape.
    ///
    /// A naive input attaches UTC as a placeholder; shape matching never
    /// consults the local zone. [`DateTimeValue::parse_iso`] resolves the
    /// matched naive value through [`local_naive_to_utc`] after the cascade.
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

/// Resolves a naive local wall-clock datetime to a UTC instant under the
/// crate's DST doctrine.
///
/// A wall-clock time written without a zone means what a human means: it is
/// interpreted in the process's local zone and stored as UTC. Ambiguous
/// fall-back times resolve to their earliest occurrence and nonexistent
/// spring-forward gap times shift forward by the gap, the convention
/// Temporal's `'compatible'` disambiguation (RFC 5545) and jiff's
/// [`Disambiguation::Compatible`] implement; an ambiguity or a gap never
/// fails to parse. Only a local timezone lookup failure (a broken tz-data or
/// OS environment) surfaces as an error.
///
/// On wasm, chrono's `Local` reports every local time as unambiguous
/// ([chrono#1701]), so the ambiguous arm is unreachable there; that target is
/// out of scope.
///
/// # Errors
///
/// - [`DateError::LocalZoneLookup`] if no local time within the 25-hour
///   backward search (nor `wall` itself) resolves, which indicates a tz-data/OS
///   lookup failure rather than a DST boundary.
///
/// [`Disambiguation::Compatible`]:
///     https://docs.rs/jiff/latest/jiff/tz/enum.Disambiguation.html
/// [chrono#1701]: https://github.com/chronotope/chrono/issues/1701
fn local_naive_to_utc(wall: NaiveDateTime) -> Result<DateTime<Utc>, DateError> {
    let zone_lookup = || DateError::LocalZoneLookup {
        input: wall.to_string().into(),
    };
    // Offset application goes through the checked forms: they return `None`
    // instead of panicking if the instant lands outside `NaiveDateTime`'s
    // range (unreachable for the 4-digit-year inputs the parsers accept).
    match Local.offset_from_local_datetime(&wall) {
        MappedLocalTime::Single(offset) => {
            let instant =
                wall.checked_sub_offset(offset).ok_or_else(zone_lookup)?;
            Ok(instant.and_utc())
        }
        MappedLocalTime::Ambiguous(a, b) => {
            // chrono orders the pair by offset value (tzfile data) or by
            // transition side (POSIX rules); the earliest occurrence is
            // always the one with the larger offset, since instant =
            // wall - offset.
            let earliest = if a.local_minus_utc() >= b.local_minus_utc() {
                a
            } else {
                b
            };
            let instant =
                wall.checked_sub_offset(earliest).ok_or_else(zone_lookup)?;
            Ok(instant.and_utc())
        }
        MappedLocalTime::None => {
            // A DST gap resolves `None` across the whole skipped span, so a
            // one-second probe would still land inside it. The gap resolver
            // finds the pre-transition offset; interpreting the wall clock
            // with it shifts the gap time forward by exactly the gap.
            let offset = resolve_gap_offset(wall, &zone_lookup)?;
            let instant =
                wall.checked_sub_offset(offset).ok_or_else(zone_lookup)?;
            Ok(instant.and_utc())
        }
    }
}

/// Finds the offset in effect just before a DST gap by stepping back one
/// hour at a time from `wall` (the widest recorded gap is 24 hours).
///
/// # Errors
///
/// - [`DateError::LocalZoneLookup`] if no nearby local time resolves within the
///   25-hour backward search, indicating a broken tz-data/OS lookup rather than
///   a gap; never a silent shift.
fn resolve_gap_offset(
    wall: NaiveDateTime,
    zone_lookup: &impl Fn() -> DateError,
) -> Result<FixedOffset, DateError> {
    let hour = TimeDelta::try_hours(1).ok_or_else(zone_lookup)?;
    let mut probe = wall;
    for _ in 0..25 {
        probe = probe.checked_sub_signed(hour).ok_or_else(zone_lookup)?;
        if let MappedLocalTime::Single(offset)
        | MappedLocalTime::Ambiguous(offset, _) =
            Local.offset_from_local_datetime(&probe)
        {
            return Ok(offset);
        }
    }
    Err(zone_lookup())
}

/// Parsed calendar date with no time-of-day component.
///
/// Wraps [`NaiveDate`] as a newtype, enforcing ISO-8601 recognition.
/// All four-digit years are accepted; two-digit years
/// are rejected to prevent chrono's silent century misinterpretation.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
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

    /// Shifts this civil date by `n` `unit`s.
    ///
    /// Pure civil arithmetic: zone-free dates never touch the DST resolver.
    ///
    /// # Errors
    ///
    /// - [`DateError::OutOfRange`] if arithmetic overflows representable
    ///   bounds.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "part of DateValue surface; called in tests and by \
                      future query temporal functions"
        )
    )]
    pub(crate) fn shift(
        self,
        n: i64,
        unit: DurationUnit,
    ) -> Result<Self, DateError> {
        match unit {
            DurationUnit::Year => {
                let months = n.checked_mul(12).ok_or(DateError::OutOfRange)?;
                shift_date_months(self.0, months).map(Self)
            }
            DurationUnit::Month => shift_date_months(self.0, n).map(Self),
            DurationUnit::Week => {
                let days = n.checked_mul(7).ok_or(DateError::OutOfRange)?;
                let days_u64 = days.unsigned_abs();
                let shifted = if days >= 0 {
                    self.0.checked_add_days(Days::new(days_u64))
                } else {
                    self.0.checked_sub_days(Days::new(days_u64))
                }
                .ok_or(DateError::OutOfRange)?;
                Ok(Self(shifted))
            }
            DurationUnit::Day => {
                let days_u64 = n.unsigned_abs();
                let shifted = if n >= 0 {
                    self.0.checked_add_days(Days::new(days_u64))
                } else {
                    self.0.checked_sub_days(Days::new(days_u64))
                }
                .ok_or(DateError::OutOfRange)?;
                Ok(Self(shifted))
            }
            DurationUnit::Millisecond => {
                let wall =
                    self.0.and_hms_opt(0, 0, 0).ok_or(DateError::OutOfRange)?;
                let delta = chrono::Duration::try_milliseconds(n)
                    .ok_or(DateError::OutOfRange)?;
                let shifted_wall = wall
                    .checked_add_signed(delta)
                    .ok_or(DateError::OutOfRange)?;
                Ok(Self(shifted_wall.date()))
            }
            DurationUnit::Second
            | DurationUnit::Minute
            | DurationUnit::Hour => {
                let wall =
                    self.0.and_hms_opt(0, 0, 0).ok_or(DateError::OutOfRange)?;
                let secs = unit
                    .fixed_seconds_i64()
                    .ok_or(DateError::OutOfRange)?
                    .checked_mul(n)
                    .ok_or(DateError::OutOfRange)?;
                let delta = chrono::Duration::try_seconds(secs)
                    .ok_or(DateError::OutOfRange)?;
                let shifted_wall = wall
                    .checked_add_signed(delta)
                    .ok_or(DateError::OutOfRange)?;
                Ok(Self(shifted_wall.date()))
            }
        }
    }

    /// Applies `duration` to this civil date.
    ///
    /// Pure civil arithmetic: zone-free dates never touch the DST resolver.
    ///
    /// # Errors
    ///
    /// - [`DateError::OutOfRange`] if arithmetic overflows or the duration
    ///   contains non-finite seconds.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "part of DateValue surface; called in tests and by \
                      future query temporal functions"
        )
    )]
    pub(crate) fn apply(
        self,
        duration: &DurationValue,
    ) -> Result<Self, DateError> {
        if let Some(parts) = duration.parts() {
            let mut current = self;
            for &(mag, unit) in parts {
                current = current.apply_part(mag, unit)?;
            }
            Ok(current)
        } else {
            // Fixed magnitude without a written shape: applied as exact
            // seconds on the civil wall clock; non-finite input is rejected
            // by the `TimeDelta` conversion.
            let wall =
                self.0.and_hms_opt(0, 0, 0).ok_or(DateError::OutOfRange)?;
            let delta = TimeDelta::try_from(duration.to_seconds())
                .map_err(|_| DateError::OutOfRange)?;
            let shifted_wall =
                wall.checked_add_signed(delta).ok_or(DateError::OutOfRange)?;
            Ok(Self(shifted_wall.date()))
        }
    }

    fn apply_part(
        self,
        mag: f64,
        unit: DurationUnit,
    ) -> Result<Self, DateError> {
        if !mag.is_finite() {
            return Err(DateError::OutOfRange);
        }
        match unit {
            DurationUnit::Day
            | DurationUnit::Week
            | DurationUnit::Month
            | DurationUnit::Year => {
                let whole =
                    mag.trunc().to_i64().ok_or(DateError::OutOfRange)?;
                let mut current = if whole != 0 {
                    self.shift(whole, unit)?
                } else {
                    self
                };
                let rem_secs = mag.fract() * unit.fixed_seconds();
                if rem_secs != 0.0 {
                    let wall = current
                        .0
                        .and_hms_opt(0, 0, 0)
                        .ok_or(DateError::OutOfRange)?;
                    let whole_secs = rem_secs
                        .trunc()
                        .to_i64()
                        .ok_or(DateError::OutOfRange)?;
                    let subsec_nanos = (rem_secs.fract() * 1e9)
                        .round()
                        .to_i64()
                        .ok_or(DateError::OutOfRange)?;
                    let delta = chrono::Duration::try_seconds(whole_secs)
                        .ok_or(DateError::OutOfRange)?
                        .checked_add(&chrono::Duration::nanoseconds(
                            subsec_nanos,
                        ))
                        .ok_or(DateError::OutOfRange)?;
                    let shifted_wall = wall
                        .checked_add_signed(delta)
                        .ok_or(DateError::OutOfRange)?;
                    current = Self(shifted_wall.date());
                }
                Ok(current)
            }
            DurationUnit::Hour
            | DurationUnit::Minute
            | DurationUnit::Second
            | DurationUnit::Millisecond => {
                let wall =
                    self.0.and_hms_opt(0, 0, 0).ok_or(DateError::OutOfRange)?;
                let part_secs = mag * unit.fixed_seconds();
                let whole_secs =
                    part_secs.trunc().to_i64().ok_or(DateError::OutOfRange)?;
                let subsec_nanos = (part_secs.fract() * 1e9)
                    .round()
                    .to_i64()
                    .ok_or(DateError::OutOfRange)?;
                let delta = chrono::Duration::try_seconds(whole_secs)
                    .ok_or(DateError::OutOfRange)?
                    .checked_add(&chrono::Duration::nanoseconds(subsec_nanos))
                    .ok_or(DateError::OutOfRange)?;
                let shifted_wall = wall
                    .checked_add_signed(delta)
                    .ok_or(DateError::OutOfRange)?;
                Ok(Self(shifted_wall.date()))
            }
        }
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
impl From<NaiveDate> for DateValue {
    #[inline]
    fn from(date: NaiveDate) -> Self {
        Self(date)
    }
}

impl FromStr for DateValue {
    type Err = DateError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_iso(s)
    }
}

/// Serializes as the canonical `YYYY-MM-DD` string (the crate-internal
/// `to_date_string` formatter).
impl Serialize for DateValue {
    #[inline]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_date_string())
    }
}

/// Deserializes via the crate-internal `parse_iso` parser, so the
/// four-digit-year rule and `YYYY-MM` acceptance apply identically to
/// inline and deserialized dates.
impl<'de> Deserialize<'de> for DateValue {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = Cow::<'de, str>::deserialize(deserializer)?;
        Self::parse_iso(&s).map_err(serde::de::Error::custom)
    }
}

/// Parsed UTC date-time instant.
///
/// Wraps [`DateTime<Utc>`] as a newtype, enforcing ISO-8601/RFC-3339
/// recognition through its crate-internal `parse_iso` parser. All values
/// are UTC-normalized; offset-bearing input is converted to UTC at parse
/// time.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DateTimeValue(DateTime<Utc>);

impl DateTimeValue {
    /// Returns the current UTC date-time.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) fn now() -> Self {
        Self(Utc::now())
    }

    /// Parses an RFC-3339 or ISO-8601 date-time string using
    /// [`DateTimeFormat::ALL`].
    ///
    /// The cascade matches shapes only; after a shape matches, a naive input
    /// resolves through [`local_naive_to_utc`] (an explicit-offset input
    /// already is the instant). Resolution failures must not be retried
    /// against later formats, so the cascade separates shape matching from
    /// resolution.
    ///
    /// # Errors
    ///
    /// - [`DateError::InvalidYearDigits`] if the year segment is not exactly 4
    ///   ASCII digits.
    /// - [`DateError::Unparseable`] if no accepted shape matches.
    /// - [`DateError::LocalZoneLookup`] if the local timezone lookup fails
    ///   while resolving a naive input.
    pub(crate) fn parse_iso(s: &str) -> Result<Self, DateError> {
        let trimmed = s.trim();
        if !DateValue::has_four_digit_year(trimmed) {
            return Err(DateError::InvalidYearDigits {
                input: trimmed.into(),
            });
        }
        let [first, rest @ ..] = DateTimeFormat::ALL;
        let mut matched = first;
        let mut result = first.parse(trimmed);
        for format in rest {
            if result.is_ok() {
                break;
            }
            matched = format;
            result = format.parse(trimmed);
        }
        let parsed = result.map_err(|source| DateError::Unparseable {
            input: trimmed.into(),
            source,
        })?;
        let instant = if matches!(matched, DateTimeFormat::Rfc3339) {
            parsed
        } else {
            // The shape pass attached UTC as a placeholder; recover the
            // wall clock (an identity round-trip) and resolve it locally.
            local_naive_to_utc(parsed.naive_utc())?
        };
        Ok(Self(instant))
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
        self.date().to_string()
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
        self.wall_or_utc().format("%H:%M:%S").to_string()
    }

    /// Returns a new date-time truncated to the start of the local day.
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

    /// Returns the local wall-clock rendering of this instant.
    ///
    /// A date or time component means the reader's calendar day and clock,
    /// so every human-facing rendering goes through this conversion. It
    /// follows the offset-based checked form:
    /// [`Local::offset_from_utc_datetime`] supplies the zone's offset for
    /// the instant and [`NaiveDateTime::checked_add_offset`] applies it,
    /// returning [`None`] instead of panicking when the local time would
    /// overflow [`NaiveDateTime`]'s range (`.naive_local()` would panic
    /// there).
    #[inline]
    #[must_use]
    pub(crate) fn local_wall(self) -> Option<NaiveDateTime> {
        let naive_utc = self.0.naive_utc();
        let offset = Local.offset_from_utc_datetime(&naive_utc);
        naive_utc.checked_add_offset(offset.fix())
    }

    /// Returns the local wall clock, falling back to the UTC wall clock when
    /// the local offset cannot be applied (see [`Self::local_wall`]).
    #[inline]
    #[must_use]
    pub(crate) fn wall_or_utc(self) -> NaiveDateTime {
        self.local_wall().unwrap_or_else(|| self.0.naive_utc())
    }

    /// Returns the local calendar date of this instant, discarding
    /// time-of-day.
    ///
    /// When the local offset cannot be applied (an instant at the extreme
    /// edge of [`NaiveDateTime`]'s range), the UTC date renders instead.
    #[inline]
    #[must_use]
    pub(crate) fn date(self) -> DateValue {
        DateValue(self.wall_or_utc().date())
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
        // Arbitrary patterns render the local wall clock, matching `Display`.
        let wall = self.wall_or_utc();
        write!(out, "{}", wall.format(pattern)).map_err(|_fmt_error| {
            DateError::InvalidPattern {
                pattern: pattern.into(),
            }
        })?;
        Ok(out)
    }

    /// Shifts this date-time by `n` `unit`s.
    ///
    /// Calendar units ([`DurationUnit::Year`], [`DurationUnit::Month`],
    /// [`DurationUnit::Week`], [`DurationUnit::Day`]) shift the local wall
    /// clock, preserving the wall-clock time across DST transitions per spec
    /// D12/D13. Sub-day units shift the stored instant exactly.
    ///
    /// # Errors
    ///
    /// - [`DateError::OutOfRange`] if arithmetic overflows.
    /// - [`DateError::LocalZoneLookup`] if the shifted local wall clock cannot
    ///   be resolved.
    pub(crate) fn shift(
        self,
        n: i64,
        unit: DurationUnit,
    ) -> Result<Self, DateError> {
        match unit {
            DurationUnit::Year => {
                let months = n.checked_mul(12).ok_or(DateError::OutOfRange)?;
                self.shift_calendar_months(months)
            }
            DurationUnit::Month => self.shift_calendar_months(n),
            DurationUnit::Week => {
                let days = n.checked_mul(7).ok_or(DateError::OutOfRange)?;
                self.shift_calendar_days(days)
            }
            DurationUnit::Day => self.shift_calendar_days(n),
            DurationUnit::Hour
            | DurationUnit::Minute
            | DurationUnit::Second => {
                let secs = unit
                    .fixed_seconds_i64()
                    .ok_or(DateError::OutOfRange)?
                    .checked_mul(n)
                    .ok_or(DateError::OutOfRange)?;
                let delta = chrono::Duration::try_seconds(secs)
                    .ok_or(DateError::OutOfRange)?;
                let instant = self
                    .0
                    .checked_add_signed(delta)
                    .ok_or(DateError::OutOfRange)?;
                Ok(Self(instant))
            }
            DurationUnit::Millisecond => {
                let delta = chrono::Duration::try_milliseconds(n)
                    .ok_or(DateError::OutOfRange)?;
                let instant = self
                    .0
                    .checked_add_signed(delta)
                    .ok_or(DateError::OutOfRange)?;
                Ok(Self(instant))
            }
        }
    }

    fn shift_calendar_months(self, months: i64) -> Result<Self, DateError> {
        let wall = self.local_wall().ok_or(DateError::OutOfRange)?;
        let new_wall = shift_months(wall, months)?;
        let instant = local_naive_to_utc(new_wall)?;
        Ok(Self(instant))
    }

    fn shift_calendar_days(self, days: i64) -> Result<Self, DateError> {
        let wall = self.local_wall().ok_or(DateError::OutOfRange)?;
        let days_u64 = days.unsigned_abs();
        let new_wall = if days >= 0 {
            wall.checked_add_days(Days::new(days_u64))
        } else {
            wall.checked_sub_days(Days::new(days_u64))
        }
        .ok_or(DateError::OutOfRange)?;
        let instant = local_naive_to_utc(new_wall)?;
        Ok(Self(instant))
    }

    /// Applies `duration` to this date-time.
    ///
    /// If `duration` has retained written parts ([`DurationValue::parts`] is
    /// [`Some`]), they are applied in written left-to-right order. Calendar
    /// units ([`DurationUnit::Day`], [`DurationUnit::Week`],
    /// [`DurationUnit::Month`], [`DurationUnit::Year`]) shift the local wall
    /// clock, preserving the wall time across DST transitions per spec D12/D13.
    /// Sub-day units shift the stored instant exactly.
    ///
    /// If `duration` has no retained parts ([`DurationValue::parts`] is
    /// [`None`]), the duration represents a fixed magnitude and is applied
    /// directly to the stored instant as exact seconds.
    ///
    /// # Errors
    ///
    /// - [`DateError::OutOfRange`] if arithmetic overflows or the duration
    ///   contains non-finite seconds.
    /// - [`DateError::LocalZoneLookup`] if a shifted local wall clock cannot be
    ///   resolved.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "part of DateTimeValue surface; called in tests and by \
                      future query temporal functions"
        )
    )]
    pub(crate) fn apply(
        self,
        duration: &DurationValue,
    ) -> Result<Self, DateError> {
        if let Some(parts) = duration.parts() {
            let mut current = self;
            for &(mag, unit) in parts {
                current = current.apply_part(mag, unit)?;
            }
            Ok(current)
        } else {
            // Fixed magnitude without a written shape: applied as exact
            // seconds on the stored instant; non-finite input is rejected by
            // the `TimeDelta` conversion.
            let delta = TimeDelta::try_from(duration.to_seconds())
                .map_err(|_| DateError::OutOfRange)?;
            let instant = self
                .0
                .checked_add_signed(delta)
                .ok_or(DateError::OutOfRange)?;
            Ok(Self(instant))
        }
    }

    fn apply_part(
        self,
        mag: f64,
        unit: DurationUnit,
    ) -> Result<Self, DateError> {
        if !mag.is_finite() {
            return Err(DateError::OutOfRange);
        }
        match unit {
            DurationUnit::Day
            | DurationUnit::Week
            | DurationUnit::Month
            | DurationUnit::Year => {
                let whole =
                    mag.trunc().to_i64().ok_or(DateError::OutOfRange)?;
                let mut current = if whole != 0 {
                    self.shift(whole, unit)?
                } else {
                    self
                };
                let rem_secs = mag.fract() * unit.fixed_seconds();
                if rem_secs != 0.0 {
                    let whole_secs = rem_secs
                        .trunc()
                        .to_i64()
                        .ok_or(DateError::OutOfRange)?;
                    let subsec_nanos = (rem_secs.fract() * 1e9)
                        .round()
                        .to_i64()
                        .ok_or(DateError::OutOfRange)?;
                    let delta = chrono::Duration::try_seconds(whole_secs)
                        .ok_or(DateError::OutOfRange)?
                        .checked_add(&chrono::Duration::nanoseconds(
                            subsec_nanos,
                        ))
                        .ok_or(DateError::OutOfRange)?;
                    // The whole part keeps the local wall clock via `shift`;
                    // the fractional remainder is still part of a calendar
                    // unit, so it round-trips through the local zone too
                    // (spec D12) instead of landing as exact seconds on the
                    // stored instant.
                    let wall =
                        current.local_wall().ok_or(DateError::OutOfRange)?;
                    let shifted_wall = wall
                        .checked_add_signed(delta)
                        .ok_or(DateError::OutOfRange)?;
                    let instant = local_naive_to_utc(shifted_wall)?;
                    current = Self(instant);
                }
                Ok(current)
            }
            DurationUnit::Hour
            | DurationUnit::Minute
            | DurationUnit::Second
            | DurationUnit::Millisecond => {
                let part_secs = mag * unit.fixed_seconds();
                let whole_secs =
                    part_secs.trunc().to_i64().ok_or(DateError::OutOfRange)?;
                let subsec_nanos = (part_secs.fract() * 1e9)
                    .round()
                    .to_i64()
                    .ok_or(DateError::OutOfRange)?;
                let delta = chrono::Duration::try_seconds(whole_secs)
                    .ok_or(DateError::OutOfRange)?
                    .checked_add(&chrono::Duration::nanoseconds(subsec_nanos))
                    .ok_or(DateError::OutOfRange)?;
                let instant = self
                    .0
                    .checked_add_signed(delta)
                    .ok_or(DateError::OutOfRange)?;
                Ok(Self(instant))
            }
        }
    }

    /// Compares this date-time against `date`, coercing `date` to midnight in
    /// the local zone (see the [`From<DateValue>`] promotion).
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

    /// Returns `true` if this date-time is exactly local-zone midnight on
    /// `date` (see the [`From<DateValue>`] promotion).
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
        // The everyday human spelling is the local wall clock (see
        // `local_wall`); when the local offset cannot be applied, the UTC
        // wall clock renders instead of panicking.
        let wall = self.wall_or_utc();
        let format = if wall.nanosecond() == 0 {
            DEFAULT_DATETIME_FORMAT
        } else {
            "%Y-%m-%dT%H:%M:%S%.f"
        };
        write!(f, "{}", wall.format(format))
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
        // A file's calendar date means the reader's local day (the same
        // doctrine as `DateTimeValue::date`), not the UTC calendar day.
        DateTimeValue::from(time).date()
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

/// Promotes a [`DateValue`] to a [`DateTimeValue`] at midnight in the local
/// zone: a zone-free civil date has no instant until a reader's zone supplies
/// one, so the promotion resolves through the crate's local-zone DST
/// resolver.
///
/// A local timezone lookup failure falls back to UTC midnight (matching
/// chrono's own silent fallback for a broken zone); strict callers resolve
/// through `local_naive_to_utc` directly to surface that failure.
impl From<DateValue> for DateTimeValue {
    #[inline]
    fn from(date: DateValue) -> Self {
        let midnight = date.0.and_time(NaiveTime::MIN);
        Self(
            local_naive_to_utc(midnight)
                .unwrap_or_else(|_zone_failure| midnight.and_utc()),
        )
    }
}

impl FromStr for DateTimeValue {
    type Err = DateError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_iso(s)
    }
}

/// Serializes as an explicit RFC 3339 `…Z` interop spelling via
/// [`DateTime::to_rfc3339_opts`], independent of [`Display`](fmt::Display)'s
/// local-naive rendering.
impl Serialize for DateTimeValue {
    #[inline]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer
            .serialize_str(&self.0.to_rfc3339_opts(SecondsFormat::Secs, true))
    }
}

/// Deserializes via the crate-internal `parse_iso` parser, so the
/// four-digit-year rule and every accepted shape (including the `…Z`
/// interop spelling) apply identically to inline and deserialized
/// date-times.
impl<'de> Deserialize<'de> for DateTimeValue {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = Cow::<'de, str>::deserialize(deserializer)?;
        Self::parse_iso(&s).map_err(serde::de::Error::custom)
    }
}

/// Error type for date/date-time parse and formatting failures.
///
/// Returned by [`DateValue`]'s and [`DateTimeValue`]'s crate-internal
/// `parse_iso` and `format_with` methods.
#[derive(Debug, Clone, Eq, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum DateError {
    /// No accepted date/time shape matched `input`.
    ///
    /// Wraps the last-attempted format's [`chrono::ParseError`].
    #[error("`{input}` is not a recognized date/time: {source}")]
    Unparseable {
        /// The raw input that failed to parse.
        input: Box<str>,
        /// The last-attempted format's underlying parse failure.
        #[source]
        source: chrono::ParseError,
    },
    /// `input`'s year segment is not exactly 4 ASCII digits.
    ///
    /// chrono's `%Y` accepts fewer digits, silently misreading the year.
    #[error("`{input}` does not have a 4-digit year")]
    InvalidYearDigits {
        /// The raw input that failed to parse.
        input: Box<str>,
    },
    /// `pattern` is not a valid strftime specifier.
    #[error("`{pattern}` is not a valid format pattern")]
    InvalidPattern {
        /// The pattern that failed to render.
        pattern: Box<str>,
    },
    /// The process's local timezone could not resolve a naive input.
    ///
    /// DST ambiguities and gaps never produce this error (they resolve
    /// deterministically); it is reserved for a broken tz-data/OS lookup.
    #[error("local timezone lookup failed for `{input}`")]
    LocalZoneLookup {
        /// The naive wall-clock input that could not be resolved.
        input: Box<str>,
    },
    /// A date/time arithmetic operation overflowed or exceeded the
    /// representable range.
    #[error("date/time value is out of range")]
    OutOfRange,
}

/// Result of measuring the difference between two date/time points.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum DateDiff {
    /// Whole units elapsed.
    Whole(i64),
    /// Exact fractional units elapsed.
    Exact(f64),
}

/// Computes the difference from `from` to `to` in `unit`s.
///
/// # Arguments
///
/// - `from`/`to`: the [`DatePoint`] pair being measured.
/// - `both_datetimes`: whether both inputs carry a time component; it selects
///   the fixed-unit frame described below.
/// - `unit`: the measurement unit.
///
/// Preserves the declared measurement split:
/// - When both inputs are datetimes (`both_datetimes == true`), fixed units
///   measure elapsed time between the stored UTC instants as exact `f64`.
/// - When either input is date-only (`both_datetimes == false`), fixed units
///   measure elapsed civil wall clocks as `i64`.
/// - Calendar units ([`DurationUnit::Year`], [`DurationUnit::Month`]) measure
///   elapsed calendar periods using day-of-month aware whole counts
///   ([`signed_years_since`], [`signed_months_since`]).
///
/// # Errors
///
/// - [`DateError::OutOfRange`] if the difference overflows representable
///   bounds.
pub(crate) fn date_diff_measurement(
    from: DatePoint,
    to: DatePoint,
    both_datetimes: bool,
    unit: DurationUnit,
) -> Result<DateDiff, DateError> {
    match unit {
        DurationUnit::Year => Ok(DateDiff::Whole(signed_years_since(
            from.wall.date(),
            to.wall.date(),
        ))),
        DurationUnit::Month => Ok(DateDiff::Whole(signed_months_since(
            from.wall.date(),
            to.wall.date(),
        ))),
        u => {
            let unit_secs = u.fixed_seconds();
            let delta = if both_datetimes {
                to.instant.signed_duration_since(from.instant)
            } else {
                to.wall.signed_duration_since(from.wall)
            };

            if both_datetimes {
                let whole_seconds = delta
                    .num_seconds()
                    .to_f64()
                    .ok_or(DateError::OutOfRange)?;
                let result = (whole_seconds
                    + f64::from(delta.subsec_nanos()) / 1e9)
                    / unit_secs;
                Ok(DateDiff::Exact(result))
            } else if u == DurationUnit::Millisecond {
                Ok(DateDiff::Whole(delta.num_milliseconds()))
            } else {
                let unit_secs_i64 =
                    u.fixed_seconds_i64().ok_or(DateError::OutOfRange)?;
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "unit_secs_i64 is 86_400, 3_600, 60, or 1 (fixed \
                              units only), never zero, so this division never \
                              panics"
                )]
                let result = delta.num_seconds() / unit_secs_i64;
                Ok(DateDiff::Whole(result))
            }
        }
    }
}

/// A point in time carrying both civil wall clock and UTC instant.
#[derive(Copy, Clone, Debug)]
pub(crate) struct DatePoint {
    /// Civil wall-clock reading.
    pub(crate) wall: NaiveDateTime,
    /// Stored UTC instant.
    pub(crate) instant: DateTime<Utc>,
}

impl DatePoint {
    /// Constructs a point from its wall clock and UTC instant.
    #[inline]
    #[must_use]
    pub(crate) const fn new(
        wall: NaiveDateTime,
        instant: DateTime<Utc>,
    ) -> Self {
        Self {
            wall,
            instant,
        }
    }
}

/// Shifts a civil wall-clock datetime by `n` `unit`s.
///
/// Calendar units ([`DurationUnit::Year`], [`DurationUnit::Month`],
/// [`DurationUnit::Week`], [`DurationUnit::Day`]) shift civil calendar
/// components directly. Sub-day units shift civil time.
///
/// # Errors
///
/// - [`DateError::OutOfRange`] if `unit` has no whole-second value or the shift
///   overflows representable bounds.
pub(crate) fn shift_wall(
    wall: NaiveDateTime,
    n: i64,
    unit: DurationUnit,
) -> Result<NaiveDateTime, DateError> {
    match unit {
        DurationUnit::Year => {
            let months = n.checked_mul(12).ok_or(DateError::OutOfRange)?;
            shift_months(wall, months)
        }
        DurationUnit::Month => shift_months(wall, n),
        DurationUnit::Week => {
            let days = n.checked_mul(7).ok_or(DateError::OutOfRange)?;
            let days_u64 = days.unsigned_abs();
            if days >= 0 {
                wall.checked_add_days(Days::new(days_u64))
            } else {
                wall.checked_sub_days(Days::new(days_u64))
            }
            .ok_or(DateError::OutOfRange)
        }
        DurationUnit::Day => {
            let days_u64 = n.unsigned_abs();
            if n >= 0 {
                wall.checked_add_days(Days::new(days_u64))
            } else {
                wall.checked_sub_days(Days::new(days_u64))
            }
            .ok_or(DateError::OutOfRange)
        }
        DurationUnit::Millisecond => {
            let delta = chrono::Duration::try_milliseconds(n)
                .ok_or(DateError::OutOfRange)?;
            wall.checked_add_signed(delta).ok_or(DateError::OutOfRange)
        }
        DurationUnit::Second | DurationUnit::Minute | DurationUnit::Hour => {
            let secs = unit
                .fixed_seconds_i64()
                .ok_or(DateError::OutOfRange)?
                .checked_mul(n)
                .ok_or(DateError::OutOfRange)?;
            let delta = chrono::Duration::try_seconds(secs)
                .ok_or(DateError::OutOfRange)?;
            wall.checked_add_signed(delta).ok_or(DateError::OutOfRange)
        }
    }
}

/// Shifts `wall` by `months` calendar months, positive or negative.
///
/// Clamps to the last day of the target month per chrono's documented behavior.
///
/// # Errors
///
/// - [`DateError::OutOfRange`] if `months` does not fit a `u32` or the
///   arithmetic overflows representable bounds.
pub(crate) fn shift_months(
    wall: NaiveDateTime,
    months: i64,
) -> Result<NaiveDateTime, DateError> {
    let months_u32 = u32::try_from(months.unsigned_abs())
        .map_err(|_| DateError::OutOfRange)?;
    if months >= 0 {
        wall.checked_add_months(Months::new(months_u32))
    } else {
        wall.checked_sub_months(Months::new(months_u32))
    }
    .ok_or(DateError::OutOfRange)
}

/// Shifts `date` by `months` calendar months, positive or negative.
///
/// Clamps to the last day of the target month per chrono's documented behavior.
///
/// # Errors
///
/// - [`DateError::OutOfRange`] if `months` does not fit a `u32` or the
///   arithmetic overflows representable bounds.
#[cfg_attr(not(test), expect(dead_code, reason = "called by DateValue::shift"))]
pub(crate) fn shift_date_months(
    date: NaiveDate,
    months: i64,
) -> Result<NaiveDate, DateError> {
    let months_u32 = u32::try_from(months.unsigned_abs())
        .map_err(|_| DateError::OutOfRange)?;
    if months >= 0 {
        date.checked_add_months(Months::new(months_u32))
    } else {
        date.checked_sub_months(Months::new(months_u32))
    }
    .ok_or(DateError::OutOfRange)
}

/// Whole calendar years from `from` to `to`, signed.
///
/// Delegates to chrono's [`NaiveDate::years_since`], which is day-of-year
/// aware: a year is not "up" until `to`'s month/day reaches `from`'s. This
/// wrapper accepts either ordering.
pub(crate) fn signed_years_since(from: NaiveDate, to: NaiveDate) -> i64 {
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
pub(crate) fn signed_months_since(from: NaiveDate, to: NaiveDate) -> i64 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TzGuard;

    fn fixed_datetime() -> DateTimeValue {
        DateTimeValue(
            Utc.with_ymd_and_hms(2026, 7, 29, 14, 30, 5).single().expect(
                "2026-07-29 14:30:05 UTC is a valid, unambiguous instant",
            ),
        )
    }

    /// Local-zone doctrine: a naive datetime means the reader's local wall
    /// clock, resolved to UTC through the DST resolver; a date-only value
    /// stays zone-free until a zone is needed. Every test here injects `TZ`
    /// via [`TzGuard`] so the asserted zone is deterministic.
    mod local_zone {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn parses_a_naive_datetime_in_the_local_zone_and_stores_utc() {
            TzGuard::set("Etc/GMT-2"); // UTC+02:00, no DST

            let parsed = DateTimeValue::parse_iso("2026-07-29 14:30:00")
                .expect("naive datetime parses");

            assert_eq!(
                parsed,
                DateTimeValue::parse_iso("2026-07-29T12:30:00Z")
                    .expect("equivalent explicit-offset instant parses")
            );
        }

        #[test]
        fn resolves_an_ambiguous_fall_back_time_to_the_earliest_occurrence() {
            TzGuard::set("America/New_York");

            let parsed = DateTimeValue::parse_iso("2026-11-01 01:30:00")
                .expect("ambiguous wall clock still parses");

            // 01:30 happens twice (EDT, then EST); the earliest occurrence
            // is the EDT reading, 05:30Z.
            assert_eq!(
                parsed,
                DateTimeValue::parse_iso("2026-11-01T05:30:00Z")
                    .expect("equivalent explicit-offset instant parses")
            );
        }

        #[test]
        fn shifts_a_spring_forward_gap_forward_by_the_gap() {
            TzGuard::set("America/New_York");

            let parsed = DateTimeValue::parse_iso("2026-03-08 02:30:00")
                .expect("gap wall clock still parses, never fails");

            // 02:30 does not exist; interpreting it with the pre-transition
            // offset shifts it forward by the one-hour gap to 03:30 EDT.
            assert_eq!(
                parsed,
                DateTimeValue::parse_iso("2026-03-08T07:30:00Z")
                    .expect("equivalent explicit-offset instant parses")
            );
        }

        #[test]
        fn resolves_local_times_adjacent_to_a_gap_without_error() {
            TzGuard::set("America/New_York");

            let before = DateTimeValue::parse_iso("2026-03-08 01:59:59")
                .expect("wall clock one second before the gap parses");
            let after = DateTimeValue::parse_iso("2026-03-08 03:00:00")
                .expect("wall clock at the gap's end parses");

            // A real gap resolves `None` only inside the gap; the adjacent
            // local times must resolve normally, not be swept into a silent
            // shift or an error.
            assert_eq!(
                before,
                DateTimeValue::parse_iso("2026-03-08T06:59:59Z")
                    .expect("equivalent explicit-offset instant parses")
            );
            assert_eq!(
                after,
                DateTimeValue::parse_iso("2026-03-08T07:00:00Z")
                    .expect("equivalent explicit-offset instant parses")
            );
        }

        #[test]
        fn promotes_a_date_value_to_local_zone_midnight() {
            TzGuard::set("Etc/GMT-2"); // UTC+02:00, no DST

            let date = DateValue::parse_iso("2026-07-29").expect("valid date");

            let promoted = DateTimeValue::from(date);

            // Civil midnight in UTC+2 is 22:00Z on the previous day, so this
            // pins the promotion to the local zone rather than UTC.
            assert_eq!(
                promoted.into_inner(),
                Utc.with_ymd_and_hms(2026, 7, 28, 22, 0, 0)
                    .single()
                    .expect("unambiguous instant")
            );
        }
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
            TzGuard::set("UTC");

            assert_eq!(
                fixed_datetime().to_datetime_string(),
                "2026-07-29T14:30:05"
            );
        }

        #[test]
        fn round_trips_fractional_seconds_in_to_datetime_string() {
            TzGuard::set("UTC");

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
            TzGuard::set("UTC");

            assert_eq!(fixed_datetime().to_date_string(), "2026-07-29");
        }

        #[test]
        fn renders_to_time_string_without_the_date() {
            TzGuard::set("UTC");

            assert_eq!(fixed_datetime().to_time_string(), "14:30:05");
        }

        #[test]
        fn truncates_to_local_midnight_in_start_of_day() {
            TzGuard::keep();

            assert_eq!(
                fixed_datetime().start_of_day(),
                DateTimeValue::from(
                    DateValue::parse_iso("2026-07-29").expect("valid date")
                )
            );
        }

        #[test]
        fn extracts_the_calendar_date_discarding_time_of_day() {
            TzGuard::set("UTC");

            let extracted = fixed_datetime().date();
            let expected =
                DateValue::parse_iso("2026-07-29").expect("valid date");
            assert_eq!(extracted, expected);
        }

        #[test]
        fn extracts_the_local_calendar_date_of_the_instant() {
            TzGuard::set("Etc/GMT-12"); // UTC+12: 14:30Z is already July 30

            let extracted = fixed_datetime().date();
            let expected =
                DateValue::parse_iso("2026-07-30").expect("valid date");
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
            TzGuard::keep();

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
            TzGuard::keep();

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
            TzGuard::keep();

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
            TzGuard::set("UTC");

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
            TzGuard::keep();

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

    mod calendar_owner {
        use pretty_assertions::{assert_eq, assert_ne};

        use super::*;

        #[test]
        fn apply_advances_the_instant_by_the_duration() {
            let base = fixed_datetime();
            let one_hour = DurationValue::parse("1h").expect("valid duration");
            let forward = base.apply(&one_hour).expect("no overflow");
            assert_eq!(
                forward,
                DateTimeValue(base.into_inner() + TimeDelta::hours(1))
            );
        }

        #[test]
        fn apply_rewinds_the_instant_by_negated_duration() {
            let base = fixed_datetime();
            let one_hour = DurationValue::parse("1h").expect("valid duration");
            let back = base.apply(&(one_hour * -1.0)).expect("no overflow");
            assert_eq!(
                back,
                DateTimeValue(base.into_inner() - TimeDelta::hours(1))
            );
        }

        #[test]
        fn overflows_near_the_representable_range_in_apply() {
            let near_max = DateTimeValue(DateTime::<Utc>::MAX_UTC);
            let one_second =
                DurationValue::parse("1s").expect("valid duration");
            assert_eq!(near_max.apply(&one_second).ok(), None);
        }

        #[test]
        fn underflows_near_the_representable_range_in_apply() {
            let near_min = DateTimeValue(DateTime::<Utc>::MIN_UTC);
            let one_second =
                DurationValue::parse("1s").expect("valid duration");
            assert_eq!(near_min.apply(&(one_second * -1.0)).ok(), None);
        }

        #[test]
        fn shift_preserves_the_local_wall_clock_for_calendar_units_across_dst()
        {
            TzGuard::set("America/New_York");

            // Saturday March 7, 2026 at 12:00 EST (UTC 17:00:00)
            let base = DateTimeValue::parse_iso("2026-03-07 12:00:00").unwrap();

            // 1 day via `shift` keeps 12:00 on the wall clock across the gap.
            let shifted_day = base.shift(1, DurationUnit::Day).unwrap();
            assert_eq!(
                shifted_day.local_wall().unwrap().to_string(),
                "2026-03-08 12:00:00"
            );

            // `shift` routes week and 7-day units through the same calendar
            // path, so they shift identically and keep 12:00.
            let shifted_week = base.shift(1, DurationUnit::Week).unwrap();
            let shifted_7d = base.shift(7, DurationUnit::Day).unwrap();
            assert_eq!(shifted_week, shifted_7d);
            assert_eq!(
                shifted_week.local_wall().unwrap().to_string(),
                "2026-03-14 12:00:00"
            );

            // 168 hours shifts the instant and lands 13:00 across the gap.
            let shifted_168h = base.shift(168, DurationUnit::Hour).unwrap();
            assert_ne!(shifted_week, shifted_168h);
            assert_eq!(
                shifted_168h.local_wall().unwrap().to_string(),
                "2026-03-14 13:00:00"
            );

            // `shift` and `apply` agree for the same calendar unit: the
            // calendar owner owns the semantics in exactly one place.
            let applied_week =
                base.apply(&DurationValue::parse("1w").unwrap()).unwrap();
            assert_eq!(shifted_week, applied_week);
        }

        #[test]
        fn a2_prime_equal_duration_values_shift_dates_differently() {
            // "1 month" == "30 days" as duration values (fixed ratios)
            let one_month = DurationValue::parse("1 month").unwrap();
            let thirty_days = DurationValue::parse("30 days").unwrap();
            assert_eq!(one_month, thirty_days);

            // Yet they shift a date differently:
            // 2026-01-15 + 1 month = 2026-02-15 (28 days later)
            // 2026-01-15 + 30 days = 2026-02-14 (30 days later)
            let base = DateValue::parse_iso("2026-01-15").unwrap();
            let shifted_month = base.apply(&one_month).unwrap();
            let shifted_days = base.apply(&thirty_days).unwrap();

            assert_eq!(
                shifted_month,
                DateValue::parse_iso("2026-02-15").unwrap()
            );
            assert_eq!(
                shifted_days,
                DateValue::parse_iso("2026-02-14").unwrap()
            );
            assert_ne!(shifted_month, shifted_days);
        }

        #[test]
        fn month_clamping_matches_chrono_documented_behavior() {
            // Chrono's [`NaiveDate::checked_add_months`] clamps the day of the
            // month to the last valid day of the target month when the target
            // day doesn't exist (its documented behavior); the calendar owner
            // delegates month shifts to it, so the clamp is pinned here.
            let jan_31_non_leap = DateValue::parse_iso("2023-01-31").unwrap();
            let feb_clamped =
                jan_31_non_leap.shift(1, DurationUnit::Month).unwrap();
            assert_eq!(
                feb_clamped,
                DateValue::parse_iso("2023-02-28").unwrap()
            );

            let jan_31_leap = DateValue::parse_iso("2024-01-31").unwrap();
            let feb_leap_clamped =
                jan_31_leap.shift(1, DurationUnit::Month).unwrap();
            assert_eq!(
                feb_leap_clamped,
                DateValue::parse_iso("2024-02-29").unwrap()
            );
        }

        #[test]
        fn day_is_calendar_application_unit_differing_from_24h_across_dst() {
            TzGuard::set("America/New_York");

            // 1d == 24h as duration values
            let one_day = DurationValue::parse("1d").unwrap();
            let twenty_four_hours = DurationValue::parse("24h").unwrap();
            assert_eq!(one_day, twenty_four_hours);

            // Saturday March 7, 2026 at 12:00:00 EST (UTC 17:00:00)
            let base = DateTimeValue::parse_iso("2026-03-07 12:00:00").unwrap();

            // 1d shifts local wall clock to Sunday March 8, 2026 at 12:00:00
            // EDT (UTC 16:00:00)
            let shifted_day = base.apply(&one_day).unwrap();
            assert_eq!(
                shifted_day.local_wall().unwrap().to_string(),
                "2026-03-08 12:00:00"
            );
            assert_eq!(
                shifted_day,
                DateTimeValue::parse_iso("2026-03-08T16:00:00Z").unwrap()
            );

            // 24h shifts stored UTC instant by 24 hours -> Sunday March 8 at
            // 13:00:00 EDT (UTC 17:00:00)
            let shifted_hours = base.apply(&twenty_four_hours).unwrap();
            assert_eq!(
                shifted_hours.local_wall().unwrap().to_string(),
                "2026-03-08 13:00:00"
            );
            assert_eq!(
                shifted_hours,
                DateTimeValue::parse_iso("2026-03-08T17:00:00Z").unwrap()
            );

            // Equal values, different shifts across DST
            assert_ne!(shifted_day, shifted_hours);
        }

        #[test]
        fn week_shifts_as_seven_wall_days_matching_7d_and_differing_from_168h()
        {
            TzGuard::set("America/New_York");

            let one_week = DurationValue::parse("1w").unwrap();
            let seven_days = DurationValue::parse("7d").unwrap();
            let hours_168 = DurationValue::parse("168h").unwrap();

            assert_eq!(one_week, seven_days);
            assert_eq!(one_week, hours_168);

            // Saturday March 7, 2026 at 12:00:00 EST
            let base = DateTimeValue::parse_iso("2026-03-07 12:00:00").unwrap();

            let shifted_week = base.apply(&one_week).unwrap();
            let shifted_7d = base.apply(&seven_days).unwrap();
            let shifted_168h = base.apply(&hours_168).unwrap();

            // 1w and 7d shift identically
            assert_eq!(shifted_week, shifted_7d);
            assert_eq!(
                shifted_week.local_wall().unwrap().to_string(),
                "2026-03-14 12:00:00"
            );

            // 1w and 168h differ across DST transition
            assert_ne!(shifted_week, shifted_168h);
            assert_eq!(
                shifted_168h.local_wall().unwrap().to_string(),
                "2026-03-14 13:00:00"
            );
        }

        #[test]
        fn apply_processes_parts_left_to_right_across_dst_transition() {
            TzGuard::set("America/New_York");

            // Saturday March 7, 2026 at 02:30:00 EST (UTC 07:30:00)
            let base = DateTimeValue::parse_iso("2026-03-07 02:30:00").unwrap();

            let d_1d_1h = DurationValue::parse("1d 1h").unwrap();
            let d_1h_1d = DurationValue::parse("1h 1d").unwrap();

            // Values are equal in duration seconds
            assert_eq!(d_1d_1h, d_1h_1d);

            // L->R application:
            // "1d 1h": 1d lands in spring-forward gap (Sunday 02:30 -> shifted
            // forward to 03:30 EDT = 07:30 UTC), then 1h advances
            // instant by 3600s -> 08:30 UTC (04:30 EDT).
            let res_1d_1h = base.apply(&d_1d_1h).unwrap();
            assert_eq!(
                res_1d_1h,
                DateTimeValue::parse_iso("2026-03-08T08:30:00Z").unwrap()
            );
            assert_eq!(
                res_1d_1h.local_wall().unwrap().to_string(),
                "2026-03-08 04:30:00"
            );

            // "1h 1d": 1h advances Saturday instant by 3600s -> Saturday 03:30
            // EST (08:30 UTC), then 1d shifts wall clock to Sunday
            // 03:30 EDT (07:30 UTC).
            let res_1h_1d = base.apply(&d_1h_1d).unwrap();
            assert_eq!(
                res_1h_1d,
                DateTimeValue::parse_iso("2026-03-08T07:30:00Z").unwrap()
            );
            assert_eq!(
                res_1h_1d.local_wall().unwrap().to_string(),
                "2026-03-08 03:30:00"
            );

            // Different written order yields different result
            assert_ne!(res_1d_1h, res_1h_1d);
        }

        #[test]
        fn fractional_calendar_part_keeps_the_local_wall_clock_across_dst() {
            TzGuard::set("America/New_York");

            // Saturday March 7, 2026 at 06:00 EST (UTC 11:00:00)
            let base = DateTimeValue::parse_iso("2026-03-07 06:00:00").unwrap();

            // 1.5d == 36h == 1d 12h as values (129,600 fixed seconds)
            let one_and_half_days = DurationValue::parse("1.5d").unwrap();
            let thirty_six_hours = DurationValue::parse("36h").unwrap();
            let written_order = DurationValue::parse("1d 12h").unwrap();
            assert_eq!(one_and_half_days, thirty_six_hours);
            assert_eq!(one_and_half_days, written_order);

            // The fractional day is a calendar-unit remainder, so it keeps the
            // local wall clock (spec D12): 1d lands Sunday 06:00 EDT, then the
            // 0.5-day remainder adds 12 wall hours to Sunday 18:00 EDT.
            let shifted_fractional = base.apply(&one_and_half_days).unwrap();
            assert_eq!(
                shifted_fractional.local_wall().unwrap().to_string(),
                "2026-03-08 18:00:00"
            );
            assert_eq!(
                shifted_fractional,
                DateTimeValue::parse_iso("2026-03-08T22:00:00Z").unwrap()
            );

            // The fractional spelling decomposes exactly like its written-order
            // equivalent "1d 12h".
            assert_eq!(shifted_fractional, base.apply(&written_order).unwrap());

            // 36h shifts the stored instant exactly and lands an hour later on
            // the wall clock across the 2026-03-08 gap: equal values, different
            // shifts (the declared A2′ incoherence, spec line 87).
            let shifted_exact = base.apply(&thirty_six_hours).unwrap();
            assert_ne!(shifted_fractional, shifted_exact);
            assert_eq!(
                shifted_exact.local_wall().unwrap().to_string(),
                "2026-03-08 19:00:00"
            );
        }

        #[test]
        fn date_value_civil_arithmetic_handles_sub_day_units() {
            let base = DateValue::parse_iso("2026-07-29").unwrap();

            // Sub-day 1 hour shift is a no-op on date (stays 2026-07-29)
            let one_hour = DurationValue::parse("1h").unwrap();
            assert_eq!(base.apply(&one_hour).unwrap(), base);

            // 24 hour shift advances the civil date
            let twenty_four_hours = DurationValue::parse("24h").unwrap();
            assert_eq!(
                base.apply(&twenty_four_hours).unwrap(),
                DateValue::parse_iso("2026-07-30").unwrap()
            );
        }

        #[test]
        fn date_diff_measurement_instant_and_civil_split() {
            let dt1 = DateTimeValue::parse_iso("2026-07-29T10:00:00Z").unwrap();
            let dt2 = DateTimeValue::parse_iso("2026-07-29T11:30:00Z").unwrap();

            // When both are datetimes, fixed units return exact f64:
            let diff_hours = date_diff_measurement(
                DatePoint::new(dt1.wall_or_utc(), dt1.into_inner()),
                DatePoint::new(dt2.wall_or_utc(), dt2.into_inner()),
                true,
                DurationUnit::Hour,
            )
            .unwrap();
            assert_eq!(diff_hours, DateDiff::Exact(1.5));

            // When either is date-only, fixed units return whole i64:
            let diff_civil = date_diff_measurement(
                DatePoint::new(dt1.wall_or_utc(), dt1.into_inner()),
                DatePoint::new(dt2.wall_or_utc(), dt2.into_inner()),
                false,
                DurationUnit::Hour,
            )
            .unwrap();
            assert_eq!(diff_civil, DateDiff::Whole(1));

            // Calendar units return whole counts:
            let d1 = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
            let d2 = NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
            let diff_months = date_diff_measurement(
                DatePoint::new(
                    d1.and_hms_opt(0, 0, 0).unwrap(),
                    dt1.into_inner(),
                ),
                DatePoint::new(
                    d2.and_hms_opt(0, 0, 0).unwrap(),
                    dt2.into_inner(),
                ),
                false,
                DurationUnit::Month,
            )
            .unwrap();
            assert_eq!(diff_months, DateDiff::Whole(2));
        }
    }

    mod comparison {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn requires_exact_midnight_in_is_equal_to_date() {
            TzGuard::keep();

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
            TzGuard::set("UTC");

            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            assert_eq!(
                fixed_datetime().cmp_date(date),
                std::cmp::Ordering::Greater
            );
        }

        #[test]
        fn cmp_date_reports_equal_at_exact_midnight() {
            TzGuard::keep();

            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let midnight = DateTimeValue::from(date);
            assert_eq!(midnight.cmp_date(date), std::cmp::Ordering::Equal);
        }

        #[test]
        fn cmp_date_reports_less_when_the_instant_is_before_midnight() {
            TzGuard::set("UTC");

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
            TzGuard::set("UTC");

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
        fn converts_from_system_time_to_the_local_date() {
            TzGuard::set("Etc/GMT+5"); // UTC-05:00, no DST

            let system_time = std::time::SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(1_000);

            // 00:16:40Z is 19:16:40 on December 31 in UTC-5.
            assert_eq!(
                DateValue::from(system_time),
                DateValue::parse_iso("1969-12-31").expect("valid date")
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
        fn converts_from_naive_date_via_from_trait() {
            let naive =
                NaiveDate::from_ymd_opt(2026, 7, 29).expect("valid date");
            let date: DateValue = naive.into();
            assert_eq!(
                date,
                DateValue::parse_iso("2026-07-29").expect("valid date")
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
            TzGuard::keep();

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
            TzGuard::set("UTC");

            assert_eq!(fixed_datetime().to_string(), "2026-07-29T14:30:05");
        }

        #[test]
        fn displays_the_local_wall_clock_of_the_instant() {
            TzGuard::set("Etc/GMT-2"); // UTC+02:00, no DST

            // The everyday human spelling is the local wall clock, so a
            // 14:30:05Z instant reads as 16:30:05 local.
            assert_eq!(fixed_datetime().to_string(), "2026-07-29T16:30:05");
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
        fn exposes_the_chrono_source_for_an_unparseable_error() {
            use std::error::Error as _;
            let err = DateValue::parse_iso("2026/08/22")
                .expect_err("unrecognized shape");
            let source = err.source().expect("chrono parse error is chained");
            assert_eq!(source.to_string(), "input contains invalid characters");
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
        fn serializes_datetime_value_with_the_explicit_rfc3339_z_suffix() {
            let json =
                serde_json::to_string(&fixed_datetime()).expect("serializable");
            assert_eq!(json, "\"2026-07-29T14:30:05Z\"");
        }

        #[test]
        fn deserializes_year_month_precision_date_through_json() {
            let restored: DateValue =
                serde_json::from_str("\"2026-07\"").expect("deserializable");
            assert_eq!(
                restored,
                DateValue::parse_iso("2026-07-01").expect("valid date")
            );
        }

        #[test]
        fn rejects_a_two_digit_year_through_json_deserialization() {
            let err = serde_json::from_str::<DateValue>("\"26-08-22\"")
                .expect_err("short year rejected");
            assert!(err.to_string().contains("does not have a 4-digit year"));
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

    mod hostile_yaml_note {
        use pretty_assertions::{assert_eq, assert_ne};

        use super::*;

        #[derive(Deserialize)]
        struct HostileNote {
            duration_extreme: DurationValue,
            duration_zero: DurationValue,
            date: DateValue,
            datetime: DateTimeValue,
        }

        #[test]
        fn parses_round_trips_and_displays_without_panic_or_lying() {
            let extreme_digits = "9".repeat(50);
            let yaml = format!(
                "duration_extreme: {extreme_digits}y\nduration_zero: \
                 \"-0m\"\ndate: \"2026-07\"\ndatetime: \
                 \"2026-07-29T14:30:00Z\"\n"
            );

            let note = noyalib::from_str::<HostileNote>(&yaml)
                .expect("hostile note deserializes without panicking");

            // Extreme-magnitude duration: a value synthesized back from its
            // parsed seconds (as arithmetic/formatting code does, having no
            // original spelling to echo) renders honestly in scientific
            // notation, never a silently truncated "0s".
            let synthesized =
                DurationValue::from_seconds(note.duration_extreme.to_seconds());
            let rendered_extreme = synthesized.to_string();
            assert_ne!(rendered_extreme, "0s");
            assert!(rendered_extreme.contains('e'));

            // "-0m": equals and orders as zero, not a hidden negative.
            assert_eq!(
                note.duration_zero,
                DurationValue::parse("0m").expect("valid duration")
            );
            assert!(note.duration_zero >= DurationValue::parse("0m").unwrap());

            // "YYYY-MM" date: precision defaults to day 1, matches inline
            // parsing exactly.
            assert_eq!(
                note.date,
                DateValue::parse_iso("2026-07-01").expect("valid date")
            );

            // "…Z" datetime: parses to the same instant as the equivalent
            // explicit-offset spelling (a naive spelling would mean the local
            // zone) and re-serializes with the same spelling.
            assert_eq!(
                note.datetime,
                DateTimeValue::parse_iso("2026-07-29T14:30:00+00:00")
                    .expect("valid datetime")
            );
            assert_eq!(
                serde_json::to_string(&note.datetime).expect("serializable"),
                "\"2026-07-29T14:30:00Z\""
            );
        }
    }
}
