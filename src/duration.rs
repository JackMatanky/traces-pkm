use std::fmt;

/// A duration measured in seconds.
///
/// Wraps `f64` with NaN-safe ordering and arithmetic traits.
/// Constructed from [`DurationValue::to_seconds`] or via conversion traits.
///
/// # Examples
///
/// ```
/// use traces_pkm::duration::{DurationSeconds, DurationValue};
///
/// let a = DurationValue::parse("1h").unwrap().to_seconds();
/// let b = DurationValue::parse("30m").unwrap().to_seconds();
/// assert_eq!(a + b, DurationSeconds::from(5400.0));
/// assert!(a > b);
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DurationSeconds(f64);

impl DurationSeconds {
    /// Returns the inner `f64` value.
    #[inline]
    #[must_use]
    pub const fn as_f64(self) -> f64 {
        self.0
    }

    /// Returns the duration as a whole number of seconds (truncated).
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
    fn from(d: DurationSeconds) -> f64 {
        d.0
    }
}

impl From<DurationSeconds> for i64 {
    fn from(d: DurationSeconds) -> i64 {
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

/// Error returned when parsing a duration string fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurationParseError;

impl fmt::Display for DurationParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid duration spelling")
    }
}

impl std::error::Error for DurationParseError {}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
enum DurationUnit {
    Millisecond,
    Second,
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Year,
}

/// Source of truth: `(spelling, kind, seconds_per_unit, is_fixed_length)`.
///
/// Every other registry is derived from this. Add new spellings here only.
const DURATIONS: &[(&str, DurationUnit, f64, bool)] = &[
    ("ms", DurationUnit::Millisecond, 0.001, true),
    ("millisecond", DurationUnit::Millisecond, 0.001, true),
    ("milliseconds", DurationUnit::Millisecond, 0.001, true),
    ("s", DurationUnit::Second, 1.0, true),
    ("sec", DurationUnit::Second, 1.0, true),
    ("secs", DurationUnit::Second, 1.0, true),
    ("second", DurationUnit::Second, 1.0, true),
    ("seconds", DurationUnit::Second, 1.0, true),
    ("m", DurationUnit::Minute, 60.0, true),
    ("min", DurationUnit::Minute, 60.0, true),
    ("mins", DurationUnit::Minute, 60.0, true),
    ("minute", DurationUnit::Minute, 60.0, true),
    ("minutes", DurationUnit::Minute, 60.0, true),
    ("h", DurationUnit::Hour, 3_600.0, true),
    ("hr", DurationUnit::Hour, 3_600.0, true),
    ("hrs", DurationUnit::Hour, 3_600.0, true),
    ("hour", DurationUnit::Hour, 3_600.0, true),
    ("hours", DurationUnit::Hour, 3_600.0, true),
    ("d", DurationUnit::Day, 86_400.0, true),
    ("day", DurationUnit::Day, 86_400.0, true),
    ("days", DurationUnit::Day, 86_400.0, true),
    ("w", DurationUnit::Week, 604_800.0, true),
    ("wk", DurationUnit::Week, 604_800.0, true),
    ("wks", DurationUnit::Week, 604_800.0, true),
    ("week", DurationUnit::Week, 604_800.0, true),
    ("weeks", DurationUnit::Week, 604_800.0, true),
    ("mo", DurationUnit::Month, 2_592_000.0, false),
    ("mos", DurationUnit::Month, 2_592_000.0, false),
    ("month", DurationUnit::Month, 2_592_000.0, false),
    ("months", DurationUnit::Month, 2_592_000.0, false),
    ("y", DurationUnit::Year, 31_536_000.0, false),
    ("yr", DurationUnit::Year, 31_536_000.0, false),
    ("yrs", DurationUnit::Year, 31_536_000.0, false),
    ("year", DurationUnit::Year, 31_536_000.0, false),
    ("years", DurationUnit::Year, 31_536_000.0, false),
];

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

