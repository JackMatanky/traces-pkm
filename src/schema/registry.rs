//! Read and resolve every Schema under a registry directory.
//!
//! The filesystem is the Schema registry: a Schema is a TOML file whose
//! filename stem is the Schema name. [`SchemaRegistry`] is the impure edge of
//! the `schema` module: it walks a directory and parses TOML. Everything past
//! that (inheritance, `excludes`, `$ref`) is [`super::resolve::resolve`], a
//! pure function tested with no filesystem at all.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs, io,
    path::Path,
    sync::Arc,
};

use walkdir::WalkDir;

use super::{
    GLOBAL_SCHEMA_NAME,
    error::{SchemaError, SchemaWarning},
    model::Schema,
    name::SchemaName,
    raw::RawSchema,
    resolve,
};
use crate::{
    file_name::BaseNameRef,
    query::{ClassExpansionMode, QuerySource, SourceAtom},
};

/// Store every resolved Schema under a registry directory.
#[derive(Clone, Debug)]
pub(crate) struct SchemaRegistry {
    /// Reference-counted per Schema, not owned outright: `.get()` and
    /// `.descendants_of()` share one Schema's field map across every caller in
    /// a render instead of deep-cloning it per lookup, mirroring
    /// [`crate::query::IndexRecord`]'s `Arc<Note>`.
    schemas: BTreeMap<SchemaName, Arc<Schema>>,
    direct_children: BTreeMap<SchemaName, BTreeSet<SchemaName>>,
}

impl SchemaRegistry {
    /// Load every Schema TOML file directly under `dir`.
    ///
    /// Reads every `*.toml` file directly under `dir` (non-recursive),
    /// parses each as a Schema keyed by its filename stem, and resolves the
    /// `extends` DAG.
    ///
    /// A missing `dir` resolves to an empty registry rather than an
    /// error: an unconfigured or not-yet-created Schema directory is absence,
    /// not corruption.
    ///
    /// # Errors
    ///
    /// - [`SchemaError::ReadDirectory`] if `dir` exists but its entries cannot
    ///   be listed.
    /// - [`SchemaError::ReadFile`] if a `.toml` file cannot be read.
    /// - [`SchemaError::Parse`] if a Schema file's TOML is malformed, contains
    ///   an unknown key, has a malformed `$ref`, or defines a field with
    ///   neither `type` nor `$ref`.
    /// - [`SchemaError::Cycle`] if the `extends` DAG contains a cycle.
    /// - [`SchemaError::RefOutOfBounds`] if a `$ref` target is outside the
    ///   Global Schema and transitive `extends` ancestor bound.
    /// - [`SchemaError::RefFieldNotFound`] if a `$ref` target is in bounds but
    ///   the target field does not exist.
    /// - [`SchemaError::AmbiguousFieldName`] if two effective fields share the
    ///   same canonical metadata key.
    pub(crate) fn load(
        dir: &Path,
    ) -> Result<(Self, Vec<SchemaWarning>), SchemaError> {
        let raw = read_raw_schemas(dir)?;
        let (schemas, warnings) = resolve::resolve(&raw)?;
        let direct_children = raw
            .iter()
            .flat_map(|(child, schema)| {
                schema
                    .extends
                    .iter()
                    .filter(|parent| {
                        parent.as_str() != GLOBAL_SCHEMA_NAME
                            && schemas.contains_key(*parent)
                    })
                    .map(|parent| (parent.clone(), child.clone()))
            })
            .fold(
                BTreeMap::<SchemaName, BTreeSet<SchemaName>>::new(),
                |mut children, (parent, child)| {
                    children.entry(parent).or_default().insert(child);
                    children
                },
            );
        let schemas = schemas
            .into_iter()
            .map(|(name, schema)| (name, Arc::new(schema)))
            .collect();
        Ok((
            Self {
                schemas,
                direct_children,
            },
            warnings,
        ))
    }

    /// Return a reference to the named Schema, or `None` if no Schema by that
    /// name resolved.
    ///
    /// Stored in [`Arc`] so repeated lookups (a Template calling `schema.get()`
    /// many times in one render) share the Schema's field map instead of
    /// deep-cloning per call.
    #[inline]
    #[must_use]
    pub(crate) fn get(&self, name: &str) -> Option<&Arc<Schema>> {
        self.schemas.get(name)
    }

    /// Return every Schema that directly or transitively extends `name`.
    ///
    /// Excludes `name` itself. Empty, not an error, if nothing extends `name`.
    #[must_use]
    pub(crate) fn descendants_of(&self, name: &str) -> Vec<Arc<Schema>> {
        self.schemas
            .values()
            .filter(|schema| schema.ancestors().contains(name))
            .cloned()
            .collect()
    }

