//! Resolved Schema domain types: [`Schema`], its [`FieldDefinition`]s, and
//! their type-specific [`FieldOptions`].
//!
//! These are the *output* shapes [`super::resolve::resolve`] produces from a
//! parsed [`super::raw::RawSchema`] set (inheritance, `excludes`, and `$ref`
//! already applied). Construction stays `pub(super)`: only
//! [`super::resolve`] builds these; the rest of the crate only reads them
//! through the `pub(crate)` accessors below.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    name::SchemaName,
    raw::{RawFieldDef, RawFieldType},
};

/// A Schema's effective Field Definitions after inheritance, `excludes`, and
/// `$ref` are applied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Schema {
    name: SchemaName,
    fields: BTreeMap<String, FieldDefinition>,
    /// Transitive `extends` targets, filtered to targets that resolved (a
    /// missing target never reaches here; see
    /// [`super::error::SchemaWarning::MissingExtendsTarget`]).
    ancestors: BTreeSet<SchemaName>,
}

impl Schema {
    /// Builds a resolved Schema from its already-merged parts.
    pub(super) fn new(
        name: SchemaName,
        fields: BTreeMap<String, FieldDefinition>,
        ancestors: BTreeSet<SchemaName>,
    ) -> Self {
        Self {
            name,
            fields,
            ancestors,
        }
    }

    /// Returns the Schema name (the source file's stem).
    #[inline]
    #[must_use]
    pub(crate) fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Returns this Schema's effective Field Definitions, keyed by name.
    #[inline]
    #[must_use]
    pub(crate) fn fields(&self) -> &BTreeMap<String, FieldDefinition> {
        &self.fields
    }

    /// Returns the named Field Definition, or `None` if it does not resolve
    /// for this Schema.
    #[inline]
    #[must_use]
    pub(crate) fn field(&self, name: &str) -> Option<&FieldDefinition> {
        self.fields.get(name)
    }

    /// Returns this Schema's transitive `extends` ancestors, used by
    /// `resolve::resolve_one` to accumulate a child's own ancestor set from
    /// its parents'.
    #[inline]
    #[must_use]
    pub(super) fn ancestors(&self) -> &BTreeSet<SchemaName> {
        &self.ancestors
    }

    /// Returns `true` if this Schema is-a `queried`: equal, or `queried` is a
    /// transitive `extends` ancestor.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      class-queries ticket \
                      (.scratch/metadata-schemas/issues/05-class-queries.md)"
        )
    )]
    pub(crate) fn is_a(&self, queried: &str) -> bool {
        self.name.as_str() == queried || self.ancestors.contains(queried)
    }
}

/// One resolved Field Definition: its type-specific [`FieldOptions`] plus
/// `required`/`multi` flags.
///
/// `required` and `multi` are reserved for future LSP/MCP guardrails and
/// stay inert here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FieldDefinition {
    options: FieldOptions,
    required: bool,
    multi: bool,
}

impl FieldDefinition {
    /// Builds a resolved Field Definition from its already-merged parts.
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

    /// Returns this field's type-specific options.
    #[inline]
    #[must_use]
    pub(crate) fn options(&self) -> &FieldOptions {
        &self.options
    }

    /// Returns this field's selectable values for the `schema` minijinja
    /// namespace's `.field()` method, or `None` if this field type has none
    /// to offer.
    ///
    /// Only `select` carries a plain value list today. `file` is also
    /// list-bearing in principle, but its options resolve live from the
    /// `FileIndex`, which this method does not yet consult; until that's
    /// wired up, it returns `None` here too rather than a value list it
    /// cannot yet honor.
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

    /// Returns `true` if this field must be set. Always `false` on the
    /// reserved Global Schema, regardless of its own TOML.
    #[inline]
    #[must_use]
    pub(crate) fn is_required(&self) -> bool {
        self.required
    }

    /// Returns `true` if this field accepts multiple values.
    #[inline]
    #[must_use]
    pub(crate) fn is_multi(&self) -> bool {
        self.multi
    }
}

/// Type-specific Field Definition options.
///
/// Pairs each [`FieldType`] with the options only that type carries, so a
/// `select` field without `values` or a `date` field with a stray `folders`
/// list cannot be represented. `select` and `file` are the only list-bearing
/// kinds: every other variant is a unit variant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FieldOptions {
    Input,
    Select {
        values: Vec<String>,
    },
    Boolean,
    Number,
    Date,
    File {
        folders: Vec<String>,
        ext: Option<String>,
        class: Vec<String>,
    },
}

impl FieldOptions {
    /// Builds fresh options for `field_type` from `raw`'s own keys, with no
    /// base definition to fall back on. Absent keys default to empty.
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

    /// Builds options for `field_type` from `raw`'s keys, falling back to
    /// `base`'s options for any key `raw` leaves unset. `base`'s options are
    /// consulted only when `base` is already the same [`FieldType`]; a `$ref`
    /// that switches type starts from empty options instead of reusing a
    /// mismatched base (for example a `select`'s `values` never leaks into an
    /// overriding `file` field).
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

    /// Returns the [`FieldType`] this variant represents.
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

/// A Field Definition's kind, mirroring [`RawFieldType`] after `$ref`
/// resolution has settled on one.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum FieldType {
    Input,
    Select,
    Boolean,
    Number,
    Date,
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
            fields.insert("title".to_owned(), field(FieldOptions::Input));
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
            fields.insert("title".to_owned(), field(FieldOptions::Input));
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
                RawFieldDef::default(),
                FieldOptions::Input
            )]
            #[case::select(
                FieldType::Select,
                RawFieldDef { values: Some(vec!["draft".to_owned(), "done".to_owned()]), ..RawFieldDef::default() },
                FieldOptions::Select { values: vec!["draft".to_owned(), "done".to_owned()] }
            )]
            #[case::boolean(
                FieldType::Boolean,
                RawFieldDef::default(),
                FieldOptions::Boolean
            )]
            #[case::number(
                FieldType::Number,
                RawFieldDef::default(),
                FieldOptions::Number
            )]
            #[case::date(
                FieldType::Date,
                RawFieldDef::default(),
                FieldOptions::Date
            )]
            #[case::file(
                FieldType::File,
                RawFieldDef {
                    folders: Some(vec!["assets".to_owned()]),
                    ext: Some("png".to_owned()),
                    class: Some(vec!["image".to_owned()]),
                    ..RawFieldDef::default()
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
                    &RawFieldDef::default(),
                );

                assert_eq!(options, FieldOptions::Select {
                    values: Vec::new()
                });
            }

            #[test]
            fn file_defaults_to_empty_filter_fields_when_raw_omits_them() {
                let options = FieldOptions::from_raw(
                    FieldType::File,
                    &RawFieldDef::default(),
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
                    ..RawFieldDef::default()
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
                let raw = RawFieldDef::default();

                let merged =
                    FieldOptions::merged(&base, FieldType::Select, &raw);

                assert_eq!(merged, base);
            }

            #[test]
            fn select_falls_back_to_empty_when_base_is_not_select() {
                let base = FieldOptions::Input;
                let raw = RawFieldDef::default();

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
                    ..RawFieldDef::default()
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
                let raw = RawFieldDef::default();

                let merged = FieldOptions::merged(&base, FieldType::File, &raw);

                assert_eq!(merged, base);
            }

            #[test]
            fn file_falls_back_to_empty_when_base_is_not_file() {
                let base = FieldOptions::Input;
                let raw = RawFieldDef::default();

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
                    ..RawFieldDef::default()
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
                let raw = RawFieldDef::default();

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
