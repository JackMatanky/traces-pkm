//! Schema-name newtypes: keeps a Schema's identity from being mixed up with
//! an unrelated `&str`/`String` (a field name, a `$ref` string, ...) anywhere
//! it is threaded through the module.
//!
//! Mirrors the crate's [`FileName`]/[`BaseName`]/[`BaseNameRef`] split
//! (`crate::file_name`): [`SchemaName`] owns its data for storage
//! (`Schema.name`, map keys); [`SchemaNameRef`] borrows for the
//! zero-allocation comparisons `resolve::SchemaGraph`'s Kahn's
//! algorithm bookkeeping and `FieldPath` need.
//!
//! Ordering matches `str`'s (a derived `Ord`/`PartialOrd` on a single-field
//! tuple struct delegates entirely to the wrapped field): `SchemaGraph`'s
//! determinism and its Global-first Kahn tie-break both depend on this.
//!
//! [`FileName`]: crate::file_name::FileName
//! [`BaseName`]: crate::file_name::BaseName
//! [`BaseNameRef`]: crate::file_name::BaseNameRef

use std::{borrow::Borrow, fmt};

use serde::Deserialize;

/// A Schema's name: its source file's stem (spec User Story 1).
#[derive(Clone, Default, Eq, PartialEq, Ord, PartialOrd, Hash, Deserialize)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "declared by the schema-registry ticket; consumed by the \
                  schema-namespace ticket \
                  (.scratch/metadata-schemas/issues/\
                  03-schema-minijinja-namespace.md)"
    )
)]
pub(crate) struct SchemaName(String);

impl SchemaName {
    /// Returns this name as a string slice.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// Borrows this name as a [`SchemaNameRef`].
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
    pub(crate) fn as_ref(&self) -> SchemaNameRef<'_> {
        SchemaNameRef(&self.0)
    }
}

impl From<SchemaNameRef<'_>> for SchemaName {
    fn from(name: SchemaNameRef<'_>) -> Self {
        Self(name.0.to_owned())
    }
}

impl From<&str> for SchemaName {
    fn from(name: &str) -> Self {
        Self(name.to_owned())
    }
}

impl Borrow<str> for SchemaName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SchemaName {
    /// Matches `str`'s own `Debug` (quoted, escaped) so wrapping a Schema
    /// name in this type never changes an error or warning message's text.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for SchemaName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// Borrowed counterpart to [`SchemaName`]: a Schema name borrowed from
/// parsed TOML data or a `$ref` string, mirroring the `&str`/`String` split
/// ([`crate::file_name::BaseNameRef`]).
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct SchemaNameRef<'a>(&'a str);

impl<'a> SchemaNameRef<'a> {
    /// Returns this name as a string slice.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
    pub(crate) fn as_str(self) -> &'a str {
        self.0
    }
}

impl<'a> From<&'a str> for SchemaNameRef<'a> {
    fn from(name: &'a str) -> Self {
        Self(name)
    }
}

impl Borrow<str> for SchemaNameRef<'_> {
    fn borrow(&self) -> &str {
        self.0
    }
}

impl fmt::Debug for SchemaNameRef<'_> {
    /// Matches `str`'s own `Debug` (quoted, escaped) — see
    /// [`SchemaName`]'s `Debug` impl for why this matters.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.0, f)
    }
}

impl fmt::Display for SchemaNameRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.0, f)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;

    #[test]
    fn ordering_matches_str_ordering() {
        let mut strs = ["global", "author", "sci_fi", "book"];
        let mut names: Vec<SchemaName> =
            strs.iter().map(|&s| SchemaName(s.to_owned())).collect();
        strs.sort_unstable();
        names.sort();

        let sorted_names: Vec<&str> =
            names.iter().map(SchemaName::as_str).collect();
        assert_eq!(sorted_names, strs);
    }

    #[test]
    fn ref_ordering_matches_str_ordering() {
        let mut strs = ["global", "author", "sci_fi", "book"];
        let mut refs: Vec<SchemaNameRef<'_>> =
            strs.iter().map(|&s| SchemaNameRef(s)).collect();
        strs.sort_unstable();
        refs.sort();

        let sorted_refs: Vec<&str> = refs.iter().map(|r| r.as_str()).collect();
        assert_eq!(sorted_refs, strs);
    }

    #[test]
    fn borrow_str_enables_map_lookup_without_allocating() {
        let mut map = BTreeMap::new();
        map.insert(SchemaName("book".to_owned()), 1);

        assert_eq!(map.get("book"), Some(&1));
    }

    #[test]
    fn ref_borrow_str_enables_set_lookup_without_allocating() {
        let mut set = BTreeSet::new();
        set.insert(SchemaNameRef("book"));

        assert!(set.contains("book"));
    }

    #[test]
    fn debug_matches_str_debug_exactly() {
        let name = SchemaName("sci_fi".to_owned());
        assert_eq!(format!("{name:?}"), format!("{:?}", "sci_fi"));

        let name_ref = SchemaNameRef("sci_fi");
        assert_eq!(format!("{name_ref:?}"), format!("{:?}", "sci_fi"));
    }

    #[test]
    fn display_matches_str_display_exactly() {
        let name = SchemaName("sci_fi".to_owned());
        assert_eq!(name.to_string(), "sci_fi");

        let name_ref = SchemaNameRef("sci_fi");
        assert_eq!(name_ref.to_string(), "sci_fi");
    }

    #[test]
    fn as_ref_round_trips_through_from() {
        let name = SchemaName("book".to_owned());
        let name_ref = name.as_ref();
        let round_tripped = SchemaName::from(name_ref);

        assert_eq!(name, round_tripped);
    }
}
