//! Schema name newtypes: owned ([`SchemaName`]) and borrowed
//! ([`SchemaNameRef`]).
//!
//! Ordering matches `str`'s, which `SchemaGraph`'s determinism depends on.

use std::{borrow::Borrow, fmt};

use serde::Deserialize;
use thiserror::Error;

use super::GLOBAL_SCHEMA_NAME;
use crate::BaseNameRef;

/// A Schema name from its source file stem.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize)]
pub(crate) struct SchemaName(String);

/// Why a [`SchemaName`] could not be constructed.
#[derive(Debug, Eq, PartialEq, Error)]
#[error("Schema name must not be empty")]
pub(crate) struct EmptySchemaName;

impl SchemaName {
    /// Return this name as a string slice.
    #[inline]
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// Borrow this name as a [`SchemaNameRef`].
    #[inline]
    #[must_use]
    pub(crate) fn as_ref(&self) -> SchemaNameRef<'_> {
        SchemaNameRef(&self.0)
    }

    /// Returns whether this is the reserved Global Schema name.
    #[inline]
    #[must_use]
    pub(crate) fn is_global(&self) -> bool {
        self.0 == GLOBAL_SCHEMA_NAME
    }

    /// Returns the reserved Global Schema name.
    ///
    /// Infallible, unlike [`Self::try_from`]: `GLOBAL_SCHEMA_NAME` is a
    /// compile-time non-empty `&'static str` literal, so there is no empty
    /// case to reject.
    #[inline]
    #[must_use]
    pub(crate) fn global() -> Self {
        Self(GLOBAL_SCHEMA_NAME.to_owned())
    }

    /// Attempts to construct a [`SchemaName`], rejecting an empty name.
    ///
    /// An inherent method rather than a `TryFrom<&str>` trait impl: with the
    /// `#[cfg(test)]`/`test-utils`-gated `From<&str>` impl below present in
    /// the same build, a manual `TryFrom<&str>` impl conflicts with std's
    /// blanket `impl<T, U> TryFrom<U> for T where U: Into<T>` (E0119). An
    /// inherent method shadows the blanket trait impl for `SchemaName::
    /// try_from(...)` call syntax without implementing the trait itself.
    ///
    /// Most callers should prefer a source-specific infallible constructor
    /// instead: [`Self::global`] for the reserved name, or the
    /// `From<`[`BaseNameRef`]`>` impl below for a Schema file's stem, both of
    /// which are non-empty by construction and never reach this check. This
    /// method exists for the one remaining case — a `$ref` schema segment
    /// parsed from user-authored TOML text — where the input is genuinely
    /// untrusted.
    ///
    /// # Errors
    ///
    /// - [`EmptySchemaName`] if `name` is empty
    pub(crate) fn try_from(name: &str) -> Result<Self, EmptySchemaName> {
        if name.is_empty() {
            return Err(EmptySchemaName);
        }
        Ok(Self(name.to_owned()))
    }
}

impl From<SchemaNameRef<'_>> for SchemaName {
    fn from(name: SchemaNameRef<'_>) -> Self {
        Self(name.0.to_owned())
    }
}

impl From<BaseNameRef<'_>> for SchemaName {
    /// Builds a [`SchemaName`] from a Schema TOML file's stem.
    ///
    /// Infallible, unlike [`SchemaName::try_from`]: [`BaseNameRef`] is
    /// always derived from [`Path::file_stem`](std::path::Path::file_stem),
    /// which never yields an empty string for a real path component, so the
    /// non-empty invariant already holds before this conversion runs.
    fn from(stem: BaseNameRef<'_>) -> Self {
        Self(stem.as_str().to_owned())
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl From<&str> for SchemaName {
    /// Test-only infallible constructor: every test fixture name is a
    /// non-empty literal, so forcing `Result` handling through hundreds of
    /// call sites buys production code nothing.
    #[expect(
        clippy::expect_used,
        reason = "test-only constructor; an invalid literal here is a test \
                  fixture bug, not a recoverable caller error"
    )]
    fn from(name: &str) -> Self {
        Self::try_from(name).expect("test schema name must not be empty")
    }
}

