//! Duration parsing, validation, and arithmetic.
//!
//! Accepts human-readable duration strings like `"1h 30m"` or `"4 yrs, 6 wks"`
//! and converts them to a total seconds value. Parts are `<number><unit>`
//! pairs separated by whitespace or commas.
//!
//! # Key types
//!
//! - [`DurationUnit`]: unit registry (parsing, seconds conversion, naming).
//!   Single source of truth; callers should not maintain their own registries.
//! - [`DurationValue`]: a parsed duration carrying its total seconds.
//! - [`DurationSeconds`]: a finite `f64` newtype with [`Ord`], [`Add`],
//!   [`Sub`], and [`Mul`].
//! - [`DurationError`]: error type for parse and conversion failures.

use std::{
    borrow::Cow,
    cmp::Ordering,
    fmt,
    hash::Hash,
    ops::{Add, Mul, Sub},
    str::FromStr,
};

use chrono::TimeDelta;
use num_traits::ToPrimitive as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A validated duration expression with its total seconds and original source
/// spelling.
///
/// Constructed exclusively via [`DurationValue::parse`],
/// [`DurationValue::parse_prefix`], [`DurationValue::from_seconds`], or the
/// [`FromStr`] implementation. The stored seconds value is always finite.
#[derive(Clone, Debug)]
pub struct DurationValue {
    seconds: DurationSeconds,
    raw: Box<str>,
}

