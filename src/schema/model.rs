//! Store resolved Schema domain types.
//!
//! [`Schema`] holds effective [`FieldDefinition`]s after inheritance,
//! `excludes`, and `$ref` are applied. Each [`FieldDefinition`] pairs
//! type-specific [`FieldOptions`] with `required`/`multi` flags.
//!
//! Construction stays `pub(super)`: only [`super::resolve`] builds these; the
//! rest of the crate reads them through `pub(crate)` accessors.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    name::SchemaName,
    raw::{RawFieldDef, RawFieldType},
};
use crate::field::FieldName;

/// Store one Schema's effective [`FieldDefinition`]s.
///
/// Fields are resolved after inheritance, `excludes`, and `$ref` application.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Schema {
    name: SchemaName,
    fields: BTreeMap<FieldName, FieldDefinition>,
    /// Transitive `extends` targets, filtered to targets that resolved (a
    /// missing target never reaches here; see
    /// [`super::error::SchemaWarning::MissingExtendsTarget`]).
    ancestors: BTreeSet<SchemaName>,
}

impl Schema {
    /// Build a resolved Schema from already-merged parts.
    pub(super) fn new(
        name: SchemaName,
        fields: BTreeMap<FieldName, FieldDefinition>,
        ancestors: BTreeSet<SchemaName>,
    ) -> Self {
        Self {
            name,
            fields,
            ancestors,
        }
    }

    /// Return the Schema name from the source file stem.
    #[inline]
    #[must_use]
    pub(crate) fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Return this Schema's effective Field Definitions, keyed by name.
    #[inline]
    #[must_use]
    pub(crate) fn fields(&self) -> &BTreeMap<FieldName, FieldDefinition> {
        &self.fields
    }

    /// Return the named Field Definition, or `None` if it does not resolve for
    /// this Schema.
    #[inline]
    #[must_use]
    pub(crate) fn field(&self, name: &str) -> Option<&FieldDefinition> {
        self.fields.get(name)
    }

    /// Return this Schema's transitive `extends` ancestors, used by
    /// `resolve::build_schema` to accumulate a child's own ancestor set from
    /// its parents'.
    #[inline]
    #[must_use]
    pub(super) fn ancestors(&self) -> &BTreeSet<SchemaName> {
        &self.ancestors
    }

    /// Test whether this Schema is-a queried class name.
    ///
    /// The `ancestors` set includes all transitive `extends` targets that
    /// resolved during [`super::resolve::resolve`], so this check covers
    /// indirect inheritance chains, such as `sci_fi` to `book` to `thing`.
    ///
    /// # Examples
    ///
    /// A `sci_fi` Schema that transitively extends `book`:
    ///
    /// - `sci_fi.is_a("sci_fi")` returns `true` for itself.
    /// - `sci_fi.is_a("book")` returns `true` for an ancestor.
    /// - `sci_fi.is_a("movie")` returns `false` for an unrelated class.
    #[inline]
    #[must_use]
    pub(crate) fn is_a(&self, queried: &str) -> bool {
        self.name.as_str() == queried || self.ancestors.contains(queried)
    }
}

/// Store one resolved field definition.
///
/// `required` and `multi` are currently inert; reserved for future LSP/MCP
/// guardrails.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FieldDefinition {
    options: FieldOptions,
    required: bool,
    multi: bool,
}

impl FieldDefinition {
    /// Build a resolved field definition from already-merged parts.
    pub(super) fn new(
        options: FieldOptions,
        required: bool,
        multi: bool,
    ) -> Self {
        Self {
            options,
            required,
            multi,
        }
    }

    /// Return this field's type-specific options.
    #[inline]
    #[must_use]
    pub(crate) fn options(&self) -> &FieldOptions {
        &self.options
    }

    /// Return this field's static selectable values for the `schema`
    /// minijinja namespace's `.field()` method, or `None` if this field type
    /// has none to offer without consulting the file index.
    ///
    /// By field type:
    ///
    /// - `select`: returns the declared `values` list.
    /// - `file`: returns `None` here because options resolve live from the
    ///   `FileIndex`; use [`Self::file_filter`] for its index filter.
    /// - All other types: `None`.
    #[inline]
    #[must_use]
    pub(crate) fn selectable_values(&self) -> Option<&[String]> {
        match &self.options {
            FieldOptions::Select {
                values,
            } => Some(values),
            FieldOptions::Input
            | FieldOptions::Boolean
            | FieldOptions::Number
            | FieldOptions::Date
            | FieldOptions::File {
                ..
            } => None,
        }
    }

