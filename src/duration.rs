//! Duration parsing, validation, and computation.
//!
//! Three-layer design:
//!
//! - **Source text** (`&str`), raw input from parser or frontmatter.
//! - **Validated** ([`DurationValue`]), parsed, carries raw text + total
//!   seconds.
//! - **Computable** ([`DurationSeconds`]), typed `f64` with [`Ord`], [`Add`],
//!   [`Sub`], [`Mul`].
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
/// [`DurationValue::parse_unit_name`]. Carries both the raw source text and the
/// parsed total seconds.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DurationValue {
    raw: String,
    seconds: DurationSeconds,
}

impl DurationValue {
    /// Parses a duration spelling (e.g., `"1h 30m"`, `"4 hrs"`).
    ///
    /// Accepts one or more `<number><unit>` parts separated by whitespace or
    /// commas. Returns `None` if the spelling is empty, contains no valid
    /// parts, or has unrecognized units.
    pub fn parse(spelling: &str) -> Option<Self> {
        let bytes = spelling.as_bytes();
        let len = bytes.len();
        let mut pos = 0;
        let mut total = 0.0f64;
        let mut parsed_any = false;

        while pos < len {
            // skip separators (whitespace, commas)
            while pos < len {
                let b = *bytes.get(pos)?;
                if !b.is_ascii_whitespace() && b != b',' {
                    break;
                }
                pos = pos.saturating_add(1);
            }
            if pos >= len {
                break;
            }

            // parse number
            let num_start = pos;
            let mut has_decimal = false;
            while pos < len {
                let b = *bytes.get(pos)?;
                if b.is_ascii_digit() {
                    pos = pos.saturating_add(1);
                } else if b == b'.' && !has_decimal {
                    has_decimal = true;
                    pos = pos.saturating_add(1);
                } else {
                    break;
                }
            }
            if num_start == pos {
                return None;
            }
            let number: f64 = core::str::from_utf8(bytes.get(num_start..pos)?)
                .ok()?
                .parse()
                .ok()?;
            if !number.is_finite() {
                return None;
            }

            // skip whitespace between number and unit
            while pos < len
                && bytes.get(pos).is_some_and(u8::is_ascii_whitespace)
            {
                pos = pos.saturating_add(1);
            }

            // parse unit
            let unit_start = pos;
            while pos < len
                && bytes.get(pos).is_some_and(u8::is_ascii_alphabetic)
            {
                pos = pos.saturating_add(1);
            }
            if unit_start == pos {
                return None;
            }
            let unit_str =
                core::str::from_utf8(bytes.get(unit_start..pos)?).ok()?;

            let kind = DurationUnit::parse(unit_str)?;
            total += number * kind.seconds();
            parsed_any = true;
        }

        parsed_any.then(|| Self {
            raw: spelling.to_owned(),
            seconds: DurationSeconds(total),
        })
    }

    /// Parses a bare unit name as a single-part duration with quantity 1.
    ///
    /// Accepts any spelling recognized by [`DurationUnit::parse`] (e.g.,
    /// `"hours"`, `"d"`, `"sec"`). Used by the template engine for
    /// date-shift operations.
    pub(crate) fn parse_unit_name(name: &str) -> Option<Self> {
        let kind = DurationUnit::parse(name)?;
        Some(Self {
            raw: name.to_owned(),
            seconds: DurationSeconds(kind.seconds()),
        })
    }

    /// Returns the total duration as [`DurationSeconds`].
    #[inline]
    #[must_use]
    pub const fn to_seconds(&self) -> DurationSeconds {
        self.seconds
    }

