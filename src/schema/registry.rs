//! Reads and resolves every Schema under a registry directory.
//!
//! The filesystem is the Schema registry: a Schema is a TOML file whose
//! filename stem is the Schema name (spec User Story 1). [`SchemaRegistry`]
//! is the impure edge of the `schema` module — it walks a directory and
//! parses TOML — everything past that (inheritance, `excludes`, `$ref`) is
//! [`super::resolve::resolve`], a pure function tested with no filesystem at
//! all.

use std::{collections::BTreeMap, ffi::OsStr, fs, io, path::Path};

use walkdir::WalkDir;

use super::{
    error::{SchemaError, SchemaWarning},
    model::Schema,
    raw::RawSchema,
    resolve,
};
use crate::file_name::BaseNameRef;

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
    schemas: BTreeMap<String, Schema>,
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
    pub(crate) fn get(&self, name: &str) -> Option<&Schema> {
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
///
/// Walks only `directory`'s immediate entries (`min_depth(1).max_depth(1)`):
/// Schemas do not nest. A `directory` that does not exist yields an empty
/// map rather than [`SchemaError::ReadDirectory`].
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
    let mut schemas = BTreeMap::new();
    let entries = WalkDir::new(directory).min_depth(1).max_depth(1);
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(source) if is_missing_root(&source) => {
                return Ok(BTreeMap::new());
            }
            Err(source) => return Err(walk_error(directory, source)),
        };
        let path = entry.path();
        if path.extension().and_then(OsStr::to_str) != Some("toml") {
            continue;
        }
        let Some(stem) = BaseNameRef::from_path(path) else {
            continue;
        };
        let stem = stem.as_str().to_owned();
        let contents = fs::read_to_string(path).map_err(|source| {
            SchemaError::ReadFile {
                path: path.to_path_buf(),
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

/// Returns `true` if `error` reports that the walk's root itself does not
/// exist, so [`read_raw_schemas`] can degrade to an empty registry.
fn is_missing_root(error: &walkdir::Error) -> bool {
    error.depth() == 0
        && error
            .io_error()
            .is_some_and(|source| source.kind() == io::ErrorKind::NotFound)
}

/// Wraps a [`walkdir::Error`] with path context as a
/// [`SchemaError::ReadDirectory`].
///
/// Falls back to `directory` if the underlying error provides no path (such
/// as rare symlink loop errors).
fn walk_error(directory: &Path, source: walkdir::Error) -> SchemaError {
    let path = source.path().unwrap_or(directory).to_path_buf();
    SchemaError::ReadDirectory {
        directory: path,
        source: source.into(),
    }
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

    #[test]
    fn ignores_a_nested_subdirectory_of_the_registry() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let nested = temp.path().join("nested");
        fs::create_dir(&nested).expect("create nested dir");
        write_schema(&nested, "hidden", "");

        let (registry, _) =
            SchemaRegistry::load(temp.path()).expect("registry loads");

        assert!(registry.get("hidden").is_none());
    }
}