    /// Return this file field's `FileIndex` filter parts.
    ///
    /// The tuple is `(folders, ext, class)`. Returns `None` for every
    /// non-`file` field type.
    #[inline]
    #[must_use]
    pub(crate) fn file_filter(&self) -> Option<FileFilterParts<'_>> {
        match &self.options {
            FieldOptions::File {
                folders,
                ext,
                class,
            } => Some((folders, ext.as_deref(), class)),
            FieldOptions::Input
            | FieldOptions::Select {
                ..
            }
            | FieldOptions::Boolean
            | FieldOptions::Number
            | FieldOptions::Date => None,
        }
    }

    /// Return `true` if this field must be set. Always `false` on the reserved
    /// Global Schema, regardless of its own TOML.
    #[inline]
    #[must_use]
    pub(crate) fn is_required(&self) -> bool {
        self.required
    }

    /// Return `true` if this field accepts multiple values.
    #[inline]
    #[must_use]
    pub(crate) fn is_multi(&self) -> bool {
        self.multi
    }
}

/// Borrow `(folders, ext, class)` filter parts from a `file` field.
type FileFilterParts<'a> = (&'a [String], Option<&'a str>, &'a [String]);

/// Represent type-specific field options.
///
/// Pairs each [`FieldType`] with the options only that type carries: a
/// `select` field without `values`, or a `date` field with a stray `folders`
/// list, cannot be represented. `select` and `file` are the only list-bearing
/// kinds; every other variant is a unit variant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FieldOptions {
    /// Accept free-form text input.
    Input,
    /// Accept one value from a configured list.
    Select {
        values: Vec<String>,
    },
    /// Accept a boolean value.
    Boolean,
    /// Accept a numeric value.
    Number,
    /// Accept a date value.
    Date,
    /// Accept a link to files matched by folder, extension, and class filters.
    File {
        folders: Vec<String>,
        ext: Option<String>,
        class: Vec<String>,
    },
}

impl FieldOptions {
    /// Build fresh options for `field_type` from `raw`'s own keys, with no
    /// base definition to fall back on.
    ///
    /// Absent keys default to empty.
    pub(super) fn from_raw(field_type: FieldType, raw: &RawFieldDef) -> Self {
        match field_type {
            FieldType::Input => Self::Input,
            FieldType::Select => Self::Select {
                values: raw.values.clone().unwrap_or_default(),
            },
            FieldType::Boolean => Self::Boolean,
            FieldType::Number => Self::Number,
            FieldType::Date => Self::Date,
            FieldType::File => Self::File {
                folders: raw.folders.clone().unwrap_or_default(),
                ext: raw.ext.clone(),
                class: raw.class.clone().unwrap_or_default(),
            },
        }
    }

    /// Build options for `field_type` from `raw`'s keys, falling back to
    /// `base`'s options for any key `raw` leaves unset.
    ///
    /// `base` is only consulted when it is the same [`FieldType`]; a `$ref`
    /// that switches type starts with empty options instead of reusing a
    /// mismatched base. For example, a `select`'s `values` never leaks into
    /// an overriding `file` field.
    ///
    /// # Examples
    ///
    /// A `select` field inheriting from a parent with `values = ["draft",
    /// "done"]`, where the child only overrides `required`:
    ///
    /// - `raw.values` is `None`: falls back to parent's `["draft", "done"]`.
    /// - `raw.values` is `Some(["todo"])`: uses `["todo"]`.
    pub(super) fn merged(
        base: &Self,
        field_type: FieldType,
        raw: &RawFieldDef,
    ) -> Self {
        match field_type {
            FieldType::Select => Self::Select {
                values: raw.values.clone().unwrap_or_else(|| match base {
                    Self::Select {
                        values,
                    } => values.clone(),
                    _ => Vec::new(),
                }),
            },
            FieldType::File => Self::File {
                folders: raw.folders.clone().unwrap_or_else(|| match base {
                    Self::File {
                        folders,
                        ..
                    } => folders.clone(),
                    _ => Vec::new(),
                }),
                ext: raw.ext.clone().or_else(|| match base {
                    Self::File {
                        ext,
                        ..
                    } => ext.clone(),
                    _ => None,
                }),
                class: raw.class.clone().unwrap_or_else(|| match base {
                    Self::File {
                        class,
                        ..
                    } => class.clone(),
                    _ => Vec::new(),
                }),
            },
            other => Self::from_raw(other, raw),
        }
    }