impl DurationValue {
    /// Parses a duration spelling (e.g., `"1h 30m"`, `"4 hrs"`).
    ///
    /// Accepts one or more `<number><unit>` parts separated by whitespace or
    /// commas. A leading `+` or `-` on the first part sets the sign of the
    /// whole duration. A part after the first may repeat a redundant `+`
    /// but never an explicit `-`.
    /// Returns a specific error for each failure mode.
    ///
    /// # Errors
    ///
    /// - [`Empty`] if the input is empty or contains only separators.
    /// - [`MissingNumber`] if a unit appears without a preceding number.
    /// - [`InvalidNumber`] if the number portion could not be parsed, or a part
    ///   after the first carries an explicit `-` sign.
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
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(DurationError::Empty);
        }

        let bytes = trimmed.as_bytes();
        let len = bytes.len();
        let mut pos = 0;
        let mut total = 0.0f64;
        let mut parsed_any = false;
        let mut is_negative = false;

        while pos < len {
            Self::skip_separators(bytes, &mut pos);
            if pos >= len {
                break;
            }

            let (number, pos_after_number) =
                Self::parse_number(bytes, pos, trimmed)?;
            pos = pos_after_number;

            Self::skip_whitespace(bytes, &mut pos);

            let (kind, pos_after_unit) = Self::parse_unit(bytes, pos, trimmed)?;
            pos = pos_after_unit;

            if number.is_sign_negative() {
                if parsed_any {
                    return Err(DurationError::InvalidNumber {
                        input: trimmed.to_owned(),
                    });
                }
                is_negative = true;
            }

            total += number.abs() * kind.seconds();
            parsed_any = true;
        }

        if !parsed_any {
            return Err(DurationError::Empty);
        }

        if is_negative {
            total = -total;
        }

        Ok(Self {
            seconds: DurationSeconds::try_from(total)?,
            raw: trimmed.into(),
        })
    }

    /// Parses a duration prefix from `input`, returning the parsed
    /// [`DurationValue`] and the number of consumed bytes.
    ///
    /// Recognizes the same `<number><unit>` grammar as [`Self::parse`],
    /// including the leading-sign rule: only the first part may carry an
    /// explicit `-`; a redundant `+` is accepted anywhere. Returns `None` if
    /// `input` does not start with a valid duration segment, if no parts
    /// could be parsed, or if a part after the first carries an explicit
    /// `-` sign.
    pub(crate) fn parse_prefix(input: &str) -> Option<(Self, usize)> {
        let bytes = input.as_bytes();
        let len = bytes.len();
        if len == 0 || !Self::can_start_duration_segment(bytes, 0) {
            return None;
        }

        let mut pos = 0;
        let mut total = 0.0f64;
        let mut parsed_any = false;
        let mut is_negative = false;
        let mut last_end = 0;

        while pos < len {
            let (number, pos_after_number) =
                Self::parse_number(bytes, pos, input).ok()?;
            pos = pos_after_number;

            Self::skip_whitespace(bytes, &mut pos);

            let (kind, pos_after_unit) =
                Self::parse_unit(bytes, pos, input).ok()?;
            pos = pos_after_unit;

            if number.is_sign_negative() {
                if parsed_any {
                    return None;
                }
                is_negative = true;
            }

            total += number.abs() * kind.seconds();
            parsed_any = true;
            last_end = pos;

            let mut next_pos = pos;
            Self::skip_separators(bytes, &mut next_pos);
            if next_pos < len
                && Self::can_start_duration_segment(bytes, next_pos)
            {
                pos = next_pos;
            } else {
                break;
            }
        }

        if !parsed_any {
            return None;
        }

        if is_negative {
            total = -total;
        }

        let seconds = DurationSeconds::try_from(total).ok()?;
        let raw = input[..last_end].trim().into();
        Some((
            Self {
                seconds,
                raw,
            },
            last_end,
        ))
    }

    /// Synthesizes a canonical [`DurationValue`] from a [`DurationSeconds`]
    /// value.
    ///
    /// Greedily decomposes `seconds.0.abs()` into Weeks (604,800s), Days
    /// (86,400s), Hours (3,600s), Minutes (60s), Seconds (1s), and
    /// Milliseconds (0.001s). If `seconds.0 == 0.0`, produces `"0s"`.
    /// Negative durations have a leading `"-"`.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "part of DurationValue surface")
    )]
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "unit_ms is non-zero (>= 1) and total_ms >= unit_ms, so \
                  division and modulo never panic"
    )]
    pub(crate) fn from_seconds(seconds: DurationSeconds) -> Self {
        let total_secs = seconds.0;
        if total_secs == 0.0 {
            return Self {
                seconds,
                raw: "0s".into(),
            };
        }

        let is_negative = total_secs < 0.0;
        let rem = total_secs.abs();
        let mut total_ms = (rem * 1_000.0).round().to_u64().unwrap_or(0);
        let units: &[(u64, &str)] = &[
            (604_800_000, "w"),
            (86_400_000, "d"),
            (3_600_000, "h"),
            (60_000, "m"),
            (1_000, "s"),
            (1, "ms"),
        ];

        let mut parts = Vec::new();
        for &(unit_ms, suffix) in units {
            if total_ms >= unit_ms {
                let count = total_ms / unit_ms;
                total_ms %= unit_ms;
                parts.push(format!("{count}{suffix}"));
            }
        }

        let raw: Box<str> = if parts.is_empty() {
            "0s".into()
        } else {
            let combined = parts.join(" ");
            if is_negative {
                format!("-{combined}").into_boxed_str()
            } else {
                combined.into_boxed_str()
            }
        };

        Self {
            seconds,
            raw,
        }
    }

    /// Returns the raw duration text.
    #[inline]
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.raw
    }

    /// Returns the total duration as [`DurationSeconds`].
    #[inline]
    #[must_use]
    pub(crate) const fn to_seconds(&self) -> DurationSeconds {
        self.seconds
    }

    /// Returns `true` if `s` can begin a duration segment (a digit, or a
    /// `+`/`-`/`.` immediately followed by one).
    ///
    /// `O(1)`: inspects at most the first three bytes. Lets callers skip
    /// [`Self::parse`]'s allocating error path for text that plainly can't
    /// be a duration, without duplicating the character-set rule it shares
    /// with [`Self::parse`] and [`Self::parse_prefix`].
    #[must_use]
    pub(crate) fn can_start(s: &str) -> bool {
        Self::can_start_duration_segment(s.as_bytes(), 0)
    }

    /// Returns `true` if `bytes[pos]` can begin a numeric duration token.
    fn can_start_duration_segment(bytes: &[u8], pos: usize) -> bool {
        let Some(&b) = bytes.get(pos) else {
            return false;
        };
        if b.is_ascii_digit() {
            return true;
        }
        if b == b'.' {
            return bytes
                .get(pos.saturating_add(1))
                .is_some_and(u8::is_ascii_digit);
        }
        if b == b'+' || b == b'-' {
            let next = bytes.get(pos.saturating_add(1));
            let next_next = bytes.get(pos.saturating_add(2));
            return match next {
                Some(c) if c.is_ascii_digit() => true,
                Some(b'.') => next_next.is_some_and(u8::is_ascii_digit),
                _ => false,
            };
        }
        false
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

    /// Parses a decimal number starting at `pos`, supporting optional leading
    /// `+` or `-`.
    fn parse_number(
        bytes: &[u8],
        mut pos: usize,
        input: &str,
    ) -> Result<(f64, usize), DurationError> {
        let num_start = pos;
        if let Some(&b) = bytes.get(pos)
            && (b == b'+' || b == b'-')
        {
            let next = bytes.get(pos.saturating_add(1));
            let next_next = bytes.get(pos.saturating_add(2));
            let is_valid_after_sign = match next {
                Some(c) if c.is_ascii_digit() => true,
                Some(b'.') => next_next.is_some_and(u8::is_ascii_digit),
                _ => false,
            };
            if !is_valid_after_sign {
                return Err(DurationError::InvalidNumber {
                    input: input.to_owned(),
                });
            }
            pos = pos.saturating_add(1);
        }
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

    /// Validates and converts a parsed number byte span into `f64`.
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

    /// Validates and converts a parsed unit byte span into [`DurationUnit`].
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

impl FromStr for DurationValue {
    type Err = DurationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl TryFrom<DurationValue> for TimeDelta {
    type Error = DurationError;

    /// Converts to a [`TimeDelta`] via the duration's total seconds, returning
    /// a `NonFiniteSeconds` error on arithmetic overflow.
    #[inline]
    fn try_from(duration: DurationValue) -> Result<Self, Self::Error> {
        Self::try_from(duration.to_seconds())
    }
}

impl TryFrom<&DurationValue> for TimeDelta {
    type Error = DurationError;

    /// Converts to a [`TimeDelta`] via the duration's total seconds, returning
    /// a `NonFiniteSeconds` error on arithmetic overflow.
    #[inline]
    fn try_from(duration: &DurationValue) -> Result<Self, Self::Error> {
        Self::try_from(duration.to_seconds())
    }
}

impl fmt::Display for DurationValue {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

/// Compares by parsed [`DurationSeconds`], not raw spelling: `"1h 30m"` and
/// `"90m"` are equal.
impl PartialEq for DurationValue {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.seconds == other.seconds
    }
}

impl Eq for DurationValue {}

/// Orders by parsed [`DurationSeconds`], not raw spelling, consistent with
/// [`PartialEq`].
impl PartialOrd for DurationValue {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DurationValue {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.seconds.cmp(&other.seconds)
    }
}

/// Hashes the parsed [`DurationSeconds`], not raw spelling, so equal values
/// per [`PartialEq`] always hash identically.
impl Hash for DurationValue {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.seconds.hash(state);
    }
}

/// Serializes as the original raw spelling, not a canonical form: two equal
/// values (e.g. `"1h 30m"` and `"90m"`) can serialize to different strings.
impl Serialize for DurationValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.raw)
    }
}

