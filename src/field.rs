//! Validated field-name/key primitives shared across Note metadata and Schema.
//!
//! [`FieldName`] is the exact identifier Schemas use: two field names are equal
//! only if their raw text matches byte-for-byte (`"status"` and `"Status"` are
//! distinct identities). [`FieldKey`] is the forgiving identifier Note
//! frontmatter and inline fields use: it additionally tracks a canonical form
//! so `"Status"`, `"status"`, and `"Due-Date"`/`"due date"`-style variants all
//! *match* (via [`FieldKey::is_match`] and friends) without being
//! interchangeable as raw text.
//!
//! Both types are constructed only through fallible conversions
//! (`TryFrom`/`FromStr`): an empty name, a name whose canonical form strips to
//! nothing, or (for [`FieldName`]) a name containing `/` cannot be constructed
//! at all ("parse, don't validate").

use std::{borrow::Borrow, fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;
use yaml_serde as serde_yaml;

/// An exact field identifier: two [`FieldName`]s are equal only when their raw
/// text matches byte-for-byte. Used for Schema field identities, where `status`
/// and `Status` must stay distinct.
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

    /// Converts this name into a forgiving [`FieldKey`], allocating its
    /// canonical form. Borrowed counterpart to `impl From<FieldName> for
    /// FieldKey`, for call sites that only have a `&FieldName`.
    #[must_use]
    pub(crate) fn to_key(&self) -> FieldKey {
        let canonical = FieldKey::canonicalize(&self.0);
        FieldKey {
            name: self.0.clone(),
            canonical,
        }
    }

    /// Validates `raw` as a [`FieldName`]: non-empty, no `/`, and a non-empty
    /// canonical form.
    ///
    /// # Errors
    ///
    /// Returns [`FieldNameError::Empty`] when `raw` is empty or
    /// whitespace-only, [`FieldNameError::ContainsSlash`] when `raw` contains
    /// `/`, and [`FieldNameError::EmptyCanonical`] when `raw`'s [`FieldKey`]
    /// canonical form would be empty.
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

    /// # Errors
    ///
    /// See [`FieldName::validate`].
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::try_from(raw)
    }
}

impl TryFrom<serde_yaml::Value> for FieldName {
    type Error = FieldNameError;

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
/// Borrowed counterpart to [`FieldName`]: a field name borrowed from parsed
/// TOML data or a `$ref` string, mirroring the [`str`]/[`String`] split used by
/// `SchemaNameRef`.
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
/// A forgiving field identifier shared by Note frontmatter and inline fields:
/// stores the original key text for display and a canonical form for
/// case-insensitive, whitespace-normalized matching.
#[derive(Clone, Debug, Eq)]
pub(crate) struct FieldKey {
    /// Original key text as written by the user.
    name: String,
    /// Canonical form for case-insensitive matching.
    canonical: String,
}

impl FieldKey {
    /// Parses `raw` into a validated field key.
    ///
    /// # Errors
    ///
    /// Returns [`FieldKeyError::Empty`] when `raw` is empty or whitespace-only,
    /// and [`FieldKeyError::EmptyCanonical`] when canonicalization strips every
    /// searchable character.
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
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// Returns the canonical key form for matching.
    #[inline]
    #[must_use]
    pub(crate) fn canonical(&self) -> &str {
        &self.canonical
    }

    /// Returns `true` if `candidate` matches this key: an exact raw name match,
    /// falling back to a canonical (case/whitespace-forgiving) match. The main
    /// entry point for checking a raw string against a [`FieldKey`]; composes
    /// [`Self::is_canonical_match`].
    #[inline]
    #[must_use]
    pub(crate) fn is_match(&self, candidate: &str) -> bool {
        self.name == candidate || self.is_canonical_match(candidate)
    }

    /// Returns `true` if `candidate`'s canonical form matches `self`'s.
    ///
    /// Checks `candidate` against the stored canonical form literally first,
    /// and only canonicalizes `candidate` (allocating) when that literal check
    /// fails: most callers already pass an already-canonical string (a schema
    /// field name, a prior canonical lookup key), so the common case never
    /// allocates.
    #[must_use]
    pub(crate) fn is_canonical_match(&self, candidate: &str) -> bool {
        self.canonical == candidate
            || self.canonical == Self::canonicalize(candidate)
    }

    /// Returns `true` if `candidate`'s raw text exactly matches `self`'s raw
    /// name text.
    ///
    /// Unlike [`Self::is_match`]/[`Self::is_canonical_match`], this never
    /// forgives a case/whitespace difference: [`FieldName`] is Schema's exact
    /// identity type, so matching a [`FieldKey`] against one stays exact.
    #[must_use]
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    pub(crate) fn is_name_match(&self, candidate: &FieldName) -> bool {
        self.name == candidate.as_str()
    }

    /// Normalizes a raw key string for case-insensitive matching.
    ///
    /// Transformations:
    /// - ASCII whitespace -> `-`
    /// - `_`, `-`, ASCII alphanumeric -> kept, lowercased
    /// - Non-ASCII (emoji, Unicode letters) -> kept, lowercased
    /// - Everything else (`!`, `@`, `(`, etc.) -> stripped
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
    /// Compares canonical forms: two keys differing only by case or
    /// whitespace style are equal, matching [`FieldKey::is_canonical_match`].
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.canonical == other.canonical
    }
}

