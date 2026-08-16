//! `file` field type definition, borrowed filter, and parsing.
//!
//! A `file` field links to notes matched by folder, extension, and class
//! filters. Parsing validates the three attributes (`folders`, `ext`, `class`)
//! and merges with a `$ref` base when present.
//!
//! [`SchemaFileFieldDefRef`] borrows the resolved filter parts for
//! [`super::SchemaFieldDef::file_filter`].

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

/// Raw `file` field options before `$ref` merge.
#[derive(Default)]
pub(super) struct SchemaFileFieldDef {
    folders: Vec<String>,
    ext: Option<String>,
    class: Vec<String>,
}

impl SchemaFileFieldDef {
    /// Borrow this definition as a [`SchemaFileFieldDefRef`].
    #[inline]
    #[must_use]
    pub(super) fn as_ref(&self) -> SchemaFileFieldDefRef<'_> {
        SchemaFileFieldDefRef {
            folders: &self.folders,
            ext: self.ext.as_deref(),
            class: &self.class,
        }
    }

    /// Parse `options` against `file`'s `folders`/`ext`/`class` attributes,
    /// merging with `base` when present.
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
                Some(SchemaFieldType::File {
                    folders,
                    ..
                }) => folders.clone(),
                _ => Vec::new(),
            }
        } else {
            def.folders
        };
        let ext = def.ext.or_else(|| match base {
            Some(SchemaFieldType::File {
                ext,
                ..
            }) => ext.clone(),
            _ => None,
        });
        let class = if def.class.is_empty() {
            match base {
                Some(SchemaFieldType::File {
                    class,
                    ..
                }) => class.clone(),
                _ => Vec::new(),
            }
        } else {
            def.class
        };
        (
            SchemaFieldType::File {
                folders,
                ext,
                class,
            },
            errors,
        )
    }
}

/// A borrowed view of a resolved `file` field's filter parts.
pub(crate) struct SchemaFileFieldDefRef<'a> {
    pub(crate) folders: &'a [String],
    pub(crate) ext: Option<&'a str>,
    pub(crate) class: &'a [String],
}
