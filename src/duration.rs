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
