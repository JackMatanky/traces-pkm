//! Schema registry and Field Resolution.
//!
//! The filesystem is the Schema registry: a Schema is a TOML file under
//! `.traces/schemas/` whose filename stem is the Schema name (spec User
//! Story 1). [`SchemaRegistry::load`] reads that directory and resolves the
//! `extends` DAG via [`resolve::resolve`], the crate's pure Field Resolution
//! engine (Kahn's topological sort, own-fields-override-parents,
//! first-listed-wins, `excludes`, bounded `$ref`).
//!
//! # Main Types
//!
//! - [`SchemaRegistry`]: Reads and resolves every Schema under a directory.
//! - [`ResolvedSchema`]: One Schema's effective Field Definitions plus its
//!   transitive `extends` ancestors for is-a matching.
//! - [`resolve::FieldDefinition`] / [`resolve::FieldOptions`] /
//!   [`resolve::FieldType`]: A resolved field's shape.
//! - [`SchemaError`] / [`SchemaWarning`]: Hard failures and recoverable
//!   degrades (see [`resolve::resolve`]'s doc comment for which is which).
//!
//! # Out of Scope
//!
//! The `schema` minijinja namespace, `file`-field `FileIndex` resolution, and
//! `query.from_class`/`tasks.from_class` consume this registry but are built
//! in later tickets
//! (`.scratch/metadata-schemas/issues/{03,04,05}-*.md`).

mod error;
mod raw;
mod resolve;

use std::{collections::BTreeMap, ffi::OsStr, fs, io, path::Path};

pub(crate) use error::{SchemaError, SchemaWarning};
use raw::RawSchema;
pub(crate) use resolve::ResolvedSchema;

/// The reserved Global Schema name (`global.toml`).
///
/// A never-required reference pool: forbidden as a Note's File Class value
/// (enforced by the class-query consumer, ticket 05) and its own `required =
/// true` fields degrade to `false` with a
/// [`SchemaWarning::StrayGlobalRequired`] during resolution.
pub(crate) const GLOBAL_SCHEMA_NAME: &str = "global";

/// Every Schema under a registry directory, resolved through `extends`,
/// `excludes`, and `$ref`.
#[derive(Clone, Debug)]
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
pub(crate) struct SchemaRegistry {
    schemas: BTreeMap<String, ResolvedSchema>,
}

impl SchemaRegistry {
    /// Reads every `*.toml` file directly under `directory` (non-recursive),
    /// parses each as a Schema keyed by its filename stem, and resolves the
    /// `extends` DAG.
    ///
    /// A missing `directory` resolves to an empty registry rather than an
    /// error, matching the lazy-validation model (spec Implementation
    /// Decisions: "a broken Schema only breaks the Template that touches
    /// it") — an unconfigured or not-yet-created registry is not "broken".
    ///
    /// # Errors
    ///
    /// - [`SchemaError::ReadDirectory`] if `directory` exists but its entries
    ///   cannot be listed.
    /// - [`SchemaError::ReadFile`] if a `.toml` file cannot be read.
    /// - [`SchemaError::Parse`] if a Schema file's TOML is malformed or
    ///   contains an unknown key.
    /// - Any error [`resolve::resolve`] returns while linearizing the `extends`
    ///   DAG.
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
    pub(crate) fn load(
        directory: &Path,
    ) -> Result<(Self, Vec<SchemaWarning>), SchemaError> {
        let raw = read_raw_schemas(directory)?;
        let (schemas, warnings) = resolve::resolve(&raw)?;
        Ok((
            Self {
                schemas,
            },
            warnings,
        ))
    }

    /// Returns the named Schema, or `None` if no Schema by that name
    /// resolved.
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
    pub(crate) fn get(&self, name: &str) -> Option<&ResolvedSchema> {
        self.schemas.get(name)
    }

    /// Returns `true` if `class` is-a `queried` (spec User Story 18).
    ///
    /// A `class` absent from the registry degrades to an exact-string match
    /// against `queried`, mirroring the spec's "a class with no Schema
    /// degrades to exact match" fallback for `from_class` (ticket 05).
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
    pub(crate) fn is_a(&self, class: &str, queried: &str) -> bool {
        self.get(class)
            .map_or_else(|| class == queried, |schema| schema.is_a(queried))
    }
}

