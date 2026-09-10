//! Duration parsing, validation, and computation.
//!
//! The primary type callers interact with is [`DurationUnit`], which carries
//! all unit knowledge (parsing, seconds, naming). [`DurationValue`] wraps a
//! parsed duration with its total seconds and last unit. [`DurationSeconds`] is
//! a typed `f64` with [`Ord`], [`Add`], [`Sub`], [`Mul`].
//!
//! All duration unit knowledge lives in [`DurationUnit`]. Callers should not
//! maintain their own unit registries.

use std::{
    cmp::Ordering,
    fmt,
    ops::{Add, Mul, Sub},
    str::FromStr,
};

/// A validated duration expression.
///
/// Constructed only via [`DurationValue::parse`] or
/// [`DurationValue::parse_unit_name`]. Stores the parsed total seconds and the
/// last unit encountered during parsing.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DurationValue {
    seconds: DurationSeconds,
    unit: Option<DurationUnit>,
}

impl DurationValue {
    /// Parses a duration spelling (e.g., `"1h 30m"`, `"4 hrs"`).
    ///
    /// Accepts one or more `<number><unit>` parts separated by whitespace or
    /// commas. Returns a specific error for each failure mode.
    ///
    /// # Errors
    ///
    /// - [`Empty`] if the input is empty or contains only separators.
    /// - [`MissingNumber`] if a unit appears without a preceding number.
    /// - [`InvalidNumber`] if the number portion could not be parsed.
    /// - [`MissingUnit`] if a number appears without a trailing unit.
    /// - [`UnknownUnit`] if the unit string is not recognized.
    /// - [`NonFiniteSeconds`] if the parsed total cannot be represented as a
    ///   finite seconds value.
    ///
    /// [`Empty`]: DurationError::Empty
    /// [`MissingNumber`]: DurationError::MissingNumber
    /// [`InvalidNumber`]: DurationError::InvalidNumber
    /// [`MissingUnit`]: DurationError::MissingUnit
    /// [`UnknownUnit`]: DurationError::UnknownUnit
    /// [`NonFiniteSeconds`]: DurationError::NonFiniteSeconds
    pub(crate) fn parse(input: &str) -> Result<Self, DurationError> {
        let bytes = input.as_bytes();
        let len = bytes.len();
        let mut pos = 0;
        let mut total = 0.0f64;
        let mut last_unit = None::<DurationUnit>;
        let mut parsed_any = false;

        while pos < len {
            Self::skip_separators(bytes, &mut pos);
            if pos >= len {
                break;
            }

            let (number, pos_after_number) =
                Self::parse_number(bytes, pos, input)?;
            pos = pos_after_number;

            Self::skip_whitespace(bytes, &mut pos);

            let (kind, pos_after_unit) = Self::parse_unit(bytes, pos, input)?;
            pos = pos_after_unit;

            total += number * kind.seconds();
            last_unit = Some(kind);
            parsed_any = true;
        }

        if !parsed_any {
            return Err(DurationError::Empty);
        }

        Ok(Self {
            seconds: DurationSeconds::try_from(total)?,
            unit: last_unit,
        })
    }

    /// Parses a bare unit name as a single-part duration with quantity 1.
    ///
    /// Accepts any spelling recognized by [`DurationUnit::parse`] (e.g.,
    /// `"hours"`, `"d"`, `"sec"`). Used by the template engine for date-shift
    /// operations.
    pub(crate) fn parse_unit_name(name: &str) -> Option<Self> {
        let kind = DurationUnit::parse(name)?;
        Some(Self {
            seconds: DurationSeconds::try_from(kind.seconds()).ok()?,
            unit: Some(kind),
        })
    }

    /// Returns the total duration as [`DurationSeconds`].
    #[inline]
    #[must_use]
    pub(crate) const fn to_seconds(&self) -> DurationSeconds {
        self.seconds
    }

    /// Returns the last parsed unit, if any.
    ///
    /// For `"1h 30m"`, returns [`DurationUnit::Minute`] (the last unit). For
    /// bare unit names like `"hours"`, returns the corresponding unit. Returns
    /// `None` only for synthetic `DurationValue` instances with no unit (which
    /// cannot be constructed through the public API).
    #[must_use]
    pub(crate) const fn last_unit(&self) -> Option<DurationUnit> {
        self.unit
    }

