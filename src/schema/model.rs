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
#[derive(Clone, Debug, PartialEq)]
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
#[derive(Clone, Debug, PartialEq)]
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
            | FieldOptions::Number {
                ..
            }
            | FieldOptions::Date {
                ..
            }
            | FieldOptions::File {
                ..
            } => None,
        }
    }

    /// Return this file field's `FileIndex` filter parts, or `None` for
    /// every non-`file` field type.
    #[inline]
    #[must_use]
    pub(crate) fn file_filter(&self) -> Option<SchemaFileFieldFilter<'_>> {
        match &self.options {
            FieldOptions::File {
                folders,
                ext,
                class,
            } => Some(SchemaFileFieldFilter {
                folders,
                ext: ext.as_deref(),
                class,
            }),
            FieldOptions::Input
            | FieldOptions::Select {
                ..
            }
            | FieldOptions::Boolean
            | FieldOptions::Number {
                ..
            }
            | FieldOptions::Date {
                ..
            } => None,
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

/// Represent type-specific field options.
///
/// Pairs each [`FieldType`] with the options only that type carries: a
/// `select` field without `values`, or a `date` field with a stray `folders`
/// list, cannot be represented. `select` and `file` are the only list-bearing
/// kinds; `number` carries `step`/`min`/`max` and `date` a `format`; the rest
/// are unit variants.
#[derive(Clone, Debug, PartialEq)]
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
    Number {
        /// Inclusive minimum; `None` when unset.
        min: Option<f64>,
        /// Inclusive maximum; `None` when unset.
        max: Option<f64>,
        /// Increment step for the numeric value; `None` when unset.
        step: Option<f64>,
    },
    /// Accept a date value.
    Date {
        /// Display/parse format (strftime); `None` when unset.
        format: Option<String>,
    },
    /// Accept a link to files matched by folder, extension, and class filters.
    File {
        folders: Vec<String>,
        ext: Option<String>,
        class: Vec<String>,
    },
}

impl FieldOptions {
    /// Build options for `field_type` from `raw`'s keys, falling back to
    /// `base`'s options for any key `raw` leaves unset. `base: None` builds
    /// fresh options with no fallback: every key `raw` leaves unset defaults
    /// to empty.
    ///
    /// `base` is only consulted when it is `Some` of the same [`FieldType`];
    /// a `$ref` that switches type, or a field with no base at all, starts
    /// from empty options instead of reusing a mismatched base. For example,
    /// a `select`'s `values` never leaks into an overriding `file` field.
    ///
    /// # Examples
    ///
    /// A `select` field inheriting from a parent with `values = ["draft",
    /// "done"]`, where the child only overrides `required`:
    ///
    /// - `raw.values` is `None`: falls back to parent's `["draft", "done"]`.
    /// - `raw.values` is `Some(["todo"])`: uses `["todo"]`.
    pub(super) fn build(
        field_type: FieldType,
        raw: &RawFieldDef,
        base: Option<&Self>,
    ) -> Self {
        match field_type {
            FieldType::Input => Self::Input,
            FieldType::Select => Self::Select {
                values: raw.values.clone().unwrap_or_else(|| match base {
                    Some(Self::Select {
                        values,
                    }) => values.clone(),
                    _ => Vec::new(),
                }),
            },
            FieldType::Boolean => Self::Boolean,
            FieldType::Number => {
                let (base_min, base_max, base_step) = match base {
                    Some(Self::Number {
                        min,
                        max,
                        step,
                    }) => (*min, *max, *step),
                    _ => (None, None, None),
                };
                Self::Number {
                    min: raw.min.or(base_min),
                    max: raw.max.or(base_max),
                    step: raw.step.or(base_step),
                }
            }
            FieldType::Date => Self::Date {
                format: raw.format.clone().or_else(|| match base {
                    Some(Self::Date {
                        format,
                    }) => format.clone(),
                    _ => None,
                }),
            },
            FieldType::File => Self::File {
                folders: raw.folders.clone().unwrap_or_else(|| match base {
                    Some(Self::File {
                        folders,
                        ..
                    }) => folders.clone(),
                    _ => Vec::new(),
                }),
                ext: raw.ext.clone().or_else(|| match base {
                    Some(Self::File {
                        ext,
                        ..
                    }) => ext.clone(),
                    _ => None,
                }),
                class: raw.class.clone().unwrap_or_else(|| match base {
                    Some(Self::File {
                        class,
                        ..
                    }) => class.clone(),
                    _ => Vec::new(),
                }),
            },
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
            Self::Number {
                ..
            } => FieldType::Number,
            Self::Date {
                ..
            } => FieldType::Date,
            Self::File {
                ..
            } => FieldType::File,
        }
    }
}

