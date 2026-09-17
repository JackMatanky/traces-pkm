//! Process Obsidian-style Markdown notes with frontmatter, inline fields,
//! wikilinks, and task lists.
//!
//! # Modules
//!
//! - [`cli`] - Command-line interface definitions, argument parsing, and
//!   command execution flow.
//! - `config` - Project configuration loading, discovery, TOML parsing, and
//!   trust verification.
//! - `dialog` - Object-safe dialog prompts for interactive and preset input.
//! - `dirs` - XDG and platform-specific directory resolution for configuration
//!   and persistent state.
//! - `dirtree` - Directory-tree traversal with classified walk errors.
//! - `field` - Validated field-name and field-key primitives shared across note
//!   metadata and schemas.
//! - `file` - File metadata and file-name newtypes shared by the index,
//!   template, and schema layers.
//! - `file_class_expander` - Implements schema-aware File Class expansion for
//!   the query domain, without either depending on the other.
//! - `file_store` - Hash-keyed file and path state store using BLAKE3-named
//!   entries.
//! - `hash` - BLAKE3 hashing for file contents and canonicalized paths.
//! - `index` - Persistent file index, note parsing, and link graph
//!   construction.
//! - `query` - Read-side source selection, record projection, transformations,
//!   and output rendering.
//! - `note` - Markdown note parsing, YAML frontmatter extraction, and task
//!   processing.
//! - `path` - Root-relative path validation and confinement.
//! - `schema` - Schema registry and field resolution: parses
//!   `.traces/schemas/*.toml` and linearizes the `extends` DAG into effective
//!   field definitions.
//! - `template` - Template loading, path expansion, custom engine bindings, and
//!   note rendering.
//! # Key Types
//!
//! Always available:
//!
//! - [`Note`], [`ListItem`], [`TaskListItem`]: Parsed note and task records.
//! - [`Tag`], [`TagError`]: Validated Markdown tags.
//! - [`SourceLine`]: Strongly-typed 1-indexed source line numbers.
//! - [`TaskStatus`], [`TaskStatusMap`], [`TaskStatusType`]: Task status symbols
//!   and lookup tables.
//! - [`FileBase`]: Filesystem metadata for indexed files.
//! - [`DialogProvider`], [`PresetDialogProvider`], [`TerminalDialogProvider`]:
//!   Interactive and preset dialog prompts.
//! - [`parse_markdown`], [`MarkdownParserInput`]: Note parsing entry points.
//!
//! Available with the `test-utils` feature:
//!
//! - `Config`, `ConfigService`, `TrustRequest`: Project configuration and trust
//!   verification.
//! - `FileIndex`, `IndexerService`: Persistent index engine and services.
//! - `QueryBuilder`, `QuerySet`, `QueryService`: Query planning and execution.
//! - `Schema`, `SchemaService`: Schema registry and inheritance graph.
//! - `TemplateService`, `CommitPolicy`, `WriteMode`: Template rendering and
//!   file writes.
//! - `Blake3FileHash`, `Blake3PathHash`: BLAKE3 hashing primitives.

mod config;
mod date;
mod delimiter;
mod dialog;
mod dirs;
mod dirtree;
mod duration;
mod env_vars;
mod field;
mod file;
mod file_class_expander;
mod file_store;
mod hash;
mod index;
mod lexer;
mod note;
mod path;
mod position;
mod query;
mod schema;
mod strsim;
mod tag;
mod task;
mod template;

pub mod cli;