    /// Return every Schema that directly extends `name`.
    ///
    /// Excludes `name` itself and all transitive descendants.
    #[must_use]
    pub(crate) fn children_of(&self, name: &str) -> Vec<Arc<Schema>> {
        self.direct_children
            .get(name)
            .into_iter()
            .flatten()
            .filter_map(|child| self.schemas.get(child))
            .cloned()
            .collect()
    }

    /// Populate `mode`'s match set from `classes` at its requested depth.
    ///
    /// Unknown class names remain in the set so a Note may still use them even
    /// without a corresponding Schema.
    pub(crate) fn expand_classes(
        &self,
        classes: &[String],
        mode: &mut ClassExpansionMode,
    ) {
        for class in classes {
            if self.get(class).is_none() {
                tracing::warn!(
                    class,
                    "query source names an unknown File Class; matching it \
                     exactly"
                );
            }
        }
        let mut expanded: BTreeSet<String> = classes.iter().cloned().collect();
        match mode {
            ClassExpansionMode::Exact(_) => {}
            ClassExpansionMode::Children(_) => {
                for class in classes {
                    expanded.extend(
                        self.children_of(class)
                            .iter()
                            .map(|schema| schema.name().to_owned()),
                    );
                }
            }
            ClassExpansionMode::Descendants(_) => {
                expanded = self.matches(classes);
            }
        }
        mode.set_classes(expanded);
    }

    /// Return the set of Schema names that match `classes`.
    ///
    /// The set includes:
    ///
    /// - Every name in `classes` itself (so a class with no Schema still
    ///   matches itself).
    /// - Every resolved Schema that is-a one of the class names.
    ///
    /// A File Class source query tests each Note's File Class against this set:
    /// a Note matches when any of its class values is in the returned set.
    /// Transitive `extends` is folded in here so the caller compares plain
    /// strings without consulting the registry per Note.
    ///
    /// # Examples
    ///
    /// Given `sci_fi` extending `book`, and `movie` unrelated:
    ///
    /// - `matches(&["book"])` returns `{"book", "sci_fi"}`.
    /// - `matches(&["movie"])` returns `{"movie"}`.
    /// - `matches(&["ghost"])` returns `{"ghost"}` (no Schema, still matches)
    #[must_use]
    pub(crate) fn matches(&self, classes: &[String]) -> BTreeSet<String> {
        let mut matches: BTreeSet<String> = classes.iter().cloned().collect();
        for (name, schema) in &self.schemas {
            if classes.iter().any(|class| schema.is_a(class)) {
                matches.insert(name.as_str().to_owned());
            }
        }
        matches
    }
}

/// Resolve every File Class leaf in `source` against `registry`.
///
/// This caller-side pre-pass keeps query parsing and matching independent of
/// the Schema registry.
pub(crate) fn resolve_sources(
    source: &mut QuerySource,
    registry: &SchemaRegistry,
) {
    let QuerySource::Expr(expression) = source else {
        return;
    };
    expression.visit_atoms_mut(&mut |atom| {
        if let SourceAtom::Class {
            names,
            mode,
        } = atom
        {
            registry.expand_classes(names, mode);
        }
    });
}

/// Read and parse every `*.toml` file directly under `dir` into a [`RawSchema`]
/// keyed by filename stem.
///
/// Walks only `dir`'s immediate entries (`min_depth(1).max_depth(1)`): Schemas
/// do not nest. A `dir` that does not exist yields an empty map rather than
/// [`SchemaError::ReadDirectory`].
///
/// # Errors
///
/// - [`SchemaError::ReadDirectory`] if `dir` exists but its entries cannot be
///   listed.
/// - [`SchemaError::ReadFile`] if a `.toml` file cannot be read.
/// - [`SchemaError::Parse`] if a Schema file's TOML is malformed, contains an
///   unknown key, has a malformed `$ref`, or omits both `type` and `$ref` for a
///   field.
fn read_raw_schemas(
    dir: &Path,
) -> Result<BTreeMap<SchemaName, RawSchema>, SchemaError> {
    let entries = WalkDir::new(dir).min_depth(1).max_depth(1);
    let mut schemas = BTreeMap::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(source) if is_missing_root(&source) => {
                return Ok(BTreeMap::new());
            }
            Err(source) => return Err(walk_error(dir, source)),
        };
        let path = entry.path();
        if path.extension().and_then(OsStr::to_str) != Some("toml") {
            continue;
        }
        let Some(stem) = BaseNameRef::from_path(path) else {
            continue;
        };
        let stem = SchemaName::from(stem.as_str());
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

/// Return `true` if `error` reports that the walk's root itself does not
/// exist, so [`read_raw_schemas`] can degrade to an empty registry.
fn is_missing_root(error: &walkdir::Error) -> bool {
    error.depth() == 0
        && error
            .io_error()
            .is_some_and(|source| source.kind() == io::ErrorKind::NotFound)
}

