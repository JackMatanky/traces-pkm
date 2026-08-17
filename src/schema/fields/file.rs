//! `file` field type definition, borrowed filter, and parsing.
//!
//! A `file` field links to notes matched by folder, extension, and class
//! filters. Parsing validates the three attributes (`folders`, `ext`, `class`)
//! and merges with a `$ref` base when present.
//!
//! [`SchemaFileFieldRef`] borrows the resolved filter parts for
//! [`file_filter`].
//!
//! [`file_filter`]: super::SchemaFieldDef::file_filter

use std::collections::BTreeMap;

use super::{
    super::raw::RawSchemaFieldType, SchemaFieldType, address::FieldAddressRef,
    error::AttributeError, parser::SchemaFieldParser,
};
use crate::field::FieldValue;

/// Resolved `file` field options.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SchemaFileField {
    folders: Vec<String>,
    ext: Option<String>,
    class: Vec<String>,
}

impl SchemaFileField {
    /// Return the matched folder paths.
    #[inline]
    #[must_use]
    pub(crate) fn folders(&self) -> &[String] {
        &self.folders
    }

    /// Return the matched file extension, if set.
    #[inline]
    #[must_use]
    pub(crate) fn ext(&self) -> Option<&str> {
        self.ext.as_deref()
    }

    /// Return the matched class tags.
    #[inline]
    #[must_use]
    pub(crate) fn class(&self) -> &[String] {
        &self.class
    }

    /// Borrow this definition as a [`SchemaFileFieldRef`].
    #[inline]
    #[must_use]
    pub(super) fn as_ref(&self) -> SchemaFileFieldRef<'_> {
        SchemaFileFieldRef {
            folders: &self.folders,
            ext: self.ext.as_deref(),
            class: &self.class,
        }
    }

    /// Build an instance for tests.
    #[cfg(test)]
    pub(crate) fn for_test(
        folders: Vec<String>,
        ext: Option<String>,
        class: Vec<String>,
    ) -> Self {
        Self {
            folders,
            ext,
            class,
        }
    }

    /// Parse `options` against `file`'s `folders`/`ext`/`class` attributes,
    /// merging with `base` when present.
    ///
    /// # Arguments
    ///
    /// * `address`: field address for error context.
    /// * `options`: raw key-value pairs from the TOML definition.
    /// * `base`: inherited field type to fall back to for unset keys.
    pub(super) fn parse(
        address: FieldAddressRef<'_>,
        options: &BTreeMap<String, FieldValue>,
        base: Option<&SchemaFieldType>,
    ) -> (SchemaFieldType, Vec<AttributeError>) {
        let (base_folders, base_ext, base_class) = match base {
            Some(SchemaFieldType::File(base_def)) => (
                base_def.folders.clone(),
                base_def.ext.clone(),
                base_def.class.clone(),
            ),
            _ => (Vec::new(), None, Vec::new()),
        };

        let mut errors = Vec::new();
        let mut parser =
            SchemaFieldParser::new(address, RawSchemaFieldType::File);

        let folders =
            parser.string_list(options, "folders", base_folders, &mut errors);
        let ext = parser.string(options, "ext", base_ext, &mut errors);
        let class =
            parser.string_list(options, "class", base_class, &mut errors);

        errors.extend(parser.finish(options));
        (
            SchemaFieldType::File(SchemaFileField {
                folders,
                ext,
                class,
            }),
            errors,
        )
    }
}

/// A borrowed view of a resolved `file` field's filter parts.
///
/// Returned by [`file_filter`].
///
/// [`file_filter`]: super::SchemaFieldDef::file_filter
pub(crate) struct SchemaFileFieldRef<'a> {
    pub(crate) folders: &'a [String],
    pub(crate) ext: Option<&'a str>,
    pub(crate) class: &'a [String],
}
