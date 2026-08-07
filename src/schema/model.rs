//! Resolved Schema domain types: [`Schema`], its [`FieldDefinition`]s, and
//! their type-specific [`FieldOptions`].
//!
//! These are the *output* shapes [`super::resolve::resolve`] produces from a
//! parsed [`super::raw::RawSchema`] set — inheritance, `excludes`, and `$ref`
//! already applied. Construction stays `pub(super)`: only [`super::resolve`]
//! builds these; the rest of the crate only reads them through the `pub(crate)`
//! accessors below.

use std::collections::{BTreeMap, BTreeSet};

use super::raw::{RawFieldDef, RawFieldType};

/// A Field Definition's kind, mirroring [`RawFieldType`] after `$ref`
/// resolution has settled on one.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "declared by the schema-registry ticket; consumed by the \
                  schema-namespace ticket \
                  (.scratch/metadata-schemas/issues/\
                  03-schema-minijinja-namespace.md)"
    )
)]
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

/// Type-specific Field Definition options.
///
/// Pairs each [`FieldType`] with the options only that type carries, so a
/// `select` field without `values` or a `date` field with a stray `folders`
/// list cannot be represented. `select` and `file` are the only list-bearing
/// kinds (spec User Story 9): every other variant is a unit variant.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "declared by the schema-registry ticket; consumed by the \
                  schema-namespace ticket \
                  (.scratch/metadata-schemas/issues/\
                  03-schema-minijinja-namespace.md)"
    )
)]
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
    /// Returns the [`FieldType`] this variant represents.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
    pub(super) fn field_type(&self) -> FieldType {
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

    /// Builds fresh options for `field_type` from `raw`'s own keys, with no
    /// base definition to fall back on. Absent keys default to empty.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
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
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
    pub(super) fn merged(
        base: &Self,
        field_type: FieldType,
        raw: &RawFieldDef,
    ) -> Self {
        match field_type {
            FieldType::Select => {
                let base_values = match base {
                    Self::Select {
                        values,
                    } => values.clone(),
                    _ => Vec::new(),
                };
                Self::Select {
                    values: raw.values.clone().unwrap_or(base_values),
                }
            }
            FieldType::File => {
                let (base_folders, base_ext, base_class) = match base {
                    Self::File {
                        folders,
                        ext,
                        class,
                    } => (folders.clone(), ext.clone(), class.clone()),
                    _ => (Vec::new(), None, Vec::new()),
                };
                Self::File {
                    folders: raw.folders.clone().unwrap_or(base_folders),
                    ext: raw.ext.clone().or(base_ext),
                    class: raw.class.clone().unwrap_or(base_class),
                }
            }
            other => Self::from_raw(other, raw),
        }
    }
}

/// One resolved Field Definition: its type-specific [`FieldOptions`] plus
/// `required`/`multi` flags.
///
/// `required`/`multi` are declared for future LSP/MCP guardrails (spec User
/// Story 3) and stay inert here.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "declared by the schema-registry ticket; consumed by the \
                  schema-namespace ticket \
                  (.scratch/metadata-schemas/issues/\
                  03-schema-minijinja-namespace.md)"
    )
)]
pub(crate) struct FieldDefinition {
    options: FieldOptions,
    required: bool,
    multi: bool,
}

impl FieldDefinition {
    /// Builds a resolved Field Definition from its already-merged parts.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
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
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
    pub(crate) fn options(&self) -> &FieldOptions {
        &self.options
    }

    /// Returns `true` if this field must be set. Always `false` on the
    /// reserved Global Schema, regardless of its own TOML.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
    pub(crate) fn is_required(&self) -> bool {
        self.required
    }

    /// Returns `true` if this field accepts multiple values.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
    pub(crate) fn is_multi(&self) -> bool {
        self.multi
    }
}

/// A Schema's effective Field Definitions after inheritance, `excludes`, and
/// `$ref` are applied.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "declared by the schema-registry ticket; consumed by the \
                  schema-namespace ticket \
                  (.scratch/metadata-schemas/issues/\
                  03-schema-minijinja-namespace.md)"
    )
)]
pub(crate) struct Schema {
    name: String,
    fields: BTreeMap<String, FieldDefinition>,
    /// Transitive `extends` targets, filtered to targets that resolved (a
    /// missing target never reaches here; see
    /// [`super::error::SchemaWarning::MissingExtendsTarget`]).
    ancestors: BTreeSet<String>,
}

impl Schema {
    /// Builds a resolved Schema from its already-merged parts.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
    pub(super) fn new(
        name: String,
        fields: BTreeMap<String, FieldDefinition>,
        ancestors: BTreeSet<String>,
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
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// Returns this Schema's effective Field Definitions, keyed by name.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
    pub(crate) fn fields(&self) -> &BTreeMap<String, FieldDefinition> {
        &self.fields
    }

    /// Returns the named Field Definition, or `None` if it does not resolve
    /// for this Schema.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
    pub(crate) fn field(&self, name: &str) -> Option<&FieldDefinition> {
        self.fields.get(name)
    }

    /// Returns this Schema's transitive `extends` ancestors, used by
    /// `resolve::resolve_one` to accumulate a child's own ancestor set from
    /// its parents'.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
    pub(super) fn ancestors(&self) -> &BTreeSet<String> {
        &self.ancestors
    }

    /// Returns `true` if this Schema is-a `queried`: equal, or `queried` is a
    /// transitive `extends` ancestor (spec User Story 18).
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
        self.name == queried || self.ancestors.contains(queried)
    }
}