/// Wrap a [`walkdir::Error`] with path context as a
/// [`SchemaError::ReadDirectory`].
///
/// Falls back to `root` if the underlying error carries no path of its own
/// (some I/O errors surface without `DirEntry` context).
fn walk_error(root: &Path, source: walkdir::Error) -> SchemaError {
    let path = source.path().unwrap_or(root).to_path_buf();
    SchemaError::ReadDirectory {
        directory: path,
        source: source.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_schema(dir: &Path, name: &str, toml: &str) {
        fs::write(dir.join(format!("{name}.toml")), toml)
            .expect("write schema fixture");
    }

    mod load {
        use pretty_assertions::assert_eq;

        use super::*;

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
        fn resolves_to_an_empty_registry_when_the_directory_is_missing() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let missing = temp.path().join("does-not-exist");

            let (registry, warnings) = SchemaRegistry::load(&missing)
                .expect("missing dir is not fatal");

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
        fn rejects_a_malformed_ref_at_parse() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.status]
                "$ref" = "global/status"
                "#,
            );

            let err = SchemaRegistry::load(temp.path())
                .expect_err("malformed $ref rejected");

            assert!(matches!(err, SchemaError::Parse { .. }));
        }

        #[test]
        fn rejects_a_field_with_neither_type_nor_ref_at_parse() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.status]
                required = true
                "#,
            );

            let err = SchemaRegistry::load(temp.path())
                .expect_err("missing type/$ref rejected");

            assert!(matches!(err, SchemaError::Parse { .. }));
        }

        #[test]
        fn propagates_a_resolve_error_when_the_extends_dag_has_a_cycle() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "a", r#"extends = ["b"]"#);
            write_schema(temp.path(), "b", r#"extends = ["a"]"#);

            let err =
                SchemaRegistry::load(temp.path()).expect_err("cycle rejected");

            assert!(matches!(err, SchemaError::Cycle { .. }));
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

        #[cfg(unix)]
        #[test]
        fn returns_a_read_directory_error_when_the_registry_directory_is_unreadable()
         {
            use std::os::unix::fs::PermissionsExt as _;

            /// Restores a locked directory's permissions on drop, even if
            /// the test panics. Otherwise, a `0o000` directory blocks the
            /// tempdir's own cleanup.
            struct RestorePermissions<'a>(&'a Path);

            impl Drop for RestorePermissions<'_> {
                fn drop(&mut self) {
                    let _ = fs::set_permissions(
                        self.0,
                        fs::Permissions::from_mode(0o700),
                    );
                }
            }

            let temp = tempfile::tempdir().expect("create temp dir");
            let locked = temp.path().join("locked");
            fs::create_dir(&locked).expect("create locked dir");
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o000))
                .expect("revoke read permission");
            let _restore = RestorePermissions(&locked);

            let err = SchemaRegistry::load(&locked)
                .expect_err("unreadable directory fails");

            assert!(matches!(err, SchemaError::ReadDirectory { .. }));
        }

        #[cfg(unix)]
        #[test]
        fn returns_a_read_file_error_when_a_schema_file_is_unreadable() {
            use std::os::unix::fs::PermissionsExt as _;

            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            let file = temp.path().join("book.toml");
            fs::set_permissions(&file, fs::Permissions::from_mode(0o000))
                .expect("revoke read permission");

            let err = SchemaRegistry::load(temp.path())
                .expect_err("unreadable file fails");

            assert!(matches!(err, SchemaError::ReadFile { .. }));
        }
    }

    mod get {
        use super::*;

        #[test]
        fn returns_the_same_arc_backed_schema_on_repeated_calls() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.status]
                type = "select"
                values = ["draft"]
                "#,
            );
            let (registry, _) =
                SchemaRegistry::load(temp.path()).expect("registry loads");

            let first = registry.get("book").expect("book resolved");
            let second = registry.get("book").expect("book resolved");

            assert!(
                Arc::ptr_eq(first, second),
                "repeated lookups must share one Arc-backed Schema, not clone \
                 a fresh one per call"
            );
        }
    }

    mod matches {
        use std::collections::BTreeSet;

        use pretty_assertions::assert_eq;

        use super::*;

        fn set(names: &[&str]) -> BTreeSet<String> {
            names.iter().map(|name| (*name).to_owned()).collect()
        }

        #[test]
        fn includes_a_class_with_no_schema() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (registry, _) =
                SchemaRegistry::load(temp.path()).expect("registry loads");

            let matches = registry.matches(&["ghost".to_owned()]);

            assert_eq!(matches, set(&["ghost"]));
        }

        #[test]
        fn includes_transitive_subclasses_of_a_class() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let (registry, _) =
                SchemaRegistry::load(temp.path()).expect("registry loads");

            let matches = registry.matches(&["book".to_owned()]);

            assert_eq!(matches, set(&["book", "sci_fi"]));
        }

        #[test]
        fn excludes_classes_unrelated_to_the_class() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "movie", "");

            let (registry, _) =
                SchemaRegistry::load(temp.path()).expect("registry loads");

            let matches = registry.matches(&["book".to_owned()]);

            assert_eq!(matches, set(&["book"]));
        }

        #[test]
        fn unions_the_matches_of_every_class() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);
            write_schema(temp.path(), "movie", "");

            let (registry, _) =
                SchemaRegistry::load(temp.path()).expect("registry loads");

            let matches =
                registry.matches(&["book".to_owned(), "movie".to_owned()]);

            assert_eq!(matches, set(&["book", "movie", "sci_fi"]));
        }

        #[test]
        fn returns_an_empty_set_for_no_classes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");

            let (registry, _) =
                SchemaRegistry::load(temp.path()).expect("registry loads");

            let matches = registry.matches(&[]);

            assert!(matches.is_empty());
        }
    }

    mod expand_classes {
        use std::collections::BTreeSet;

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::query::{ClassExpansionMode, QuerySource, SourceAtom};

        fn set(names: &[&str]) -> BTreeSet<String> {
            names.iter().map(|name| (*name).to_owned()).collect()
        }

        fn registry() -> SchemaRegistry {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "thing", "");
            write_schema(temp.path(), "book", r#"extends = ["thing"]"#);
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);
            write_schema(temp.path(), "space_opera", r#"extends = ["sci_fi"]"#);
            SchemaRegistry::load(temp.path()).expect("registry loads").0
        }

        #[test]
        fn children_of_returns_only_direct_extenders() {
            let registry = registry();
            let names: Vec<String> = registry
                .children_of("thing")
                .into_iter()
                .map(|schema| schema.name().to_owned())
                .collect();

            assert_eq!(names, vec!["book".to_owned()]);
        }

        #[test]
        fn expansion_modes_are_incremental() {
            let registry = registry();
            let names = vec!["thing".to_owned()];
            let mut exact = ClassExpansionMode::Exact(BTreeSet::new());
            let mut children = ClassExpansionMode::Children(BTreeSet::new());
            let mut descendants =
                ClassExpansionMode::Descendants(BTreeSet::new());

            registry.expand_classes(&names, &mut exact);
            registry.expand_classes(&names, &mut children);
            registry.expand_classes(&names, &mut descendants);

            assert_eq!(exact.classes(), &set(&["thing"]));
            assert_eq!(children.classes(), &set(&["book", "thing"]));
            assert_eq!(
                descendants.classes(),
                &set(&["book", "sci_fi", "space_opera", "thing"])
            );
        }

        #[test]
        fn resolve_sources_walks_nested_expressions_and_preserves_unknowns() {
            let registry = registry();
            let mut source = QuerySource::parse("@thing+ and not @ghost*")
                .expect("source parses");

            resolve_sources(&mut source, &registry);

            let QuerySource::Expr(expression) = &mut source else {
                panic!("expected expression source");
            };
            let mut classes = Vec::new();
            expression.visit_atoms_mut(&mut |atom| {
                if let SourceAtom::Class {
                    names,
                    mode,
                } = atom
                {
                    classes.push((names.clone(), mode.classes().clone()));
                }
            });
            assert_eq!(classes, vec![
                (vec!["thing".to_owned()], set(&["book", "thing"]),),
                (vec!["ghost".to_owned()], set(&["ghost"])),
            ]);
        }
    }

    mod descendants_of {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_a_direct_extender() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let (registry, _) =
                SchemaRegistry::load(temp.path()).expect("registry loads");

            let descendants = registry.descendants_of("book");
            let names: Vec<&str> =
                descendants.iter().map(|schema| schema.name()).collect();

            assert_eq!(names, vec!["sci_fi"]);
        }

        #[test]
        fn returns_a_transitive_descendant_through_an_intermediate_schema() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "thing", "");
            write_schema(temp.path(), "book", r#"extends = ["thing"]"#);
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let (registry, _) =
                SchemaRegistry::load(temp.path()).expect("registry loads");

            let descendants = registry.descendants_of("thing");
            let names: Vec<&str> =
                descendants.iter().map(|schema| schema.name()).collect();

            assert_eq!(names, vec!["book", "sci_fi"]);
        }

        #[test]
        fn returns_empty_for_a_leaf_schema() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let (registry, _) =
                SchemaRegistry::load(temp.path()).expect("registry loads");

            assert!(registry.descendants_of("sci_fi").is_empty());
        }

        #[test]
        fn returns_empty_for_a_name_with_no_schema() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");

            let (registry, _) =
                SchemaRegistry::load(temp.path()).expect("registry loads");

            assert!(registry.descendants_of("ghost").is_empty());
        }
    }
}