    /// Return the [`FieldType`] this variant represents.
    #[inline]
    #[must_use]
    pub(super) fn kind(&self) -> FieldType {
        match self {
            Self::Input => FieldType::Input,
            Self::Select {
                ..
            } => FieldType::Select,
            Self::Boolean => FieldType::Boolean,
            Self::Number => FieldType::Number,
            Self::Date => FieldType::Date,
            Self::File {
                ..
            } => FieldType::File,
        }
    }
}

/// Represent a field kind after `$ref` resolution.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum FieldType {
    /// Free-form text input.
    Input,
    /// Configured selectable values.
    Select,
    /// Boolean value.
    Boolean,
    /// Numeric value.
    Number,
    /// Date value.
    Date,
    /// File link with optional filters.
    File,
}

impl From<RawFieldType> for FieldType {
    #[inline]
    fn from(raw: RawFieldType) -> Self {
        match raw {
            RawFieldType::Input => Self::Input,
            RawFieldType::Select => Self::Select,
            RawFieldType::Boolean => Self::Boolean,
            RawFieldType::Number => Self::Number,
            RawFieldType::Date => Self::Date,
            RawFieldType::File => Self::File,
        }
    }
}

#[cfg(test)]
mod tests {
    mod schema {
        use std::collections::{BTreeMap, BTreeSet};

        use pretty_assertions::assert_eq;

        use super::super::*;

        fn field(options: FieldOptions) -> FieldDefinition {
            FieldDefinition::new(options, false, false)
        }

        #[test]
        fn new_stores_the_given_name_fields_and_ancestors() {
            let mut fields = BTreeMap::new();
            fields.insert(
                FieldName::try_from("title").expect("valid test field name"),
                field(FieldOptions::Input),
            );
            let mut ancestors = BTreeSet::new();
            ancestors.insert(SchemaName::from("thing"));

            let schema = Schema::new(
                SchemaName::from("book"),
                fields.clone(),
                ancestors.clone(),
            );

            assert_eq!(schema.name(), "book");
            assert_eq!(schema.fields(), &fields);
            assert_eq!(schema.ancestors(), &ancestors);
        }

        #[test]
        fn field_returns_the_named_definition_when_present() {
            let mut fields = BTreeMap::new();
            fields.insert(
                FieldName::try_from("title").expect("valid test field name"),
                field(FieldOptions::Input),
            );
            let schema =
                Schema::new(SchemaName::from("book"), fields, BTreeSet::new());

            assert_eq!(
                schema.field("title"),
                Some(&field(FieldOptions::Input))
            );
        }

        #[test]
        fn field_returns_none_when_the_name_is_absent() {
            let schema = Schema::new(
                SchemaName::from("book"),
                BTreeMap::new(),
                BTreeSet::new(),
            );

            assert_eq!(schema.field("missing"), None);
        }

        #[test]
        fn is_a_matches_when_queried_equals_its_own_name() {
            let schema = Schema::new(
                SchemaName::from("book"),
                BTreeMap::new(),
                BTreeSet::new(),
            );

            assert!(schema.is_a("book"));
        }

        #[test]
        fn is_a_matches_when_queried_is_a_transitive_ancestor() {
            let mut ancestors = BTreeSet::new();
            ancestors.insert(SchemaName::from("thing"));
            let schema = Schema::new(
                SchemaName::from("book"),
                BTreeMap::new(),
                ancestors,
            );

            assert!(schema.is_a("thing"));
        }

        #[test]
        fn is_a_does_not_match_an_unrelated_name() {
            let schema = Schema::new(
                SchemaName::from("book"),
                BTreeMap::new(),
                BTreeSet::new(),
            );

            assert!(!schema.is_a("movie"));
        }
    }

    mod field_definition {
        use pretty_assertions::assert_eq;

        use super::super::*;

        #[test]
        fn new_stores_the_given_options_required_and_multi() {
            let definition =
                FieldDefinition::new(FieldOptions::Boolean, true, true);

            assert_eq!(definition.options(), &FieldOptions::Boolean);
            assert!(definition.is_required());
            assert!(definition.is_multi());
        }

        mod selectable_values {
            use pretty_assertions::assert_eq;
            use rstest::rstest;

            use super::super::super::*;

            #[test]
            fn returns_the_values_list_for_a_select_field() {
                let field = FieldDefinition::new(
                    FieldOptions::Select {
                        values: vec!["draft".to_owned(), "done".to_owned()],
                    },
                    false,
                    false,
                );

                assert_eq!(
                    field.selectable_values(),
                    Some(["draft".to_owned(), "done".to_owned()].as_slice())
                );
            }

