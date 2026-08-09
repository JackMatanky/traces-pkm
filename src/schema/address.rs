//! Field address types for `$ref` parsing and resolution.
//!
//! [`FieldAddress`] owns a `#<schema>/<field>` coordinate parsed from TOML.
//! [`FieldAddressRef`] borrows the same shape while resolving a current field,
//! following the crate's owned/borrowed newtype split.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer};
use thiserror::Error;

use super::name::{SchemaName, SchemaNameRef};
use crate::field::{FieldName, FieldNameError, FieldNameRef};

/// An owned field address: `#<schema>/<field>`.
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct FieldAddress {
    schema: SchemaName,
    field: FieldName,
}

impl FieldAddress {
    /// Returns the addressed Schema's name.
    #[inline]
    #[must_use]
    pub(crate) fn schema(&self) -> &SchemaName {
        &self.schema
    }

    /// Returns the addressed field's name.
    #[inline]
    #[must_use]
    pub(crate) fn field(&self) -> &FieldName {
        &self.field
    }
}

impl TryFrom<&str> for FieldAddress {
    type Error = FieldAddressError;

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

    /// # Errors
    ///
    /// See [`FieldAddress::try_from`].
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::try_from(raw)
    }
}

impl fmt::Display for FieldAddress {
    /// Writes `#<schema>/<field>` directly into the formatter, without
    /// allocating an intermediate `String`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}/{}", self.schema, self.field)
    }
}

impl fmt::Debug for FieldAddress {
    /// Matches `str`'s own `Debug` (quoted, escaped) applied to this address's
    /// `#<schema>/<field>` display form, so wrapping a `$ref` in this type
    /// never changes an error message's text.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.to_string(), f)
    }
}

impl<'de> Deserialize<'de> for FieldAddress {
    /// Deserializes from a string shaped `#<schema>/<field>` and validates it.
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

/// Borrowed counterpart to [`FieldAddress`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct FieldAddressRef<'a> {
    schema: SchemaNameRef<'a>,
    field: FieldNameRef<'a>,
}

impl<'a> FieldAddressRef<'a> {
    /// Builds a borrowed Schema field address.
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

    /// Returns the addressed Schema's name.
    #[inline]
    #[must_use]
    pub(crate) fn schema(self) -> SchemaNameRef<'a> {
        self.schema
    }

    /// Returns the addressed field's name.
    #[inline]
    #[must_use]
    pub(crate) fn field(self) -> FieldNameRef<'a> {
        self.field
    }
}

/// A [`FieldAddress`] failed to parse.
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
}