    /// Advances `pos` past whitespace and commas.
    fn skip_separators(bytes: &[u8], pos: &mut usize) {
        let len = bytes.len();
        while *pos < len {
            let Some(&b) = bytes.get(*pos) else {
                break;
            };
            if !b.is_ascii_whitespace() && b != b',' {
                break;
            }
            *pos = (*pos).saturating_add(1);
        }
    }

    /// Advances `pos` past ASCII whitespace.
    fn skip_whitespace(bytes: &[u8], pos: &mut usize) {
        let len = bytes.len();
        while *pos < len {
            let Some(&b) = bytes.get(*pos) else {
                break;
            };
            if !b.is_ascii_whitespace() {
                break;
            }
            *pos = (*pos).saturating_add(1);
        }
    }

    /// Parses a decimal number starting at `pos`.
    fn parse_number(
        bytes: &[u8],
        mut pos: usize,
        input: &str,
    ) -> Result<(f64, usize), DurationError> {
        let num_start = pos;
        let mut has_decimal = false;
        while pos < bytes.len() {
            let Some(&b) = bytes.get(pos) else {
                break;
            };
            if b.is_ascii_digit() {
                pos = pos.saturating_add(1);
            } else if b == b'.' && !has_decimal {
                has_decimal = true;
                pos = pos.saturating_add(1);
            } else {
                break;
            }
        }
        Self::parsed_number(bytes, num_start, pos, input)
    }

    /// Converts a parsed number byte span into `f64`.
    fn parsed_number(
        bytes: &[u8],
        start: usize,
        end: usize,
        input: &str,
    ) -> Result<(f64, usize), DurationError> {
        if start == end {
            return Err(DurationError::MissingNumber {
                input: input.to_owned(),
            });
        }
        let Some(num_slice) = bytes.get(start..end) else {
            return Err(DurationError::MissingNumber {
                input: input.to_owned(),
            });
        };
        let number: f64 = core::str::from_utf8(num_slice)
            .map_err(|_| DurationError::InvalidNumber {
                input: input.to_owned(),
            })?
            .parse()
            .map_err(|_| DurationError::InvalidNumber {
                input: input.to_owned(),
            })?;
        if !number.is_finite() {
            return Err(DurationError::InvalidNumber {
                input: input.to_owned(),
            });
        }
        Ok((number, end))
    }

    /// Parses a unit string starting at `pos`.
    fn parse_unit(
        bytes: &[u8],
        mut pos: usize,
        input: &str,
    ) -> Result<(DurationUnit, usize), DurationError> {
        let unit_start = pos;
        while pos < bytes.len() {
            let Some(&b) = bytes.get(pos) else {
                break;
            };
            if !b.is_ascii_alphabetic() {
                break;
            }
            pos = pos.saturating_add(1);
        }
        Self::parsed_unit(bytes, unit_start, pos, input)
    }

    /// Converts a parsed unit byte span into [`DurationUnit`].
    fn parsed_unit(
        bytes: &[u8],
        start: usize,
        end: usize,
        input: &str,
    ) -> Result<(DurationUnit, usize), DurationError> {
        if start == end {
            return Err(DurationError::MissingUnit {
                input: input.to_owned(),
            });
        }
        let Some(unit_slice) = bytes.get(start..end) else {
            return Err(DurationError::MissingUnit {
                input: input.to_owned(),
            });
        };
        let unit_str = core::str::from_utf8(unit_slice).map_err(|_| {
            DurationError::UnknownUnit {
                input: input.to_owned(),
            }
        })?;
        let kind = DurationUnit::parse(unit_str).ok_or_else(|| {
            DurationError::UnknownUnit {
                input: input.to_owned(),
            }
        })?;
        Ok((kind, end))
    }
}

/// Enables `"1h 30m".parse::<DurationValue>()`.
///
/// # Errors
///
/// - [`DurationError`] if [`DurationValue::parse`] rejects `s`.
impl FromStr for DurationValue {
    type Err = DurationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// A recognized duration unit.
///
/// Single source of truth for unit parsing, seconds conversion, and naming.
/// Callers that need type-safe dispatch should match on `DurationUnit`
/// directly rather than converting to strings.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub(crate) enum DurationUnit {
    Millisecond,
    Second,
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Year,
}