            #[test]
            fn returns_an_empty_slice_for_a_select_field_with_no_values() {
                let field = FieldDefinition::new(
                    FieldOptions::Select {
                        values: Vec::new(),
                    },
                    false,
                    false,
                );

                assert_eq!(field.selectable_values(), Some([].as_slice()));
            }

            #[rstest]
            #[case::input(FieldOptions::Input)]
            #[case::boolean(FieldOptions::Boolean)]
            #[case::number(FieldOptions::Number)]
            #[case::date(FieldOptions::Date)]
            #[case::file(FieldOptions::File {
                folders: vec!["assets".to_owned()],
                ext: Some("png".to_owned()),
                class: vec!["image".to_owned()],
            })]
            fn returns_none_for_a_non_select_field_type(
                #[case] options: FieldOptions,
            ) {
                let field = FieldDefinition::new(options, false, false);

                assert_eq!(field.selectable_values(), None);
            }
        }
    }

    mod field_options {
        mod from_raw {
            use pretty_assertions::assert_eq;
            use rstest::rstest;

            use super::super::super::*;

            #[rstest]
            #[case::input(
                FieldType::Input,
                RawFieldDef::direct(RawFieldType::Input),
                FieldOptions::Input
            )]
            #[case::select(
                FieldType::Select,
                RawFieldDef { values: Some(vec!["draft".to_owned(), "done".to_owned()]), ..RawFieldDef::direct(RawFieldType::Input) },
                FieldOptions::Select { values: vec!["draft".to_owned(), "done".to_owned()] }
            )]
            #[case::boolean(
                FieldType::Boolean,
                RawFieldDef::direct(RawFieldType::Input),
                FieldOptions::Boolean
            )]
            #[case::number(
                FieldType::Number,
                RawFieldDef::direct(RawFieldType::Input),
                FieldOptions::Number
            )]
            #[case::date(
                FieldType::Date,
                RawFieldDef::direct(RawFieldType::Input),
                FieldOptions::Date
            )]
            #[case::file(
                FieldType::File,
                RawFieldDef {
                    folders: Some(vec!["assets".to_owned()]),
                    ext: Some("png".to_owned()),
                    class: Some(vec!["image".to_owned()]),
                    ..RawFieldDef::direct(RawFieldType::Input)
                },
                FieldOptions::File {
                    folders: vec!["assets".to_owned()],
                    ext: Some("png".to_owned()),
                    class: vec!["image".to_owned()],
                }
            )]
            fn maps_each_field_type_to_its_own_options(
                #[case] field_type: FieldType,
                #[case] raw: RawFieldDef,
                #[case] expected: FieldOptions,
            ) {
                assert_eq!(FieldOptions::from_raw(field_type, &raw), expected);
            }

            #[test]
            fn select_defaults_to_empty_values_when_raw_omits_them() {
                let options = FieldOptions::from_raw(
                    FieldType::Select,
                    &RawFieldDef::direct(RawFieldType::Input),
                );

                assert_eq!(options, FieldOptions::Select {
                    values: Vec::new()
                });
            }

            #[test]
            fn file_defaults_to_empty_filter_fields_when_raw_omits_them() {
                let options = FieldOptions::from_raw(
                    FieldType::File,
                    &RawFieldDef::direct(RawFieldType::Input),
                );

                assert_eq!(options, FieldOptions::File {
                    folders: Vec::new(),
                    ext: None,
                    class: Vec::new()
                });
            }
        }

        mod merged {
            use pretty_assertions::assert_eq;

            use super::super::super::*;

            #[test]
            fn select_uses_raws_values_when_present() {
                let base = FieldOptions::Select {
                    values: vec!["old".to_owned()],
                };
                let raw = RawFieldDef {
                    values: Some(vec!["new".to_owned()]),
                    ..RawFieldDef::direct(RawFieldType::Input)
                };

                let merged =
                    FieldOptions::merged(&base, FieldType::Select, &raw);

                assert_eq!(merged, FieldOptions::Select {
                    values: vec!["new".to_owned()]
                });
            }

            #[test]
            fn select_falls_back_to_bases_values_when_raw_omits_them() {
                let base = FieldOptions::Select {
                    values: vec!["old".to_owned()],
                };
                let raw = RawFieldDef::direct(RawFieldType::Input);

                let merged =
                    FieldOptions::merged(&base, FieldType::Select, &raw);

                assert_eq!(merged, base);
            }

            #[test]
            fn select_falls_back_to_empty_when_base_is_not_select() {
                let base = FieldOptions::Input;
                let raw = RawFieldDef::direct(RawFieldType::Input);

                let merged =
                    FieldOptions::merged(&base, FieldType::Select, &raw);

                assert_eq!(merged, FieldOptions::Select {
                    values: Vec::new()
                });
            }

            #[test]
            fn file_uses_raws_fields_when_present() {
                let base = FieldOptions::File {
                    folders: vec!["old".to_owned()],
                    ext: Some("old".to_owned()),
                    class: vec!["old".to_owned()],
                };
                let raw = RawFieldDef {
                    folders: Some(vec!["new".to_owned()]),
                    ext: Some("new".to_owned()),
                    class: Some(vec!["new".to_owned()]),
                    ..RawFieldDef::direct(RawFieldType::Input)
                };

                let merged = FieldOptions::merged(&base, FieldType::File, &raw);

                assert_eq!(merged, FieldOptions::File {
                    folders: vec!["new".to_owned()],
                    ext: Some("new".to_owned()),
                    class: vec!["new".to_owned()],
                });
            }

            #[test]
            fn file_falls_back_to_bases_fields_when_raw_omits_them() {
                let base = FieldOptions::File {
                    folders: vec!["old".to_owned()],
                    ext: Some("old".to_owned()),
                    class: vec!["old".to_owned()],
                };
                let raw = RawFieldDef::direct(RawFieldType::Input);

                let merged = FieldOptions::merged(&base, FieldType::File, &raw);

                assert_eq!(merged, base);
            }

            #[test]
            fn file_falls_back_to_empty_when_base_is_not_file() {
                let base = FieldOptions::Input;
                let raw = RawFieldDef::direct(RawFieldType::Input);

                let merged = FieldOptions::merged(&base, FieldType::File, &raw);

                assert_eq!(merged, FieldOptions::File {
                    folders: Vec::new(),
                    ext: None,
                    class: Vec::new()
                });
            }

            #[test]
            fn file_fields_fall_back_independently_per_subfield() {
                // Each of folders/ext/class resolves raw-vs-base on its own:
                // overriding one must not force the others to fall back too.
                let base = FieldOptions::File {
                    folders: vec!["base-folder".to_owned()],
                    ext: Some("base-ext".to_owned()),
                    class: vec!["base-class".to_owned()],
                };
                let raw = RawFieldDef {
                    folders: Some(vec!["raw-folder".to_owned()]),
                    ext: None,
                    class: None,
                    ..RawFieldDef::direct(RawFieldType::Input)
                };

                let merged = FieldOptions::merged(&base, FieldType::File, &raw);

                assert_eq!(merged, FieldOptions::File {
                    folders: vec!["raw-folder".to_owned()],
                    ext: Some("base-ext".to_owned()),
                    class: vec!["base-class".to_owned()],
                });
            }

            #[test]
            fn non_list_types_ignore_base_and_delegate_to_from_raw() {
                let base = FieldOptions::Select {
                    values: vec!["leaked?".to_owned()],
                };
                let raw = RawFieldDef::direct(RawFieldType::Input);

                let merged =
                    FieldOptions::merged(&base, FieldType::Input, &raw);

                assert_eq!(merged, FieldOptions::Input);
            }
        }

        mod kind {
            use pretty_assertions::assert_eq;
            use rstest::rstest;

            use super::super::super::*;

            #[rstest]
            #[case::input(FieldOptions::Input, FieldType::Input)]
            #[case::select(FieldOptions::Select { values: Vec::new() }, FieldType::Select)]
            #[case::boolean(FieldOptions::Boolean, FieldType::Boolean)]
            #[case::number(FieldOptions::Number, FieldType::Number)]
            #[case::date(FieldOptions::Date, FieldType::Date)]
            #[case::file(
                FieldOptions::File { folders: Vec::new(), ext: None, class: Vec::new() },
                FieldType::File
            )]
            fn returns_the_field_type_matching_the_variant(
                #[case] options: FieldOptions,
                #[case] expected: FieldType,
            ) {
                assert_eq!(options.kind(), expected);
            }
        }
    }

    mod field_type {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::super::*;

        #[rstest]
        #[case::input(RawFieldType::Input, FieldType::Input)]
        #[case::select(RawFieldType::Select, FieldType::Select)]
        #[case::boolean(RawFieldType::Boolean, FieldType::Boolean)]
        #[case::number(RawFieldType::Number, FieldType::Number)]
        #[case::date(RawFieldType::Date, FieldType::Date)]
        #[case::file(RawFieldType::File, FieldType::File)]
        fn from_raw_field_type_maps_each_variant(
            #[case] raw: RawFieldType,
            #[case] expected: FieldType,
        ) {
            assert_eq!(FieldType::from(raw), expected);
        }
    }
}
