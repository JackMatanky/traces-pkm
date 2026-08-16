//! Validate exact and forgiving field identifiers, and carry field values
//! parsed from TOML, JSON, or YAML.
//!
//! # Main Types
//!
//! - [`FieldName`] - Exact schema field identity
//! - [`FieldNameRef`] - Borrowed field-name identity
//! - [`FieldKey`] - Forgiving note field identity with a canonical form
//! - [`FieldValue`] - Owned, format-agnostic field value
//! - [`FieldValueRef`] - Zero-copy borrowed field value
//! - [`FieldNameError`] - Field-name parse failure
//! - [`FieldKeyError`] - Field-key parse failure
//!
//! [`FieldName`] preserves exact identity: `status` and `Status` are distinct.
//! [`FieldKey`] preserves the original text for display and stores a canonical
//! form for forgiving matches across case, whitespace, and punctuation changes.
//!
//! [`FieldValueRef`] borrows from a document's source text wherever the backing
//! TOML/JSON/YAML deserializer supports it (an unescaped string borrows
//! directly; anything needing processing still allocates). This follows the
//! crate's owned/borrowed newtype split: [`FieldValue`] is the owned
//! counterpart, built once a value must outlive its source text.

use std::{
    borrow::{Borrow, Cow},
    collections::BTreeMap,
    fmt,
    marker::PhantomData,
    str::FromStr,
};

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, Error as _, MapAccess, SeqAccess, Visitor},
    ser::{SerializeMap, SerializeSeq},
};
use thiserror::Error;
use yaml_serde as serde_yaml;

/// Stores an exact schema field identifier.
///
/// Equality and ordering use the raw text. `status` and `Status` remain
/// different identities.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct FieldName(String);

impl FieldName {
    /// Returns this name as a string slice.
    #[inline]
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// Borrows this name as a [`FieldNameRef`].
    #[inline]
    #[must_use]
    pub(crate) fn as_ref(&self) -> FieldNameRef<'_> {
        FieldNameRef(&self.0)
    }

    /// Converts this name into a forgiving [`FieldKey`].
    ///
    /// Allocates the canonical form while preserving the exact name text.
    #[must_use]
    pub(crate) fn to_key(&self) -> FieldKey {
        let canonical = FieldKey::canonicalize(&self.0);
        FieldKey {
            name: self.0.clone(),
            canonical,
        }
    }

    /// Validates `raw` as a field name.
    ///
    /// # Errors
    ///
    /// - [`FieldNameError::Empty`] if `raw` is empty or whitespace-only
    /// - [`FieldNameError::ContainsSlash`] if `raw` contains `/`
    /// - [`FieldNameError::EmptyCanonical`] if `raw` has no searchable
    ///   characters after [`FieldKey`] canonicalization
    fn validate(raw: &str) -> Result<(), FieldNameError> {
        if raw.trim().is_empty() {
            return Err(FieldNameError::Empty);
        }
        if raw.contains('/') {
            return Err(FieldNameError::ContainsSlash {
                name: raw.to_owned(),
            });
        }
        if FieldKey::is_canonical_empty(raw) {
            return Err(FieldNameError::EmptyCanonical {
                name: raw.to_owned(),
            });
        }
        Ok(())
    }
}

impl TryFrom<String> for FieldName {
    type Error = FieldNameError;

    /// Validates `raw` and constructs a [`FieldName`].
    ///
    /// # Errors
    ///
    /// See [`FieldName::validate`].
    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::validate(&raw)?;
        Ok(Self(raw))
    }
}

impl TryFrom<&str> for FieldName {
    type Error = FieldNameError;

    /// Validates `raw` and constructs a [`FieldName`].
    ///
    /// # Errors
    ///
    /// See [`FieldName::validate`].
    fn try_from(raw: &str) -> Result<Self, Self::Error> {
        Self::validate(raw)?;
        Ok(Self(raw.to_owned()))
    }
}

impl FromStr for FieldName {
    type Err = FieldNameError;

    /// Parses `raw` as a [`FieldName`].
    ///
    /// # Errors
    ///
    /// See [`FieldName::validate`].
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::try_from(raw)
    }
}

impl TryFrom<serde_yaml::Value> for FieldName {
    type Error = FieldNameError;

    /// Coerces a YAML scalar `value` into a [`FieldName`].
    ///
    /// # Errors
    ///
    /// Returns [`FieldNameError::UnsupportedYamlKey`] for `Null`, `Sequence`,
    /// `Mapping`, and `Tagged` values; otherwise see [`FieldName::validate`].
    fn try_from(value: serde_yaml::Value) -> Result<Self, Self::Error> {
        let raw = yaml_scalar_to_string(value)
            .ok_or(FieldNameError::UnsupportedYamlKey)?;
        Self::try_from(raw)
    }
}

impl From<FieldNameRef<'_>> for FieldName {
    fn from(name: FieldNameRef<'_>) -> Self {
        Self(name.0.to_owned())
    }
}

impl Borrow<str> for FieldName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for FieldName {
    /// Matches `str`'s own `Debug` (quoted, escaped) so wrapping a field name
    /// in this type never changes an error message's text.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for FieldName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl<'de> Deserialize<'de> for FieldName {
    /// Deserializes from a string and validates it as a [`FieldName`].
    ///
    /// # Errors
    ///
    /// See [`FieldName::validate`].
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::try_from(raw).map_err(D::Error::custom)
    }
}

/// Borrows an exact schema field identifier.
///
/// Use this where a validated field name is needed without allocating an owned
/// [`FieldName`].
#[derive(Copy, Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct FieldNameRef<'a>(&'a str);

impl<'a> FieldNameRef<'a> {
    /// Returns this name as a string slice.
    #[inline]
    #[must_use]
    pub(crate) fn as_str(self) -> &'a str {
        self.0
    }
}

impl<'a> TryFrom<&'a str> for FieldNameRef<'a> {
    type Error = FieldNameError;

    /// Validates `raw` and borrows it as a [`FieldNameRef`].
    ///
    /// # Errors
    ///
    /// See [`FieldName::validate`].
    fn try_from(raw: &'a str) -> Result<Self, Self::Error> {
        FieldName::validate(raw)?;
        Ok(Self(raw))
    }
}

