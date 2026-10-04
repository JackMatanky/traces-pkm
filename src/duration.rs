//! Duration parsing, validation, and arithmetic.
//!
//! Accepts human-readable duration strings like `"1h 30m"` or `"4 yrs, 6 wks"`
//! and converts them to a total seconds value. Parts are `<number><unit>` pairs
//! separated by whitespace or commas.
//!
//! # Key types
//!
//! - [`DurationUnit`]: unit registry (parsing, seconds conversion, naming).
//!   Single source of truth; callers should not maintain their own registries.
//! - [`DurationValue`]: a parsed duration carrying its total seconds, its
//!   original spelling, and its retained written parts (the regime witness).
//! - [`DurationSeconds`]: a finite `f64` newtype with [`Ord`], [`Add`],
//!   [`Sub`], and [`Mul`].
//! - [`DurationError`]: error type for parse and conversion failures.

use std::{
    borrow::Cow,
    cmp::Ordering,
    fmt::{self, Write as _},
    hash::Hash,
    ops::{Add, Mul, Sub},
    str::FromStr,
};

use chrono::TimeDelta;
use num_traits::ToPrimitive as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Retained duration components in written left-to-right order.
type DurationParts = Box<[(f64, DurationUnit)]>;

/// A validated duration expression with its total seconds and original source
/// spelling.
///
/// Constructed exclusively via its crate-internal `parse`, `parse_prefix`,
/// and `from_seconds` constructors, or the [`FromStr`] implementation. The
/// stored seconds value is always finite when constructed through those paths;
/// duration arithmetic operations such as [`Mul`] may produce non-finite values
/// that are subsequently rejected by consuming conversions.
#[derive(Clone, Debug)]
pub struct DurationValue {
    raw: Box<str>,
    seconds: DurationSeconds,
    parts: Option<DurationParts>,
}

impl DurationValue {
    /// The six fixed sub-year units used for greedy millisecond decomposition
    /// in [`DurationValue::from_seconds`].
    ///
    /// Month and Year are intentionally omitted from magnitude-derived
    /// conversion (N18, N2): their lengths are calendar-dependent and
    /// cannot be soundly synthesized from a raw magnitude.
    const SUB_YEAR_DECOMPOSITION_UNITS: [(DurationUnit, &str); 6] = [
        (DurationUnit::Week, "w"),
        (DurationUnit::Day, "d"),
        (DurationUnit::Hour, "h"),
        (DurationUnit::Minute, "m"),
        (DurationUnit::Second, "s"),
        (DurationUnit::Millisecond, "ms"),
    ];

    /// Scans one `<number><unit>` part starting at `pos`, returning the
    /// magnitude, unit, and end offset.
    ///
    /// Shared per-part mechanism of [`Self::parse`] and
    /// [`Self::parse_prefix`]; the loops keep their own continuation,
    /// sign, and termination policies.
    fn scan_part(
        bytes: &[u8],
        pos: usize,
        input: &str,
    ) -> Result<(f64, DurationUnit, usize), DurationError> {
        let (number, after_number) = Self::parse_number(bytes, pos, input)?;
        let mut cursor = after_number;
        Self::skip_whitespace(bytes, &mut cursor);
        let (unit, after_unit) = Self::parse_unit(bytes, cursor, input)?;
        Ok((number, unit, after_unit))
    }

    /// Negates every magnitude in `parts` when `is_negative` is `true`.
    ///
    /// Both parse loops end with this fold: only the first part may carry
    /// the explicit sign, so the retained magnitudes are non-negative
    /// until this point.
    fn apply_sign(parts: &mut [(f64, DurationUnit)], is_negative: bool) {
        let sign = if is_negative {
            -1.0
        } else {
            1.0
        };
        for (mag, _) in parts {
            *mag *= sign;
        }
    }