/// Deserializes via [`Self::parse`], so an unparseable string is a hard
/// deserialization error rather than a lossy fallback.
impl<'de> Deserialize<'de> for DurationValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = Cow::<'de, str>::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// A recognized duration unit.
///
/// Single source of truth for unit parsing, seconds conversion, and naming.
/// Match on [`DurationUnit`] directly for type-safe dispatch rather than
/// converting to strings.
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
    /// Accepts strings up to 16 bytes. Returns `None` if the string is empty,
    /// exceeds 16 bytes, or does not match a known unit name.
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

/// Case-insensitive unit string to [`DurationUnit`] mapping.
///
/// Contains all accepted abbreviations and full names. Keys are stored
/// lowercase; lookup in [`DurationUnit::parse`] lowercases the input before
/// matching.
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
/// Wraps `f64` with NaN-safe ordering and arithmetic. Always finite when
/// constructed through [`DurationValue::to_seconds`] or
/// [`DurationSeconds::try_from`].
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

impl TryFrom<DurationSeconds> for TimeDelta {
    type Error = DurationError;

    /// Converts to a [`TimeDelta`], returning a `NonFiniteSeconds` error on
    /// arithmetic overflow.
    #[inline]
    fn try_from(seconds: DurationSeconds) -> Result<Self, Self::Error> {
        let total = seconds.0;
        let whole = total.trunc();
        let frac = total.fract();
        let (secs, nanos) = if frac < 0.0 {
            (whole - 1.0, (frac + 1.0) * 1_000_000_000.0)
        } else {
            (whole, frac * 1_000_000_000.0)
        };
        secs.to_i64()
            .zip(nanos.round().to_u32())
            .and_then(|(s, n)| Self::new(s, n))
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

impl Hash for DurationSeconds {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
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
/// Covers two failure domains: parsing human-readable duration text (via
/// [`DurationValue::parse`]) and converting raw seconds into a
/// [`DurationSeconds`].
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
        use pretty_assertions::assert_eq;

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
            assert_eq!(
                dv.to_seconds(),
                DurationSeconds::try_from(3_600.0).unwrap()
            );
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

        #[test]
        fn hashes_equal_seconds_identically() {
            use std::{
                collections::hash_map::DefaultHasher,
                hash::{Hash, Hasher},
            };

            fn hash_val<T: Hash>(val: &T) -> u64 {
                let mut hasher = DefaultHasher::new();
                val.hash(&mut hasher);
                hasher.finish()
            }

            let a = DurationSeconds::try_from(100.0).unwrap();
            let b = DurationSeconds::try_from(100.0).unwrap();
            assert_eq!(hash_val(&a), hash_val(&b));
        }
    }

    mod duration_value_parse {
        use pretty_assertions::assert_eq;
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
                DurationValue::parse(input).unwrap().to_seconds(),
                DurationSeconds::try_from(expected).unwrap()
            );
        }