impl DurationUnit {
    /// Case-insensitive lookup of a unit string.
    ///
    /// Accepts strings up to 16 bytes. Returns `None` for empty strings,
    /// strings longer than 16 bytes, or unrecognized unit names.
    #[must_use]
    pub(crate) fn parse(unit: &str) -> Option<Self> {
        let mut buf = [0u8; 16];
        let slice = buf.get_mut(..unit.len())?;
        slice.copy_from_slice(unit.as_bytes());
        slice.make_ascii_lowercase();
        let lower = core::str::from_utf8(slice).ok()?;
        UNIT_MAP.get(lower).copied()
    }

    /// Seconds per unit.
    pub(crate) const fn seconds(self) -> f64 {
        match self {
            Self::Millisecond => 0.001,
            Self::Second => 1.0,
            Self::Minute => 60.0,
            Self::Hour => 3_600.0,
            Self::Day => 86_400.0,
            Self::Week => 604_800.0,
            Self::Month => 2_592_000.0,
            Self::Year => 31_536_000.0,
        }
    }

    /// Whole seconds per unit as `i64`.
    ///
    /// Returns `None` for [`Self::Millisecond`], which is fractional seconds.
    pub(crate) const fn seconds_i64(self) -> Option<i64> {
        match self {
            Self::Millisecond => None,
            Self::Second => Some(1),
            Self::Minute => Some(60),
            Self::Hour => Some(3_600),
            Self::Day => Some(86_400),
            Self::Week => Some(604_800),
            Self::Month => Some(2_592_000),
            Self::Year => Some(31_536_000),
        }
    }
}

/// Case-insensitive unit string → [`DurationUnit`].
static UNIT_MAP: phf::Map<&'static str, DurationUnit> = phf::phf_map! {
    "ms" => DurationUnit::Millisecond,
    "millisecond" => DurationUnit::Millisecond,
    "milliseconds" => DurationUnit::Millisecond,
    "s" => DurationUnit::Second,
    "sec" => DurationUnit::Second,
    "secs" => DurationUnit::Second,
    "second" => DurationUnit::Second,
    "seconds" => DurationUnit::Second,
    "m" => DurationUnit::Minute,
    "min" => DurationUnit::Minute,
    "mins" => DurationUnit::Minute,
    "minute" => DurationUnit::Minute,
    "minutes" => DurationUnit::Minute,
    "h" => DurationUnit::Hour,
    "hr" => DurationUnit::Hour,
    "hrs" => DurationUnit::Hour,
    "hour" => DurationUnit::Hour,
    "hours" => DurationUnit::Hour,
    "d" => DurationUnit::Day,
    "day" => DurationUnit::Day,
    "days" => DurationUnit::Day,
    "w" => DurationUnit::Week,
    "wk" => DurationUnit::Week,
    "wks" => DurationUnit::Week,
    "week" => DurationUnit::Week,
    "weeks" => DurationUnit::Week,
    "mo" => DurationUnit::Month,
    "mos" => DurationUnit::Month,
    "month" => DurationUnit::Month,
    "months" => DurationUnit::Month,
    "y" => DurationUnit::Year,
    "yr" => DurationUnit::Year,
    "yrs" => DurationUnit::Year,
    "year" => DurationUnit::Year,
    "years" => DurationUnit::Year,
};

/// A duration measured in seconds.
///
/// Wraps `f64` with NaN-safe ordering and arithmetic traits. Always finite when
/// constructed through [`DurationValue::to_seconds`].
#[derive(Copy, Clone, Debug)]
pub(crate) struct DurationSeconds(f64);

impl TryFrom<f64> for DurationSeconds {
    type Error = DurationError;

    fn try_from(secs: f64) -> Result<Self, Self::Error> {
        secs.is_finite()
            .then_some(Self(secs))
            .ok_or(DurationError::NonFiniteSeconds)
    }
}

impl PartialEq for DurationSeconds {
    fn eq(&self, other: &Self) -> bool {
        self.0.total_cmp(&other.0) == Ordering::Equal
    }
}

impl Eq for DurationSeconds {}

impl PartialOrd for DurationSeconds {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DurationSeconds {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl fmt::Display for DurationSeconds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Add for DurationSeconds {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl Sub for DurationSeconds {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl Mul<f64> for DurationSeconds {
    type Output = Self;

    fn mul(self, rhs: f64) -> Self {
        Self(self.0 * rhs)
    }
}

impl Mul<DurationSeconds> for f64 {
    type Output = DurationSeconds;