    /// Parses a duration spelling (e.g., `"1h 30m"`, `"4 hrs"`).
    ///
    /// Accepts one or more `<number><unit>` parts separated by whitespace or
    /// commas. A leading `+` or `-` on the first part sets the sign of the
    /// whole duration. A part after the first may repeat a redundant `+` but
    /// never an explicit `-`.
    ///
    /// Returns a specific error for each failure mode.
    ///
    /// # Errors
    ///
    /// - [`Empty`] if the input is empty or contains only separators.
    /// - [`MissingNumber`] if a unit appears without a preceding number.
    /// - [`InvalidNumber`] if a `+`/`-` sign is not immediately followed by a
    ///   digit or by `.` and a digit, or a part after the first carries an
    ///   explicit `-` sign.
    /// - [`MalformedNumber`] if the number portion is not valid float syntax.
    /// - [`MissingUnit`] if a number appears without a trailing unit.
    /// - [`UnknownUnit`] if the unit string is not recognized.
    /// - [`NonFiniteNumber`] if a single part's number overflows to infinity.
    /// - [`NonFiniteSeconds`] if the parsed total cannot be represented as a
    ///   finite seconds value.
    ///
    /// [`Empty`]: DurationError::Empty
    /// [`MissingNumber`]: DurationError::MissingNumber
    /// [`InvalidNumber`]: DurationError::InvalidNumber
    /// [`MalformedNumber`]: DurationError::MalformedNumber
    /// [`MissingUnit`]: DurationError::MissingUnit
    /// [`UnknownUnit`]: DurationError::UnknownUnit
    /// [`NonFiniteNumber`]: DurationError::NonFiniteNumber
    /// [`NonFiniteSeconds`]: DurationError::NonFiniteSeconds
    pub(crate) fn parse(input: &str) -> Result<Self, DurationError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(DurationError::Empty);
        }

        let bytes = trimmed.as_bytes();
        let len = bytes.len();
        let mut pos = 0;
        let mut parsed_any = false;
        let mut is_negative = false;
        let mut raw_parts = Vec::new();

        while pos < len {
            Self::skip_separators(bytes, &mut pos);
            if pos >= len {
                break;
            }

            let (number, kind, pos_after_part) =
                Self::scan_part(bytes, pos, trimmed)?;
            pos = pos_after_part;

            if number.is_sign_negative() {
                if parsed_any {
                    return Err(DurationError::InvalidNumber {
                        input: trimmed.to_owned(),
                    });
                }
                is_negative = true;
            }

            raw_parts.push((number.abs(), kind));
            parsed_any = true;
        }

        if !parsed_any {
            return Err(DurationError::Empty);
        }

        Self::apply_sign(&mut raw_parts, is_negative);

        let total = Self::fold_parts(&raw_parts);

        Ok(Self {
            raw: trimmed.into(),
            seconds: DurationSeconds::try_from(total)?,
            parts: Some(raw_parts.into_boxed_slice()),
        })
    }

    /// Parses a duration prefix from `input`, returning the parsed
    /// [`DurationValue`] and the number of consumed bytes.
    ///
    /// Recognizes the same `<number><unit>` grammar as [`Self::parse`],
    /// including the leading-sign rule: only the first part may carry an
    /// explicit `-`; a redundant `+` is accepted anywhere. Returns `None` if
    /// `input` does not start with a valid duration segment, if no parts could
    /// be parsed, or if a part after the first carries an explicit `-` sign.
    pub(crate) fn parse_prefix(input: &str) -> Option<(Self, usize)> {
        let bytes = input.as_bytes();
        let len = bytes.len();
        if len == 0 || !Self::can_start_duration_segment(bytes, 0) {
            return None;
        }

        let mut pos = 0;
        let mut parsed_any = false;
        let mut is_negative = false;
        let mut last_end = 0;
        let mut raw_parts = Vec::new();

        while pos < len {
            let (number, kind, pos_after_part) =
                Self::scan_part(bytes, pos, input).ok()?;
            pos = pos_after_part;

            if number.is_sign_negative() {
                if parsed_any {
                    return None;
                }
                is_negative = true;
            }

            raw_parts.push((number.abs(), kind));
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

        Self::apply_sign(&mut raw_parts, is_negative);

        let total = Self::fold_parts(&raw_parts);
        let seconds = DurationSeconds::try_from(total).ok()?;
        let raw = input[..last_end].trim().into();
        Some((
            Self {
                raw,
                seconds,
                parts: Some(raw_parts.into_boxed_slice()),
            },
            last_end,
        ))
    }

    /// Synthesizes a canonical [`DurationValue`] from a [`DurationSeconds`]
    /// value.
    ///
    /// Greedily decomposes `seconds.0.abs()` into Weeks, Days, Hours, Minutes,
    /// Seconds, and Milliseconds using ratios derived from
    /// [`DurationUnit::fixed_seconds`]. Month and Year are intentionally
    /// omitted (N18, N2): their lengths are calendar-dependent and cannot be
    /// soundly synthesized from a raw magnitude. If `seconds.0 == 0.0`,
    /// produces `"0s"`. Negative durations have a leading `"-"`. When the
    /// millisecond decomposition can't represent a nonzero magnitude
    /// (rounds a sub-millisecond remainder to zero, or overflows `u64` at
    /// the other extreme), falls back to [`DurationSeconds`]'s own
    /// [`Display`](fmt::Display) so the result never lies as `"0s"`.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "part of DurationValue surface; called in tests and by \
                      future classify/temporal callers"
        )
    )]
    #[inline]
    pub(crate) fn from_seconds(seconds: DurationSeconds) -> Self {
        Self {
            raw: Self::canonical_raw(seconds),
            seconds,
            parts: None,
        }
    }

    /// Synthesizes the canonical raw spelling for a fixed-magnitude
    /// duration: the greedy sub-year decomposition of
    /// [`Self::SUB_YEAR_DECOMPOSITION_UNITS`], or [`DurationSeconds`]'s
    /// [`Display`](fmt::Display) dialect when the decomposition can't
    /// represent the magnitude (see [`Self::from_seconds`]).
    ///
    /// Accumulates into one `String`: decomposition emits at most six
    /// integer counts, so no per-part intermediate strings are needed.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "unit_ms is non-zero (>= 1) and total_ms >= unit_ms, so \
                  division and modulo never panic"
    )]
    fn canonical_raw(seconds: DurationSeconds) -> Box<str> {
        let total_secs = seconds.0;
        if total_secs == 0.0 {
            return "0s".into();
        }

        let is_negative = total_secs < 0.0;
        let rem = total_secs.abs();
        let mut total_ms = (rem * 1_000.0).round().to_u64().unwrap_or(0);
        let mut raw = String::new();
        if is_negative {
            raw.push('-');
        }
        let mut decomposed = false;
        for &(unit, suffix) in &Self::SUB_YEAR_DECOMPOSITION_UNITS {
            let unit_ms =
                (unit.fixed_seconds() * 1_000.0).round().to_u64().unwrap_or(0);
            if total_ms >= unit_ms {
                let count = total_ms / unit_ms;
                total_ms %= unit_ms;
                if decomposed {
                    raw.push(' ');
                }
                // `u64`'s `Display` into a `String` is infallible.
                let _ = write!(raw, "{count}{suffix}");
                decomposed = true;
            }
        }

        // Millisecond decomposition can't represent every magnitude: it
        // rounds a sub-millisecond remainder to zero, and overflows `u64`
        // for a large enough magnitude (silently read back as 0 above).
        // Either way nothing was written for a nonzero `total_secs` here;
        // fall back to the honest `DurationSeconds` dialect (which carries
        // its own sign) instead of lying with `"0s"`.
        if decomposed {
            raw.into_boxed_str()
        } else {
            format!("{}s", DurationSeconds(total_secs)).into_boxed_str()
        }
    }

    /// Returns the parsed duration parts, if retained.
    ///
    /// Values parsed from text retain their written parts and serve as a
    /// regime witness: [`Some`] containing [`DurationUnit::Day`],
    /// [`DurationUnit::Week`], [`DurationUnit::Month`], or
    /// [`DurationUnit::Year`] indicates calendar application semantics, while
    /// [`Some`] with only sub-day units or [`None`] (synthesized from seconds)
    /// indicates fixed-magnitude semantics.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "part of DurationValue surface; called in tests and by \
                      future classify/temporal callers"
        )
    )]
    #[inline]
    #[must_use]
    pub(crate) fn parts(&self) -> Option<&[(f64, DurationUnit)]> {
        self.parts.as_deref()
    }

    /// Returns `true` if this duration contains any calendar application unit
    /// ([`DurationUnit::Day`], [`DurationUnit::Week`], [`DurationUnit::Month`],
    /// or [`DurationUnit::Year`]).
    ///
    /// Matches every [`DurationUnit`] variant exhaustively without wildcards
    /// per `pat-exhaustive-enum`.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "part of DurationValue surface; called in tests and by \
                      future classify/temporal callers"
        )
    )]
    #[inline]
    #[must_use]
    pub(crate) fn is_calendar(&self) -> bool {
        self.parts.as_deref().is_some_and(|parts| {
            parts.iter().any(|&(_, unit)| match unit {
                DurationUnit::Day
                | DurationUnit::Week
                | DurationUnit::Month
                | DurationUnit::Year => true,
                DurationUnit::Millisecond
                | DurationUnit::Second
                | DurationUnit::Minute
                | DurationUnit::Hour => false,
            })
        })
    }

    /// Folds duration parts in left-to-right order to compute total seconds.
    #[inline]
    #[expect(
        clippy::suboptimal_flops,
        reason = "mul_add fuses to a single rounding step and would silently \
                  change user-visible Duration floats that tests pin with \
                  exact equality; not a hot path"
    )]
    fn fold_parts(parts: &[(f64, DurationUnit)]) -> f64 {
        parts.iter().fold(0.0f64, |acc, &(magnitude, unit)| {
            acc + magnitude * unit.fixed_seconds()
        })
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
            return Self::digits_after_sign(
                bytes.get(pos.saturating_add(1)),
                bytes.get(pos.saturating_add(2)),
            );
        }
        false
    }

    /// Returns `true` if the bytes following a `+`/`-` sign begin a valid
    /// number: a digit, or `.` followed by one.
    ///
    /// Single owner of the sign rule shared by
    /// [`Self::can_start_duration_segment`] and [`Self::parse_number`].
    fn digits_after_sign(next: Option<&u8>, next_next: Option<&u8>) -> bool {
        match next {
            Some(c) if c.is_ascii_digit() => true,
            Some(b'.') => next_next.is_some_and(u8::is_ascii_digit),
            _ => false,
        }
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

    /// Parses a decimal number starting at `pos`, supporting an optional
    /// leading `+` or `-` and an optional trailing exponent (`"1e5"`,
    /// `".5e-3"`), the same float syntax the [`DurationSeconds`]
    /// [`Display`](fmt::Display) dialect emits for extreme magnitudes.
    fn parse_number(
        bytes: &[u8],
        mut pos: usize,
        input: &str,
    ) -> Result<(f64, usize), DurationError> {
        let num_start = pos;
        if let Some(&b) = bytes.get(pos)
            && (b == b'+' || b == b'-')
        {
            if !Self::digits_after_sign(
                bytes.get(pos.saturating_add(1)),
                bytes.get(pos.saturating_add(2)),
            ) {
                return Err(DurationError::InvalidNumber {
                    input: input.to_owned(),
                });
            }
            pos = pos.saturating_add(1);
        }
        let mut has_decimal = false;
        let mut has_digit = false;
        while pos < bytes.len() {
            let Some(&b) = bytes.get(pos) else {
                break;
            };
            if b.is_ascii_digit() {
                has_digit = true;
                pos = pos.saturating_add(1);
            } else if b == b'.' && !has_decimal {
                has_decimal = true;
                pos = pos.saturating_add(1);
            } else {
                break;
            }
        }
        // An exponent may follow a digit-bearing mantissa. A truncated
        // exponent (`"1e"`, `"1e+"`) is not consumed: the `e` falls to the
        // unit scanner and is reported as an unknown unit, as before.
        if has_digit && matches!(bytes.get(pos), Some(b'e' | b'E')) {
            let mut exp_pos = pos.saturating_add(1);
            if matches!(bytes.get(exp_pos), Some(b'+' | b'-')) {
                exp_pos = exp_pos.saturating_add(1);
            }
            if bytes.get(exp_pos).is_some_and(u8::is_ascii_digit) {
                while bytes.get(exp_pos).is_some_and(u8::is_ascii_digit) {
                    exp_pos = exp_pos.saturating_add(1);
                }
                pos = exp_pos;
            }
        }
        Self::parsed_number(num_start, pos, input)
    }

    /// Validates and converts a parsed number byte span into `f64`.
    fn parsed_number(
        start: usize,
        end: usize,
        input: &str,
    ) -> Result<(f64, usize), DurationError> {
        if start == end {
            return Err(DurationError::MissingNumber {
                input: input.to_owned(),
            });
        }
        // `start`/`end` are byte offsets produced by scanning only
        // single-byte ASCII (`+`/`-`/`.`/digit/`e`/`E`), so they always land
        // on char boundaries within `input`: a direct `str` slice can't fail.
        let Some(text) = input.get(start..end) else {
            return Err(DurationError::MissingNumber {
                input: input.to_owned(),
            });
        };
        let number: f64 =
            text.parse().map_err(|source| DurationError::MalformedNumber {
                input: input.to_owned(),
                source,
            })?;
        if !number.is_finite() {
            return Err(DurationError::NonFiniteNumber {
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

    #[inline]
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

impl fmt::Display for DurationValue {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

/// Compares by parsed seconds, not raw spelling: `"1h 30m"` and `"90m"` are
/// equal.
impl PartialEq for DurationValue {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.seconds == other.seconds
    }
}

impl Eq for DurationValue {}

/// Orders by parsed seconds, not raw spelling, consistent with
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

/// Hashes the parsed seconds, not raw spelling, so equal values per
/// [`PartialEq`] always hash identically.
impl Hash for DurationValue {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.seconds.hash(state);
    }
}

impl Add for DurationValue {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self {
        self.combine(rhs, false)
    }
}

impl Sub for DurationValue {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self {
        self.combine(rhs, true)
    }
}

impl DurationValue {
    /// Combines `rhs` with `self`, negating `rhs`'s parts (and seconds,
    /// in the fixed regime) when `negate_rhs` is `true` — the single
    /// implementation behind [`Add`] and [`Sub`].
    ///
    /// When both operands have retained parts ([`Some`]), the parts are
    /// concatenated in left-to-right order and the total seconds are
    /// computed via a single fold over the combined parts, preserving the
    /// bit-exact Σ invariant by construction. When either operand has no
    /// retained parts ([`None`]), seconds are added or subtracted directly
    /// and the result carries `None` (fixed regime). Negative zero is
    /// normalized to positive zero.
    fn combine(self, rhs: Self, negate_rhs: bool) -> Self {
        if let (Some(lhs_parts), Some(rhs_parts)) = (self.parts, rhs.parts) {
            let mut combined = Vec::with_capacity(
                lhs_parts.len().saturating_add(rhs_parts.len()),
            );
            combined.extend_from_slice(&lhs_parts);
            if negate_rhs {
                combined
                    .extend(rhs_parts.iter().map(|&(mag, unit)| (-mag, unit)));
            } else {
                combined.extend_from_slice(&rhs_parts);
            }
            let fold_total = Self::fold_parts(&combined);
            let seconds = DurationSeconds::normalized(fold_total);
            Self {
                raw: Self::canonical_raw(seconds),
                seconds,
                parts: Some(combined.into_boxed_slice()),
            }
        } else {
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "duration arithmetic preserves total_cmp ordering \
                          and handles overflow without panicking"
            )]
            let seconds = if negate_rhs {
                self.seconds - rhs.seconds
            } else {
                self.seconds + rhs.seconds
            };
            Self {
                raw: Self::canonical_raw(seconds),
                seconds,
                parts: None,
            }
        }
    }
}

/// Scales a duration by a scalar factor.
///
/// When parts are retained ([`Some`]), each part's magnitude is scaled by `rhs`
/// and seconds are computed as the single left-to-right fold over the scaled
/// parts. When parts are [`None`], seconds are scaled directly.
///
/// A non-finite scalar (`NaN` or `±inf`) or overflow yields a non-finite
/// seconds value that is rejected by subsequent consuming conversions with
/// [`DurationError::NonFiniteSeconds`]; this operation never panics.
impl Mul<f64> for DurationValue {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f64) -> Self {
        if let Some(parts) = self.parts {
            let scaled: Vec<(f64, DurationUnit)> =
                parts.iter().map(|&(mag, unit)| (mag * rhs, unit)).collect();
            let fold_total = Self::fold_parts(&scaled);
            let seconds = DurationSeconds::normalized(fold_total);
            Self {
                raw: Self::canonical_raw(seconds),
                seconds,
                parts: Some(scaled.into_boxed_slice()),
            }
        } else {
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "duration scaling preserves non-finite results \
                          without panicking"
            )]
            let seconds = self.seconds * rhs;
            Self {
                raw: Self::canonical_raw(seconds),
                seconds,
                parts: None,
            }
        }
    }
}