impl Borrow<str> for FieldNameRef<'_> {
    fn borrow(&self) -> &str {
        self.0
    }
}

impl fmt::Debug for FieldNameRef<'_> {
    /// Matches `str`'s own `Debug` (quoted, escaped); see [`FieldName`]'s
    /// `Debug` impl for why this matters.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.0, f)
    }
}

impl fmt::Display for FieldNameRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.0, f)
    }
}

/// Stores a forgiving note field identifier.
///
/// Keeps two forms:
/// - `name` stores the original key text for display and serialization
/// - `canonical` stores the normalized text used for matching
///
/// Equality compares canonical forms, so `Status`, `status`, and `status!`
/// compare equal when their canonical forms match.
#[derive(Clone, Debug, Eq)]
pub(crate) struct FieldKey {
    /// Original key text as written by the user.
    name: String,
    /// Canonical form for case-insensitive matching.
    canonical: String,
}

impl FieldKey {
    /// Validates `raw` and constructs a [`FieldKey`].
    ///
    /// # Errors
    ///
    /// - [`FieldKeyError::Empty`] if `raw` is empty or whitespace-only
    /// - [`FieldKeyError::EmptyCanonical`] if canonicalization strips every
    ///   searchable character
    pub(crate) fn try_new(
        raw: impl Into<String>,
    ) -> Result<Self, FieldKeyError> {
        let raw = raw.into();
        if raw.trim().is_empty() {
            return Err(FieldKeyError::Empty);
        }
        if Self::is_canonical_empty(&raw) {
            return Err(FieldKeyError::EmptyCanonical {
                name: raw,
            });
        }
        let canonical = Self::canonicalize(&raw);
        Ok(Self {
            name: raw,
            canonical,
        })
    }

    /// Returns the original key text.
    #[inline]
    #[must_use]
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// Returns the canonical key form for matching.
    #[inline]
    #[must_use]
    pub(crate) fn canonical(&self) -> &str {
        &self.canonical
    }

    /// Returns `true` if `candidate` matches this key.
    ///
    /// Matching succeeds when either condition holds:
    /// - `candidate` exactly equals [`Self::name`]
    /// - `candidate` canonicalizes to [`Self::canonical`]
    #[inline]
    #[must_use]
    pub(crate) fn is_match(&self, candidate: &str) -> bool {
        self.name == candidate || self.is_canonical_match(candidate)
    }

    /// Returns `true` if `candidate` matches this key's canonical form.
    ///
    /// Checks `candidate` as already-canonical text first, then canonicalizes
    /// it only if the literal check fails.
    #[must_use]
    pub(crate) fn is_canonical_match(&self, candidate: &str) -> bool {
        self.canonical == candidate
            || self.canonical == Self::canonicalize(candidate)
    }

    /// Returns `true` if `candidate` exactly matches this key's raw name.
    ///
    /// Does not canonicalize. A case or punctuation difference fails even when
    /// [`Self::is_match`] would accept it.
    #[must_use]
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    pub(crate) fn is_name_match(&self, candidate: &FieldName) -> bool {
        self.name == candidate.as_str()
    }

    /// Canonicalizes a raw key for forgiving field matching.
    ///
    /// Character transformations, applied left to right:
    /// - ASCII whitespace is substituted with `-`
    /// - `_` and `-` are kept unchanged
    /// - ASCII letters are kept and lowercased
    /// - ASCII digits are kept unchanged
    /// - Non-ASCII characters are kept and lowercased with
    ///   [`char::to_lowercase`]
    /// - All other ASCII punctuation and symbols are stripped
    ///
    /// Consecutive whitespace produces consecutive `-` characters. Existing `-`
    /// characters are not collapsed with substituted whitespace.
    fn canonicalize(raw: &str) -> String {
        let mut result = String::with_capacity(raw.len());
        for ch in raw.chars() {
            if ch.is_ascii_whitespace() {
                result.push('-');
                continue;
            }
            if Self::is_kept(ch) {
                for c in ch.to_lowercase() {
                    result.push(c);
                }
            }
            // strip everything else
        }
        result
    }

    /// Returns `true` if `ch` survives [`Self::canonicalize`] as itself, rather
    /// than being substituted with `-` or stripped: `_`, `-`, ASCII
    /// alphanumeric, or non-ASCII.
    #[inline]
    fn is_kept(ch: char) -> bool {
        ch == '_' || ch == '-' || ch.is_ascii_alphanumeric() || !ch.is_ascii()
    }

    /// Returns `true` if [`Self::canonicalize`] would strip `raw` to an empty
    /// string, without allocating the canonical form.
    fn is_canonical_empty(raw: &str) -> bool {
        raw.chars().all(|ch| !ch.is_ascii_whitespace() && !Self::is_kept(ch))
    }
}

impl From<FieldName> for FieldKey {
    /// Converts an owned [`FieldName`] into a [`FieldKey`], consuming its text
    /// and allocating only the canonical form.
    fn from(name: FieldName) -> Self {
        let canonical = Self::canonicalize(&name.0);
        Self {
            name: name.0,
            canonical,
        }
    }
}

impl PartialEq for FieldKey {
    /// Compares canonical forms: two keys differing only by case or whitespace
    /// style are equal, matching [`FieldKey::is_canonical_match`].
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.canonical == other.canonical
    }
}

impl TryFrom<String> for FieldKey {
    type Error = FieldKeyError;

    /// Validates `raw` and constructs a [`FieldKey`].
    ///
    /// # Errors
    ///
    /// See [`FieldKey::try_new`].
    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::try_new(raw)
    }
}

impl TryFrom<&str> for FieldKey {
    type Error = FieldKeyError;

    /// Validates `raw` and constructs a [`FieldKey`].
    ///
    /// # Errors
    ///
    /// See [`FieldKey::try_new`].
    fn try_from(raw: &str) -> Result<Self, Self::Error> {
        Self::try_new(raw)
    }
}

impl FromStr for FieldKey {
    type Err = FieldKeyError;

