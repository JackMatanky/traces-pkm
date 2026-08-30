//! `#<schema>/<field>` address parsing and representation.

use std::{borrow::Cow, fmt, str::FromStr};

use serde::{Deserialize, Deserializer};
use thiserror::Error;

use super::super::{SchemaName, SchemaNameRef};
use crate::field::{FieldName, FieldNameError, FieldNameRef};

/// An owned `#<schema>/<field>` coordinate parsed from a `$ref` string.
///
/// Constructed via [`TryFrom<&str>`](TryFrom) or [`FromStr`].
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct FieldAddress {
    schema: SchemaName,
    field: FieldName,
}

impl FieldAddress {
    /// Return the addressed Schema's name.
    #[inline]
    #[must_use]
    pub(crate) const fn schema(&self) -> &SchemaName {
        &self.schema
    }

    /// Return the addressed field's name.
    #[inline]
    #[must_use]
    pub(crate) const fn field(&self) -> &FieldName {
        &self.field
    }

    /// Borrow this address as a [`FieldAddressRef`].
    ///
    /// Test-only: production code builds [`FieldAddressRef`] directly.
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
    /// - [`Malformed`] if `raw` is not shaped `#<schema>/<field>` with both
    ///   segments non-empty.
    /// - [`FieldName`] if the field segment fails [`FieldName`] validation.
    ///
    /// [`Malformed`]: FieldAddressError::Malformed
    /// [`FieldName`]: FieldAddressError::FieldName
    #[expect(
        clippy::expect_used,
        reason = "schema.is_empty() checked two lines above"
    )]
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
            schema: SchemaName::try_from(schema)
                .expect("non-empty checked above"),
            field: FieldName::try_from(field)?,
        })
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

    /// Parse a string slice into a [`FieldAddress`].
    ///
    /// # Errors
    ///
    /// Same as [`TryFrom<&str>`] for [`FieldAddress`].
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::try_from(raw)
    }
}

impl fmt::Display for FieldAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}/{}", self.schema, self.field)
    }
}

impl fmt::Debug for FieldAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\"#{}/{}\"", self.schema, self.field)
    }
}

impl<'de> Deserialize<'de> for FieldAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = Cow::<'de, str>::deserialize(deserializer)?;
        Self::try_from(raw.as_ref()).map_err(serde::de::Error::custom)
    }
}

/// A borrowed `#<schema>/<field>` coordinate.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct FieldAddressRef<'a> {
    schema: SchemaNameRef<'a>,
    field: FieldNameRef<'a>,
}

impl<'a> FieldAddressRef<'a> {
    /// Build a borrowed field address from its schema and field parts.
    #[inline]
    #[must_use]
    pub(crate) const fn new(
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
    pub(crate) const fn schema(self) -> SchemaNameRef<'a> {
        self.schema
    }

    /// Return the addressed field's name.
    #[inline]
    #[must_use]
    pub(crate) const fn field(self) -> FieldNameRef<'a> {
        self.field
    }
}

/// Why a [`FieldAddress`] failed to parse.
#[derive(Debug, Error)]
pub(crate) enum FieldAddressError {
    /// The input was not shaped `#<schema>/<field>` with both segments
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
    use rstest::rstest;

    use super::*;

    #[test]
    fn parses_a_well_formed_address() {
        let address = FieldAddress::try_from("#book/status").expect("parses");

        assert_eq!(address.schema(), &SchemaName::from("book"));
        assert_eq!(address.field().as_str(), "status");
    }

    #[rstest]
    #[case::missing_hash_prefix("book/status")]
    #[case::missing_slash_separator("#bookstatus")]
    #[case::empty_schema_segment("#/status")]
    #[case::empty_field_segment("#book/")]
    fn rejects_malformed_references(#[case] input: &str) {
        assert!(matches!(
            FieldAddress::try_from(input),
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

    #[test]
    fn from_field_address_ref_round_trips_through_try_from() {
        let owned = FieldAddress::try_from("#book/status").expect("parses");
        let borrowed = owned.as_ref();

        let round_tripped = FieldAddress::from(borrowed);

        assert_eq!(round_tripped, owned);
    }

    #[test]
    fn from_str_parses_via_the_str_parse_method() {
        let address: FieldAddress = "#book/status".parse().expect("parses");

        assert_eq!(address.to_string(), "#book/status");
    }

    mod deserialize {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn deserializes_a_well_formed_reference_string() {
            let address: FieldAddress =
                serde_json::from_str("\"#book/status\"")
                    .expect("valid reference");

            assert_eq!(address.to_string(), "#book/status");
        }

        #[test]
        fn rejects_a_malformed_reference_string() {
            let result: Result<FieldAddress, _> =
                serde_json::from_str("\"not-a-ref\"");

            assert!(result.is_err());
        }
    }
}