/// Scales a duration by a scalar factor.
impl Mul<DurationValue> for f64 {
    type Output = DurationValue;

    #[inline]
    fn mul(self, rhs: DurationValue) -> DurationValue {
        rhs.mul(self)
    }
}

/// Serializes as the original raw spelling, not a canonical form: two equal
/// values (e.g. `"1h 30m"` and `"90m"`) can serialize to different strings.
impl Serialize for DurationValue {
    #[inline]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.raw)
    }
}

/// Deserializes via the same parser as [`FromStr`], so an unparseable string
/// is a hard deserialization error rather than a lossy fallback.
impl<'de> Deserialize<'de> for DurationValue {
    #[inline]
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

    /// Fixed seconds per unit for duration magnitude identity.
    ///
    /// This is the sole unit-ratio table in the codebase. Calendar application
    /// units (Day, Week, Month, and Year) have nominal fixed ratios defined
    /// here solely for duration value identity and magnitude ordering;
    /// their date-shifting behavior is owned exclusively by the date
    /// module's calendar owner.
    #[must_use]
    pub(crate) const fn fixed_seconds(self) -> f64 {
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

    /// Whole fixed seconds per unit as `i64`, derived from
    /// [`Self::fixed_seconds`].
    ///
    /// Returns `None` for [`Self::Millisecond`], which is fractional seconds.
    /// Enumerates every variant exhaustively per `pat-exhaustive-enum`.
    #[must_use]
    #[expect(
        clippy::as_conversions,
        reason = "derives whole seconds from the sole fixed_seconds ratio \
                  definition"
    )]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "fixed_seconds for whole-second units always fit in i64"
    )]
    pub(crate) const fn fixed_seconds_i64(self) -> Option<i64> {
        match self {
            Self::Millisecond => None,
            Self::Second
            | Self::Minute
            | Self::Hour
            | Self::Day
            | Self::Week
            | Self::Month
            | Self::Year => Some(self.fixed_seconds() as i64),
        }
    }
}