    /// Parses `raw` as a [`FieldKey`].
    ///
    /// # Errors
    ///
    /// See [`FieldKey::try_new`].
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::try_new(raw)
    }
}

impl TryFrom<serde_yaml::Value> for FieldKey {
    type Error = FieldKeyError;

    /// Coerces a YAML scalar `value` into a [`FieldKey`].
    ///
    /// # Errors
    ///
    /// Returns [`FieldKeyError::UnsupportedYamlKey`] for `Null`, `Sequence`,
    /// `Mapping`, and `Tagged` values; otherwise see [`FieldKey::try_new`].
    fn try_from(value: serde_yaml::Value) -> Result<Self, Self::Error> {
        let raw = yaml_scalar_to_string(value)
            .ok_or(FieldKeyError::UnsupportedYamlKey)?;
        Self::try_new(raw)
    }
}

impl Serialize for FieldKey {
    /// Serializes as the original key text ([`FieldKey::name`]), not the
    /// canonical form.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.name.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for FieldKey {
    /// Deserializes from a string and validates it as a [`FieldKey`].
    ///
    /// # Errors
    ///
    /// See [`FieldKey::try_new`].
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::try_new(raw).map_err(D::Error::custom)
    }
}

/// Stores an owned value parsed from a TOML, JSON, or YAML document.
///
/// The crate's canonical value representation once a value must outlive its
/// source text: a values-file cache entry, an inline value object's
/// hand-authored passthrough keys. [`FieldValueRef`] is the zero-copy borrowed
/// counterpart; convert one to the other with `.into()`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FieldValue {
    /// Empty or missing value.
    Null,
    /// Boolean value (`true` or `false`).
    Bool(bool),
    /// Whole-number value.
    Int(i64),
    /// Floating-point value.
    Float(f64),
    /// Plain text value.
    String(String),
    /// Ordered list value.
    List(Vec<FieldValue>),
    /// Keyed object value, stored in a deterministically ordered map.
    Object(BTreeMap<String, FieldValue>),
}

impl FieldValue {
    /// Returns the inner value for [`FieldValue::Float`] or
    /// [`FieldValue::Int`], converting an integer to `f64`, or `None` for any
    /// other kind.
    ///
    /// Mirrors [`FieldValueRef::as_f64`]: schema field-attribute validation
    /// (`schema::fields`) accepts a TOML integer or float interchangeably for
    /// a `number`-type field's `min`/`max`/`step`.
    #[inline]
    #[must_use]
    pub(crate) fn as_f64(&self) -> Option<f64> {
        match *self {
            Self::Float(f) => Some(f),
            #[expect(
                clippy::as_conversions,
                clippy::cast_precision_loss,
                reason = "integer field value converted to f64"
            )]
            Self::Int(i) => Some(i as f64),
            _ => None,
        }
    }
}

impl From<FieldValueRef<'_>> for FieldValue {
    fn from(value: FieldValueRef<'_>) -> Self {
        match value {
            FieldValueRef::Null => Self::Null,
            FieldValueRef::Bool(b) => Self::Bool(b),
            FieldValueRef::Int(i) => Self::Int(i),
            FieldValueRef::Float(f) => Self::Float(f),
            FieldValueRef::String(s) => Self::String(s.into_owned()),
            FieldValueRef::List(arr) => {
                Self::List(arr.into_iter().map(Into::into).collect())
            }
            FieldValueRef::Object(map) => Self::Object(
                map.into_iter()
                    .map(|(k, v)| (k.into_owned(), v.into()))
                    .collect(),
            ),
        }
    }
}

impl Serialize for FieldValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Null => serializer.serialize_unit(),
            Self::Bool(b) => serializer.serialize_bool(*b),
            Self::Int(i) => serializer.serialize_i64(*i),
            Self::Float(f) => serializer.serialize_f64(*f),
            Self::String(s) => serializer.serialize_str(s),
            Self::List(arr) => {
                let mut seq = serializer.serialize_seq(Some(arr.len()))?;
                for elem in arr {
                    seq.serialize_element(elem)?;
                }
                seq.end()
            }
            Self::Object(map) => {
                let mut ser_map = serializer.serialize_map(Some(map.len()))?;
                for (k, v) in map {
                    ser_map.serialize_entry(k, v)?;
                }
                ser_map.end()
            }
        }
    }
}

/// Deserializes by delegating to [`FieldValueRef`]'s zero-copy [`Visitor`] and
/// immediately owning the result: the same parsing logic, minus the borrow.
impl<'de> Deserialize<'de> for FieldValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        FieldValueRef::deserialize(deserializer).map(Into::into)
    }
}

/// Borrows a value parsed from a TOML, JSON, or YAML document.
///
/// Zero-copy on the common path: TOML, JSON, and YAML's deserializers all call
/// [`Visitor::visit_borrowed_str`] for an unescaped string, so
/// [`Cow::Borrowed`] holds a slice of the source text directly, no allocation.
/// A string needing processing (an escape sequence, a multi-line/folded scalar)
/// falls back to [`Cow::Owned`], same as any [`Deserialize`] impl over
/// [`Cow<str>`].
///
/// Scoped to the source text's lifetime `'a`; convert to the owned
/// [`FieldValue`] with `.into()` before a value must outlive that text.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FieldValueRef<'a> {
    /// Empty or missing value.
    Null,
    /// Boolean value (`true` or `false`).
    Bool(bool),
    /// Whole-number value.
    Int(i64),
    /// Floating-point value.
    Float(f64),
    /// Plain text value, borrowed from the source document where possible.
    String(Cow<'a, str>),
    /// Ordered list value.
    List(Vec<FieldValueRef<'a>>),
    /// Keyed object value, stored in a deterministically ordered map.
    Object(BTreeMap<Cow<'a, str>, FieldValueRef<'a>>),
}

impl FieldValueRef<'_> {
    /// Returns the inner value for [`FieldValueRef::Float`] or
    /// [`FieldValueRef::Int`], converting an integer to `f64`, or `None` for
    /// any other kind.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "consumer lands with the values-source redesign"
        )
    )]
    pub(crate) fn as_f64(&self) -> Option<f64> {
        match *self {
            Self::Float(f) => Some(f),
            #[expect(
                clippy::as_conversions,
                clippy::cast_precision_loss,
                reason = "integer field value converted to f64"
            )]
            Self::Int(i) => Some(i as f64),
            _ => None,
        }
    }
}