/// [`DurationUnit`] → seconds per unit.
const DURATIONS_TO_SECONDS: &[(DurationUnit, f64)] = &[
    (DurationUnit::Millisecond, 0.001),
    (DurationUnit::Second, 1.0),
    (DurationUnit::Minute, 60.0),
    (DurationUnit::Hour, 3_600.0),
    (DurationUnit::Day, 86_400.0),
    (DurationUnit::Week, 604_800.0),
    (DurationUnit::Month, 2_592_000.0),
    (DurationUnit::Year, 31_536_000.0),
];

/// [`DurationUnit`] → all recognized spellings for that unit.
const UNIT_NAMES: &[(DurationUnit, &[&str])] = &[
    (DurationUnit::Millisecond, &["ms", "millisecond", "milliseconds"]),
    (DurationUnit::Second, &["s", "sec", "secs", "second", "seconds"]),
    (DurationUnit::Minute, &["m", "min", "mins", "minute", "minutes"]),
    (DurationUnit::Hour, &["h", "hr", "hrs", "hour", "hours"]),
    (DurationUnit::Day, &["d", "day", "days"]),
    (DurationUnit::Week, &["w", "wk", "wks", "week", "weeks"]),
    (DurationUnit::Month, &["mo", "mos", "month", "months"]),
    (DurationUnit::Year, &["y", "yr", "yrs", "year", "years"]),
];

/// Case-insensitive lookup of a unit string.
pub(crate) fn parse_unit(unit: &str) -> Option<DurationUnit> {
    let mut buf = [0u8; 16];
    let slice = buf.get_mut(..unit.len())?;
    slice.copy_from_slice(unit.as_bytes());
    slice.make_ascii_lowercase();
    let lower = core::str::from_utf8(slice).ok()?;
    UNIT_MAP.get(lower).copied()
}

/// Seconds per [`DurationUnit`].
const fn unit_seconds(unit: DurationUnit) -> f64 {
    match unit {
        DurationUnit::Millisecond => 0.001,
        DurationUnit::Second => 1.0,
        DurationUnit::Minute => 60.0,
        DurationUnit::Hour => 3_600.0,
        DurationUnit::Day => 86_400.0,
        DurationUnit::Week => 604_800.0,
        DurationUnit::Month => 2_592_000.0,
        DurationUnit::Year => 31_536_000.0,
    }
}

/// Canonical singular name for a [`DurationUnit`].
const fn unit_name_of(unit: DurationUnit) -> &'static str {
    match unit {
        DurationUnit::Millisecond => "millisecond",
        DurationUnit::Second => "second",
        DurationUnit::Minute => "minute",
        DurationUnit::Hour => "hour",
        DurationUnit::Day => "day",
        DurationUnit::Week => "week",
        DurationUnit::Month => "month",
        DurationUnit::Year => "year",
    }
}

/// A validated duration expression.
///
/// Constructed only via [`DurationValue::parse`] or
/// [`DurationValue::parse_unit_name`]. Carries both the raw source text
/// and the parsed total seconds.
///
/// # Examples
///
/// ```
/// use traces_pkm::duration::DurationValue;
///
/// let dur = DurationValue::parse("1h 30m").expect("valid");
/// assert_eq!(dur.to_seconds().as_f64(), 5400.0);
/// assert_eq!(dur.as_raw(), "1h 30m");
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct DurationValue {
    raw: String,
    seconds: DurationSeconds,
}

impl DurationValue {
    /// Parses a duration spelling (e.g., `"1h 30m"`, `"4 hrs"`).
    ///
    /// Returns `None` if the spelling is empty, contains no valid parts,
    /// or has unrecognized units.
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