/// Reads and parses every `*.toml` file directly under `directory` into a
/// [`RawSchema`] keyed by filename stem.
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
fn read_raw_schemas(
    directory: &Path,
) -> Result<BTreeMap<String, RawSchema>, SchemaError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Ok(BTreeMap::new());
        }
        Err(source) => {
            return Err(SchemaError::ReadDirectory {
                directory: directory.to_path_buf(),
                source,
            });
        }
    };

    let mut schemas = BTreeMap::new();
    for entry in entries {
        let entry = entry.map_err(|source| SchemaError::ReadDirectory {
            directory: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(OsStr::to_str) != Some("toml") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(OsStr::to_str) else {
            continue;
        };
        let stem = stem.to_owned();
        let contents = fs::read_to_string(&path).map_err(|source| {
            SchemaError::ReadFile {
                path: path.clone(),
                source,
            }
        })?;
        let raw: RawSchema =
            toml::from_str(&contents).map_err(|source| SchemaError::Parse {
                schema: stem.clone(),
                source: Box::new(source),
            })?;
        schemas.insert(stem, raw);
    }
    Ok(schemas)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn write_schema(dir: &Path, name: &str, toml: &str) {
        fs::write(dir.join(format!("{name}.toml")), toml)
            .expect("write schema fixture");
    }

    #[test]
    fn parses_a_schema_directory_keyed_by_filename_stem() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_schema(
            temp.path(),
            "book",
            r#"
            [fields.status]
            type = "select"
            values = ["draft", "done"]
            "#,
        );

        let (registry, warnings) =
            SchemaRegistry::load(temp.path()).expect("registry loads");

        assert!(warnings.is_empty());
        let book = registry.get("book").expect("book resolved");
        assert_eq!(book.name(), "book");
        assert!(book.field("status").is_some());
    }

    #[test]
    fn ignores_non_toml_files_in_the_registry_directory() {
        let temp = tempfile::tempdir().expect("create temp dir");
        fs::write(temp.path().join("README.md"), "not a schema")
            .expect("write non-schema file");

        let (registry, _) =
            SchemaRegistry::load(temp.path()).expect("registry loads");

        assert!(registry.get("README").is_none());
    }

    #[test]
    fn a_missing_registry_directory_resolves_to_an_empty_registry() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let missing = temp.path().join("does-not-exist");

        let (registry, warnings) =
            SchemaRegistry::load(&missing).expect("missing dir is not fatal");

        assert!(warnings.is_empty());
        assert!(registry.get("anything").is_none());
    }

    #[test]
    fn rejects_an_unknown_top_level_key_at_parse() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_schema(temp.path(), "book", "typo_key = true\n");

        let err = SchemaRegistry::load(temp.path())
            .expect_err("unknown key rejected");

        assert!(matches!(err, SchemaError::Parse { .. }));
    }

    #[test]
    fn rejects_an_unknown_field_key_at_parse() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_schema(
            temp.path(),
            "book",
            r#"
            [fields.status]
            type = "select"
            values = ["draft"]
            typo_key = true
            "#,
        );

        let err = SchemaRegistry::load(temp.path())
            .expect_err("unknown field key rejected");

        assert!(matches!(err, SchemaError::Parse { .. }));
    }

    #[test]
    fn is_a_degrades_to_exact_match_for_a_class_with_no_schema() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let (registry, _) =
            SchemaRegistry::load(temp.path()).expect("registry loads");

        assert!(registry.is_a("ghost", "ghost"));
        assert!(!registry.is_a("ghost", "book"));
    }

    #[test]
    fn is_a_matches_transitively_through_the_registry() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_schema(temp.path(), "book", "");
        write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

        let (registry, _) =
            SchemaRegistry::load(temp.path()).expect("registry loads");

        assert!(registry.is_a("sci_fi", "book"));
    }
}