impl Serialize for FieldValueRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Null => serializer.serialize_unit(),
            Self::Bool(b) => serializer.serialize_bool(*b),
            Self::Int(i) => serializer.serialize_i64(*i),
            Self::Float(f) => serializer.serialize_f64(*f),
            Self::String(s) => serializer.serialize_str(s),
            Self::List(arr) => {
                let mut seq = serializer.serialize_seq(Some(arr.len()))?;
                for elem in arr {
                    seq.serialize_element(elem)?;
                }
                seq.end()
            }
            Self::Object(map) => {
                let mut ser_map = serializer.serialize_map(Some(map.len()))?;
                for (k, v) in map {
                    ser_map.serialize_entry(k, v)?;
                }
                ser_map.end()
            }
        }
    }
}

/// Deserializes from any self-describing format (TOML, JSON, or YAML) with
/// zero-copy borrowing: [`Deserializer::deserialize_any`] drives whichever
/// `visit_*` method matches the source data, borrowing text from `'de` wherever
/// the format's deserializer supports it.
impl<'de: 'a, 'a> Deserialize<'de> for FieldValueRef<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct FieldValueRefVisitor<'a>(PhantomData<&'a ()>);

        impl<'de: 'a, 'a> Visitor<'de> for FieldValueRefVisitor<'a> {
            type Value = FieldValueRef<'a>;

            fn expecting(
                &self,
                formatter: &mut fmt::Formatter<'_>,
            ) -> fmt::Result {
                formatter.write_str("a TOML, JSON, or YAML field value")
            }

            fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E> {
                Ok(FieldValueRef::Bool(v))
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
                Ok(FieldValueRef::Int(v))
            }

            /// Saturates at [`i64::MAX`] for a magnitude beyond `i64`'s range.
            /// Values files in this domain (Schema `select`/`multi` options)
            /// never need integers that large, so keeping a single
            /// [`FieldValueRef::Int`] variant is worth that ceiling.
            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
                Ok(FieldValueRef::Int(i64::try_from(v).unwrap_or(i64::MAX)))
            }

            fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E> {
                Ok(FieldValueRef::Float(v))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(FieldValueRef::String(Cow::Owned(v.to_owned())))
            }

            fn visit_borrowed_str<E>(self, v: &'a str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(FieldValueRef::String(Cow::Borrowed(v)))
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(FieldValueRef::String(Cow::Owned(v)))
            }

            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(FieldValueRef::Null)
            }

            fn visit_some<D>(
                self,
                deserializer: D,
            ) -> Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                Deserialize::deserialize(deserializer)
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(FieldValueRef::Null)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut vec = Vec::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(elem) = seq.next_element()? {
                    vec.push(elem);
                }
                Ok(FieldValueRef::List(vec))
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut btree = BTreeMap::new();
                while let Some((key, value)) =
                    map.next_entry::<Cow<'a, str>, FieldValueRef<'a>>()?
                {
                    btree.insert(key, value);
                }
                Ok(FieldValueRef::Object(btree))
            }
        }

        deserializer.deserialize_any(FieldValueRefVisitor(PhantomData))
    }
}

/// Reports why a [`FieldName`] could not be parsed.
#[derive(Debug, Error)]
pub(crate) enum FieldNameError {
    /// Rejects an empty or whitespace-only name.
    #[error("field name is empty")]
    Empty,
    /// Rejects a name containing `/`.
    ///
    /// Slash is reserved for `$ref` path segments.
    #[error("field name {name:?} cannot contain `/`")]
    ContainsSlash {
        name: String,
    },
    /// Rejects a name with no searchable canonical characters.
    #[error("field name {name:?} has no searchable characters")]
    EmptyCanonical {
        name: String,
    },
    /// Rejects a YAML value that cannot be represented as scalar field text.
    #[error("YAML value cannot be used as a field name")]
    UnsupportedYamlKey,
}

/// Reports why a [`FieldKey`] could not be parsed.
#[derive(Debug, Error)]
pub(crate) enum FieldKeyError {
    /// Rejects an empty or whitespace-only key.
    #[error("field key is empty")]
    Empty,
    /// Rejects a key with no searchable canonical characters.
    #[error("field key {name:?} has no searchable characters")]
    EmptyCanonical {
        name: String,
    },
    /// Rejects a YAML value that cannot be represented as scalar field text.
    #[error("YAML value cannot be used as a field key")]
    UnsupportedYamlKey,
}

/// Coerces a YAML scalar into raw text usable as a field key/name.
///
/// Returns `None` for YAML values that cannot stand as a key: `Null`,
/// `Sequence`, `Mapping`, and `Tagged`.
fn yaml_scalar_to_string(value: serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::String(s) => Some(s),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        serde_yaml::Value::Null
        | serde_yaml::Value::Sequence(_)
        | serde_yaml::Value::Mapping(_)
        | serde_yaml::Value::Tagged(_) => None,
    }
}

/// Finds the candidate nearest to `input` by edit distance.
///
/// Algorithm:
/// - Compute [`edit_distance`] from `input` to each candidate name
/// - Select the candidate with the smallest distance
/// - Accept it only when distance is at most half of `input`'s character count,
///   rounded up, with a minimum threshold of 1
///
/// Ties keep iterator order through [`Iterator::min_by_key`]. Complexity is
/// `O(n * a * b)` time, where `n` is the number of candidates and `a` and `b`
/// are input and candidate character counts. Extra space is `O(b)` per
/// candidate.
pub(crate) fn closest_match<'a, T>(
    candidates: impl Iterator<Item = (T, &'a str)>,
    input: &str,
) -> Option<T> {
    let threshold = input.chars().count().div_ceil(2).max(1);
    candidates
        .map(|(item, name)| (item, edit_distance(input, name)))
        .min_by_key(|&(_, distance)| distance)
        .filter(|&(_, distance)| distance <= threshold)
        .map(|(item, _)| item)
}