    #[inline]
    fn mul(self, rhs: DurationSeconds) -> DurationSeconds {
        DurationSeconds(self * rhs.0)
    }
}

/// Error returned when a duration operation fails.
///
/// Raised when duration text cannot be parsed or raw duration seconds cannot
/// be represented safely.
#[derive(Debug, Clone, thiserror::Error)]
pub enum DurationError {
    /// Input is empty or contains only separators.
    #[error("duration is empty")]
    Empty,

    /// A unit appears without a preceding number (e.g., `"h"` or `"hours"`).
    #[error("no number before unit in `{input}`")]
    MissingNumber {
        /// The raw input that failed to parse.
        input: String,
    },

    /// The number portion could not be parsed (e.g., `"1.2.3h"`).
    #[error("invalid number in `{input}`")]
    InvalidNumber {
        /// The raw input that failed to parse.
        input: String,
    },

    /// A number appears without a trailing unit (e.g., `"1"` or `"42"`).
    #[error("no unit after number in `{input}`")]
    MissingUnit {
        /// The raw input that failed to parse.
        input: String,
    },

    /// The unit string is not recognized (e.g., `"1x"`).
    #[error("unknown unit in `{input}`")]
    UnknownUnit {
        /// The raw input that failed to parse.
        input: String,
    },

    /// A raw seconds value is `NaN` or infinite.
    #[error("duration seconds must be finite")]
    NonFiniteSeconds,
}

#[cfg(test)]
mod tests {
    use super::*;

    mod duration_seconds_ops {
        use super::*;

        #[test]
        fn add_combines_seconds() {
            let a = DurationSeconds::try_from(100.0).unwrap();
            let b = DurationSeconds::try_from(200.0).unwrap();
            assert_eq!(a + b, DurationSeconds::try_from(300.0).unwrap());
        }

        #[test]
        fn sub_subtracts_seconds() {
            let a = DurationSeconds::try_from(300.0).unwrap();
            let b = DurationSeconds::try_from(100.0).unwrap();
            assert_eq!(a - b, DurationSeconds::try_from(200.0).unwrap());
        }

        #[test]
        fn mul_scales_seconds() {
            let a = DurationSeconds::try_from(100.0).unwrap();
            assert_eq!(a * 3.0, DurationSeconds::try_from(300.0).unwrap());
            assert_eq!(3.0 * a, DurationSeconds::try_from(300.0).unwrap());
        }

        #[test]
        fn ord_uses_total_cmp() {
            let a = DurationSeconds::try_from(100.0).unwrap();
            let b = DurationSeconds::try_from(200.0).unwrap();
            assert!(a < b);
            assert!(b > a);
            assert_eq!(a, DurationSeconds::try_from(100.0).unwrap());
        }

        #[test]
        fn try_from_rejects_nan() {
            assert!(DurationSeconds::try_from(f64::NAN).is_err());
        }

        #[test]
        fn try_from_rejects_infinity() {
            assert!(DurationSeconds::try_from(f64::INFINITY).is_err());
            assert!(DurationSeconds::try_from(f64::NEG_INFINITY).is_err());
        }

        #[test]
        fn from_duration_value() {
            let dv = DurationValue::parse("1h").unwrap();
            assert_eq!(dv.to_seconds().0, 3_600.0);
        }

        #[test]
        fn display_formats_as_number() {
            assert_eq!(
                DurationSeconds::try_from(3600.0).unwrap().to_string(),
                "3600"
            );
            assert_eq!(
                DurationSeconds::try_from(0.5).unwrap().to_string(),
                "0.5"
            );
        }
    }

    mod duration_value_parse {
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::milliseconds("500ms", 0.5)]
        #[case::seconds("90s", 90.0)]
        #[case::minutes("30m", 1_800.0)]
        #[case::hours("1h", 3_600.0)]
        #[case::days("2d", 172_800.0)]
        #[case::weeks("1w", 604_800.0)]
        #[case::months("3mo", 7_776_000.0)]
        #[case::years("1y", 31_536_000.0)]
        fn parses_single_unit(#[case] input: &str, #[case] expected: f64) {
            assert_eq!(
                DurationValue::parse(input).unwrap().to_seconds().0,
                expected
            );
        }