    /// Returns the raw source spelling.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; documented deliberate \
                      API, split from the fields() iterator that is used"
        )
    )]
    pub(crate) fn as_raw(&self) -> &str {
        &self.raw
    }

    /// Returns the seconds value for fixed-length units, or `None` for
    /// variable-length units (months, years).
    ///
    /// For multi-part durations, returns the last unit's value.
    #[must_use]
    pub fn diff_seconds(&self) -> Option<DurationSeconds> {
        let unit = self.last_unit()?;
        match unit {
            DurationUnit::Millisecond
            | DurationUnit::Second
            | DurationUnit::Minute
            | DurationUnit::Hour
            | DurationUnit::Day
            | DurationUnit::Week => Some(DurationSeconds(unit.seconds())),
            DurationUnit::Month | DurationUnit::Year => None,
        }
    }

    /// Returns the canonical singular name of the last parsed unit.
    ///
    /// For `"1h 30m"`, returns `"minute"` (the last unit). Falls back to
    /// parsing the raw text as a single unit name.
    #[must_use]
    pub fn unit_name(&self) -> &str {
        self.last_unit()
            .or_else(|| DurationUnit::parse(&self.raw))
            .map_or("", DurationUnit::name)
    }

    /// Scans backward from the end of `raw` to extract the trailing unit
    /// string. Returns `None` if the raw text has no trailing alphabetic
    /// characters (e.g., `"1"` or `""`).
    fn last_unit(&self) -> Option<DurationUnit> {
        let bytes = self.raw.as_bytes();
        let len = bytes.len();
        let mut pos = len;
        while pos > 0
            && bytes
                .get(pos.saturating_sub(1))
                .is_some_and(u8::is_ascii_alphabetic)
        {
            pos = pos.saturating_sub(1);
        }
        // pos == len: no trailing alphabetic chars (e.g. "1" or "")
        // pos == 0: entire string is alphabetic (e.g. "days")
        // pos > 0: trailing unit after digits (e.g. "1h")
        if pos == len {
            return None;
        }
        let unit_str = core::str::from_utf8(bytes.get(pos..)?).ok()?;
        DurationUnit::parse(unit_str)
    }
}

impl From<DurationValue> for DurationSeconds {
    fn from(d: DurationValue) -> Self {
        d.seconds
    }
}

/// Enables `"1h 30m".parse::<DurationValue>()`.
///
/// # Errors
///
/// - [`DurationError::Parse`] if the string is empty, contains no valid parts,
///   or has unrecognized units.
impl FromStr for DurationValue {
    type Err = DurationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or_else(|| DurationError::Parse {
            input: s.to_owned(),
        })
    }
}