/// Human-readable list of accepted unit spellings for error messages.
///
/// Kept beside [`UNIT_MAP`] so the two cannot drift: extend this when a
/// unit or spelling family is added to the registry.
pub(crate) const UNIT_HINT: &str =
    "\"years\"/\"y\", \"months\"/\"mo\", \"weeks\"/\"w\", \"days\"/\"d\", \
     \"hours\"/\"h\", \"minutes\"/\"m\", \"seconds\"/\"s\", or \"ms\"";

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
/// Wraps `f64` with NaN-safe ordering and arithmetic; the arithmetic
/// operators do not re-validate finiteness, so an operation that overflows
/// can yield a non-finite result. Always finite when constructed through
/// [`DurationValue::to_seconds`] or [`DurationSeconds::try_from`]. A signed
/// zero is normalized to positive zero at construction, so `"-0m"` and
/// `"0m"` compare, order, and hash identically.
#[derive(Copy, Clone, Debug)]
pub(crate) struct DurationSeconds(pub(crate) f64);

/// At or above this magnitude, [`DurationSeconds`]'s [`Display`](fmt::Display)
/// switches to scientific notation.
const DISPLAY_EXPONENT_UPPER: f64 = 1e15;

/// Below this magnitude (excluding exact zero), [`DurationSeconds`]'s
/// [`Display`](fmt::Display) switches to scientific notation.
const DISPLAY_EXPONENT_LOWER: f64 = 1e-6;

impl DurationSeconds {
    /// Constructs from a raw `f64`, normalizing a signed zero to positive
    /// zero so the type's invariant (see the type docs) holds everywhere,
    /// not just at parse time.
    #[inline]
    fn normalized(value: f64) -> Self {
        Self(if value == 0.0 {
            0.0
        } else {
            value
        })
    }
}

impl TryFrom<f64> for DurationSeconds {
    type Error = DurationError;

    /// Normalizes a signed zero to positive zero (see the type docs).
    #[inline]
    fn try_from(secs: f64) -> Result<Self, Self::Error> {
        secs.is_finite()
            .then(|| Self::normalized(secs))
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

/// Renders in scientific notation once the magnitude reaches or clears
/// [`DISPLAY_EXPONENT_UPPER`] or falls below [`DISPLAY_EXPONENT_LOWER`]
/// (exact zero excluded), so an extreme magnitude never dumps a
/// hundreds-of-digits decimal literal.
impl fmt::Display for DurationSeconds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.0;
        let abs = value.abs();
        if abs != 0.0
            && !(DISPLAY_EXPONENT_LOWER..DISPLAY_EXPONENT_UPPER).contains(&abs)
        {
            write!(f, "{value:e}")
        } else {
            write!(f, "{value}")
        }
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
        Self::normalized(self.0 + rhs.0)
    }
}

impl Sub for DurationSeconds {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self::normalized(self.0 - rhs.0)
    }
}

impl Mul<f64> for DurationSeconds {
    type Output = Self;

    fn mul(self, rhs: f64) -> Self {
        Self::normalized(self.0 * rhs)
    }
}

impl Mul<DurationSeconds> for f64 {
    type Output = DurationSeconds;

    #[inline]
    fn mul(self, rhs: DurationSeconds) -> DurationSeconds {
        DurationSeconds::normalized(self * rhs.0)
    }
}