/// Calculates the Levenshtein edit distance between two strings.
///
/// Uses the two-row Wagner-Fischer dynamic-programming algorithm over Unicode
/// scalar values. The result is the minimum number of single-character
/// insertions, deletions, or substitutions needed to transform `a` into `b`.
///
/// Complexity is `O(a * b)` time and `O(b)` extra space, where `a` and `b` are
/// the character counts of the inputs.
pub(crate) fn edit_distance(a: &str, b: &str) -> usize {
    let b_chars: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b_chars.len()).collect();
    for (i, ch_a) in a.chars().enumerate() {
        let mut next_row = Vec::with_capacity(row.len());
        next_row.push(i.saturating_add(1));
        for (j, &ch_b) in b_chars.iter().enumerate() {
            let substitution_cost = usize::from(ch_a != ch_b);
            let deletion = row
                .get(j.saturating_add(1))
                .copied()
                .unwrap_or(usize::MAX)
                .saturating_add(1);
            let insertion = next_row
                .get(j)
                .copied()
                .unwrap_or(usize::MAX)
                .saturating_add(1);
            let substitution = row
                .get(j)
                .copied()
                .unwrap_or(usize::MAX)
                .saturating_add(substitution_cost);
            next_row.push(deletion.min(insertion).min(substitution));
        }
        row = next_row;
    }
    row.last().copied().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    mod field_name {
        mod constructor {
            use pretty_assertions::assert_eq;

            use super::super::super::*;

            #[test]
            fn accepts_a_well_formed_name() {
                let name = FieldName::try_from("Status").expect("valid name");
                assert_eq!(name.as_str(), "Status");
            }

            #[test]
            fn constructs_from_an_owned_string() {
                let name = FieldName::try_from("Status".to_owned())
                    .expect("valid name");
                assert_eq!(name.as_str(), "Status");
            }

            #[test]
            fn parses_via_from_str() {
                let name: FieldName = "Status".parse().expect("valid name");
                assert_eq!(name.as_str(), "Status");
            }

            #[test]
            fn deserializes_a_valid_name() {
                let name: FieldName =
                    serde_json::from_str(r#""Status""#).expect("valid name");
                assert_eq!(name.as_str(), "Status");
            }

            #[test]
            fn deserialize_rejects_an_invalid_name() {
                let result: Result<FieldName, _> =
                    serde_json::from_str(r#""""#);
                assert!(result.is_err(), "empty name must fail validation");
            }

            #[test]
            fn rejects_an_empty_name() {
                assert!(matches!(
                    FieldName::try_from(""),
                    Err(FieldNameError::Empty)
                ));
            }

            #[test]
            fn rejects_a_whitespace_only_name() {
                assert!(matches!(
                    FieldName::try_from("   "),
                    Err(FieldNameError::Empty)
                ));
            }

            #[test]
            fn rejects_a_name_containing_a_slash() {
                assert!(matches!(
                    FieldName::try_from("global/status"),
                    Err(FieldNameError::ContainsSlash { .. })
                ));
            }

            #[test]
            fn rejects_a_name_with_an_empty_canonical_form() {
                assert!(matches!(
                    FieldName::try_from("!!!"),
                    Err(FieldNameError::EmptyCanonical { .. })
                ));
            }

            #[test]
            fn field_name_ref_constructs_a_valid_name() {
                let name =
                    FieldNameRef::try_from("Status").expect("valid name");
                assert_eq!(name.as_str(), "Status");
            }

            #[test]
            fn field_name_ref_rejects_an_invalid_name() {
                assert!(matches!(
                    FieldNameRef::try_from(""),
                    Err(FieldNameError::Empty)
                ));
            }
        }

        mod yaml {
            use pretty_assertions::assert_eq;
            use rstest::rstest;

            use super::super::super::*;

            #[rstest]
            #[case::string_scalar("hello", "hello")]
            #[case::number_scalar("3", "3")]
            #[case::bool_scalar("true", "true")]
            fn accepts_a_scalar(
                #[case] yaml_source: &str,
                #[case] expected: &str,
            ) {
                let value: serde_yaml::Value =
                    serde_yaml::from_str(yaml_source).expect("valid yaml");
                let name = FieldName::try_from(value).expect("valid name");
                assert_eq!(name.as_str(), expected);
            }

            #[test]
            fn rejects_a_null_value() {
                let value: serde_yaml::Value =
                    serde_yaml::from_str("null").expect("valid yaml");
                assert!(matches!(
                    FieldName::try_from(value),
                    Err(FieldNameError::UnsupportedYamlKey)
                ));
            }

            #[test]
            fn rejects_an_empty_string_after_coercion() {
                let value: serde_yaml::Value =
                    serde_yaml::from_str(r#""""#).expect("valid yaml");
                assert!(matches!(
                    FieldName::try_from(value),
                    Err(FieldNameError::Empty)
                ));
            }
        }

        use pretty_assertions::{assert_eq, assert_ne};

        use super::super::*;

        #[test]
        fn rejects_case_insensitive_equality() {
            let lower = FieldName::try_from("status").expect("valid");
            let upper = FieldName::try_from("Status").expect("valid");
            assert_ne!(lower, upper);
        }

        #[test]
        fn to_key_computes_the_canonical_form() {
            let name = FieldName::try_from("Time Played").expect("valid");
            assert_eq!(name.to_key().canonical(), "time-played");
        }

        #[test]
        fn round_trips_through_field_name_ref() {
            let owned = FieldName::try_from("status").expect("valid");
            let borrowed = owned.as_ref();
            assert_eq!(FieldName::from(borrowed), owned);
        }

        #[test]
        fn field_name_debug_matches_strs_quoted_escaped_format() {
            let name = FieldName::try_from(r#"say "hi""#).expect("valid name");
            assert_eq!(format!("{name:?}"), format!("{:?}", r#"say "hi""#));
        }

        #[test]
        fn field_name_ref_debug_matches_strs_quoted_escaped_format() {
            let name =
                FieldNameRef::try_from(r#"say "hi""#).expect("valid name");
            assert_eq!(format!("{name:?}"), format!("{:?}", r#"say "hi""#));
        }
    }

    mod field_key {
        mod constructor {
            use pretty_assertions::assert_eq;

            use super::super::super::*;

            #[test]
            fn returns_the_original_name_text() {
                let key = FieldKey::try_new("Status").expect("valid key");
                assert_eq!(key.name(), "Status");
            }

            #[test]
            fn constructs_via_try_from_string() {
                let key =
                    FieldKey::try_from("Status".to_owned()).expect("valid key");
                assert_eq!(key.name(), "Status");
            }

            #[test]
            fn constructs_via_try_from_str() {
                let key = FieldKey::try_from("Status").expect("valid key");
                assert_eq!(key.name(), "Status");
            }

            #[test]
            fn parses_via_from_str() {
                let key: FieldKey = "Status".parse().expect("valid key");
                assert_eq!(key.name(), "Status");
            }

            #[test]
            fn deserializes_a_valid_key() {
                let key: FieldKey =
                    serde_json::from_str(r#""Status""#).expect("valid key");
                assert_eq!(key.name(), "Status");
            }

            #[test]
            fn deserialize_rejects_an_invalid_key() {
                let result: Result<FieldKey, _> = serde_json::from_str(r#""""#);
                assert!(result.is_err(), "empty key must fail validation");
            }

            #[test]
            fn rejects_an_empty_key() {
                assert!(matches!(
                    FieldKey::try_new(""),
                    Err(FieldKeyError::Empty)
                ));
            }

            #[test]
            fn rejects_a_whitespace_only_key() {
                assert!(matches!(
                    FieldKey::try_new("   "),
                    Err(FieldKeyError::Empty)
                ));
            }

            #[test]
            fn rejects_a_key_with_an_empty_canonical_form() {
                assert!(matches!(
                    FieldKey::try_new("!!!"),
                    Err(FieldKeyError::EmptyCanonical { .. })
                ));
            }
        }

        mod yaml {
            use pretty_assertions::assert_eq;

            use super::super::super::*;

            #[test]
            fn accepts_a_string_scalar() {
                let value: serde_yaml::Value =
                    serde_yaml::from_str("hello").expect("valid yaml");
                let key = FieldKey::try_from(value).expect("valid key");
                assert_eq!(key.name(), "hello");
            }

            #[test]
            fn accepts_a_number_scalar() {
                let value: serde_yaml::Value =
                    serde_yaml::from_str("3").expect("valid yaml");
                let key = FieldKey::try_from(value).expect("valid key");
                assert_eq!(key.name(), "3");
            }

            #[test]
            fn rejects_a_null_value() {
                let value: serde_yaml::Value =
                    serde_yaml::from_str("null").expect("valid yaml");
                assert!(matches!(
                    FieldKey::try_from(value),
                    Err(FieldKeyError::UnsupportedYamlKey)
                ));
            }
        }

        mod canonicalization {
            use pretty_assertions::assert_eq;
            use rstest::rstest;

            use super::super::super::*;

            #[rstest]
            #[case::mixed_case_and_whitespace("Time Played", "time-played")]
            #[case::ascii_uppercase("Status", "status")]
            #[case::ascii_digits("field2", "field2")]
            #[case::single_whitespace_run("due date", "due-date")]
            #[case::consecutive_whitespace_stays_consecutive("a  b", "a--b")]
            #[case::strips_punctuation("field-name!", "field-name")]
            #[case::keeps_underscore_and_hyphen(
                "my_field-name",
                "my_field-name"
            )]
            #[case::keeps_non_ascii_emoji("🗓️due", "🗓️due")]
            #[case::lowercases_non_ascii_letters("CAFÉ", "café")]
            fn canonicalizes(#[case] raw: &str, #[case] expected: &str) {
                let key = FieldKey::try_new(raw).expect("valid key");
                assert_eq!(key.canonical(), expected);
            }
        }

        mod matching {
            use super::super::super::*;

            #[test]
            fn is_match_matches_the_exact_raw_name() {
                let key = FieldKey::try_new("Status").expect("valid key");
                assert!(key.is_match("Status"));
            }

            #[test]
            fn is_match_falls_back_to_canonical_form() {
                let key = FieldKey::try_new("Status").expect("valid key");
                assert!(key.is_match("status"));
                assert!(!key.is_match("other"));
            }

            #[test]
            fn is_canonical_match_uses_canonical_form() {
                let key = FieldKey::try_new("Status").expect("valid key");
                assert!(key.is_canonical_match("status"));
                assert!(key.is_canonical_match("Status"));
                assert!(!key.is_canonical_match("other"));
            }

            #[test]
            fn is_name_match_requires_exact_raw_text() {
                let key = FieldKey::try_new("Status").expect("valid key");
                let exact = FieldName::try_from("Status").expect("valid name");
                let different_case =
                    FieldName::try_from("status").expect("valid name");
                assert!(key.is_name_match(&exact));
                assert!(!key.is_name_match(&different_case));
            }
        }

        use pretty_assertions::{assert_eq, assert_ne};

        use super::super::*;

        #[test]
        fn partial_eq_uses_canonical_form() {
            let a = FieldKey::try_new("Status").expect("valid key");
            let b = FieldKey::try_new("status").expect("valid key");
            assert_eq!(a, b);
        }

        #[test]
        fn partial_eq_treats_different_canonical_forms_as_unequal() {
            let a = FieldKey::try_new("Status").expect("valid key");
            let b = FieldKey::try_new("Priority").expect("valid key");
            assert_ne!(a, b);
        }

        #[test]
        fn constructs_from_a_field_name() {
            let name = FieldName::try_from("Status").expect("valid name");
            let key = FieldKey::from(name);
            assert_eq!(key.name(), "Status");
            assert_eq!(key.canonical(), "status");
        }

        #[test]
        fn serializes_the_original_text_not_the_canonical_form() {
            let key = FieldKey::try_new("Status").expect("valid key");
            let json = serde_json::to_string(&key).expect("serializes");
            assert_eq!(json, "\"Status\"");
        }
    }

    mod field_value {
        use pretty_assertions::assert_eq;

        use super::super::*;

        /// Test-only map lookup: production code either destructures a known
        /// variant directly or, for `order`, uses [`FieldValueRef::as_f64`].
        /// No other accessor earns its keep outside this test module.
        fn get<'v, 'a>(
            value: &'v FieldValueRef<'a>,
            key: &str,
        ) -> Option<&'v FieldValueRef<'a>> {
            match value {
                FieldValueRef::Object(m) => m.get(key),
                _ => None,
            }
        }

        /// Owned counterpart of [`get`].
        fn get_owned<'v>(
            value: &'v FieldValue,
            key: &str,
        ) -> Option<&'v FieldValue> {
            match value {
                FieldValue::Object(m) => m.get(key),
                _ => None,
            }
        }

        fn as_str<'v>(value: &'v FieldValueRef<'_>) -> Option<&'v str> {
            match value {
                FieldValueRef::String(s) => Some(s.as_ref()),
                _ => None,
            }
        }

        mod accessors {
            use pretty_assertions::assert_eq;
            use rstest::rstest;

            use super::super::super::*;

            #[rstest]
            #[case::int(FieldValueRef::Int(-3), Some(-3.0))]
            #[case::float(FieldValueRef::Float(1.5), Some(1.5))]
            #[case::non_numeric_variant(FieldValueRef::Bool(true), None)]
            fn as_f64_converts_numbers_and_rejects_other_variants(
                #[case] value: FieldValueRef<'static>,
                #[case] expected: Option<f64>,
            ) {
                assert_eq!(value.as_f64(), expected);
            }
        }

        mod deserialization {
            use std::borrow::Cow;

            use pretty_assertions::assert_eq;

            use super::{super::super::*, as_str, get, get_owned};

            #[test]
            fn borrows_an_unescaped_string_from_the_source_text() {
                let source = r#"{"value": "afghanistan"}"#;
                let value: FieldValueRef<'_> =
                    serde_json::from_str(source).expect("valid json");
                let entry = get(&value, "value").expect("value key present");
                assert!(
                    matches!(entry, FieldValueRef::String(Cow::Borrowed(_))),
                    "expected a zero-copy borrow, got {entry:?}"
                );
                assert_eq!(as_str(entry), Some("afghanistan"));
            }

            #[test]
            fn owns_a_string_needing_escape_processing() {
                let source = r#"{"value": "line one\nline two"}"#;
                let value: FieldValueRef<'_> =
                    serde_json::from_str(source).expect("valid json");
                let entry = get(&value, "value").expect("value key present");
                assert!(
                    matches!(entry, FieldValueRef::String(Cow::Owned(_))),
                    "expected an owned string after escape processing, got \
                     {entry:?}"
                );
                assert_eq!(as_str(entry), Some("line one\nline two"));
            }

            #[test]
            fn dispatches_every_value_kind_to_the_matching_variant() {
                let source = r#"{
                    "n": null,
                    "b": true,
                    "i": -3,
                    "u": 3,
                    "f": 1.5,
                    "s": "hi",
                    "a": [1, 2],
                    "o": {"k": "v"}
                }"#;
                let value: FieldValueRef<'_> =
                    serde_json::from_str(source).expect("valid json");
                assert_eq!(get(&value, "n"), Some(&FieldValueRef::Null));
                assert_eq!(get(&value, "b"), Some(&FieldValueRef::Bool(true)));
                assert_eq!(get(&value, "i"), Some(&FieldValueRef::Int(-3)));
                assert_eq!(get(&value, "u"), Some(&FieldValueRef::Int(3)));
                assert_eq!(get(&value, "f"), Some(&FieldValueRef::Float(1.5)));
                assert_eq!(get(&value, "s").and_then(as_str), Some("hi"));
                let array = match get(&value, "a") {
                    Some(FieldValueRef::List(a)) => Some(a.as_slice()),
                    _ => None,
                }
                .expect("array key present");
                assert_eq!(
                    array,
                    [FieldValueRef::Int(1), FieldValueRef::Int(2)].as_slice()
                );
                let nested = get(&value, "o").and_then(|o| get(o, "k"));
                assert_eq!(nested.and_then(as_str), Some("v"));
            }

            #[test]
            fn parses_an_empty_list_and_object() {
                let source = r#"{"a": [], "o": {}}"#;
                let value: FieldValueRef<'_> =
                    serde_json::from_str(source).expect("valid json");
                assert_eq!(
                    get(&value, "a"),
                    Some(&FieldValueRef::List(vec![]))
                );
                assert_eq!(
                    get(&value, "o"),
                    Some(&FieldValueRef::Object(BTreeMap::new()))
                );
            }

            #[test]
            fn saturates_a_u64_beyond_i64_range_at_i64_max() {
                let source = r#"{"huge": 18446744073709551615}"#;
                let value: FieldValueRef<'_> =
                    serde_json::from_str(source).expect("valid json");
                assert_eq!(
                    get(&value, "huge"),
                    Some(&FieldValueRef::Int(i64::MAX)),
                    "u64::MAX has no i64 representation; saturates instead of \
                     wrapping or erroring"
                );
            }

            #[test]
            fn saturates_at_the_first_u64_value_past_i64_max() {
                let source = r#"{"boundary": 9223372036854775808}"#;
                let value: FieldValueRef<'_> =
                    serde_json::from_str(source).expect("valid json");
                assert_eq!(
                    get(&value, "boundary"),
                    Some(&FieldValueRef::Int(i64::MAX)),
                    "i64::MAX as u64 + 1 is the first value that must saturate"
                );
            }

            #[test]
            fn parses_toml_the_same_shape_as_json() {
                let source = "n = false\ni = -3\nu = 3\nf = 1.5\ns = \"hi\"\n";
                let value: FieldValueRef<'_> =
                    toml::from_str(source).expect("valid toml");
                assert_eq!(get(&value, "i"), Some(&FieldValueRef::Int(-3)));
                assert_eq!(get(&value, "f"), Some(&FieldValueRef::Float(1.5)));
                assert_eq!(get(&value, "s").and_then(as_str), Some("hi"));
            }

            #[test]
            fn returns_an_error_for_malformed_source() {
                let result: Result<FieldValueRef<'_>, _> =
                    serde_json::from_str("{not valid json}");
                assert!(result.is_err());
            }

            #[test]
            fn field_value_owns_the_same_shape() {
                let source = r#"{"a": [1, "hi", null], "b": {"k": true}}"#;
                let value: FieldValue =
                    serde_json::from_str(source).expect("valid json");
                assert_eq!(
                    get_owned(&value, "a"),
                    Some(&FieldValue::List(vec![
                        FieldValue::Int(1),
                        FieldValue::String("hi".to_owned()),
                        FieldValue::Null,
                    ]))
                );
                assert_eq!(
                    get_owned(&value, "b"),
                    Some(&FieldValue::Object(BTreeMap::from([(
                        "k".to_owned(),
                        FieldValue::Bool(true)
                    )])))
                );
            }
        }

        #[test]
        fn into_owned_conversion_matches_direct_deserialization() {
            let source = r#"{"a": [1, "hi", null], "b": {"k": true}}"#;
            let borrowed: FieldValueRef<'_> =
                serde_json::from_str(source).expect("valid json");
            let owned: FieldValue =
                serde_json::from_str(source).expect("valid json");
            assert_eq!(FieldValue::from(borrowed), owned);
        }

        mod serialization {
            use std::borrow::Cow;

            use pretty_assertions::assert_eq;
            use rstest::rstest;

            use super::super::super::*;

            #[rstest]
            #[case::null(FieldValueRef::Null, "null")]
            #[case::bool(FieldValueRef::Bool(true), "true")]
            #[case::int(FieldValueRef::Int(-3), "-3")]
            #[case::float(FieldValueRef::Float(1.5), "1.5")]
            #[case::string(
                FieldValueRef::String(Cow::Borrowed("hi")),
                "\"hi\""
            )]
            #[case::list(
                FieldValueRef::List(vec![FieldValueRef::Bool(true)]),
                "[true]"
            )]
            #[case::object(
                FieldValueRef::Object(BTreeMap::from([(
                    Cow::Borrowed("k"),
                    FieldValueRef::Int(1),
                )])),
                "{\"k\":1}"
            )]
            fn produces_the_expected_json_for_every_variant(
                #[case] value: FieldValueRef<'static>,
                #[case] expected_json: &str,
            ) {
                assert_eq!(
                    serde_json::to_string(&value).expect("serializes"),
                    expected_json
                );
                let owned = FieldValue::from(value);
                assert_eq!(
                    serde_json::to_string(&owned).expect("serializes"),
                    expected_json
                );
            }

            #[test]
            fn round_trips_through_json() {
                let value = FieldValue::Object(BTreeMap::from([
                    ("value".to_owned(), FieldValue::String("jan".to_owned())),
                    ("order".to_owned(), FieldValue::Int(-1)),
                    (
                        "tags".to_owned(),
                        FieldValue::List(vec![FieldValue::Bool(true)]),
                    ),
                ]));
                let json = serde_json::to_string(&value).expect("serializes");
                let round_tripped: FieldValue =
                    serde_json::from_str(&json).expect("deserializes");
                assert_eq!(round_tripped, value);
            }
        }
    }

    mod suggest {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::super::*;

        #[rstest]
        #[case::identical("name", "name", 0)]
        #[case::classic_kitten_sitting("kitten", "sitting", 3)]
        #[case::empty_a("", "abc", 3)]
        #[case::empty_b("abc", "", 3)]
        #[case::single_insertion("nam", "name", 1)]
        #[case::single_deletion("name", "nam", 1)]
        #[case::single_substitution("cat", "hat", 1)]
        #[case::multi_byte_unicode_substitution("café", "cafe", 1)]
        fn edit_distance_computes_the_minimum_operation_count(
            #[case] a: &str,
            #[case] b: &str,
            #[case] expected: usize,
        ) {
            assert_eq!(edit_distance(a, b), expected);
        }

        #[test]
        fn closest_match_matches_within_the_half_length_threshold() {
            let candidates = ["path", "name", "folder"];
            assert_eq!(
                closest_match(candidates.into_iter().map(|c| (c, c)), "nam"),
                Some("name")
            );
        }

        #[test]
        fn closest_match_accepts_a_match_exactly_at_the_threshold() {
            // "ab" has threshold ceil(2/2).max(1) = 1; "abc" is distance 1
            // away (one insertion): right at the threshold, still accepted.
            let candidates = ["abc"];
            assert_eq!(
                closest_match(candidates.into_iter().map(|c| (c, c)), "ab"),
                Some("abc")
            );
        }

        #[test]
        fn closest_match_rejects_a_match_past_the_threshold() {
            // "na" has threshold ceil(2/2).max(1) = 1, but its distance to
            // "name" is 2 (insert "m", "e"): too far to suggest.
            let candidates = ["name"];
            assert_eq!(
                closest_match(candidates.into_iter().map(|c| (c, c)), "na"),
                None
            );
        }

        #[test]
        fn closest_match_uses_a_minimum_threshold_of_one_for_empty_input() {
            // An empty input has threshold ceil(0/2).max(1) = 1, not 0: a
            // single-character candidate is still an acceptable suggestion.
            let candidates = ["a"];
            assert_eq!(
                closest_match(candidates.into_iter().map(|c| (c, c)), ""),
                Some("a")
            );
        }

        #[test]
        fn closest_match_returns_none_for_an_empty_candidate_list() {
            assert_eq!(
                closest_match(std::iter::empty::<(&str, &str)>(), "name"),
                None
            );
        }

        #[test]
        fn closest_match_breaks_ties_by_iteration_order() {
            // Both "cat" and "bat" are distance 1 from "mat"; the first
            // candidate in iteration order wins.
            let candidates = ["cat", "bat"];
            assert_eq!(
                closest_match(candidates.into_iter().map(|c| (c, c)), "mat"),
                Some("cat")
            );
        }
    }
}