/// Borrow a `file` field's `FileIndex` filter parts.
pub(crate) struct SchemaFileFieldFilter<'a> {
    pub(crate) folders: &'a [String],
    pub(crate) ext: Option<&'a str>,
    pub(crate) class: &'a [String],
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
            #[case::number(FieldOptions::Number { min: None, max: None, step: None })]
            #[case::date(FieldOptions::Date { format: None })]
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
        mod build {
            mod without_base {
                use pretty_assertions::assert_eq;
                use rstest::rstest;

                use super::super::super::super::*;

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
                    FieldOptions::Number { min: None, max: None, step: None }
                )]
                #[case::date(
                    FieldType::Date,
                    RawFieldDef::direct(RawFieldType::Input),
                    FieldOptions::Date { format: None }
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
                    assert_eq!(
                        FieldOptions::build(field_type, &raw, None),
                        expected
                    );
                }

                #[test]
                fn select_defaults_to_empty_values_when_raw_omits_them() {
                    let options = FieldOptions::build(
                        FieldType::Select,
                        &RawFieldDef::direct(RawFieldType::Input),
                        None,
                    );

                    assert_eq!(options, FieldOptions::Select {
                        values: Vec::new()
                    });
                }

                #[test]
                fn number_uses_raws_bounds_when_present() {
                    let raw = RawFieldDef {
                        min: Some(0.0),
                        max: Some(1.0),
                        step: Some(0.25),
                        ..RawFieldDef::direct(RawFieldType::Number)
                    };

                    let options =
                        FieldOptions::build(FieldType::Number, &raw, None);

                    assert_eq!(options, FieldOptions::Number {
                        min: Some(0.0),
                        max: Some(1.0),
                        step: Some(0.25),
                    });
                }

                #[test]
                fn date_uses_raws_format_when_present() {
                    let raw = RawFieldDef {
                        format: Some("%Y".to_owned()),
                        ..RawFieldDef::direct(RawFieldType::Date)
                    };

                    let options =
                        FieldOptions::build(FieldType::Date, &raw, None);

                    assert_eq!(options, FieldOptions::Date {
                        format: Some("%Y".to_owned()),
                    });
                }

                #[test]
                fn file_defaults_to_empty_filter_fields_when_raw_omits_them() {
                    let options = FieldOptions::build(
                        FieldType::File,
                        &RawFieldDef::direct(RawFieldType::Input),
                        None,
                    );

                    assert_eq!(options, FieldOptions::File {
                        folders: Vec::new(),
                        ext: None,
                        class: Vec::new()
                    });
                }
            }

            mod with_base {
                use pretty_assertions::assert_eq;

                use super::super::super::super::*;

                #[test]
                fn select_uses_raws_values_when_present() {
                    let base = FieldOptions::Select {
                        values: vec!["old".to_owned()],
                    };
                    let raw = RawFieldDef {
                        values: Some(vec!["new".to_owned()]),
                        ..RawFieldDef::direct(RawFieldType::Input)
                    };

                    let merged = FieldOptions::build(
                        FieldType::Select,
                        &raw,
                        Some(&base),
                    );

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

                    let merged = FieldOptions::build(
                        FieldType::Select,
                        &raw,
                        Some(&base),
                    );

                    assert_eq!(merged, base);
                }

                #[test]
                fn select_falls_back_to_empty_when_base_is_not_select() {
                    let base = FieldOptions::Input;
                    let raw = RawFieldDef::direct(RawFieldType::Input);

                    let merged = FieldOptions::build(
                        FieldType::Select,
                        &raw,
                        Some(&base),
                    );

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

                    let merged =
                        FieldOptions::build(FieldType::File, &raw, Some(&base));

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

                    let merged =
                        FieldOptions::build(FieldType::File, &raw, Some(&base));

                    assert_eq!(merged, base);
                }

                #[test]
                fn file_falls_back_to_empty_when_base_is_not_file() {
                    let base = FieldOptions::Input;
                    let raw = RawFieldDef::direct(RawFieldType::Input);

                    let merged =
                        FieldOptions::build(FieldType::File, &raw, Some(&base));

                    assert_eq!(merged, FieldOptions::File {
                        folders: Vec::new(),
                        ext: None,
                        class: Vec::new()
                    });
                }

                #[test]
                fn file_fields_fall_back_independently_per_subfield() {
                    // Each of folders/ext/class resolves raw-vs-base on its
                    // own: overriding one must not force the others to fall
                    // back too.
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

                    let merged =
                        FieldOptions::build(FieldType::File, &raw, Some(&base));

                    assert_eq!(merged, FieldOptions::File {
                        folders: vec!["raw-folder".to_owned()],
                        ext: Some("base-ext".to_owned()),
                        class: vec!["base-class".to_owned()],
                    });
                }

                #[test]
                fn non_list_types_ignore_base_and_default_to_the_bare_variant()
                {
                    let base = FieldOptions::Select {
                        values: vec!["leaked?".to_owned()],
                    };
                    let raw = RawFieldDef::direct(RawFieldType::Input);

                    let merged = FieldOptions::build(
                        FieldType::Input,
                        &raw,
                        Some(&base),
                    );

                    assert_eq!(merged, FieldOptions::Input);
                }

                #[test]
                fn number_uses_raws_bounds_over_the_base() {
                    let base = FieldOptions::Number {
                        min: Some(0.0),
                        max: Some(10.0),
                        step: Some(1.0),
                    };
                    let raw = RawFieldDef {
                        min: Some(1.0),
                        max: Some(5.0),
                        step: Some(0.5),
                        ..RawFieldDef::direct(RawFieldType::Number)
                    };

                    let merged = FieldOptions::build(
                        FieldType::Number,
                        &raw,
                        Some(&base),
                    );

                    assert_eq!(merged, FieldOptions::Number {
                        min: Some(1.0),
                        max: Some(5.0),
                        step: Some(0.5),
                    });
                }

                #[test]
                fn number_falls_back_to_bases_bounds_when_raw_omits_them() {
                    let base = FieldOptions::Number {
                        min: Some(0.0),
                        max: Some(10.0),
                        step: Some(1.0),
                    };
                    let raw = RawFieldDef::direct(RawFieldType::Number);

                    let merged = FieldOptions::build(
                        FieldType::Number,
                        &raw,
                        Some(&base),
                    );

                    assert_eq!(merged, base);
                }

                #[test]
                fn date_uses_raws_format_over_the_base() {
                    let base = FieldOptions::Date {
                        format: Some("%Y".to_owned()),
                    };
                    let raw = RawFieldDef {
                        format: Some("%Y-%m-%d".to_owned()),
                        ..RawFieldDef::direct(RawFieldType::Date)
                    };

                    let merged =
                        FieldOptions::build(FieldType::Date, &raw, Some(&base));

                    assert_eq!(merged, FieldOptions::Date {
                        format: Some("%Y-%m-%d".to_owned()),
                    });
                }

                #[test]
                fn date_falls_back_to_bases_format_when_raw_omits_it() {
                    let base = FieldOptions::Date {
                        format: Some("%Y".to_owned()),
                    };
                    let raw = RawFieldDef::direct(RawFieldType::Date);

                    let merged =
                        FieldOptions::build(FieldType::Date, &raw, Some(&base));

                    assert_eq!(merged, base);
                }
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
            #[case::number(FieldOptions::Number { min: None, max: None, step: None }, FieldType::Number)]
            #[case::date(FieldOptions::Date { format: None }, FieldType::Date)]
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
