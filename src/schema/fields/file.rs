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

use super::{SchemaFieldType, parser::SchemaFieldParser};
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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "reserved for future schema consumers")
    )]
    pub(crate) fn folders(&self) -> &[String] {
        &self.folders
    }

    /// Return the matched file extension, if set.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "reserved for future schema consumers")
    )]
    pub(crate) fn ext(&self) -> Option<&str> {
        self.ext.as_deref()
    }

    /// Return the matched class tags.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "reserved for future schema consumers")
    )]
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
    /// * `parser`: pre-constructed parser for this field.
    /// * `options`: raw key-value pairs from the TOML definition.
    /// * `base`: inherited field type to fall back to for unset keys.
    pub(super) fn parse(
        parser: &mut SchemaFieldParser<'_>,
        options: &BTreeMap<String, FieldValue>,
        base: Option<&SchemaFieldType>,
    ) -> SchemaFieldType {
        let (base_folders, base_ext, base_class) = match base {
            Some(SchemaFieldType::File(base_def)) => (
                base_def.folders.clone(),
                base_def.ext.clone(),
                base_def.class.clone(),
            ),
            _ => (Vec::new(), None, Vec::new()),
        };

        let folders = parser.string_list(options, "folders", base_folders);
        let ext = parser.string(options, "ext", base_ext);
        let class = parser.string_list(options, "class", base_class);

        SchemaFieldType::File(SchemaFileField {
            folders,
            ext,
            class,
        })
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
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use pretty_assertions::assert_eq;

    use super::{
        super::{
            super::error::SchemaFieldParserError, address::FieldAddress,
            parser::SchemaFieldParser,
        },
        *,
    };

    fn address() -> FieldAddress {
        FieldAddress::try_from("#book/field").expect("valid ref")
    }

    fn options(pairs: &[(&str, FieldValue)]) -> BTreeMap<String, FieldValue> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
    }

    #[test]
    fn collects_folders_ext_and_class() {
        let opts = options(&[
            (
                "folders",
                FieldValue::List(vec![FieldValue::String("assets".to_owned())]),
            ),
            ("ext", FieldValue::String("png".to_owned())),
            (
                "class",
                FieldValue::List(vec![FieldValue::String("image".to_owned())]),
            ),
        ]);

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::File(SchemaFileField::default()),
        );
        let field_type = SchemaFileField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert!(errors.is_empty());
        assert_eq!(
            field_type,
            SchemaFieldType::File(SchemaFileField::for_test(
                vec!["assets".to_owned()],
                Some("png".to_owned()),
                vec!["image".to_owned()],
            ))
        );
    }

    #[test]
    fn falls_back_independently_per_subfield() {
        let base = SchemaFieldType::File(SchemaFileField::for_test(
            vec!["base-folder".to_owned()],
            Some("base-ext".to_owned()),
            vec!["base-class".to_owned()],
        ));
        let opts = options(&[(
            "folders",
            FieldValue::List(vec![FieldValue::String("raw-folder".to_owned())]),
        )]);

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::File(SchemaFileField::default()),
        );
        let field_type =
            SchemaFileField::parse(&mut parser, &opts, Some(&base));
        let errors = parser.finish(&opts);

        assert!(errors.is_empty());
        assert_eq!(
            field_type,
            SchemaFieldType::File(SchemaFileField::for_test(
                vec!["raw-folder".to_owned()],
                Some("base-ext".to_owned()),
                vec!["base-class".to_owned()],
            ))
        );
    }

    #[test]
    fn rejects_unknown_key() {
        let opts = options(&[("bogus", FieldValue::String("x".to_owned()))]);

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::File(SchemaFileField::default()),
        );
        let _ = SchemaFileField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], SchemaFieldParserError::UnknownKey { .. }));
    }

    #[test]
    fn returns_type_mismatch_when_folders_is_not_a_list() {
        let opts = options(&[(
            "folders",
            FieldValue::String("not-a-list".to_owned()),
        )]);

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::File(SchemaFileField::default()),
        );
        let _ = SchemaFileField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
            SchemaFieldParserError::TypeMismatch { .. }
        ));
    }

    #[test]
    fn returns_type_mismatch_when_ext_is_not_a_string() {
        let opts = options(&[("ext", FieldValue::Int(123))]);

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::File(SchemaFileField::default()),
        );
        let _ = SchemaFileField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
            SchemaFieldParserError::TypeMismatch { .. }
        ));
    }
}