/// Error returned when a duration operation fails.
///
/// Covers two failure domains: parsing human-readable duration text (via
/// [`DurationValue`]'s crate-internal parser) and converting raw seconds
/// into a validated seconds value.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
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

    /// The number portion has a malformed sign (e.g., `"+h"`), or a part
    /// after the first carries an explicit `-` (e.g., `"1h -30m"`).
    #[error("invalid number in `{input}`")]
    InvalidNumber {
        /// The raw input that failed to parse.
        input: String,
    },

    /// The number portion is not valid float syntax (e.g., a bare `"."`).
    #[error("invalid number syntax in `{input}`: {source}")]
    MalformedNumber {
        /// The raw input that failed to parse.
        input: String,
        /// The underlying float-parse failure.
        #[source]
        source: std::num::ParseFloatError,
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

    /// A single part's number overflows to infinity (e.g., a 400-digit
    /// literal).
    #[error("number overflows to infinity in `{input}`")]
    NonFiniteNumber {
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

    mod value {
        use super::*;

        mod parse {
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

            #[rstest]
            #[case::with_space("1h 30m")]
            #[case::with_comma("1h, 30m")]
            #[case::without_separator("1h30m")]
            #[case::consecutive_separators("1  h  30  m")]
            fn parses_multi_part(#[case] input: &str) {
                assert_eq!(
                    DurationValue::parse(input).unwrap().to_seconds(),
                    DurationSeconds::try_from(5_400.0).unwrap()
                );
            }

            #[test]
            fn applies_the_leading_sign_to_the_whole_duration() {
                assert_eq!(
                    DurationValue::parse("-1h 30m").unwrap().to_seconds(),
                    DurationSeconds::try_from(-5_400.0).unwrap()
                );
            }

            #[rstest]
            #[case::unsigned("1.5h", 5_400.0)]
            #[case::without_leading_digit(".5h", 1_800.0)]
            #[case::negative_without_leading_digit("-.5h", -1_800.0)]
            #[case::positive_with_leading_plus("+.5h", 1_800.0)]
            #[case::exponent("1e3s", 1_000.0)]
            #[case::negative_exponent("1e-2h", 36.0)]
            #[case::explicit_exponent_sign("1e+2d", 8_640_000.0)]
            #[case::uppercase_exponent("1E3m", 60_000.0)]
            #[case::exponent_after_decimal(".5e2s", 50.0)]
            fn parses_decimal(#[case] input: &str, #[case] expected: f64) {
                assert_eq!(
                    DurationValue::parse(input).unwrap().to_seconds(),
                    DurationSeconds::try_from(expected).unwrap()
                );
            }

            #[test]
            fn trims_leading_trailing_whitespace() {
                let d = DurationValue::parse(" 1h ").unwrap();
                assert_eq!(
                    d.to_seconds(),
                    DurationSeconds::try_from(3_600.0).unwrap()
                );
            }

            #[rstest]
            #[case::trailing_space("1h ")]
            #[case::trailing_comma("1h,")]
            fn accepts_trailing_separator(#[case] input: &str) {
                let d = DurationValue::parse(input).unwrap();
                assert_eq!(
                    d.to_seconds(),
                    DurationSeconds::try_from(3_600.0).unwrap()
                );
            }

            #[test]
            fn accepts_redundant_plus_on_non_leading_part() {
                let d = DurationValue::parse("1h +30m").unwrap();
                assert_eq!(
                    d.to_seconds(),
                    DurationSeconds::try_from(5_400.0).unwrap()
                );
            }

            mod error {
                use pretty_assertions::assert_eq;
                use rstest::rstest;

                use super::*;

                #[rstest]
                #[case::empty("")]
                #[case::whitespace_only("   ")]
                #[case::separator_only(",")]
                fn rejects_empty_input(#[case] input: &str) {
                    assert!(matches!(
                        DurationValue::parse(input),
                        Err(DurationError::Empty)
                    ));
                }

                #[rstest]
                #[case::positive_sign("+h")]
                #[case::negative_sign("-h")]
                fn rejects_sign_without_digit(#[case] input: &str) {
                    let err = DurationValue::parse(input).unwrap_err();
                    assert!(matches!(err, DurationError::InvalidNumber { .. }));
                }

                #[test]
                fn rejects_negative_on_non_leading_part() {
                    let err = DurationValue::parse("1h -30m").unwrap_err();
                    assert!(matches!(err, DurationError::InvalidNumber { .. }));
                }

                #[test]
                fn rejects_malformed_number_syntax_with_a_chained_source() {
                    use std::error::Error as _;
                    let err = DurationValue::parse(".h").unwrap_err();
                    assert!(matches!(
                        err,
                        DurationError::MalformedNumber { .. }
                    ));
                    assert!(err.source().is_some());
                }

                #[test]
                fn treats_a_single_part_overflowing_to_infinity_as_non_finite()
                {
                    let overflowing = format!("1{}h", "0".repeat(400));
                    let err = DurationValue::parse(&overflowing).unwrap_err();
                    assert!(matches!(
                        &err,
                        DurationError::NonFiniteNumber { input }
                        if input == &overflowing
                    ));
                }

                #[test]
                fn rejects_a_multi_part_fold_overflowing_to_infinity() {
                    // Each part is finite (1e308 is below `f64::MAX`), but
                    // their Σ overflows: the parse-level guard is
                    // `NonFiniteSeconds`, not `NonFiniteNumber`, which
                    // targets per-part magnitudes.
                    let huge =
                        format!("1{}s 1{}s", "0".repeat(308), "0".repeat(308));
                    let err = DurationValue::parse(&huge).unwrap_err();
                    assert!(matches!(err, DurationError::NonFiniteSeconds));
                }

                #[rstest]
                #[case::bare_unit("h")]
                #[case::unit_without_number(", h")]
                fn rejects_unit_without_number(#[case] input: &str) {
                    assert!(matches!(
                        DurationValue::parse(input),
                        Err(DurationError::MissingNumber { .. })
                    ));
                }

                #[rstest]
                #[case::double_decimal("1.2.3h")]
                #[case::bare_number("1")]
                fn rejects_number_without_unit(#[case] input: &str) {
                    assert!(matches!(
                        DurationValue::parse(input),
                        Err(DurationError::MissingUnit { .. })
                    ));
                }

                #[rstest]
                #[case::unknown_unit("1x")]
                #[case::longer_than_16_bytes("1aaaaaaaaaaaaaaaaa")]
                // A truncated exponent (`"1e"`, `"1e+"`) is not consumed as
                // a number: the `e` falls to the unit scanner, which
                // rejects it as unknown rather than misreading the value.
                #[case::truncated_exponent("1e")]
                #[case::exponent_sign_without_digits("1e+")]
                fn rejects_unknown_or_long_unit(#[case] input: &str) {
                    assert!(DurationValue::parse(input).is_err());
                }

                #[rstest]
                #[case("1fortnight", "unknown unit in `1fortnight`")]
                #[case("hours", "no number before unit in `hours`")]
                #[case("42", "no unit after number in `42`")]
                fn returns_error_message(
                    #[case] input: &str,
                    #[case] expected: &str,
                ) {
                    let err = DurationValue::parse(input).unwrap_err();
                    assert_eq!(err.to_string(), expected);
                }
            }

            mod prefix {
                use pretty_assertions::assert_eq;
                use rstest::rstest;

                use super::*;

                #[rstest]
                #[case::simple("45m]", 3, "45m", 2_700.0)]
                #[case::decimal(".5h remainder", 3, ".5h", 1_800.0)]
                #[case::multi_part("1h, 30m, extra", 7, "1h, 30m", 5_400.0)]
                #[case::negative("-15m extra", 4, "-15m", -900.0)]
                #[case::positive("+15m extra", 4, "+15m", 900.0)]
                #[case::decimal_after_sign("-.5h rest", 4, "-.5h", -1_800.0)]
                #[case::exact_end("1h", 2, "1h", 3_600.0)]
                fn parses_valid_prefix(
                    #[case] input: &str,
                    #[case] expected_consumed: usize,
                    #[case] expected_str: &str,
                    #[case] expected_secs: f64,
                ) {
                    let (dv, consumed) =
                        DurationValue::parse_prefix(input).expect("prefix");
                    assert_eq!(consumed, expected_consumed);
                    assert_eq!(dv.as_str(), expected_str);
                    assert_eq!(
                        dv.to_seconds(),
                        DurationSeconds::try_from(expected_secs).unwrap()
                    );
                }

                #[rstest]
                #[case::non_duration("not a duration")]
                #[case::hyphen_bullet("- 15m")]
                #[case::non_leading_negative("1h -30m")]
                #[case::empty("")]
                fn rejects_invalid_prefix(#[case] input: &str) {
                    assert!(DurationValue::parse_prefix(input).is_none());
                }
            }

            mod can_start {
                use rstest::rstest;

                use super::*;

                #[rstest]
                #[case::digit("1h")]
                #[case::decimal_point(".5h")]
                #[case::plus_sign("+1h")]
                #[case::minus_sign("-30m")]
                #[case::sign_with_decimal("+.5h")]
                fn accepts_valid_start(#[case] input: &str) {
                    assert!(DurationValue::can_start(input));
                }

                #[rstest]
                #[case::non_numeric("hello")]
                #[case::empty("")]
                #[case::lone_sign("+")]
                #[case::lone_decimal_point(".")]
                fn rejects_invalid_start(#[case] input: &str) {
                    assert!(!DurationValue::can_start(input));
                }
            }
        }

        mod constructor {
            use pretty_assertions::assert_eq;
            use rstest::rstest;

            use super::*;

            #[rstest]
            #[case::zero(0.0, "0s")]
            #[case::positive(5_400.0, "1h 30m")]
            #[case::negative(-5_400.0, "-1h 30m")]
            fn synthesizes(#[case] raw_secs: f64, #[case] expected_str: &str) {
                let secs = DurationSeconds::try_from(raw_secs).unwrap();
                let dv = DurationValue::from_seconds(secs);
                assert_eq!(dv.as_str(), expected_str);
                assert_eq!(dv.to_seconds(), secs);
            }

            #[rstest]
            #[case::negative_zero(-0.0, "0s")]
            #[case::sub_second(0.5, "500ms")]
            #[case::large_compound(
                604_800.0 + 86_400.0 + 3_600.0 + 60.0 + 1.0 + 0.5,
                "1w 1d 1h 1m 1s 500ms"
            )]
            fn handles(#[case] raw_secs: f64, #[case] expected_str: &str) {
                let secs = DurationSeconds::try_from(raw_secs).unwrap();
                let dv = DurationValue::from_seconds(secs);
                assert_eq!(dv.as_str(), expected_str);
            }

            #[rstest]
            #[case::seconds(90.0)]
            #[case::minutes(5_400.0)]
            #[case::compound(
                604_800.0 + 86_400.0 + 3_600.0 + 60.0 + 1.0 + 0.5
            )]
            #[case::negative(-5_400.0)]
            #[case::negative_compound(-(86_400.0 + 3_600.0))]
            #[case::scientific_huge(1e300)]
            #[case::scientific_negative_huge(-1e300)]
            #[case::scientific_tiny(1e-300)]
            fn roundtrips_through_parser(#[case] raw_seconds: f64) {
                let secs = DurationSeconds::try_from(raw_seconds).unwrap();
                let synthesized = DurationValue::from_seconds(secs);
                let reparsed = DurationValue::parse(synthesized.as_str())
                    .expect("synthesized string must parse");
                assert_eq!(reparsed.to_seconds(), secs);
            }
        }

        mod equality {
            use pretty_assertions::{assert_eq, assert_ne};

            use super::*;

            #[test]
            fn equates_semantically_equivalent_spellings() {
                let a = DurationValue::parse("1h 30m").unwrap();
                let b = DurationValue::parse("90m").unwrap();
                assert_eq!(a, b);
            }

            #[test]
            fn distinguishes_different_durations() {
                let a = DurationValue::parse("1h").unwrap();
                let b = DurationValue::parse("2h").unwrap();
                assert_ne!(a, b);
            }

            #[test]
            fn treats_negative_zero_as_equal_to_positive_zero() {
                let neg = DurationValue::parse("-0m").unwrap();
                let pos = DurationValue::parse("0m").unwrap();
                assert_eq!(neg, pos);
            }
        }

        mod ordering {
            use super::*;

            #[test]
            fn partial_ord_matches_ord() {
                let a = DurationValue::parse("30m").unwrap();
                let b = DurationValue::parse("1h").unwrap();
                assert!(a < b);
                assert!(b > a);
            }

            #[test]
            fn orders_signed_durations_correctly() {
                let neg = DurationValue::parse("-15m").unwrap();
                let zero = DurationValue::parse("0m").unwrap();
                let pos = DurationValue::parse("+15m").unwrap();
                assert!(neg < zero);
                assert!(zero < pos);
            }

            #[test]
            fn orders_negative_zero_as_greater_than_or_equal_to_positive_zero()
            {
                let neg_zero = DurationValue::parse("-0m").unwrap();
                let zero = DurationValue::parse("0m").unwrap();
                assert!(neg_zero >= zero);
            }
        }

        mod hashing {
            use std::{
                collections::hash_map::DefaultHasher,
                hash::{Hash, Hasher},
            };

            use rstest::rstest;

            use super::*;

            fn hash_val<T: Hash>(val: &T) -> u64 {
                let mut hasher = DefaultHasher::new();
                val.hash(&mut hasher);
                hasher.finish()
            }

            #[rstest]
            #[case::equal("1h 30m", "90m", true)]
            #[case::different("1h", "2h", false)]
            fn duration_value_hash(
                #[case] a: &str,
                #[case] b: &str,
                #[case] should_match: bool,
            ) {
                let a = DurationValue::parse(a).unwrap();
                let b = DurationValue::parse(b).unwrap();
                if should_match {
                    assert_eq!(hash_val(&a), hash_val(&b));
                } else {
                    assert_ne!(hash_val(&a), hash_val(&b));
                }
            }

            #[rstest]
            #[case::equal(100.0, 100.0, true)]
            #[case::different(100.0, 200.0, false)]
            fn seconds_hash(
                #[case] a: f64,
                #[case] b: f64,
                #[case] should_match: bool,
            ) {
                let a = DurationSeconds::try_from(a).unwrap();
                let b = DurationSeconds::try_from(b).unwrap();
                if should_match {
                    assert_eq!(hash_val(&a), hash_val(&b));
                } else {
                    assert_ne!(hash_val(&a), hash_val(&b));
                }
            }
        }

        mod formatting {
            use pretty_assertions::assert_eq;
            use rstest::rstest;

            use super::*;

            #[test]
            fn returns_raw_spelling_for_duration_value() {
                let d = DurationValue::parse("4 yrs, 6 wks").unwrap();
                assert_eq!(format!("{d}"), "4 yrs, 6 wks");
                assert_eq!(d.as_str(), "4 yrs, 6 wks");
            }

            #[test]
            fn seconds_displays_as_number() {
                assert_eq!(
                    DurationSeconds::try_from(3600.0).unwrap().to_string(),
                    "3600"
                );
                assert_eq!(
                    DurationSeconds::try_from(0.5).unwrap().to_string(),
                    "0.5"
                );
            }

            #[rstest]
            #[case::huge_positive(1e300, "1e300")]
            #[case::huge_negative(-1e300, "-1e300")]
            #[case::tiny_positive(1e-300, "1e-300")]
            #[case::at_upper_threshold(DISPLAY_EXPONENT_UPPER, "1e15")]
            #[case::just_below_upper_threshold(1e14, "100000000000000")]
            #[case::at_lower_threshold(DISPLAY_EXPONENT_LOWER, "0.000001")]
            #[case::just_above_lower_threshold(0.0004, "0.0004")]
            fn seconds_display_switches_dialect_by_magnitude(
                #[case] value: f64,
                #[case] expected: &str,
            ) {
                assert_eq!(
                    DurationSeconds::try_from(value).unwrap().to_string(),
                    expected
                );
            }

            #[rstest]
            #[case::huge_positive(1e300, "1e300s")]
            #[case::huge_negative(-1e300, "-1e300s")]
            #[case::tiny_positive(0.0004, "0.0004s")]
            fn from_seconds_never_lies_about_a_nonzero_magnitude(
                #[case] value: f64,
                #[case] expected: &str,
            ) {
                let seconds = DurationSeconds::try_from(value).unwrap();
                assert_eq!(
                    DurationValue::from_seconds(seconds).as_str(),
                    expected
                );
            }
        }

        mod serialization {
            use pretty_assertions::assert_eq;

            use super::*;

            #[test]
            fn serializes_to_raw_string() {
                let dv = DurationValue::parse("1h 30m").unwrap();
                let json = serde_json::to_string(&dv).unwrap();
                assert_eq!(json, "\"1h 30m\"");
            }
        }

        mod deserialization {
            use pretty_assertions::assert_eq;

            use super::*;

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
            fn rejects_unparseable_duration_string() {
                let json = "\"not a duration\"";
                let err =
                    serde_json::from_str::<DurationValue>(json).unwrap_err();
                assert!(err.to_string().contains("no number before unit"));
            }
        }

        mod conversions {
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
                let err =
                    "not a duration".parse::<DurationValue>().unwrap_err();
                assert!(matches!(err, DurationError::MissingNumber { .. }));
            }
        }
        mod parts {
            use pretty_assertions::assert_eq;
            use rstest::rstest;

            use super::*;

            #[test]
            fn parts_carry_written_magnitude_with_whole_duration_sign_applied()
            {
                // Arrange
                let input = "-1h 30m";

                // Act
                let dv = DurationValue::parse(input).unwrap();
                let parts = dv.parts().unwrap();

                // Assert (N15b corrected convention)
                assert_eq!(parts.len(), 2);
                assert_eq!(
                    parts.first().copied(),
                    Some((-1.0, DurationUnit::Hour))
                );
                assert_eq!(
                    parts.get(1).copied(),
                    Some((-30.0, DurationUnit::Minute))
                );
            }

            #[test]
            fn prefix_retains_identical_parts_to_full_parse() {
                // Arrange
                let text = "-1h 30m trailing";

                // Act
                let (prefix_dv, consumed) =
                    DurationValue::parse_prefix(text).unwrap();
                let full_dv = DurationValue::parse("-1h 30m").unwrap();

                // Assert
                assert_eq!(consumed, 7);
                assert_eq!(prefix_dv.parts(), full_dv.parts());
                assert_eq!(prefix_dv.to_seconds(), full_dv.to_seconds());
            }

            #[rstest]
            #[case::simple("1h 30m")]
            #[case::negative("-1h 30m")]
            #[case::mixed_units("4 yrs, 6 wks")]
            #[case::sub_second("500ms")]
            #[case::float_values("0.5h 15m")]
            #[case::float_non_associative("10000000000000000s 1s 1s")]
            fn bit_exact_sigma_invariant_for_parsed_values(
                #[case] input: &str,
            ) {
                // Arrange & Act
                let dv = DurationValue::parse(input).unwrap();
                let parts = dv.parts().unwrap();
                let fold_total = DurationValue::fold_parts(parts);

                // Assert: bit-exact equality between the fold over parts and
                // the stored seconds, compared as raw `f64` so a signed-zero
                // divergence could not hide behind `total_cmp`.
                #[expect(
                    clippy::float_cmp,
                    reason = "ticket 03 pins the bit-exact Σ invariant with \
                              raw `f64` `==`"
                )]
                let bit_exact = fold_total == dv.to_seconds().0;
                assert!(
                    bit_exact,
                    "Σ fold must reconstruct stored seconds bit-exactly"
                );
            }
        }

        mod arithmetic {
            use pretty_assertions::assert_eq;

            use super::*;

            #[test]
            fn add_and_sub_combine_parts_for_some_operands() {
                // Arrange
                let expected = DurationValue::parse("1h 30m").unwrap();

                // Act
                let sum = DurationValue::parse("1h").unwrap()
                    + DurationValue::parse("30m").unwrap();

                // Assert: `1h` + `30m` equals the `1h 30m` spelling
                // (value-level pin, spec line 88), and the
                // fold-based result survives subtraction of the
                // same operand.
                assert_eq!(sum, expected);
                let sub = expected - DurationValue::parse("30m").unwrap();
                assert_eq!(
                    sub.to_seconds(),
                    DurationSeconds::try_from(3_600.0).unwrap()
                );

                // `Sub` keeps the negated rhs entry as the regime witness.
                let sub_parts =
                    sub.parts().expect("Some+Some Sub retains parts");
                assert_eq!(sub_parts.len(), 3);
                assert_eq!(
                    sub_parts.last().copied(),
                    Some((-30.0, DurationUnit::Minute))
                );
            }

            #[test]
            fn bit_exact_sigma_invariant_for_arithmetic_results() {
                // Arrange
                let a = DurationValue::parse("10000000000000000s").unwrap();
                let b = DurationValue::parse("1s 1s").unwrap();

                // Act
                let sum = a + b;
                let sum_parts = sum.parts().unwrap();
                let fold_total = DurationValue::fold_parts(sum_parts);

                // Assert: fold over the combined parts bit-exactly equals the
                // stored seconds, compared as raw `f64`.
                #[expect(
                    clippy::float_cmp,
                    reason = "ticket 03 pins the bit-exact Σ invariant with \
                              raw `f64` `==`"
                )]
                let bit_exact = fold_total == sum.to_seconds().0;
                assert!(
                    bit_exact,
                    "Σ fold must reconstruct stored seconds bit-exactly"
                );
            }

            #[test]
            fn arithmetic_with_none_operand_clears_parts_and_adds_seconds() {
                // Arrange
                let none_dv = DurationValue::from_seconds(
                    DurationSeconds::try_from(3_600.0).unwrap(),
                );
                let some_dv = DurationValue::parse("30m").unwrap();

                // Act
                let sum = none_dv + some_dv;

                // Assert
                assert!(sum.parts().is_none());
                assert_eq!(
                    sum.to_seconds(),
                    DurationSeconds::try_from(5_400.0).unwrap()
                );
            }

            #[test]
            fn sub_with_none_operand_clears_parts_and_subtracts_seconds() {
                // Arrange
                let none_dv = DurationValue::from_seconds(
                    DurationSeconds::try_from(3_600.0).unwrap(),
                );
                let some_dv = DurationValue::parse("30m").unwrap();

                // Act
                let diff = none_dv - some_dv;

                // Assert: mixed `Some`+`None` ⇒ `None` witness; seconds
                // subtract directly (`Sub` = add of the negated rhs).
                assert!(diff.parts().is_none());
                assert_eq!(
                    diff.to_seconds(),
                    DurationSeconds::try_from(1_800.0).unwrap()
                );
            }

            #[test]
            fn mul_scales_parts_and_preserves_bit_exact_sigma() {
                // Arrange
                let dv = DurationValue::parse("1h 30m").unwrap();

                // Act
                let scaled = dv * 2.5;
                let parts = scaled.parts().unwrap();
                let fold_total = DurationValue::fold_parts(parts);

                // Assert
                assert_eq!(parts.len(), 2);
                assert_eq!(
                    parts.first().copied(),
                    Some((2.5, DurationUnit::Hour))
                );
                assert_eq!(
                    parts.get(1).copied(),
                    Some((75.0, DurationUnit::Minute))
                );
                #[expect(
                    clippy::float_cmp,
                    reason = "ticket 03 pins the bit-exact Σ invariant with \
                              raw `f64` `==`"
                )]
                let bit_exact = fold_total == scaled.to_seconds().0;
                assert!(
                    bit_exact,
                    "Σ fold must reconstruct stored seconds bit-exactly"
                );
            }

            #[test]
            fn mul_non_finite_scalar_produces_non_finite_seconds_and_never_panics()
             {
                // Arrange
                let dv = DurationValue::parse("1h").unwrap();

                // Act
                let inf_dv = dv.clone() * f64::INFINITY;
                let nan_dv = dv * f64::NAN;

                // Assert
                assert!(!inf_dv.to_seconds().0.is_finite());
                assert_eq!(inf_dv.as_str(), "infs");
                assert!(matches!(
                    TimeDelta::try_from(inf_dv),
                    Err(DurationError::NonFiniteSeconds)
                ));

                assert!(!nan_dv.to_seconds().0.is_finite());
                assert!(matches!(
                    TimeDelta::try_from(nan_dv),
                    Err(DurationError::NonFiniteSeconds)
                ));
            }

            #[test]
            fn mul_overflow_leaves_non_finite_seconds_rejected_by_conversions()
            {
                // Arrange: `1s` has a unit ratio of exactly 1, so scaling by
                // `f64::MAX` stays finite and the next doubling overflows.
                let dv = DurationValue::parse("1s").unwrap();

                // Act
                let huge = dv * f64::MAX;
                assert!(huge.to_seconds().0.is_finite());
                let overflowed = huge * 2.0;

                // Assert: the overflow leaves non-finite seconds that every
                // consuming conversion rejects, and the display never lies as
                // `"0s"`.
                assert!(!overflowed.to_seconds().0.is_finite());
                assert_eq!(overflowed.as_str(), "infs");
                assert!(matches!(
                    TimeDelta::try_from(overflowed),
                    Err(DurationError::NonFiniteSeconds)
                ));
            }
        }

        mod regime {
            use super::*;

            #[test]
            fn regime_witness_distinguishes_calendar_from_fixed_units() {
                // Calendar units
                assert!(DurationValue::parse("1d").unwrap().is_calendar());
                assert!(DurationValue::parse("1w").unwrap().is_calendar());
                assert!(DurationValue::parse("1mo").unwrap().is_calendar());
                assert!(DurationValue::parse("1y").unwrap().is_calendar());
                assert!(DurationValue::parse("1d 2h").unwrap().is_calendar());

                // Sub-day fixed units
                assert!(!DurationValue::parse("1h").unwrap().is_calendar());
                assert!(!DurationValue::parse("30m").unwrap().is_calendar());
                assert!(!DurationValue::parse("45s").unwrap().is_calendar());
                assert!(!DurationValue::parse("500ms").unwrap().is_calendar());
                assert!(
                    !DurationValue::parse("1h 30m 10s").unwrap().is_calendar()
                );

                // None (synthesized) values are fixed regime
                let synth = DurationValue::from_seconds(
                    DurationSeconds::try_from(86_400.0).unwrap(),
                );
                assert!(!synth.is_calendar());
            }
        }
    }

    mod unit {
        use super::*;

        mod parse {
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

            #[rstest]
            #[case::unknown("foo")]
            #[case::exactly_16_bytes("aaaaaaaaaaaaaaaa")]
            #[case::longer_than_16_bytes("aaaaaaaaaaaaaaaaa")]
            #[case::empty("")]
            fn rejects_unit(#[case] input: &str) {
                assert!(DurationUnit::parse(input).is_none());
            }
        }

        mod seconds {
            use super::*;

            #[test]
            fn fixed_seconds_are_positive_for_all_variants() {
                for entry in UNIT_MAP.entries() {
                    assert!(entry.1.fixed_seconds() > 0.0);
                }
            }

            #[test]
            fn fixed_seconds_i64_matches_fixed_seconds_for_each_variant() {
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
                    assert_eq!(unit.fixed_seconds_i64(), expected);
                }
            }
        }
    }

    mod seconds {
        use super::*;

        mod arithmetic {
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
            fn mul_normalizes_a_resulting_signed_zero_from_either_operand_order()
             {
                let zero = DurationSeconds::try_from(0.0).unwrap();
                assert_eq!(zero * -1.0, zero);
                assert_eq!(-1.0 * zero, zero);
                assert!(zero * -1.0 >= zero);
            }
        }

        mod ordering {
            use pretty_assertions::assert_eq;

            use super::*;

            #[test]
            fn orders_by_total_cmp() {
                let a = DurationSeconds::try_from(100.0).unwrap();
                let b = DurationSeconds::try_from(200.0).unwrap();
                assert!(a < b);
                assert!(b > a);
                assert_eq!(a, DurationSeconds::try_from(100.0).unwrap());
            }

            #[test]
            fn treats_negative_zero_as_equal_to_positive_zero() {
                let neg = DurationSeconds::try_from(-0.0).unwrap();
                let pos = DurationSeconds::try_from(0.0).unwrap();
                assert_eq!(neg, pos);
                assert!(neg >= pos && pos >= neg);
            }
        }

        mod conversions {
            use pretty_assertions::assert_eq;

            use super::*;

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
            fn extracts_seconds_from_duration_value() {
                let dv = DurationValue::parse("1h").unwrap();
                assert_eq!(
                    dv.to_seconds(),
                    DurationSeconds::try_from(3_600.0).unwrap()
                );
            }
        }
    }

    mod time_delta_conversion {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::whole_second("1h", TimeDelta::hours(1))]
        #[case::subsecond("500ms", TimeDelta::milliseconds(500))]
        fn converts_duration_value(
            #[case] input: &str,
            #[case] expected: TimeDelta,
        ) {
            let duration = DurationValue::parse(input).expect("valid duration");
            let converted = TimeDelta::try_from(duration).expect("in range");
            assert_eq!(converted, expected);
        }

        #[rstest]
        #[case::zero(0.0, TimeDelta::zero())]
        #[case::negative_whole(-10.0, TimeDelta::seconds(-10))]
        #[case::negative_fractional(
            -90.5,
            TimeDelta::milliseconds(-90_500)
        )]
        #[case::sub_microsecond(0.000_000_001, TimeDelta::nanoseconds(1))]
        fn converts_duration_seconds(
            #[case] input_secs: f64,
            #[case] expected: TimeDelta,
        ) {
            let seconds =
                DurationSeconds::try_from(input_secs).expect("finite seconds");
            let converted = TimeDelta::try_from(seconds).expect("in range");
            assert_eq!(converted, expected);
        }

        #[rstest]
        #[case::extreme_positive(1e300)]
        #[case::extreme_negative(-1e300)]
        fn rejects_seconds_outside_representable_range(
            #[case] input_secs: f64,
        ) {
            let seconds =
                DurationSeconds::try_from(input_secs).expect("finite seconds");
            let result = TimeDelta::try_from(seconds);
            assert!(matches!(result, Err(DurationError::NonFiniteSeconds)));
        }
    }
}