        #[test]
        fn parses_multi_part_with_space() {
            let d = DurationValue::parse("1h 30m").unwrap();
            assert_eq!(d.to_seconds().0, 5_400.0);
            assert_eq!(d.last_unit(), Some(DurationUnit::Minute));
        }

        #[test]
        fn parses_multi_part_without_separator() {
            assert_eq!(
                DurationValue::parse("1h30m").unwrap().to_seconds().0,
                5_400.0
            );
        }

        #[test]
        fn parses_multi_part_with_comma() {
            let d = DurationValue::parse("4 yrs, 6 wks").unwrap();
            assert_eq!(d.to_seconds().0, 4.0 * 31_536_000.0 + 6.0 * 604_800.0);
        }

        #[test]
        fn parses_decimal_numbers() {
            assert_eq!(
                DurationValue::parse("1.5h").unwrap().to_seconds().0,
                5_400.0
            );
        }

        #[test]
        fn parses_decimal_without_leading_digit() {
            assert_eq!(
                DurationValue::parse(".5h").unwrap().to_seconds().0,
                1_800.0
            );
        }

        #[test]
        fn parses_with_leading_trailing_whitespace() {
            let d = DurationValue::parse(" 1h ").unwrap();
            assert_eq!(d.to_seconds().0, 3_600.0);
        }

        #[test]
        fn parses_with_multiple_consecutive_separators() {
            let d = DurationValue::parse("1  h  30  m").unwrap();
            assert_eq!(d.to_seconds().0, 5_400.0);
        }

        #[rstest]
        #[case::empty("")]
        #[case::whitespace_only("   ")]
        fn returns_empty_error_for_empty_input(#[case] input: &str) {
            assert!(matches!(
                DurationValue::parse(input),
                Err(DurationError::Empty)
            ));
        }

        #[rstest]
        #[case::bare_unit("h")]
        #[case::unit_without_number(", h")]
        fn returns_missing_number_for_unit_without_number(#[case] input: &str) {
            assert!(matches!(
                DurationValue::parse(input),
                Err(DurationError::MissingNumber { .. })
            ));
        }

        #[rstest]
        #[case::double_dot("1.2.3h")]
        fn returns_missing_unit_for_malformed_number(#[case] input: &str) {
            assert!(matches!(
                DurationValue::parse(input),
                Err(DurationError::MissingUnit { .. })
            ));
        }

        #[test]
        fn returns_missing_unit_for_number_without_unit() {
            assert!(matches!(
                DurationValue::parse("1"),
                Err(DurationError::MissingUnit { .. })
            ));
        }

        #[test]
        fn returns_unknown_unit_for_unrecognized_unit() {
            assert!(matches!(
                DurationValue::parse("1x"),
                Err(DurationError::UnknownUnit { .. })
            ));
        }

        #[test]
        fn error_message_for_unknown_unit() {
            let err = DurationValue::parse("1fortnight").unwrap_err();
            assert_eq!(err.to_string(), "unknown unit in `1fortnight`");
        }

        #[test]
        fn error_message_for_missing_number() {
            let err = DurationValue::parse("hours").unwrap_err();
            assert_eq!(err.to_string(), "no number before unit in `hours`");
        }

        #[test]
        fn error_message_for_missing_unit() {
            let err = DurationValue::parse("42").unwrap_err();
            assert_eq!(err.to_string(), "no unit after number in `42`");
        }

        #[test]
        fn returns_none_for_unit_longer_than_16_bytes() {
            let long_unit = "a".repeat(17);
            let input = format!("1{long_unit}");
            assert!(DurationValue::parse(&input).is_err());
        }
    }

    mod parse_unit_name {
        use rstest::rstest;

        use super::*;

        #[test]
        fn parses_bare_unit_names() {
            let d = DurationValue::parse_unit_name("hours").unwrap();
            assert_eq!(d.to_seconds().0, 3_600.0);
            assert_eq!(d.last_unit(), Some(DurationUnit::Hour));
        }

        #[rstest]
        #[case::ms("ms")]
        #[case::s("s")]
        #[case::sec("sec")]
        #[case::m("m")]
        #[case::min("min")]
        #[case::h("h")]
        #[case::hr("hr")]
        #[case::d("d")]
        #[case::w("w")]
        #[case::wk("wk")]
        #[case::mo("mo")]
        #[case::y("y")]
        #[case::yr("yr")]
        fn parses_all_abbreviations(#[case] abbr: &str) {
            assert!(
                DurationValue::parse_unit_name(abbr).is_some(),
                "failed for abbreviation: {abbr}"
            );
        }

