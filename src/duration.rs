//! Duration parsing, validation, and computation.
//!
//! Three-layer design:
//!
//! - **Source text** (`&str`) — raw input from parser or frontmatter.
//! - **Validated** ([`DurationValue`]) — parsed, carries raw text + total
//!   seconds.
//! - **Computable** ([`DurationSeconds`]) — typed `f64` with `Ord`, `Add`,
//!   `Sub`, `Mul`.
//!
//! All duration unit knowledge lives in [`DurationUnit`]. Callers should not
//! maintain their own unit registries.

use std::fmt;

/// A duration measured in seconds.
///
/// Wraps `f64` with NaN-safe ordering and arithmetic traits. Constructed from
/// [`DurationValue::to_seconds`] or via conversion traits. Always finite —
/// callers must not construct with `NaN` or infinity.
#[derive(Copy, Clone, Debug)]
pub(crate) struct DurationSeconds(f64);

impl PartialEq for DurationSeconds {
    fn eq(&self, other: &Self) -> bool {
        self.0.total_cmp(&other.0) == std::cmp::Ordering::Equal
    }
}

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
    pub const fn as_i64(self) -> i64 {
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "duration seconds truncated to i64 for template date \
                      arithmetic"
        )]
        {
            self.0 as i64
        }
    }
}

impl From<f64> for DurationSeconds {
    fn from(secs: f64) -> Self {
        Self(secs)
    }
}

impl From<DurationSeconds> for f64 {
    fn from(d: DurationSeconds) -> Self {
        d.0
    }
}

impl From<DurationSeconds> for i64 {
    fn from(d: DurationSeconds) -> Self {
        d.as_i64()
    }
}

impl fmt::Display for DurationSeconds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Eq for DurationSeconds {}

impl PartialOrd for DurationSeconds {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DurationSeconds {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl std::ops::Add for DurationSeconds {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl std::ops::Sub for DurationSeconds {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl std::ops::Mul<f64> for DurationSeconds {
    type Output = Self;

    fn mul(self, rhs: f64) -> Self {
        Self(self.0 * rhs)
    }
}

impl std::ops::Mul<DurationSeconds> for f64 {
    type Output = DurationSeconds;

    fn mul(self, rhs: DurationSeconds) -> DurationSeconds {
        DurationSeconds(self * rhs.0)
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
    /// Returns `None` if the spelling is empty, contains no valid parts, or has
    /// unrecognized units.
    pub fn parse(spelling: &str) -> Option<Self> {
        let bytes = spelling.as_bytes();
        let len = bytes.len();
        let mut pos = 0;
        let mut total = 0.0f64;
        let mut parsed_any = false;

        while pos < len {
            // skip separators (whitespace, commas)
            while pos < len
                && (bytes[pos].is_ascii_whitespace() || bytes[pos] == b',')
            {
                pos += 1;
            }
            if pos >= len {
                break;
            }

            // parse number
            let num_start = pos;
            let mut has_decimal = false;
            while pos < len {
                if bytes[pos].is_ascii_digit() {
                    pos += 1;
                } else if bytes[pos] == b'.' && !has_decimal {
                    has_decimal = true;
                    pos += 1;
                } else {
                    break;
                }
            }
            if num_start == pos {
                return None;
            }
            let number: f64 = core::str::from_utf8(&bytes[num_start..pos])
                .ok()?
                .parse()
                .ok()?;
            if !number.is_finite() {
                return None;
            }

            // skip whitespace between number and unit
            while pos < len && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }

            // parse unit
            let unit_start = pos;
            while pos < len && bytes[pos].is_ascii_alphabetic() {
                pos += 1;
            }
            if unit_start == pos {
                return None;
            }
            let unit_str =
                core::str::from_utf8(&bytes[unit_start..pos]).ok()?;

            let kind = DurationUnit::parse(unit_str)?;
            total += number * kind.seconds();
            parsed_any = true;
        }

        parsed_any.then(|| Self {
            raw: spelling.to_owned(),
            seconds: DurationSeconds(total),
        })
    }

    /// Parses a bare unit name (e.g., `"hours"`, `"d"`) as a single-part
    /// duration with an implicit quantity of 1. Used by the template engine.
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
    #[allow(
        dead_code,
        reason = "used in tests and doc examples within this module"
    )]
    pub(crate) fn as_raw(&self) -> &str {
        &self.raw
    }

    /// Returns `Some(DurationSeconds)` for fixed-length units, `None` for
    /// variable-length units (months, years). For multi-part durations, returns
    /// the last unit's value.
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

    /// Returns the canonical name of the last parsed unit (e.g., `"hour"`).
    #[must_use]
    pub fn unit_name(&self) -> &str {
        self.last_unit()
            .or_else(|| DurationUnit::parse(&self.raw))
            .map_or("", DurationUnit::name)
    }