#[cfg(any(test, feature = "test-utils"))]
pub use config::{Config, ConfigService, TaskConfig, TrustRequest};
#[cfg(not(any(test, feature = "test-utils")))]
pub(crate) use config::{Config, ConfigService, TaskConfig, TrustRequest};
pub use date::DateValue;
pub(crate) use date::{
    DEFAULT_DATE_FORMAT, DEFAULT_DATETIME_FORMAT, DateTimeValue,
};
pub(crate) use delimiter::DelimiterType;
pub use dialog::{
    DialogError, DialogProvider, DialogResult, PresetDialogProvider,
    TerminalDialogProvider,
};
pub(crate) use dirtree::{DirTree, DirTreeError};
pub(crate) use duration::{DurationSeconds, DurationUnit, DurationValue};
pub(crate) use field::{
    FieldKey, FieldKeyRef, FieldName, FieldNameError, FieldNameRef, FieldValue,
};
pub use file::FileBase;
#[cfg(any(test, feature = "test-utils"))]
pub use file::FileFormat;
pub(crate) use file::{BaseName, BaseNameRef, FileName};
pub(crate) use file_store::{
    FileStateStore, FileStateStoreError, FileStoreCleanMode,
};
#[cfg(any(test, feature = "test-utils"))]
pub use hash::{Blake3FileHash, Blake3PathHash};
#[cfg(not(any(test, feature = "test-utils")))]
pub(crate) use hash::{Blake3FileHash, Blake3PathHash};
#[cfg(not(any(test, feature = "test-utils")))]
pub(crate) use index::IndexerService;
#[cfg(any(test, feature = "test-utils"))]
pub use index::{
    FileEntry, FileIndex, IndexerService, InlinkMap, SyncReport,
    path as path_codec,
};
pub(crate) use lexer::{
    LexError, LexTokenStream, LexedToken, TokenSpec, lexical_unquote,
};
pub(crate) use note::NoteFieldType;
pub use note::{
    ListItem, ListItemType, ListText, MarkdownParserInput, Note,
    NoteFieldValue, NoteFieldValueRef, TaskDates, TaskListItem, TaskPriority,
    descendants_of, parse_markdown,
};
pub(crate) use position::ByteOffset;
pub use position::SourceLine;
#[cfg(any(test, feature = "test-utils"))]
pub use query::{
    QueryBuilder, QueryResult, QueryRow, QueryService, QuerySet, SourceSelector,
};
#[cfg(any(test, feature = "test-utils"))]
pub use schema::{Schema, SchemaFieldDef, SchemaService, SchemaServiceError};
pub use tag::{Tag, TagError};
pub use task::{TaskStatus, TaskStatusMap, TaskStatusSymbol, TaskStatusType};
#[cfg(not(any(test, feature = "test-utils")))]
#[expect(unused_imports, reason = "crate re-export")]
pub(crate) use template::TemplateService;
#[cfg(any(test, feature = "test-utils"))]
pub use template::{
    CommitPolicy, RenderFailureKind, TemplatePathInput, TemplateService,
    WriteMode, WriteOutcome,
};
#[cfg(any(test, feature = "test-utils"))]
pub use test_support::{
    DEFAULT_SCHEMA_VALUES_DIR, DEFAULT_SCHEMAS_DIR, DEFAULT_TEMPLATES_DIR,
    TestProject, build_test_index, create_trusted_project, fixture_service,
    parse_note, parse_note_str, parse_tag, resolve_safe_path, write_note,
    write_schema, write_template,
};