            let kind = parse_unit(unit_str)?;
            total += number * unit_seconds(kind);
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
        let kind = parse_unit(name)?;
        Some(Self {
            raw: name.to_owned(),
            seconds: DurationSeconds(unit_seconds(kind)),
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
    pub fn as_raw(&self) -> &str {
        &self.raw
    }

    /// Returns `Some(DurationSeconds)` for fixed-length units, `None` for
    /// variable-length units (months, years). For multi-part durations,
    /// returns the last unit's value.
    #[must_use]
    pub fn diff_seconds(&self) -> Option<DurationSeconds> {
        let last = self.last_unit()?;
        match last {
            DurationUnit::Millisecond => Some(DurationSeconds(0.0)),
            DurationUnit::Second => Some(DurationSeconds(1.0)),
            DurationUnit::Minute => Some(DurationSeconds(60.0)),
            DurationUnit::Hour => Some(DurationSeconds(3_600.0)),
            DurationUnit::Day => Some(DurationSeconds(86_400.0)),
            DurationUnit::Week => Some(DurationSeconds(604_800.0)),
            DurationUnit::Month | DurationUnit::Year => None,
        }
    }

    /// Returns the canonical name of the last parsed unit (e.g., `"hour"`).
    #[must_use]
    pub fn unit_name(&self) -> &str {
        self.last_unit()
            .or_else(|| parse_unit(&self.raw))
            .map(unit_name_of)
            .unwrap_or("")
    }

    /// Scans backward from the end of `raw` to extract the last unit string.
    fn last_unit(&self) -> Option<DurationUnit> {
        let bytes = self.raw.as_bytes();
        let len = bytes.len();
        let mut pos = len;
        while pos > 0 && bytes[pos - 1].is_ascii_alphabetic() {
            pos -= 1;
        }
        if pos == len || pos == 0 {
            return None;
        }
        let unit_str = core::str::from_utf8(&bytes[pos..]).ok()?;
        parse_unit(unit_str)
    }
}

impl From<DurationValue> for DurationSeconds {
    fn from(d: DurationValue) -> Self {
        d.seconds
    }
}

/// Total seconds for a duration spelling like `"1h 30m"`.
///
/// Equivalent to `DurationValue::parse(spelling).map(DurationSeconds::from)`.
/// Prefer [`DurationValue::parse`] when the validated value is reused.
#[inline]
#[must_use]
pub fn duration_seconds(spelling: &str) -> Option<DurationSeconds> {
    DurationValue::parse(spelling).map(|d| d.to_seconds())
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

    mod parse_unit_helper {
        use super::*;

        #[test]
        fn accepts_all_recognized_spellings() {
            for &(spelling, kind, _, _) in DURATIONS {
                assert_eq!(parse_unit(spelling).unwrap(), kind, "{spelling}");
            }
        }

        #[test]
        fn is_case_insensitive() {
            assert_eq!(parse_unit("H").unwrap(), DurationUnit::Hour);
            assert_eq!(parse_unit("HOUR").unwrap(), DurationUnit::Hour);
        }
    }

    mod registry_consistency {
        use super::*;

        #[test]
        fn durations_covers_all_unit_types() {
            let mut seen = std::collections::HashSet::new();
            for &(_, kind, _, _) in DURATIONS {
                seen.insert(kind);
            }
            assert_eq!(seen.len(), 8);
        }

        #[test]
        fn durations_seconds_match_durations_to_seconds() {
            for &(kind, expected) in DURATIONS_TO_SECONDS {
                for &(_, k, secs, _) in
                    DURATIONS.iter().filter(|&&(_, k, _, _)| k == kind)
                {
                    assert_eq!(secs, expected, "disagree for {k:?}");
                }
            }
        }

        #[test]
        fn durations_names_match_unit_names() {
            for &(kind, expected_names) in UNIT_NAMES {
                let actual: Vec<&str> = DURATIONS
                    .iter()
                    .filter(|&&(_, k, _, _)| k == kind)
                    .map(|&(s, _, _, _)| s)
                    .collect();
                assert_eq!(
                    actual.as_slice(),
                    expected_names,
                    "disagree for {kind:?}"
                );
            }
        }
    }

    mod template_consistency {
        use super::*;

        #[test]
        fn all_unit_names_parse_as_single_part() {
            for &(kind, names) in UNIT_NAMES {
                let canonical = unit_name_of(kind);
                for name in names {
                    let d = DurationValue::parse_unit_name(name)
                        .unwrap_or_else(|| panic!("{name} must parse"));
                    assert_eq!(d.unit_name(), canonical, "for {name}");
                }
            }
        }
    }
}