impl TryFrom<String> for FieldKey {
    type Error = FieldKeyError;

    /// # Errors
    ///
    /// See [`FieldKey::try_new`].
    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::try_new(raw)
    }
}

impl TryFrom<&str> for FieldKey {
    type Error = FieldKeyError;

    /// # Errors
    ///
    /// See [`FieldKey::try_new`].
    fn try_from(raw: &str) -> Result<Self, Self::Error> {
        Self::try_new(raw)
    }
}

impl FromStr for FieldKey {
    type Err = FieldKeyError;

    /// # Errors
    ///
    /// See [`FieldKey::try_new`].
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::try_new(raw)
    }
}

impl TryFrom<serde_yaml::Value> for FieldKey {
    type Error = FieldKeyError;

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

/// A [`FieldName`] failed to parse.
#[derive(Debug, Error)]
pub(crate) enum FieldNameError {
    /// The raw name was empty or whitespace-only.
    #[error("field name is empty")]
    Empty,
    /// The raw name contained a `/`, which would be ambiguous alongside
    /// `$ref` path segments.
    #[error("field name {name:?} cannot contain `/`")]
    ContainsSlash {
        name: String,
    },
    /// The raw name's [`FieldKey`] canonical form stripped to nothing.
    #[error("field name {name:?} has no searchable characters")]
    EmptyCanonical {
        name: String,
    },
    /// The source YAML value cannot stand as a field name.
    #[error("YAML value cannot be used as a field name")]
    UnsupportedYamlKey,
}

/// A [`FieldKey`] failed to parse.
#[derive(Debug, Error)]
pub(crate) enum FieldKeyError {
    /// The raw key was empty or whitespace-only.
    #[error("field key is empty")]
    Empty,
    /// The raw key's canonical form stripped to nothing.
    #[error("field key {name:?} has no searchable characters")]
    EmptyCanonical {
        name: String,
    },
    /// The source YAML value cannot stand as a field key.
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

/// Finds the item with the smallest edit distance to `input` among
/// `candidates`, each paired with the text to compare `input` against.
///
/// The matching threshold is half of `input`'s character count rounded up, with
/// a minimum threshold of 1 (`input.chars().count().div_ceil(2).max(1)`). This
/// threshold ensures single-character typos (such as `"nam"` for `"name"`)
/// match while preventing unrelated words (such as `"bogus"`) from matching any
/// candidate.
///
/// Returns the candidate item with the smallest edit distance if that distance
/// does not exceed the calculated threshold. Returns [`None`] if `candidates`
/// is empty or no candidate falls within the matching threshold.
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
/// Computes the minimum number of single-character insertions, deletions, or
/// substitutions required to transform `a` into `b` using an iterative two-row
/// Wagner-Fischer algorithm.
///
/// Returns the edit distance as a [`usize`].
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
        use pretty_assertions::assert_eq;

        use super::super::*;

        #[test]
        fn accepts_a_well_formed_name() {
            let name = FieldName::try_from("Status").expect("valid name");
            assert_eq!(name.as_str(), "Status");
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
        fn exact_identity_distinguishes_case() {
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
    }

    mod field_key {
        use pretty_assertions::assert_eq;

        use super::super::*;

        #[test]
        fn stores_original_name() {
            let key = FieldKey::try_new("Status").expect("valid key");
            assert_eq!(key.name(), "Status");
        }

        #[test]
        fn computes_canonical_form() {
            let key = FieldKey::try_new("Time Played").expect("valid key");
            assert_eq!(key.canonical(), "time-played");
        }

        #[test]
        fn lowercases_ascii() {
            let key = FieldKey::try_new("Status").expect("valid key");
            assert_eq!(key.canonical(), "status");
        }

        #[test]
        fn replaces_whitespace_with_hyphens() {
            let key = FieldKey::try_new("due date").expect("valid key");
            assert_eq!(key.canonical(), "due-date");
        }

        #[test]
        fn strips_special_characters() {
            let key = FieldKey::try_new("field-name!").expect("valid key");
            assert_eq!(key.canonical(), "field-name");
        }

        #[test]
        fn preserves_underscores_and_hyphens() {
            let key = FieldKey::try_new("my_field-name").expect("valid key");
            assert_eq!(key.canonical(), "my_field-name");
        }

        #[test]
        fn preserves_emoji() {
            let key = FieldKey::try_new("🗓️due").expect("valid key");
            assert_eq!(key.canonical(), "🗓️due");
        }

        #[test]
        fn rejects_an_empty_key() {
            assert!(matches!(FieldKey::try_new(""), Err(FieldKeyError::Empty)));
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

        #[test]
        fn partial_eq_uses_canonical_form() {
            let a = FieldKey::try_new("Status").expect("valid key");
            let b = FieldKey::try_new("status").expect("valid key");
            assert_eq!(a, b);
        }

        #[test]
        fn field_name_converts_into_field_key() {
            let name = FieldName::try_from("Status").expect("valid name");
            let key = FieldKey::from(name);
            assert_eq!(key.name(), "Status");
            assert_eq!(key.canonical(), "status");
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