/// Build isolated fixtures for the crate's own `#[cfg(test)]` suites and, under
/// the `test-utils` feature, for external `tests/`/`benches/` consumers.
///
/// - [`TestProject`] encapsulates an isolated workspace fixture managing paths,
///   configuration, trust records, schemas, templates, and on-disk index
///   persistence.
/// - [`fixture_service`] returns a [`ConfigService`] backed by temporary
///   directories.
/// - [`create_trusted_project`] writes a minimal config and trusts it.
/// - [`write_note`], [`write_template`], and [`write_schema`] write fixture
///   files.
/// - [`parse_note`], [`parse_note_str`], and [`build_test_index`] construct
///   in-memory notes and indexes with zero disk I/O.
/// - [`parse_tag`] parses tag string slices for fixture data.
#[cfg(any(test, feature = "test-utils"))]
mod test_support {
    #![expect(
        clippy::expect_used,
        reason = "fixture-only code; a failed .expect() here means the \
                  fixture itself is broken and should panic immediately"
    )]

    use std::{
        path::{Component, Path, PathBuf},
        sync::Arc,
    };

    use crate::{
        Config, ConfigService, FileIndex, IndexerService, MarkdownParserInput,
        Note, Tag,
        config::{Discovered, LocalConfigFile, TrustRequest},
        parse_markdown,
    };

    /// Default template directory relative to project root.
    pub const DEFAULT_TEMPLATES_DIR: &str = "templates";

    /// Default schemas directory relative to project root.
    pub const DEFAULT_SCHEMAS_DIR: &str = ".traces/schemas";

    /// Default schema values directory relative to project root.
    pub const DEFAULT_SCHEMA_VALUES_DIR: &str = ".traces/schemas/values";

    /// Encapsulates an isolated workspace fixture directory, configuration,
    /// trust records, and on-disk index persistence for tests.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use traces_pkm::TestProject;
    /// let temp = tempfile::tempdir().unwrap();
    /// let project = TestProject::trusted(temp.path().join("project"));
    /// project.write_note("notes/sample.md", "# Sample\n");
    /// let index = project.build_index();
    /// assert_eq!(index.entries().len(), 1);
    /// ```
    #[derive(Clone, Debug)]
    pub struct TestProject {
        root: PathBuf,
        service: ConfigService,
    }

    impl TestProject {
        fn project_service(root: &Path) -> ConfigService {
            let state_root = root.join(".traces");
            fixture_service(&state_root)
        }

        /// Creates an empty directory with an isolated config service and no
        /// `.traces/` directory.
        ///
        /// # Panics
        ///
        /// - Panics if `root` cannot be created. Fixture-only code: a panic
        ///   here means the fixture setup itself is broken.
        #[inline]
        #[must_use]
        pub fn empty<P: Into<PathBuf>>(root: P) -> Self {
            let root = root.into();
            std::fs::create_dir_all(&root).expect("create project dir");
            let service = Self::project_service(&root);
            Self {
                root,
                service,
            }
        }

        /// Writes `.traces/config.toml` without recording trust in the service.
        ///
        /// # Panics
        ///
        /// - Panics if the config file cannot be written. Fixture-only code.
        #[inline]
        #[must_use]
        pub fn untrusted<P: Into<PathBuf>>(root: P) -> Self {
            let root = root.into();
            std::fs::create_dir_all(&root).expect("create project dir");
            let config_path = root.join(".traces/config.toml");
            std::fs::create_dir_all(
                config_path.parent().expect("config parent"),
            )
            .expect("create config parent");
            std::fs::write(
                &config_path,
                "[templates]\ndirectory = \"templates\"\n",
            )
            .expect("write config file");
            let service = Self::project_service(&root);
            Self {
                root,
                service,
            }
        }

        /// Writes `.traces/config.toml` and records it as trusted in the
        /// isolated trust store.
        ///
        /// # Panics
        ///
        /// - Panics if the fixture setup fails; see [`create_trusted_project`].
        #[inline]
        #[must_use]
        pub fn trusted<P: Into<PathBuf>>(root: P) -> Self {
            let root = root.into();
            let service = Self::project_service(&root);
            let _ = create_trusted_project(&service, &root);
            Self {
                root,
                service,
            }
        }

        /// Returns the project root directory.
        #[inline]
        #[must_use]
        pub fn root(&self) -> &Path {
            &self.root
        }

        /// Returns the backing [`ConfigService`].
        #[inline]
        #[must_use]
        pub fn service(&self) -> &ConfigService {
            &self.service
        }

        /// Returns test [`Config`] rooted at this project, with templates
        /// configured if `templates/` exists.
        #[inline]
        #[must_use]
        pub fn config(&self) -> Config {
            let base = Config::test_default(&self.root);
            if self.root.join(DEFAULT_TEMPLATES_DIR).is_dir() {
                base.with_templates()
            } else {
                base
            }
        }

        /// Records trust for this project in its isolated trust store.
        ///
        /// # Panics
        ///
        /// - Panics if `.traces/config.toml` is missing or invalid.
        ///   Fixture-only code: a panic here means the test's fixture data is
        ///   wrong.
        #[inline]
        pub fn trust(&self) {
            let config_path = self.root.join(".traces/config.toml");
            let config = LocalConfigFile::<Discovered>::try_new(config_path)
                .expect("valid local config for trust");
            self.service
                .trust(&TrustRequest::from(&config))
                .expect("trust project config");
        }

        /// Removes trust for this project, returning the count of removed
        /// entries.
        ///
        /// # Panics
        ///
        /// - Panics if the trust entry cannot be removed. Fixture-only code.
        #[inline]
        #[must_use]
        pub fn untrust(&self) -> usize {
            self.service
                .untrust(&TrustRequest::from(self.root.as_path()))
                .expect("untrust project")
        }

        /// Safely writes content to `rel_path` beneath project root.
        ///
        /// # Panics
        ///
        /// - Panics if the path escapes `root` or the file cannot be written.
        ///   Fixture-only code: a panic here means the fixture setup is broken.
        #[inline]
        pub fn write_file<P: AsRef<Path>>(
            &self,
            rel_path: P,
            content: &str,
        ) -> PathBuf {
            let path = resolve_safe_path(&self.root, rel_path.as_ref());
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .expect("create parent directory");
            }
            std::fs::write(&path, content).expect("write file");
            path
        }

        /// Writes a note relative to the project root.
        ///
        /// # Panics
        ///
        /// - Panics if `rel_path` escapes `root` or the note cannot be written.
        ///   Fixture-only code: a panic here means the fixture setup is broken.
        #[inline]
        pub fn write_note<P: AsRef<Path>>(
            &self,
            rel_path: P,
            content: &str,
        ) -> PathBuf {
            self.write_file(rel_path, content)
        }

        /// Writes a template file under `templates/`.
        ///
        /// # Panics
        ///
        /// - Panics if the path escapes `root` or the template cannot be
        ///   written. Fixture-only code.
        #[inline]
        pub fn write_template<P: AsRef<Path>>(
            &self,
            name: P,
            content: &str,
        ) -> PathBuf {
            self.write_file(
                Path::new(DEFAULT_TEMPLATES_DIR).join(name.as_ref()),
                content,
            )
        }

        /// Writes a schema file under `.traces/schemas/`.
        ///
        /// # Panics
        ///
        /// - Panics if the path escapes `root` or the schema cannot be written.
        ///   Fixture-only code.
        #[inline]
        pub fn write_schema<P: AsRef<Path>>(
            &self,
            name: P,
            toml: &str,
        ) -> PathBuf {
            let file_name = format!("{}.toml", name.as_ref().display());
            self.write_file(
                Path::new(DEFAULT_SCHEMAS_DIR).join(file_name),
                toml,
            )
        }

        /// Writes a schema value file under `.traces/schemas/values/`.
        ///
        /// # Panics
        ///
        /// - Panics if the path escapes `root` or the file cannot be written.
        ///   Fixture-only code.
        #[inline]
        pub fn write_schema_value<P: AsRef<Path>>(
            &self,
            rel_path: P,
            toml: &str,
        ) -> PathBuf {
            self.write_file(
                Path::new(DEFAULT_SCHEMA_VALUES_DIR).join(rel_path.as_ref()),
                toml,
            )
        }

        /// Returns an [`IndexerService`] configured for this project.
        #[inline]
        #[must_use]
        pub fn indexer(&self) -> IndexerService {
            IndexerService::new(&self.root).with_config(&self.config())
        }

        /// Builds an in-memory index from disk files.
        ///
        /// # Panics
        ///
        /// - Panics if the index build fails. Fixture-only code.
        #[inline]
        #[must_use]
        pub fn build_index(&self) -> FileIndex {
            self.indexer().build().expect("build index")
        }

        /// Builds and persists the index to `index.redb`, returning both.
        ///
        /// # Panics
        ///
        /// - Panics if the build or persist fails. Fixture-only code.
        #[inline]
        #[must_use]
        pub fn persist_index(&self) -> (IndexerService, FileIndex) {
            let indexer = self.indexer();
            let index = indexer.build().expect("build index");
            indexer.persist(&index).expect("persist index");
            (indexer, index)
        }
    }

    /// Parses a Markdown note in memory with a custom path.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use traces_pkm::parse_note;
    /// let note = parse_note("notes/daily.md", "# Today\n\n- [ ] Task");
    /// assert_eq!(note.path().to_str(), Some("notes/daily.md"));
    /// ```
    #[inline]
    #[must_use]
    pub fn parse_note<P: AsRef<Path>>(path: P, src: &str) -> Note {
        parse_markdown(&MarkdownParserInput::for_test(path.as_ref(), src))
    }

    /// Parses a Markdown note in memory with the default path `note.md`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use traces_pkm::parse_note_str;
    /// let note = parse_note_str("# Sample\n\nContent");
    /// assert_eq!(note.path().to_str(), Some("note.md"));
    /// ```
    #[inline]
    #[must_use]
    pub fn parse_note_str(src: &str) -> Note {
        parse_note(Path::new("note.md"), src)
    }

    /// Builds an in-memory [`FileIndex`] wrapped in an [`Arc`] from note path
    /// and content pairs with zero disk I/O.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use traces_pkm::build_test_index;
    /// let index = build_test_index(&[("a.md", "# A"), ("b.md", "# B")]);
    /// assert_eq!(index.entries().len(), 2);
    /// ```
    #[inline]
    #[must_use]
    pub fn build_test_index(notes: &[(&str, &str)]) -> Arc<FileIndex> {
        Arc::new(FileIndex::new_test(notes))
    }

    /// Parses a [`Tag`] string slice for tests.
    ///
    /// # Panics
    ///
    /// - Panics if `s` is not a valid tag. Fixture-only code: a panic here
    ///   means the test's fixture data is wrong.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use traces_pkm::parse_tag;
    /// let tag = parse_tag("#projects/active");
    /// assert_eq!(tag.as_str(), "#projects/active");
    /// ```
    #[inline]
    #[must_use]
    pub fn parse_tag(s: &str) -> Tag {
        Tag::parse(s).expect("valid test tag")
    }

    /// Resolves a project-relative fixture path beneath `root`.
    ///
    /// Prevents accidental writes outside the fixture directory by rejecting
    /// absolute paths and parent-directory traversal (`..`).
    ///
    /// # Panics
    ///
    /// - Panics if `rel_path` is absolute or contains a `..` component.
    #[inline]
    #[must_use]
    pub fn resolve_safe_path<P: AsRef<Path>>(
        root: &Path,
        rel_path: P,
    ) -> PathBuf {
        let rel_path = rel_path.as_ref();
        assert!(
            rel_path.is_relative()
                && !rel_path
                    .components()
                    .any(|part| part == Component::ParentDir),
            "fixture path must stay inside project root: {}",
            rel_path.display()
        );
        root.join(rel_path)
    }

    /// Creates a [`ConfigService`] backed by isolated tracked-config and trust
    /// stores under `root`, never the real OS state directories.
    #[inline]
    #[must_use]
    pub fn fixture_service(root: &Path) -> ConfigService {
        ConfigService::at(root.join("tracked-store"), root.join("trust-store"))
    }

    /// Writes a minimal local config at `root/.traces/config.toml` pointing at
    /// `root/templates` (creating that directory), and records `root` as
    /// trusted in `service`'s trust store.
    ///
    /// # Panics
    ///
    /// - Panics if `root` cannot be created, the config file cannot be written,
    ///   or trust cannot be recorded. Fixture-only code: a panic here means the
    ///   fixture setup itself is broken.
    #[inline]
    #[must_use]
    pub fn create_trusted_project(
        service: &ConfigService,
        root: &Path,
    ) -> PathBuf {
        std::fs::create_dir_all(root).expect("create project dir");
        let config_path = root.join(".traces/config.toml");
        std::fs::create_dir_all(config_path.parent().expect("config parent"))
            .expect("create config parent");
        std::fs::write(
            &config_path,
            "[templates]\ndirectory = \"templates\"\n",
        )
        .expect("write config file");
        let config =
            LocalConfigFile::<Discovered>::try_new(config_path.clone())
                .expect("valid local config");
        service
            .trust(&TrustRequest::from(&config))
            .expect("trust project config");
        config_path
    }

    /// Writes a Markdown note at `root.join(rel_path)`, creating parent
    /// directories as needed.
    ///
    /// # Panics
    ///
    /// - Panics if the path escapes `root` or the note cannot be written.
    ///   Fixture-only code: a panic here means the fixture setup is broken.
    #[inline]
    pub fn write_note<P: AsRef<Path>>(
        root: &Path,
        rel_path: P,
        content: &str,
    ) -> PathBuf {
        let path = resolve_safe_path(root, rel_path.as_ref());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create note parent dir");
        }
        std::fs::write(&path, content).expect("write note");
        path
    }

    /// Writes a template file into `root/templates/name`, creating the
    /// `templates` directory if absent.
    ///
    /// # Panics
    ///
    /// - Panics if the path escapes `root` or the template cannot be written.
    ///   Fixture-only code: a panic here means the fixture setup is broken.
    #[inline]
    pub fn write_template<P: AsRef<Path>>(
        root: &Path,
        name: P,
        source: &str,
    ) -> PathBuf {
        let path = resolve_safe_path(
            root,
            Path::new(DEFAULT_TEMPLATES_DIR).join(name.as_ref()),
        );
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create templates dir");
        }
        std::fs::write(&path, source).expect("write template");
        path
    }

    /// Writes a schema file into `root/.traces/schemas/name.toml`, creating
    /// directories if needed.
    ///
    /// # Panics
    ///
    /// - Panics if the path escapes `root` or the schema cannot be written.
    ///   Fixture-only code: a panic here means the fixture setup is broken.
    #[inline]
    pub fn write_schema<P: AsRef<Path>>(
        root: &Path,
        name: P,
        toml: &str,
    ) -> PathBuf {
        let file_name = format!("{}.toml", name.as_ref().display());
        let path = resolve_safe_path(
            root,
            Path::new(DEFAULT_SCHEMAS_DIR).join(file_name),
        );
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create schemas dir");
        }
        std::fs::write(&path, toml).expect("write schema");
        path
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        mod path_safety {
            use super::*;

            #[test]
            fn resolves_valid_relative_paths() {
                let root = Path::new("/workspace");
                let resolved =
                    resolve_safe_path(root, Path::new("notes/daily.md"));
                assert_eq!(resolved, Path::new("/workspace/notes/daily.md"));
            }

            #[test]
            #[should_panic(
                expected = "fixture path must stay inside project root"
            )]
            fn rejects_absolute_paths() {
                let root = Path::new("/workspace");
                let _ = resolve_safe_path(root, Path::new("/etc/passwd"));
            }

            #[test]
            #[should_panic(
                expected = "fixture path must stay inside project root"
            )]
            fn rejects_parent_directory_traversal() {
                let root = Path::new("/workspace");
                let _ = resolve_safe_path(root, Path::new("../escape.md"));
            }
        }

        mod service {
            use super::*;

            #[test]
            fn returns_usable_service_in_temporary_directory() {
                let temp = tempfile::tempdir().expect("create temp dir");
                let service = fixture_service(temp.path());
                let result = service.load(temp.path());
                assert!(result.is_err(), "empty dir should fail discovery");
            }
        }

        mod project {
            use pretty_assertions::assert_eq;

            use super::*;

            #[test]
            fn creates_empty_workspace_without_traces_config() {
                let temp = tempfile::tempdir().expect("create temp dir");
                let project = TestProject::empty(temp.path().join("proj"));
                assert!(project.root().is_dir());
                assert!(!project.root().join(".traces/config.toml").exists());
            }

            #[test]
            fn creates_trusted_workspace_and_persists_index() {
                let temp = tempfile::tempdir().expect("create temp dir");
                let project = TestProject::trusted(temp.path().join("proj"));
                assert_eq!(project.root(), temp.path().join("proj").as_path());
                project.write_note("notes/sample.md", "# Sample\n");
                project.write_template("daily.md", "Template");
                project.write_schema("tag_schema", "fields = {}\n");
                project.write_schema_value("vals.toml", "vals = []\n");

                let (indexer, index) = project.persist_index();
                let reloaded = indexer.load().expect("load persisted index");
                assert_eq!(reloaded.entries().len(), index.entries().len());
                assert!(
                    reloaded.entries().iter().any(
                        |e| e.file().path() == Path::new("notes/sample.md")
                    )
                );
                assert!(reloaded.entries().iter().any(
                    |e| e.file().path() == Path::new("templates/daily.md")
                ));
            }

            #[test]
            fn manages_trust_and_untrust_lifecycle() {
                let temp = tempfile::tempdir().expect("create temp dir");
                let project = TestProject::untrusted(temp.path().join("proj"));
                project.trust();
                let removed = project.untrust();
                assert_eq!(removed, 1);
            }

            #[test]
            fn config_detects_templates_directory() {
                let temp = tempfile::tempdir().expect("create temp dir");
                let project = TestProject::empty(temp.path().join("proj"));
                assert!(project.config().local_template_dir().is_none());

                project.write_template("test.md", "content");
                assert_eq!(
                    project.config().local_template_dir(),
                    Some(project.root().join("templates").as_path())
                );
            }
        }
        mod memory {
            use pretty_assertions::assert_eq;

            use super::*;

            #[test]
            fn parses_notes_and_builds_index_without_disk() {
                let note = parse_note_str("# Test Note\n");
                assert_eq!(note.path(), Path::new("note.md"));

                let index =
                    build_test_index(&[("a.md", "# A"), ("b.md", "# B")]);
                assert_eq!(index.entries().len(), 2);

                let tag = parse_tag("#project");
                assert_eq!(tag.as_str(), "#project");
            }

            #[test]
            fn scan_skips_traces_dir() {
                let temp = tempfile::tempdir().expect("create temp dir");
                let project = TestProject::trusted(temp.path().join("proj"));
                project.write_note("a.md", "# A\n");
                let index = project.build_index();
                let paths: Vec<&Path> =
                    index.entries().iter().map(|e| e.file().path()).collect();
                assert_eq!(paths, [Path::new("a.md")]);
            }
        }
    }
}
