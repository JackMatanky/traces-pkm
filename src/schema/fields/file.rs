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
    super::raw::RawSchemaFieldType,
    SchemaFieldType,
    address::FieldAddressRef,
    error::{
        AttributeError, expect_string, expect_string_list, type_mismatch,
        unknown_key,
    },
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
        let mut def = Self::default();
        let mut errors = Vec::new();
        for (key, value) in options {
            match key.as_str() {
                "folders" => match expect_string_list(value) {
                    Some(folders) => def.folders = folders,
                    None => errors.push(type_mismatch(
                        address,
                        RawSchemaFieldType::File,
                        key,
                        value,
                        "an array of strings",
                    )),
                },
                "ext" => match expect_string(value) {
                    Some(ext) => def.ext = Some(ext),
                    None => errors.push(type_mismatch(
                        address,
                        RawSchemaFieldType::File,
                        key,
                        value,
                        "a string",
                    )),
                },
                "class" => match expect_string_list(value) {
                    Some(class) => def.class = class,
                    None => errors.push(type_mismatch(
                        address,
                        RawSchemaFieldType::File,
                        key,
                        value,
                        "an array of strings",
                    )),
                },
                _ => {
                    errors.push(unknown_key(
                        address,
                        RawSchemaFieldType::File,
                        key,
                    ));
                }
            }
        }
        let folders = if def.folders.is_empty() {
            match base {
                Some(SchemaFieldType::File(base_def)) => {
                    base_def.folders.clone()
                }
                _ => Vec::new(),
            }
        } else {
            def.folders
        };
        let ext = def.ext.or_else(|| match base {
            Some(SchemaFieldType::File(base_def)) => base_def.ext.clone(),
            _ => None,
        });
        let class = if def.class.is_empty() {
            match base {
                Some(SchemaFieldType::File(base_def)) => base_def.class.clone(),
                _ => Vec::new(),
            }
        } else {
            def.class
        };
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