        #[test]
        fn parses_multi_part_with_space() {
            let d = DurationValue::parse("1h 30m").unwrap();
            assert_eq!(
                d.to_seconds(),
                DurationSeconds::try_from(5_400.0).unwrap()
            );
        }

        #[test]
        fn parses_signed_durations_in_correct_order() {
            let neg = DurationValue::parse("-15m").unwrap();
            let zero = DurationValue::parse("0m").unwrap();
            let pos = DurationValue::parse("+15m").unwrap();
            assert!(neg < zero);
            assert!(zero < pos);
        }

        #[test]
        fn rejects_a_negative_sign_on_a_non_leading_component() {
            let err = DurationValue::parse("1h -30m").unwrap_err();
            assert!(matches!(err, DurationError::InvalidNumber { .. }));
        }

        #[test]
        fn accepts_a_redundant_positive_sign_on_a_non_leading_component() {
            let d = DurationValue::parse("1h +30m").unwrap();
            assert_eq!(
                d.to_seconds(),
                DurationSeconds::try_from(5_400.0).unwrap()
            );
        }

        #[test]
        fn parses_a_signed_decimal_without_a_leading_digit() {
            let d = DurationValue::parse("-.5h").unwrap();
            assert_eq!(
                d.to_seconds(),
                DurationSeconds::try_from(-1_800.0).unwrap()
            );
        }

        #[test]
        fn equates_semantically_equivalent_spellings() {
            let a = DurationValue::parse("1h 30m").unwrap();
            let b = DurationValue::parse("90m").unwrap();
            assert_eq!(a, b);
        }

        #[test]
        fn hashes_semantically_equivalent_spellings_identically() {
            use std::{
                collections::hash_map::DefaultHasher,
                hash::{Hash, Hasher},
            };

            fn hash_val<T: Hash>(val: &T) -> u64 {
                let mut hasher = DefaultHasher::new();
                val.hash(&mut hasher);
                hasher.finish()
            }

            let a = DurationValue::parse("1h 30m").unwrap();
            let b = DurationValue::parse("90m").unwrap();
            assert_eq!(hash_val(&a), hash_val(&b));
        }

        #[test]
        fn preserves_display_fidelity_of_raw_spelling() {
            let d = DurationValue::parse("4 yrs, 6 wks").unwrap();
            assert_eq!(format!("{d}"), "4 yrs, 6 wks");
            assert_eq!(d.as_str(), "4 yrs, 6 wks");
        }

