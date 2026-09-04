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

use std::sync::Arc;

use indexmap::IndexMap;

use super::{SchemaFieldType, parser::SchemaFieldParser};
use crate::FieldValue;

/// Resolved `file` field options.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SchemaFileField {
    folders: Vec<String>,
    ext: Option<String>,
    class: Vec<String>,
}

impl SchemaFileField {
    /// Return the matched folder paths.
    #[inline]
    #[must_use]
    #[cfg(test)]
    pub(crate) fn folders(&self) -> &[String] {
        &self.folders
    }

    /// Return the matched file extension, if set.
    #[inline]
    #[must_use]
    #[cfg(test)]
    pub(crate) fn ext(&self) -> Option<&str> {
        self.ext.as_deref()
    }

    /// Return the matched class tags.
    #[inline]
    #[must_use]
    #[cfg(test)]
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
    pub(crate) const fn for_test(
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
        options: &IndexMap<String, FieldValue>,
        base: Option<&Self>,
    ) -> SchemaFieldType {
        let folders = parser.string_list(
            options,
            "folders",
            base.map(|b| b.folders.clone()).unwrap_or_default(),
        );
        let ext =
            parser.string(options, "ext", base.and_then(|b| b.ext.clone()));
        let class = parser.string_list(
            options,
            "class",
            base.map(|b| b.class.clone()).unwrap_or_default(),
        );

        SchemaFieldType::File(Arc::new(Self {
            folders,
            ext,
            class,
        }))
    }
}

/// A borrowed view of a resolved `file` field's filter parts.
///
/// Returned by
/// [`SchemaFieldDef::file_filter`](super::SchemaFieldDef::file_filter).
#[derive(Copy, Clone)]
pub(crate) struct SchemaFileFieldRef<'a> {
    pub(crate) folders: &'a [String],
    pub(crate) ext: Option<&'a str>,
    pub(crate) class: &'a [String],
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use super::*;
    use crate::schema::fields::{
        SchemaFieldTypeTag, address::FieldAddress,
        error::SchemaFieldParserError, parser::SchemaFieldParser,
    };

    mod accessors {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_configured_folders() {
            let field = SchemaFileField {
                folders: vec!["notes".to_owned(), "docs".to_owned()],
                ext: None,
                class: vec![],
            };
            assert_eq!(field.folders(), &["notes", "docs"]);
        }

        #[test]
        fn returns_configured_ext() {
            let field = SchemaFileField {
                folders: vec![],
                ext: Some("md".to_owned()),
                class: vec![],
            };
            assert_eq!(field.ext(), Some("md"));
        }

        #[test]
        fn returns_configured_class() {
            let field = SchemaFileField {
                folders: vec![],
                ext: None,
                class: vec!["project".to_owned(), "active".to_owned()],
            };
            assert_eq!(field.class(), &["project", "active"]);
        }

        #[test]
        fn returns_empty_vec_for_unset_folders() {
            let field = SchemaFileField::default();
            assert!(field.folders().is_empty());
        }

        #[test]
        fn returns_none_for_unset_ext() {
            let field = SchemaFileField::default();
            assert_eq!(field.ext(), None);
        }

        #[test]
        fn returns_empty_vec_for_unset_class() {
            let field = SchemaFileField::default();
            assert!(field.class().is_empty());
        }
    }

    fn address() -> FieldAddress {
        FieldAddress::try_from("#book/field").expect("valid ref")
    }

    fn options(pairs: &[(&str, FieldValue)]) -> IndexMap<String, FieldValue> {
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
        let mut parser =
            SchemaFieldParser::new(addr.as_ref(), SchemaFieldTypeTag::File);
        let field_type = SchemaFileField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert!(errors.is_empty());
        assert_eq!(
            field_type,
            SchemaFieldType::File(Arc::new(SchemaFileField::for_test(
                vec!["assets".to_owned()],
                Some("png".to_owned()),
                vec!["image".to_owned()],
            )))
        );
    }

    #[test]
    fn falls_back_independently_per_subfield() {
        let base = SchemaFileField::for_test(
            vec!["base-folder".to_owned()],
            Some("base-ext".to_owned()),
            vec!["base-class".to_owned()],
        );
        let opts = options(&[(
            "folders",
            FieldValue::List(vec![FieldValue::String("raw-folder".to_owned())]),
        )]);

        let addr = address();
        let mut parser =
            SchemaFieldParser::new(addr.as_ref(), SchemaFieldTypeTag::File);
        let field_type =
            SchemaFileField::parse(&mut parser, &opts, Some(&base));
        let errors = parser.finish(&opts);

        assert!(errors.is_empty());
        assert_eq!(
            field_type,
            SchemaFieldType::File(Arc::new(SchemaFileField::for_test(
                vec!["raw-folder".to_owned()],
                Some("base-ext".to_owned()),
                vec!["base-class".to_owned()],
            )))
        );
    }

    #[test]
    fn rejects_unknown_key() {
        let opts = options(&[("bogus", FieldValue::String("x".to_owned()))]);

        let addr = address();
        let mut parser =
            SchemaFieldParser::new(addr.as_ref(), SchemaFieldTypeTag::File);
        let _ = SchemaFileField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors.first().expect("expected error"),
            SchemaFieldParserError::UnknownKey { .. }
        ));
    }

    #[rstest]
    #[case::folders_not_a_list("folders", FieldValue::String("not-a-list".to_owned()))]
    #[case::ext_not_a_string("ext", FieldValue::Int(123))]
    fn returns_type_mismatch_for_wrong_value_shape(
        #[case] key: &str,
        #[case] value: FieldValue,
    ) {
        let opts = options(&[(key, value)]);

        let addr = address();
        let mut parser =
            SchemaFieldParser::new(addr.as_ref(), SchemaFieldTypeTag::File);
        let _ = SchemaFileField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors.first().expect("expected error"),
            SchemaFieldParserError::TypeMismatch { .. }
        ));
    }
}