impl Borrow<str> for SchemaName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SchemaName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for SchemaName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// Borrow a Schema name from parsed TOML data or a `$ref` string.
#[derive(Copy, Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct SchemaNameRef<'a>(&'a str);

impl<'a> SchemaNameRef<'a> {
    /// Return this name as a string slice.
    #[inline]
    #[must_use]
    pub(crate) const fn as_str(self) -> &'a str {
        self.0
    }
}

impl SchemaNameRef<'_> {
    /// Returns whether this is the reserved Global Schema name.
    #[inline]
    #[must_use]
    pub(crate) fn is_global(self) -> bool {
        self.0 == GLOBAL_SCHEMA_NAME
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
    mod ordering {
        use pretty_assertions::assert_eq;

        use super::super::*;

        #[test]
        fn schema_name_ordering_matches_str_ordering() {
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
        fn schema_name_ref_ordering_matches_str_ordering() {
            let mut strs = ["global", "author", "sci_fi", "book"];
            let mut refs: Vec<SchemaNameRef<'_>> =
                strs.iter().map(|&s| SchemaNameRef(s)).collect();
            strs.sort_unstable();
            refs.sort();

            let sorted_refs: Vec<&str> =
                refs.iter().map(|r| r.as_str()).collect();
            assert_eq!(sorted_refs, strs);
        }
    }

    mod borrowing {
        use std::collections::{BTreeMap, BTreeSet};

        use pretty_assertions::assert_eq;

        use super::super::*;

        #[test]
        fn schema_name_borrow_str_enables_map_lookup_without_allocating() {
            let mut map = BTreeMap::new();
            map.insert(SchemaName("book".to_owned()), 1);

            assert_eq!(map.get("book"), Some(&1));
        }

        #[test]
        fn schema_name_ref_borrow_str_enables_set_lookup_without_allocating() {
            let mut set = BTreeSet::new();
            set.insert(SchemaNameRef("book"));

            assert!(set.contains("book"));
        }
    }

    mod formatting {
        use pretty_assertions::assert_eq;

        use super::super::*;

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
    }

    mod conversions {
        use pretty_assertions::assert_eq;

        use super::super::*;

        #[test]
        fn schema_name_from_str_owns_a_copy_of_the_given_name() {
            let name = SchemaName::from("book");

            assert_eq!(name.as_str(), "book");
        }

        #[test]
        fn schema_name_ref_from_str_borrows_the_given_name() {
            let name_ref = SchemaNameRef::from("book");

            assert_eq!(name_ref.as_str(), "book");
        }

        #[test]
        fn as_ref_round_trips_through_from() {
            let name = SchemaName("book".to_owned());
            let name_ref = name.as_ref();
            let round_tripped = SchemaName::from(name_ref);

            assert_eq!(name, round_tripped);
        }

        #[test]
        fn try_from_rejects_an_empty_name() {
            assert!(SchemaName::try_from("").is_err());
        }

        #[test]
        fn from_base_name_ref_owns_a_copy_of_the_stem() {
            let stem =
                BaseNameRef::from_path(std::path::Path::new("book.toml"))
                    .expect("valid path");

            assert_eq!(SchemaName::from(stem).as_str(), "book");
        }
    }

    mod predicates {
        use super::super::*;

        #[test]
        fn is_global_matches_only_the_reserved_name() {
            assert!(SchemaName::from("global").is_global());
            assert!(!SchemaName::from("book").is_global());
            assert!(SchemaNameRef::from("global").is_global());
            assert!(!SchemaNameRef::from("book").is_global());
        }

        #[test]
        fn global_returns_the_reserved_name() {
            assert_eq!(SchemaName::global().as_str(), "global");
            assert!(SchemaName::global().is_global());
        }
    }
}
