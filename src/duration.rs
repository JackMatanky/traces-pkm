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
