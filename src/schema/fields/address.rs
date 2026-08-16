//! Parse and carry `$ref` field addresses.
//!
//! [`FieldAddress`] owns a `#<schema>/<field>` coordinate parsed from TOML.
//! [`FieldAddressRef`] borrows the same shape while resolving a current field,
//! following the crate's owned/borrowed newtype split.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer};
use thiserror::Error;

use super::super::name::{SchemaName, SchemaNameRef};
use crate::field::{FieldName, FieldNameError, FieldNameRef};

/// Store an owned `#<schema>/<field>` address.
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct FieldAddress {
    schema: SchemaName,
    field: FieldName,
}

impl FieldAddress {
    /// Return the addressed Schema's name.
    #[inline]
    #[must_use]
    pub(crate) fn schema(&self) -> &SchemaName {
        &self.schema
    }

    /// Return the addressed field's name.
    #[inline]
    #[must_use]
    pub(crate) fn field(&self) -> &FieldName {
        &self.field
    }

    /// Borrow this address as a [`FieldAddressRef`].
    ///
    /// Test-only: production code always builds a [`FieldAddressRef`] directly
    /// from its `SchemaNameRef`/`FieldNameRef` parts; this exists so
    /// cross-module test fixtures (`schema::fields`, `schema::service`) can
    /// turn an already-built [`FieldAddress`] test fixture into one without
    /// duplicating [`FieldAddressRef::new`] call sites.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) fn as_ref(&self) -> FieldAddressRef<'_> {
        FieldAddressRef::new(self.schema.as_ref(), self.field.as_ref())
    }
}

impl TryFrom<&str> for FieldAddress {
    type Error = FieldAddressError;

    /// Parse a string into an owned field address.
    ///
    /// # Errors
    ///
    /// Returns [`FieldAddressError::Malformed`] when `raw` is not shaped
    /// `#<schema>/<field>` with both segments non-empty, and
    /// [`FieldAddressError::FieldName`] when the field segment fails
    /// [`FieldName`] validation.
    fn try_from(raw: &str) -> Result<Self, Self::Error> {
        let malformed = || FieldAddressError::Malformed {
            reference: raw.to_owned(),
        };
        let stripped = raw.strip_prefix('#').ok_or_else(malformed)?;
        let (schema, field) = stripped.split_once('/').ok_or_else(malformed)?;
        if schema.is_empty() || field.is_empty() {
            return Err(malformed());
        }
        Ok(Self {
            schema: SchemaName::from(schema),
            field: FieldName::try_from(field)?,
        })
    }
}

impl TryFrom<String> for FieldAddress {
    type Error = FieldAddressError;

    /// Parse an owned string into an owned field address.
    ///
    /// # Errors
    ///
    /// See [`FieldAddress::try_from`].
    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::try_from(raw.as_str())
    }
}

impl From<FieldAddressRef<'_>> for FieldAddress {
    fn from(address: FieldAddressRef<'_>) -> Self {
        Self {
            schema: SchemaName::from(address.schema),
            field: FieldName::from(address.field),
        }
    }
}

impl FromStr for FieldAddress {
    type Err = FieldAddressError;

    /// Parse a string into an owned field address.
    ///
    /// # Errors
    ///
    /// See [`FieldAddress::try_from`].
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::try_from(raw)
    }
}

impl fmt::Display for FieldAddress {
    /// Write `#<schema>/<field>` directly into the formatter, without
    /// allocating an intermediate `String`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}/{}", self.schema, self.field)
    }
}

impl fmt::Debug for FieldAddress {
    /// Match `str`'s own `Debug` (quoted, escaped) applied to this address's
    /// `#<schema>/<field>` display form, so wrapping a `$ref` in this type
    /// never changes an error message's text.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.to_string(), f)
    }
}

impl<'de> Deserialize<'de> for FieldAddress {
    /// Deserialize from a string shaped `#<schema>/<field>` and validate it.
    ///
    /// # Errors
    ///
    /// See [`FieldAddress::try_from`].
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::try_from(raw).map_err(serde::de::Error::custom)
    }
}

/// Borrow a `#<schema>/<field>` address.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct FieldAddressRef<'a> {
    schema: SchemaNameRef<'a>,
    field: FieldNameRef<'a>,
}

impl<'a> FieldAddressRef<'a> {
    /// Build a borrowed Schema field address.
    #[inline]
    #[must_use]
    pub(crate) fn new(
        schema: SchemaNameRef<'a>,
        field: FieldNameRef<'a>,
    ) -> Self {
        Self {
            schema,
            field,
        }
    }

    /// Return the addressed Schema's name.
    #[inline]
    #[must_use]
    pub(crate) fn schema(self) -> SchemaNameRef<'a> {
        self.schema
    }

    /// Return the addressed field's name.
    #[inline]
    #[must_use]
    pub(crate) fn field(self) -> FieldNameRef<'a> {
        self.field
    }
}

/// Describe why a [`FieldAddress`] failed to parse.
#[derive(Debug, Error)]
pub(crate) enum FieldAddressError {
    /// `reference` was not shaped `#<schema>/<field>` with both segments
    /// non-empty.
    #[error("malformed $ref {reference:?}: expected `#<schema>/<field>`")]
    Malformed {
        reference: String,
    },
    /// The field segment failed [`FieldName`] validation.
    #[error(transparent)]
    FieldName(#[from] FieldNameError),
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn parses_a_well_formed_address() {
        let address = FieldAddress::try_from("#book/status").expect("parses");

        assert_eq!(address.schema(), &SchemaName::from("book"));
        assert_eq!(address.field().as_str(), "status");
    }

    #[test]
    fn rejects_a_reference_missing_the_hash_prefix() {
        assert!(matches!(
            FieldAddress::try_from("book/status"),
            Err(FieldAddressError::Malformed { .. })
        ));
    }

    #[test]
    fn rejects_a_reference_missing_the_slash_separator() {
        assert!(matches!(
            FieldAddress::try_from("#bookstatus"),
            Err(FieldAddressError::Malformed { .. })
        ));
    }

    #[test]
    fn rejects_a_reference_with_an_empty_schema_segment() {
        assert!(matches!(
            FieldAddress::try_from("#/status"),
            Err(FieldAddressError::Malformed { .. })
        ));
    }

    #[test]
    fn rejects_a_reference_with_an_empty_field_segment() {
        assert!(matches!(
            FieldAddress::try_from("#book/"),
            Err(FieldAddressError::Malformed { .. })
        ));
    }

    #[test]
    fn rejects_a_reference_whose_field_segment_fails_field_name_validation() {
        assert!(matches!(
            FieldAddress::try_from("#book/!!!"),
            Err(FieldAddressError::FieldName(_))
        ));
    }

    #[test]
    fn display_round_trips_the_original_shape() {
        let address = FieldAddress::try_from("#book/status").expect("parses");

        assert_eq!(address.to_string(), "#book/status");
    }

    #[test]
    fn debug_matches_the_quoted_display_form() {
        let address = FieldAddress::try_from("#book/status").expect("parses");

        assert_eq!(format!("{address:?}"), "\"#book/status\"");
    }

    #[test]
    fn as_ref_round_trips_through_new() {
        let address = FieldAddress::try_from("#book/status").expect("parses");

        let reference = address.as_ref();

        assert_eq!(reference.schema().as_str(), "book");
        assert_eq!(reference.field().as_str(), "status");
    }
}