/// A recognized duration unit.
///
/// Single source of truth for unit parsing, seconds conversion, and naming.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
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
    pub const fn seconds(self) -> f64 {
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

    /// Canonical singular name (e.g., `"hour"`, `"month"`).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Millisecond => "millisecond",
            Self::Second => "second",
            Self::Minute => "minute",
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",
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
/// Wraps `f64` with NaN-safe ordering and arithmetic traits. Constructed from
/// [`DurationValue::to_seconds`] or via conversion traits. Always finite;
/// callers must not construct with `NaN` or infinity.
#[derive(Copy, Clone, Debug)]
pub(crate) struct DurationSeconds(f64);

impl DurationSeconds {
    /// Returns the inner `f64` value.
    #[inline]
    #[must_use]
    pub const fn as_f64(self) -> f64 {
        self.0
    }

    /// Returns the duration as a whole number of seconds (truncated toward
    /// zero). Fractional seconds are discarded.
    #[inline]
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        clippy::as_conversions,
        reason = "f64-to-i64 has no const safe alternative in std; callers \
                  bound values to duration-range magnitudes"
    )]
    pub fn as_i64(self) -> i64 {
        self.0.trunc() as i64
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

impl From<f64> for DurationSeconds {
    fn from(secs: f64) -> Self {
        Self(secs)
    }
}

impl From<DurationSeconds> for f64 {
    #[inline]
    fn from(d: DurationSeconds) -> Self {
        d.0
    }
}

impl From<DurationSeconds> for i64 {
    #[inline]
    fn from(d: DurationSeconds) -> Self {
        d.as_i64()
    }
}

/// Error returned when a duration operation fails.
///
/// Raised by [`DurationValue::from_str`] when the input cannot be parsed.
#[derive(Debug, Clone, thiserror::Error)]
pub enum DurationError {
    /// The input string is not a valid duration spelling.
    #[error("invalid duration `{input}`")]
    Parse {
        /// The raw input that failed to parse.
        input: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    mod duration_seconds_ops {
        use super::*;

        #[test]
        fn add_combines_seconds() {
            let a = DurationSeconds::from(100.0);
            let b = DurationSeconds::from(200.0);
            assert_eq!(a + b, DurationSeconds::from(300.0));
        }

        #[test]
        fn sub_subtracts_seconds() {
            let a = DurationSeconds::from(300.0);
            let b = DurationSeconds::from(100.0);
            assert_eq!(a - b, DurationSeconds::from(200.0));
        }

        #[test]
        fn mul_scales_seconds() {
            let a = DurationSeconds::from(100.0);
            assert_eq!(a * 3.0, DurationSeconds::from(300.0));
            assert_eq!(3.0 * a, DurationSeconds::from(300.0));
        }

        #[test]
        fn ord_uses_total_cmp() {
            let a = DurationSeconds::from(100.0);
            let b = DurationSeconds::from(200.0);
            assert!(a < b);
            assert!(b > a);
            assert_eq!(a, DurationSeconds::from(100.0));
        }

        #[test]
        fn nan_cmp_is_equal() {
            let nan = DurationSeconds::from(f64::NAN);
            assert_eq!(nan, nan);
        }

        #[test]
        fn nan_ordering_is_total_cmp() {
            let nan = DurationSeconds::from(f64::NAN);
            let zero = DurationSeconds::from(0.0);
            assert_eq!(nan.cmp(&zero), Ordering::Greater);
            assert_eq!(zero.cmp(&nan), Ordering::Less);
        }

        #[test]
        fn from_f64_roundtrip() {
            let d = DurationSeconds::from(42.5);
            let f: f64 = d.into();
            assert_eq!(f, 42.5);
        }

        #[test]
        fn from_f64_zero() {
            let d = DurationSeconds::from(0.0);
            assert_eq!(d.as_f64(), 0.0);
        }

        #[test]
        fn from_duration_value() {
            let dv = DurationValue::parse("1h").unwrap();
            let ds: DurationSeconds = dv.into();
            assert_eq!(ds.as_f64(), 3_600.0);
        }

        #[test]
        fn as_i64_truncates_toward_zero() {
            assert_eq!(DurationSeconds::from(1.9).as_i64(), 1);
            assert_eq!(DurationSeconds::from(-1.9).as_i64(), -1);
            assert_eq!(DurationSeconds::from(0.0).as_i64(), 0);
        }

        #[test]
        fn display_formats_as_number() {
            assert_eq!(DurationSeconds::from(3600.0).to_string(), "3600");
            assert_eq!(DurationSeconds::from(0.5).to_string(), "0.5");
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
                DurationValue::parse(input).unwrap().to_seconds().as_f64(),
                expected
            );
        }

        #[test]
        fn parses_multi_part_with_space() {
            let d = DurationValue::parse("1h 30m").unwrap();
            assert_eq!(d.to_seconds().as_f64(), 5_400.0);
            assert_eq!(d.as_raw(), "1h 30m");
        }

        #[test]
        fn parses_multi_part_without_separator() {
            assert_eq!(
                DurationValue::parse("1h30m").unwrap().to_seconds().as_f64(),
                5_400.0
            );
        }

        #[test]
        fn parses_multi_part_with_comma() {
            let d = DurationValue::parse("4 yrs, 6 wks").unwrap();
            assert_eq!(
                d.to_seconds().as_f64(),
                4.0 * 31_536_000.0 + 6.0 * 604_800.0
            );
        }

        #[test]
        fn parses_decimal_numbers() {
            assert_eq!(
                DurationValue::parse("1.5h").unwrap().to_seconds().as_f64(),
                5_400.0
            );
        }

        #[test]
        fn parses_decimal_without_leading_digit() {
            assert_eq!(
                DurationValue::parse(".5h").unwrap().to_seconds().as_f64(),
                1_800.0
            );
        }

        #[test]
        fn parses_with_leading_trailing_whitespace() {
            let d = DurationValue::parse(" 1h ").unwrap();
            assert_eq!(d.to_seconds().as_f64(), 3_600.0);
            assert_eq!(d.as_raw(), " 1h ");
        }

        #[test]
        fn parses_with_multiple_consecutive_separators() {
            let d = DurationValue::parse("1  h  30  m").unwrap();
            assert_eq!(d.to_seconds().as_f64(), 5_400.0);
        }

        #[rstest]
        #[case::empty("")]
        #[case::whitespace_only("   ")]
        #[case::invalid_unit("1h invalid")]
        #[case::no_unit("1")]
        #[case::no_number("h")]
        #[case::multiple_decimals("1.2.3h")]
        #[case::only_separators_and_unit(", h")]
        fn returns_none_for_invalid_input(#[case] input: &str) {
            assert!(DurationValue::parse(input).is_none());
        }

        #[test]
        fn returns_none_for_unit_longer_than_16_bytes() {
            let long_unit = "a".repeat(17);
            let input = format!("1{long_unit}");
            assert!(DurationValue::parse(&input).is_none());
        }
    }

    mod duration_value_diff_seconds {
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::milliseconds("500ms")]
        #[case::seconds("30s")]
        #[case::minutes("5m")]
        #[case::hours("2h")]
        #[case::days("1d")]
        #[case::weeks("1w")]
        fn returns_some_for_fixed_units(#[case] input: &str) {
            assert!(
                DurationValue::parse(input).unwrap().diff_seconds().is_some(),
                "expected Some for fixed unit: {input}"
            );
        }

        #[rstest]
        #[case::months("3mo")]
        #[case::years("2y")]
        fn returns_none_for_variable_units(#[case] input: &str) {
            assert!(
                DurationValue::parse(input).unwrap().diff_seconds().is_none(),
                "expected None for variable unit: {input}"
            );
        }

        #[rstest]
        #[case::milliseconds("500ms", 0.001)]
        #[case::seconds("30s", 1.0)]
        #[case::minutes("5m", 60.0)]
        #[case::hours("2h", 3_600.0)]
        #[case::days("1d", 86_400.0)]
        #[case::weeks("1w", 604_800.0)]
        fn returns_correct_seconds_for_each_fixed_unit(
            #[case] input: &str,
            #[case] expected: f64,
        ) {
            let got = DurationValue::parse(input)
                .unwrap()
                .diff_seconds()
                .unwrap()
                .as_f64();
            assert_eq!(got, expected);
        }

        #[test]
        fn returns_last_unit_for_multi_part() {
            let d = DurationValue::parse("1h 30m").unwrap();
            assert_eq!(d.diff_seconds().unwrap().as_f64(), 60.0);
        }
    }

    mod duration_value_unit_name {
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::hours("1h", "hour")]
        #[case::days("30d", "day")]
        #[case::minutes("5m", "minute")]
        #[case::milliseconds("100ms", "millisecond")]
        #[case::seconds("30s", "second")]
        #[case::weeks("2w", "week")]
        #[case::months("3mo", "month")]
        #[case::years("1y", "year")]
        fn returns_canonical_name_for_single_unit(
            #[case] input: &str,
            #[case] expected: &str,
        ) {
            assert_eq!(
                DurationValue::parse(input).unwrap().unit_name(),
                expected
            );
        }

        #[test]
        fn returns_last_unit_for_multi_part() {
            assert_eq!(
                DurationValue::parse("1h 30m").unwrap().unit_name(),
                "minute"
            );
        }

        #[test]
        fn returns_empty_for_no_trailing_unit() {
            let d = DurationValue {
                raw: "1".to_owned(),
                seconds: DurationSeconds(1.0),
            };
            assert_eq!(d.unit_name(), "");
        }

        #[test]
        fn falls_back_to_raw_text_parsing() {
            let d = DurationValue {
                raw: "hours".to_owned(),
                seconds: DurationSeconds(3_600.0),
            };
            assert_eq!(d.unit_name(), "hour");
        }
    }

    mod parse_unit_name {
        use rstest::rstest;

        use super::*;

        #[test]
        fn parses_bare_unit_names() {
            let d = DurationValue::parse_unit_name("hours").unwrap();
            assert_eq!(d.to_seconds().as_f64(), 3_600.0);
            assert_eq!(d.as_raw(), "hours");
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
            assert_eq!(d.to_seconds().as_f64(), 5_400.0);
        }

        #[test]
        fn returns_parse_error_for_invalid_input() {
            let err = "not a duration".parse::<DurationValue>().unwrap_err();
            assert!(matches!(
                err,
                DurationError::Parse { ref input }
                    if input == "not a duration"
            ));
        }

        #[test]
        fn error_display_contains_input() {
            let err = "bad".parse::<DurationValue>().unwrap_err();
            assert!(err.to_string().contains("bad"));
        }

        #[test]
        fn returns_parse_error_for_empty_input() {
            let err = "".parse::<DurationValue>().unwrap_err();
            assert!(matches!(
                err,
                DurationError::Parse { ref input }
                    if input.is_empty()
            ));
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
        fn names_are_unique() {
            let all = [
                DurationUnit::Millisecond,
                DurationUnit::Second,
                DurationUnit::Minute,
                DurationUnit::Hour,
                DurationUnit::Day,
                DurationUnit::Week,
                DurationUnit::Month,
                DurationUnit::Year,
            ];
            let mut seen = std::collections::HashSet::new();
            for unit in all {
                let name = unit.name();
                assert!(seen.insert(name), "duplicate name: {name}");
            }
        }

        #[test]
        fn parse_roundtrips_each_unit() {
            let all = [
                DurationUnit::Millisecond,
                DurationUnit::Second,
                DurationUnit::Minute,
                DurationUnit::Hour,
                DurationUnit::Day,
                DurationUnit::Week,
                DurationUnit::Month,
                DurationUnit::Year,
            ];
            for unit in all {
                let name = unit.name();
                assert_eq!(
                    DurationUnit::parse(name).unwrap(),
                    unit,
                    "roundtrip failed for {name}"
                );
            }
        }
    }
}