        #[test]
        fn is_case_insensitive() {
            assert!(DurationValue::parse_unit_name("H").is_some());
            assert!(DurationValue::parse_unit_name("Hours").is_some());
            assert!(DurationValue::parse_unit_name("HOURS").is_some());
        }

        #[test]
        fn rejects_unknown() {
            assert!(DurationValue::parse_unit_name("foo").is_none());
        }
    }

    mod duration_unit_parse {
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::single_upper("H", DurationUnit::Hour)]
        #[case::all_upper("HOUR", DurationUnit::Hour)]
        #[case::lower_plural("hours", DurationUnit::Hour)]
        fn is_case_insensitive(
            #[case] input: &str,
            #[case] expected: DurationUnit,
        ) {
            assert_eq!(DurationUnit::parse(input).unwrap(), expected);
        }

        #[test]
        fn rejects_unknown() {
            assert!(DurationUnit::parse("foo").is_none());
        }

        #[test]
        fn returns_none_for_string_exactly_16_bytes() {
            let exactly_16 = "a".repeat(16);
            assert!(DurationUnit::parse(&exactly_16).is_none());
        }

        #[test]
        fn returns_none_for_string_longer_than_16_bytes() {
            let long = "a".repeat(17);
            assert!(DurationUnit::parse(&long).is_none());
        }

        #[test]
        fn returns_none_for_empty_string() {
            assert!(DurationUnit::parse("").is_none());
        }
    }

    mod from_str {
        use super::*;

        #[test]
        fn roundtrips_valid_input() {
            let d: DurationValue = "1h 30m".parse().unwrap();
            assert_eq!(d.to_seconds().0, 5_400.0);
        }

        #[test]
        fn returns_error_for_invalid_input() {
            let err = "not a duration".parse::<DurationValue>().unwrap_err();
            assert!(matches!(err, DurationError::MissingNumber { .. }));
        }

        #[test]
        fn error_display_contains_context() {
            let err = "1fortnight".parse::<DurationValue>().unwrap_err();
            assert_eq!(err.to_string(), "unknown unit in `1fortnight`");
        }

        #[test]
        fn returns_empty_error_for_empty_input() {
            let err = "".parse::<DurationValue>().unwrap_err();
            assert!(matches!(err, DurationError::Empty));
        }
    }

    mod registry_consistency {
        use super::*;

        #[test]
        fn unit_map_covers_all_unit_types() {
            let mut seen = std::collections::HashSet::new();
            for entry in UNIT_MAP.entries() {
                seen.insert(*entry.1);
            }
            assert_eq!(seen.len(), 8);
        }

        #[test]
        fn seconds_are_consistent() {
            for entry in UNIT_MAP.entries() {
                let secs = entry.1.seconds();
                assert!(secs > 0.0, "{:?} has non-positive seconds", entry.0);
            }
        }

        #[test]
        fn seconds_i64_matches_seconds() {
            let units = [
                (DurationUnit::Millisecond, None),
                (DurationUnit::Second, Some(1)),
                (DurationUnit::Minute, Some(60)),
                (DurationUnit::Hour, Some(3_600)),
                (DurationUnit::Day, Some(86_400)),
                (DurationUnit::Week, Some(604_800)),
                (DurationUnit::Month, Some(2_592_000)),
                (DurationUnit::Year, Some(31_536_000)),
            ];
            for (unit, expected) in units {
                assert_eq!(unit.seconds_i64(), expected);
            }
        }

        #[test]
        fn parse_roundtrips_each_unit() {
            let spellings: &[(&str, DurationUnit)] = &[
                ("ms", DurationUnit::Millisecond),
                ("s", DurationUnit::Second),
                ("m", DurationUnit::Minute),
                ("h", DurationUnit::Hour),
                ("d", DurationUnit::Day),
                ("w", DurationUnit::Week),
                ("mo", DurationUnit::Month),
                ("y", DurationUnit::Year),
            ];
            for &(spelling, expected) in spellings {
                assert_eq!(
                    DurationUnit::parse(spelling).unwrap(),
                    expected,
                    "roundtrip failed for {spelling}"
                );
            }
        }
    }
}