        #[test]
        fn parses_multi_part_without_separator() {
            assert_eq!(
                DurationValue::parse("1h30m").unwrap().to_seconds(),
                DurationSeconds::try_from(5_400.0).unwrap()
            );
        }

        #[test]
        fn parses_multi_part_with_comma() {
            let d = DurationValue::parse("4 yrs, 6 wks").unwrap();
            assert_eq!(
                d.to_seconds(),
                DurationSeconds::try_from(4.0 * 31_536_000.0 + 6.0 * 604_800.0)
                    .unwrap()
            );
        }

        #[test]
        fn parses_decimal_numbers() {
            assert_eq!(
                DurationValue::parse("1.5h").unwrap().to_seconds(),
                DurationSeconds::try_from(5_400.0).unwrap()
            );
        }

        #[test]
        fn parses_decimal_without_leading_digit() {
            assert_eq!(
                DurationValue::parse(".5h").unwrap().to_seconds(),
                DurationSeconds::try_from(1_800.0).unwrap()
            );
        }

        #[test]
        fn parses_with_leading_trailing_whitespace() {
            let d = DurationValue::parse(" 1h ").unwrap();
            assert_eq!(
                d.to_seconds(),
                DurationSeconds::try_from(3_600.0).unwrap()
            );
        }

        #[test]
        fn parses_with_multiple_consecutive_separators() {
            let d = DurationValue::parse("1  h  30  m").unwrap();
            assert_eq!(
                d.to_seconds(),
                DurationSeconds::try_from(5_400.0).unwrap()
            );
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

    mod duration_unit_parse {
        use pretty_assertions::assert_eq;
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

        #[rstest]
        #[case::ms("ms", DurationUnit::Millisecond)]
        #[case::millisecond("millisecond", DurationUnit::Millisecond)]
        #[case::milliseconds("milliseconds", DurationUnit::Millisecond)]
        #[case::s("s", DurationUnit::Second)]
        #[case::sec("sec", DurationUnit::Second)]
        #[case::secs("secs", DurationUnit::Second)]
        #[case::second("second", DurationUnit::Second)]
        #[case::seconds("seconds", DurationUnit::Second)]
        #[case::m("m", DurationUnit::Minute)]
        #[case::min("min", DurationUnit::Minute)]
        #[case::mins("mins", DurationUnit::Minute)]
        #[case::minute("minute", DurationUnit::Minute)]
        #[case::minutes("minutes", DurationUnit::Minute)]
        #[case::h("h", DurationUnit::Hour)]
        #[case::hr("hr", DurationUnit::Hour)]
        #[case::hrs("hrs", DurationUnit::Hour)]
        #[case::hour("hour", DurationUnit::Hour)]
        #[case::hours("hours", DurationUnit::Hour)]
        #[case::d("d", DurationUnit::Day)]
        #[case::day("day", DurationUnit::Day)]
        #[case::days("days", DurationUnit::Day)]
        #[case::w("w", DurationUnit::Week)]
        #[case::wk("wk", DurationUnit::Week)]
        #[case::wks("wks", DurationUnit::Week)]
        #[case::week("week", DurationUnit::Week)]
        #[case::weeks("weeks", DurationUnit::Week)]
        #[case::mo("mo", DurationUnit::Month)]
        #[case::mos("mos", DurationUnit::Month)]
        #[case::month("month", DurationUnit::Month)]
        #[case::months("months", DurationUnit::Month)]
        #[case::y("y", DurationUnit::Year)]
        #[case::yr("yr", DurationUnit::Year)]
        #[case::yrs("yrs", DurationUnit::Year)]
        #[case::year("year", DurationUnit::Year)]
        #[case::years("years", DurationUnit::Year)]
        fn parses_all_units_and_abbreviations(
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
        use pretty_assertions::assert_eq;

        use super::*;
        #[test]
        fn roundtrips_valid_input() {
            let d: DurationValue = "1h 30m".parse().unwrap();
            assert_eq!(
                d.to_seconds(),
                DurationSeconds::try_from(5_400.0).unwrap()
            );
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
        use pretty_assertions::assert_eq;

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

    mod time_delta_conversion {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn converts_a_whole_second_duration() {
            let duration = DurationValue::parse("1h").expect("valid duration");
            let converted = TimeDelta::try_from(duration).expect("in range");
            assert_eq!(converted, TimeDelta::hours(1));
        }

        #[test]
        fn converts_a_subsecond_duration() {
            let duration =
                DurationValue::parse("500ms").expect("valid duration");
            let converted = TimeDelta::try_from(duration).expect("in range");
            assert_eq!(converted, TimeDelta::milliseconds(500));
        }

        #[test]
        fn converts_a_zero_duration() {
            let seconds =
                DurationSeconds::try_from(0.0).expect("finite seconds");
            let converted = TimeDelta::try_from(seconds).expect("in range");
            assert_eq!(converted, TimeDelta::zero());
        }

        #[test]
        fn converts_a_negative_whole_duration() {
            let seconds =
                DurationSeconds::try_from(-10.0).expect("finite seconds");
            let converted = TimeDelta::try_from(seconds).expect("in range");
            assert_eq!(converted, TimeDelta::seconds(-10));
        }

        #[test]
        fn converts_a_negative_fractional_duration() {
            let seconds =
                DurationSeconds::try_from(-90.5).expect("finite seconds");
            let converted = TimeDelta::try_from(seconds).expect("in range");
            assert_eq!(converted, TimeDelta::milliseconds(-90_500));
        }

        #[test]
        fn rounds_a_sub_microsecond_duration_to_the_nearest_nanosecond() {
            let seconds = DurationSeconds::try_from(0.000_000_001)
                .expect("finite seconds");
            let converted = TimeDelta::try_from(seconds).expect("in range");
            assert_eq!(converted, TimeDelta::nanoseconds(1));
        }

        #[test]
        fn rejects_a_seconds_value_outside_the_representable_range() {
            let seconds =
                DurationSeconds::try_from(1e300).expect("finite seconds");
            let result = TimeDelta::try_from(seconds);
            assert!(matches!(result, Err(DurationError::NonFiniteSeconds)));
        }

        #[test]
        fn rejects_an_extreme_negative_seconds_value_outside_the_representable_range()
         {
            let seconds =
                DurationSeconds::try_from(-1e300).expect("finite seconds");
            let result = TimeDelta::try_from(seconds);
            assert!(matches!(result, Err(DurationError::NonFiniteSeconds)));
        }

        #[test]
        fn delegates_to_duration_seconds() {
            let duration = DurationValue::parse("30m").expect("valid duration");
            let converted = TimeDelta::try_from(&duration).expect("in range");
            let via_seconds =
                TimeDelta::try_from(duration.to_seconds()).expect("in range");
            assert_eq!(converted, via_seconds);
        }
    }

    mod duration_value_prefix {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn parses_valid_prefix_and_advances_to_boundary() {
            let (dv, consumed) =
                DurationValue::parse_prefix("45m]").expect("prefix");
            assert_eq!(consumed, 3);
            assert_eq!(dv.as_str(), "45m");
            assert_eq!(
                dv.to_seconds(),
                DurationSeconds::try_from(2_700.0).unwrap()
            );
        }

        #[test]
        fn parses_a_decimal_prefix_without_a_leading_digit() {
            let (dv, consumed) =
                DurationValue::parse_prefix(".5h remainder").expect("prefix");
            assert_eq!(consumed, 3);
            assert_eq!(dv.as_str(), ".5h");
            assert_eq!(
                dv.to_seconds(),
                DurationSeconds::try_from(1_800.0).unwrap()
            );
        }

        #[test]
        fn parses_multi_part_prefix_without_trailing_separators() {
            let (dv, consumed) =
                DurationValue::parse_prefix("1h, 30m, extra").expect("prefix");
            assert_eq!(consumed, 7);
            assert_eq!(dv.as_str(), "1h, 30m");
            assert_eq!(
                dv.to_seconds(),
                DurationSeconds::try_from(5_400.0).unwrap()
            );
        }

        #[test]
        fn rejects_non_duration_prefix() {
            assert!(DurationValue::parse_prefix("not a duration").is_none());
        }

        #[test]
        fn rejects_hyphen_bullet_without_immediate_digit() {
            assert!(DurationValue::parse_prefix("- 15m").is_none());
        }

        #[test]
        fn rejects_a_negative_sign_on_a_non_leading_component() {
            assert!(DurationValue::parse_prefix("1h -30m").is_none());
        }
    }

    mod duration_value_can_start {
        use super::*;

        #[test]
        fn accepts_a_leading_digit() {
            assert!(DurationValue::can_start("1h"));
        }

        #[test]
        fn accepts_a_leading_decimal_point_followed_by_a_digit() {
            assert!(DurationValue::can_start(".5h"));
        }

        #[test]
        fn accepts_a_leading_sign_followed_by_a_digit() {
            assert!(DurationValue::can_start("+1h"));
            assert!(DurationValue::can_start("-30m"));
        }

        #[test]
        fn accepts_a_leading_sign_followed_by_a_decimal_digit() {
            assert!(DurationValue::can_start("+.5h"));
        }

        #[test]
        fn rejects_a_non_numeric_leading_character() {
            assert!(!DurationValue::can_start("hello"));
        }

        #[test]
        fn rejects_an_empty_string() {
            assert!(!DurationValue::can_start(""));
        }

        #[test]
        fn rejects_a_lone_sign_with_no_following_digit() {
            assert!(!DurationValue::can_start("+"));
        }

        #[test]
        fn rejects_a_lone_decimal_point_with_no_following_digit() {
            assert!(!DurationValue::can_start("."));
        }
    }

    mod duration_value_synthesis {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[test]
        fn synthesizes_zero_duration_as_0s() {
            let zero = DurationSeconds::try_from(0.0).unwrap();
            let dv = DurationValue::from_seconds(zero);
            assert_eq!(dv.as_str(), "0s");
            assert_eq!(dv.to_seconds(), zero);
        }

        #[test]
        fn synthesizes_positive_decomposed_units() {
            let secs = DurationSeconds::try_from(5_400.0).unwrap(); // 1h 30m
            let dv = DurationValue::from_seconds(secs);
            assert_eq!(dv.as_str(), "1h 30m");
            assert_eq!(dv.to_seconds(), secs);
        }

        #[test]
        fn synthesizes_negative_decomposed_units_with_leading_minus() {
            let secs = DurationSeconds::try_from(-5_400.0).unwrap();
            let dv = DurationValue::from_seconds(secs);
            assert_eq!(dv.as_str(), "-1h 30m");
            assert_eq!(dv.to_seconds(), secs);
        }

        #[rstest]
        #[case::seconds(90.0)]
        #[case::minutes(5_400.0)]
        #[case::compound(604_800.0 + 86_400.0 + 3_600.0 + 60.0 + 1.0 + 0.5)]
        #[case::negative(-5_400.0)]
        #[case::negative_compound(-(86_400.0 + 3_600.0))]
        fn roundtrips_synthesized_duration_through_parser(
            #[case] raw_seconds: f64,
        ) {
            let secs = DurationSeconds::try_from(raw_seconds).unwrap();
            let synthesized = DurationValue::from_seconds(secs);
            let reparsed = DurationValue::parse(synthesized.as_str())
                .expect("synthesized string must parse");
            assert_eq!(reparsed.to_seconds(), secs);
        }
    }

    mod duration_value_serde {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn serializes_to_raw_string() {
            let dv = DurationValue::parse("1h 30m").unwrap();
            let json = serde_json::to_string(&dv).unwrap();
            assert_eq!(json, "\"1h 30m\"");
        }

        #[test]
        fn deserializes_from_raw_string() {
            let json = "\"4 yrs, 6 wks\"";
            let dv: DurationValue = serde_json::from_str(json).unwrap();
            assert_eq!(dv.as_str(), "4 yrs, 6 wks");
            assert_eq!(
                dv.to_seconds(),
                DurationValue::parse("4 yrs, 6 wks").unwrap().to_seconds()
            );
        }

        #[test]
        fn rejects_deserializing_an_unparseable_duration_string() {
            let json = "\"not a duration\"";
            let err = serde_json::from_str::<DurationValue>(json).unwrap_err();
            assert!(err.to_string().contains("no number before unit"));
        }
    }
}