    /// Scans backward from the end of `raw` to extract the last unit string.
    fn last_unit(&self) -> Option<DurationUnit> {
        let bytes = self.raw.as_bytes();
        let len = bytes.len();
        let mut pos = len;
        while pos > 0 && bytes[pos - 1].is_ascii_alphabetic() {
            pos -= 1;
        }
        // pos == len: no trailing alphabetic chars (e.g. "1" or "")
        // pos == 0: entire string is alphabetic (e.g. "days")
        // pos > 0: trailing unit after digits (e.g. "1h")
        if pos == len {
            return None;
        }
        let unit_str = core::str::from_utf8(&bytes[pos..]).ok()?;
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
/// Returns [`DurationError::Parse`] if the string is empty, contains no valid
/// parts, or has unrecognized units.
impl std::str::FromStr for DurationValue {
    type Err = DurationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or(DurationError::Parse {
            input: s.to_owned(),
        })
    }
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
        }

        #[test]
        fn from_f64_roundtrip() {
            let d = DurationSeconds::from(42.5);
            let f: f64 = d.into();
            assert_eq!(f, 42.5);
        }

        #[test]
        fn from_duration_value() {
            let dv = DurationValue::parse("1h").unwrap();
            let ds: DurationSeconds = dv.into();
            assert_eq!(ds.as_f64(), 3_600.0);
        }
    }

    mod duration_value_parse {
        use super::*;

        #[test]
        fn parses_single_part() {
            assert_eq!(
                DurationValue::parse("1h").unwrap().to_seconds().as_f64(),
                3_600.0
            );
            assert_eq!(
                DurationValue::parse("30m").unwrap().to_seconds().as_f64(),
                1_800.0
            );
            assert_eq!(
                DurationValue::parse("500ms").unwrap().to_seconds().as_f64(),
                0.5
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
        fn returns_none_for_empty() {
            assert!(DurationValue::parse("").is_none());
        }

        #[test]
        fn returns_none_for_whitespace_only() {
            assert!(DurationValue::parse("   ").is_none());
        }

        #[test]
        fn returns_none_for_invalid_unit() {
            assert!(DurationValue::parse("1h invalid").is_none());
        }

        #[test]
        fn returns_none_for_no_unit() {
            assert!(DurationValue::parse("1").is_none());
        }

        #[test]
        fn returns_none_for_no_number() {
            assert!(DurationValue::parse("h").is_none());
        }
    }

    mod duration_value_diff_seconds {
        use super::*;

        #[test]
        fn returns_some_for_fixed_units() {
            assert!(
                DurationValue::parse("1h").unwrap().diff_seconds().is_some()
            );
            assert!(
                DurationValue::parse("30d").unwrap().diff_seconds().is_some()
            );
        }

        #[test]
        fn returns_none_for_variable_units() {
            assert!(
                DurationValue::parse("3mo").unwrap().diff_seconds().is_none()
            );
            assert!(
                DurationValue::parse("2y").unwrap().diff_seconds().is_none()
            );
        }

        #[test]
        fn returns_correct_seconds_for_fixed_units() {
            let h = DurationValue::parse("1h").unwrap().diff_seconds().unwrap();
            assert_eq!(h.as_f64(), 3_600.0);
        }
    }

    mod duration_value_unit_name {
        use super::*;

        #[test]
        fn returns_canonical_name_for_single_unit() {
            assert_eq!(DurationValue::parse("1h").unwrap().unit_name(), "hour");
            assert_eq!(DurationValue::parse("30d").unwrap().unit_name(), "day");
        }

        #[test]
        fn returns_last_unit_for_multi_part() {
            assert_eq!(
                DurationValue::parse("1h 30m").unwrap().unit_name(),
                "minute"
            );
        }
    }

    mod parse_unit_name {
        use super::*;

        #[test]
        fn parses_bare_unit_names() {
            let d = DurationValue::parse_unit_name("hours").unwrap();
            assert_eq!(d.to_seconds().as_f64(), 3_600.0);
            assert_eq!(d.as_raw(), "hours");
        }

        #[test]
        fn is_case_insensitive() {
            assert!(DurationValue::parse_unit_name("H").is_some());
            assert!(DurationValue::parse_unit_name("Hours").is_some());
        }

        #[test]
        fn rejects_unknown() {
            assert!(DurationValue::parse_unit_name("foo").is_none());
        }
    }

    mod duration_unit_parse {
        use super::*;

        #[test]
        fn is_case_insensitive() {
            assert_eq!(DurationUnit::parse("H").unwrap(), DurationUnit::Hour);
            assert_eq!(
                DurationUnit::parse("HOUR").unwrap(),
                DurationUnit::Hour
            );
            assert_eq!(
                DurationUnit::parse("hours").unwrap(),
                DurationUnit::Hour
            );
        }

        #[test]
        fn rejects_unknown() {
            assert!(DurationUnit::parse("foo").is_none());
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
    }
}
