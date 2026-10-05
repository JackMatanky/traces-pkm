//! Resolved configuration model produced by the builder pipeline.
//!
//! # Types
//!
//! - [`Config`] merges local and global settings into read-only resolved
//!   values.
//! - [`TemplateConfig`] preserves local and global template directories.
//! - [`SchemasConfig`] resolves the `[schemas]` class field and registry path.
//! - [`FrontmatterConfig`] resolves `[frontmatter]` key names.
//! - [`DateFieldConfig`] pairs a date frontmatter key with its format.
//! - [`TaskConfig`] resolves `[tasks]` task statuses and tag filters.
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use super::{
    error::ConfigFileError,
    raw::{
        RawDateFieldConfig, RawFrontmatterConfig, RawTaskConfig,
        RawTaskStatusKind,
    },
};
use crate::{
    FieldName, FieldNameError, Tag, TaskStatus, TaskStatusMap,
    TaskStatusSymbol, TaskStatusType,
    path::{PathError, RelativePath, SafeRelativePath},
};

/// Default local schemas directory when unconfigured.
pub(super) const DEFAULT_LOCAL_SCHEMAS_DIR: &str = ".traces/schemas/";

/// Default `[templates] directory` for the local layer when unconfigured.
pub(crate) const DEFAULT_LOCAL_TEMPLATES_DIR: &str = ".traces/templates";

/// Default `[schemas] class_field` when unconfigured.
pub(super) const DEFAULT_CLASS_FIELD: &str = "class";

/// Default `[frontmatter] title` key when unconfigured.
const DEFAULT_TITLE_FIELD: &str = "title";

/// Default `[frontmatter] aliases` key when unconfigured.
const DEFAULT_ALIASES_FIELD: &str = "aliases";

/// Default `[frontmatter] tags` key when unconfigured.
const DEFAULT_TAGS_FIELD: &str = "tags";

/// Default `[frontmatter] date_created.name` key when unconfigured.
const DEFAULT_DATE_CREATED_FIELD: &str = "date_created";

/// Default `[frontmatter] date_modified.name` key when unconfigured.
const DEFAULT_DATE_MODIFIED_FIELD: &str = "date_modified";

/// Default date format applied to both date roles when unconfigured.
const DEFAULT_DATE_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

/// Resolved config ready for consumers after discovery, trust checks, and
/// merging.
#[derive(Clone, Debug)]
pub struct Config {
    root: Arc<Path>,
    templates: TemplateConfig,
    schemas: SchemasConfig,
    frontmatter: FrontmatterConfig,
    tasks: TaskConfig,
}

impl Config {
    /// Creates a resolved config from builder-owned parts.
    #[inline]
    #[must_use]
    pub(super) fn new(
        templates: TemplateConfig,
        schemas: SchemasConfig,
        frontmatter: FrontmatterConfig,
        tasks: TaskConfig,
        root: PathBuf,
    ) -> Self {
        Self {
            root: Arc::from(root.into_boxed_path()),
            templates,
            schemas,
            frontmatter,
            tasks,
        }
    }

    /// Returns the project root directory used as the local resolution base.
    ///
    /// Relative template and output paths are resolved against this root.
    #[inline]
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the local template directory, if configured.
    #[inline]
    #[must_use]
    pub fn local_template_dir(&self) -> Option<&Path> {
        self.templates.local()
    }

    /// Returns the global template directory, if configured.
    #[inline]
    #[must_use]
    pub fn global_template_dir(&self) -> Option<&Path> {
        self.templates.global()
    }

    /// Returns the resolved output directory, or [`root`] when no output
    /// directory was configured.
    ///
    /// Relative configured paths are resolved against [`root`]; absolute paths
    /// are preserved.
    ///
    /// [`root`]: Self::root
    #[inline]
    #[must_use]
    pub fn output_dir(&self) -> &Path {
        self.templates.output()
    }

    /// Returns the resolved `[schemas]` settings.
    #[inline]
    #[must_use]
    pub const fn schemas(&self) -> &SchemasConfig {
        &self.schemas
    }

    /// Returns the resolved `[frontmatter]` settings.
    #[inline]
    #[must_use]
    pub const fn frontmatter(&self) -> &FrontmatterConfig {
        &self.frontmatter
    }

    /// Returns the resolved `[tasks]` settings.
    #[inline]
    #[must_use]
    pub const fn tasks(&self) -> &TaskConfig {
        &self.tasks
    }

    /// Returns the project root as a cheaply shareable `'static` path, for
    /// consumers (minijinja namespace objects) that cannot borrow `&Config`.
    #[inline]
    #[must_use]
    pub(crate) fn root_arc(&self) -> Arc<Path> {
        Arc::clone(&self.root)
    }

    /// Returns the `[schemas] class_field` name as a cheaply shareable
    /// `'static` string, for consumers that cannot borrow `&Config`.
    #[inline]
    #[must_use]
    pub(crate) fn class_field_arc(&self) -> Arc<str> {
        Arc::from(self.schemas().class_field_name())
    }

    /// Builds config directly for tests that do not exercise discovery.
    ///
    /// Prefer [`super::service::ConfigService::at`] and TOML fixtures for
    /// integration-style tests that need the real loading pipeline.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn for_test(
        root: PathBuf,
        local: Option<PathBuf>,
        global: Option<PathBuf>,
        output: PathBuf,
    ) -> Self {
        let root: Arc<Path> = Arc::from(root.into_boxed_path());
        let schemas = SchemasConfig::default_for_root(&root);
        Self {
            root,
            templates: TemplateConfig::new(local, global, output),
            schemas,
            frontmatter: FrontmatterConfig::default(),
            tasks: TaskConfig::default(),
        }
    }

    /// Builds config with default paths rooted at `root` for tests.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn test_default<P: Into<PathBuf>>(root: P) -> Self {
        let root = root.into();
        let root: Arc<Path> = Arc::from(root.into_boxed_path());
        let templates = TemplateConfig::new(None, None, root.to_path_buf());
        let schemas = SchemasConfig::default_for_root(&root);
        Self {
            root,
            templates,
            schemas,
            frontmatter: FrontmatterConfig::default(),
            tasks: TaskConfig::default(),
        }
    }

    /// Configures templates rooted at `root/templates` for tests.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn with_templates(mut self) -> Self {
        self.templates = TemplateConfig::new(
            Some(self.root.join("templates")),
            None,
            self.root.to_path_buf(),
        );
        self
    }

    /// Overrides the `[frontmatter]` resolution on a test-built config, for
    /// tests that exercise non-default label resolution.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn with_frontmatter(mut self, frontmatter: FrontmatterConfig) -> Self {
        self.frontmatter = frontmatter;
        self
    }

    /// Overrides the `[tasks]` resolution on a test-built config.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn with_tasks(mut self, tasks: TaskConfig) -> Self {
        self.tasks = tasks;
        self
    }

    /// Overrides the `[schemas]` resolution on a test-built config.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn with_schemas(mut self, schemas: SchemasConfig) -> Self {
        self.schemas = schemas;
        self
    }
}

/// Template directories and output path from merged config.
///
/// Local and global directories are kept separate so template lookup preserves
/// local-first precedence without re-reading config files.
#[derive(Clone, Debug)]
pub(super) struct TemplateConfig {
    local: Option<PathBuf>,
    global: Option<PathBuf>,
    output: PathBuf,
}

impl TemplateConfig {
    /// Creates a template config from builder-owned parts.
    #[inline]
    #[must_use]
    pub(super) const fn new(
        local: Option<PathBuf>,
        global: Option<PathBuf>,
        output: PathBuf,
    ) -> Self {
        Self {
            local,
            global,
            output,
        }
    }

    /// Returns the local project template directory, if set.
    #[inline]
    #[must_use]
    pub(super) fn local(&self) -> Option<&Path> {
        self.local.as_deref()
    }

    /// Returns the global template directory, if set.
    #[inline]
    #[must_use]
    pub(super) fn global(&self) -> Option<&Path> {
        self.global.as_deref()
    }

    /// Returns the configured output directory, or the config root when absent.
    #[inline]
    #[must_use]
    pub(super) fn output(&self) -> &Path {
        &self.output
    }
}

/// Schema settings providing the class field name and registry directory for
/// template lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemasConfig {
    directory: PathBuf,
    class_field: FieldName,
}

impl SchemasConfig {
    /// Creates schemas config from already-validated, resolved parts.
    ///
    /// `directory` must already be resolved against the originating config
    /// layer's root; this constructor performs no further validation.
    #[inline]
    #[must_use]
    pub(super) const fn new(
        directory: PathBuf,
        class_field: FieldName,
    ) -> Self {
        Self {
            directory,
            class_field,
        }
    }

    /// Returns the frontmatter key naming a Note's File Class(es).
    ///
    /// Defaults to `class` when unconfigured.
    #[inline]
    #[must_use]
    pub fn class_field_name(&self) -> &str {
        self.class_field.as_str()
    }

    /// Returns the schema registry directory.
    ///
    /// Values produced by config loading are resolved against the root of the
    /// layer that supplied a configured directory. When no layer configures a
    /// directory, the default is resolved against the local project root.
    /// `SchemasConfig::default()` keeps `.traces/schemas/` relative because it
    /// has no config root.
    #[inline]
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Builds schemas config directly for tests that do not exercise TOML
    /// loading; `class_field` must pass field-name validation.
    ///
    /// # Panics
    ///
    /// Panics if `class_field` is not a valid field key.
    #[cfg(any(test, feature = "test-utils"))]
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "test inputs must validate; Default carries the same \
                  expectation"
    )]
    pub fn for_test(class_field: &str) -> Self {
        Self {
            directory: PathBuf::from(DEFAULT_LOCAL_SCHEMAS_DIR),
            class_field: FieldName::try_from(class_field)
                .expect("test class field validates as a field key"),
        }
    }

    /// Builds default schemas config with the directory resolved against
    /// `root`, for test helpers that have a real project root to anchor against
    /// (e.g. [`Config::test_default`]/[`Config::for_test`]).
    #[cfg(any(test, feature = "test-utils"))]
    #[must_use]
    pub(super) fn default_for_root(root: &Path) -> Self {
        Self {
            directory: root.join(DEFAULT_LOCAL_SCHEMAS_DIR),
            ..Self::default()
        }
    }
}

impl Default for SchemasConfig {
    /// # Panics
    ///
    /// Never in practice: [`DEFAULT_CLASS_FIELD`] is a hardcoded, always-valid
    /// field key.
    #[inline]
    #[expect(
        clippy::expect_used,
        reason = "DEFAULT_CLASS_FIELD is a hardcoded constant; failure here \
                  means the constant itself is malformed, not a recoverable \
                  caller error"
    )]
    fn default() -> Self {
        Self {
            directory: PathBuf::from(DEFAULT_LOCAL_SCHEMAS_DIR),
            class_field: FieldName::try_from(DEFAULT_CLASS_FIELD)
                .expect("DEFAULT_CLASS_FIELD is a valid field key"),
        }
    }
}

/// Resolved `[frontmatter]` settings mapping key names for title, aliases,
/// tags, and date roles.
#[derive(Clone, Debug)]
pub struct FrontmatterConfig {
    title: FieldName,
    aliases: FieldName,
    tags: FieldName,
    date_created: DateFieldConfig,
    date_modified: DateFieldConfig,
}

impl FrontmatterConfig {
    /// Returns the frontmatter key holding a Note's display title.
    #[inline]
    #[must_use]
    pub fn title_name(&self) -> &str {
        self.title.as_str()
    }

    /// Returns the frontmatter key holding a Note's aliases.
    #[inline]
    #[must_use]
    pub fn aliases_name(&self) -> &str {
        self.aliases.as_str()
    }

    /// Returns the frontmatter key holding a Note's tags.
    #[inline]
    #[must_use]
    pub fn tags_name(&self) -> &str {
        self.tags.as_str()
    }

    /// Returns the creation-timestamp frontmatter key and date format.
    #[inline]
    #[must_use]
    pub const fn date_created(&self) -> &DateFieldConfig {
        &self.date_created
    }

    /// Returns the modification-timestamp frontmatter key and date format.
    #[inline]
    #[must_use]
    pub const fn date_modified(&self) -> &DateFieldConfig {
        &self.date_modified
    }

    /// Builds a frontmatter config with custom title/aliases keys for tests
    /// that exercise non-default label resolution.
    ///
    /// # Panics
    ///
    /// If `title` or `aliases` fails `FieldName` validation (empty or
    /// whitespace-only): a test-fixture bug, not a runtime error path.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "test-only constructor; an invalid literal here is a test \
                  fixture bug, not a recoverable caller error"
    )]
    pub fn for_test<T: Into<String>, A: Into<String>>(
        title: T,
        aliases: A,
    ) -> Self {
        Self {
            title: FieldName::try_from(title.into())
                .expect("test fixture title is a valid field key"),
            aliases: FieldName::try_from(aliases.into())
                .expect("test fixture aliases is a valid field key"),
            ..Self::default()
        }
    }

    /// Overrides the tags key on a test-built frontmatter config, for tests
    /// that exercise non-default tags-key resolution.
    ///
    /// # Panics
    ///
    /// If `tags` fails `FieldName` validation (empty or whitespace-only): a
    /// test-fixture bug, not a runtime error path.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "test-only constructor; an invalid literal here is a test \
                  fixture bug, not a recoverable caller error"
    )]
    pub fn with_tags_name<T: Into<String>>(mut self, tags: T) -> Self {
        self.tags = FieldName::try_from(tags.into())
            .expect("test fixture tags is a valid field key");
        self
    }
}

impl Default for FrontmatterConfig {
    /// # Panics
    ///
    /// Never in practice: [`DEFAULT_TITLE_FIELD`]/[`DEFAULT_ALIASES_FIELD`]/
    /// [`DEFAULT_TAGS_FIELD`] are hardcoded, always-valid field keys.
    #[inline]
    #[expect(
        clippy::expect_used,
        reason = "DEFAULT_TITLE_FIELD/DEFAULT_ALIASES_FIELD/\
                  DEFAULT_TAGS_FIELD are hardcoded constants; failure here \
                  means a constant itself is malformed, not a recoverable \
                  caller error"
    )]
    fn default() -> Self {
        Self {
            title: FieldName::try_from(DEFAULT_TITLE_FIELD)
                .expect("DEFAULT_TITLE_FIELD is a valid field key"),
            aliases: FieldName::try_from(DEFAULT_ALIASES_FIELD)
                .expect("DEFAULT_ALIASES_FIELD is a valid field key"),
            tags: FieldName::try_from(DEFAULT_TAGS_FIELD)
                .expect("DEFAULT_TAGS_FIELD is a valid field key"),
            date_created: DateFieldConfig::default_for(
                DEFAULT_DATE_CREATED_FIELD,
            ),
            date_modified: DateFieldConfig::default_for(
                DEFAULT_DATE_MODIFIED_FIELD,
            ),
        }
    }
}

impl TryFrom<RawFrontmatterConfig> for FrontmatterConfig {
    type Error = ConfigFileError;

    /// # Errors
    ///
    /// - [`ConfigFileError::InvalidFieldKey`] if `title`, `aliases`, or `tags`
    ///   fails field name validation.
    #[inline]
    fn try_from(raw: RawFrontmatterConfig) -> Result<Self, Self::Error> {
        let invalid_key =
            |source| ConfigFileError::invalid_field_key("frontmatter", source);
        Ok(Self {
            title: FieldName::try_from(
                raw.title.unwrap_or_else(|| DEFAULT_TITLE_FIELD.to_owned()),
            )
            .map_err(invalid_key)?,
            aliases: FieldName::try_from(
                raw.aliases.unwrap_or_else(|| DEFAULT_ALIASES_FIELD.to_owned()),
            )
            .map_err(invalid_key)?,
            tags: FieldName::try_from(
                raw.tags.unwrap_or_else(|| DEFAULT_TAGS_FIELD.to_owned()),
            )
            .map_err(invalid_key)?,
            date_created: DateFieldConfig::from_raw_or_default(
                raw.date_created,
                DEFAULT_DATE_CREATED_FIELD,
            )
            .map_err(invalid_key)?,
            date_modified: DateFieldConfig::from_raw_or_default(
                raw.date_modified,
                DEFAULT_DATE_MODIFIED_FIELD,
            )
            .map_err(invalid_key)?,
        })
    }
}

/// Resolved `[tasks]` settings: the task status lookup map and the tag filters
/// that classify status-marked list items as Tasks.
#[derive(Clone, Debug)]
pub struct TaskConfig {
    statuses: TaskStatusMap,
    tag_filters: Vec<Tag>,
}

impl TaskConfig {
    /// Returns the resolved task status lookup map.
    ///
    /// `pub(crate)`, not part of `Config`'s public accessor surface: the lookup
    /// table is parser-internal plumbing, unlike [`Self::tag_filters`] which is
    /// a genuine resolved-setting read.
    #[inline]
    #[must_use]
    pub(crate) const fn statuses(&self) -> &TaskStatusMap {
        &self.statuses
    }

    /// Returns the configured task tag filters.
    ///
    /// Empty means no filter is configured: every status-marked list item
    /// becomes a Task.
    #[inline]
    #[must_use]
    pub fn tag_filters(&self) -> &[Tag] {
        &self.tag_filters
    }

    /// Builds a task config with custom tag filters for tests.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn for_test(tag_filters: Vec<Tag>) -> Self {
        Self {
            statuses: TaskStatusMap::default(),
            tag_filters,
        }
    }

    /// Builds a task config with tag filters parsed from string slices for
    /// tests.
    ///
    /// # Panics
    ///
    /// - Panics if a tag does not parse. Fixture-only code: a panic here means
    ///   the test's fixture data is wrong.
    #[cfg(any(test, feature = "test-utils"))]
    #[expect(
        clippy::expect_used,
        reason = "test-only constructor; an invalid literal here is a test \
                  fixture bug, not a recoverable caller error"
    )]
    #[inline]
    #[must_use]
    pub fn from_tags(tags: &[&str]) -> Self {
        let tag_filters =
            tags.iter().map(|&t| Tag::parse(t).expect("valid tag")).collect();
        Self::for_test(tag_filters)
    }
}

impl Default for TaskConfig {
    #[inline]
    fn default() -> Self {
        Self {
            statuses: TaskStatusMap::default(),
            tag_filters: Vec::new(),
        }
    }
}

impl TryFrom<RawTaskConfig> for TaskConfig {
    type Error = ConfigFileError;

    /// # Errors
    ///
    /// - [`ConfigFileError::Task`] if a configured task status uses a
    ///   prohibited delimiter symbol: `(`, `)`, `[`, `]`, `{`, or `}`.
    /// - [`ConfigFileError::InvalidTagFilter`] when a `tag_filters` entry does
    ///   not normalize into a valid [`Tag`].
    #[inline]
    fn try_from(raw: RawTaskConfig) -> Result<Self, Self::Error> {
        let mut statuses = TaskStatusMap::default();
        for status in raw.statuses {
            statuses.insert(TaskStatus::new(
                TaskStatusSymbol::new(status.symbol),
                status.name,
                status.kind.into(),
            ))?;
        }
        let tag_filters = raw
            .tag_filters
            .into_iter()
            .map(|entry| {
                Tag::parse_lenient(&entry).map_err(|source| {
                    ConfigFileError::InvalidTagFilter {
                        entry,
                        source,
                    }
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            statuses,
            tag_filters,
        })
    }
}

impl From<RawTaskStatusKind> for TaskStatusType {
    #[inline]
    fn from(kind: RawTaskStatusKind) -> Self {
        match kind {
            RawTaskStatusKind::Todo => Self::Todo,
            RawTaskStatusKind::InProgress => Self::InProgress,
            RawTaskStatusKind::OnHold => Self::OnHold,
            RawTaskStatusKind::Done => Self::Done,
            RawTaskStatusKind::Cancelled => Self::Cancelled,
            RawTaskStatusKind::NonTask => Self::NonTask,
        }
    }
}

/// Resolved `[schemas]` settings providing the class field name and registry
/// directory for template lookup.
///
/// A safe, root-relative subdirectory path configured in TOML (e.g. `[schemas]
/// directory`).
///
/// Guaranteed to be relative, contain only normal components (no `..`), and be
/// non-empty.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConfigSubDir(RelativePath);

impl ConfigSubDir {
    /// Returns the subdirectory as a relative [`Path`].
    #[inline]
    #[must_use]
    pub fn as_path(&self) -> &Path {
        self.0.as_ref()
    }

    /// Resolves this subdirectory against `root`, guaranteeing the resolved
    /// path stays within `root`.
    ///
    /// # Errors
    ///
    /// - [`ConfigFileError::InvalidSubDir`] if the resolved path escapes `root`
    ///   or fails verification.
    pub(crate) fn resolve_against(
        &self,
        root: &Path,
    ) -> Result<PathBuf, ConfigFileError> {
        SafeRelativePath::parse(root, self.as_path())
            .map(SafeRelativePath::into_path_buf)
            .map_err(|source| ConfigFileError::InvalidSubDir {
                path: self.as_path().to_path_buf(),
                source,
            })
    }

    /// Validates a configured subdirectory, falling back to `default_rel` when
    /// unconfigured.
    ///
    /// # Errors
    ///
    /// - [`PathError`] if the raw or default value is not a valid safe relative
    ///   path.
    pub(super) fn from_raw_or_default(
        raw: Option<PathBuf>,
        default_rel: &'static str,
    ) -> Result<Self, PathError> {
        match raw {
            Some(path) => Self::try_from(path),
            None => Self::try_from(default_rel),
        }
    }
}

impl TryFrom<PathBuf> for ConfigSubDir {
    type Error = PathError;

    #[inline]
    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        RelativePath::parse(&path).map(Self)
    }
}

impl TryFrom<&str> for ConfigSubDir {
    type Error = PathError;

    #[inline]
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        RelativePath::parse(Path::new(s)).map(Self)
    }
}

/// A frontmatter key name and its date format string.
#[derive(Clone, Debug)]
pub struct DateFieldConfig {
    name: FieldName,
    format: String,
}

impl DateFieldConfig {
    /// Returns the frontmatter key name.
    #[inline]
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Returns the date format string applied to the key's value.
    #[inline]
    #[must_use]
    pub fn format(&self) -> &str {
        &self.format
    }

    /// Builds a default date field config for `name` using the shared default
    /// date format.
    ///
    /// # Panics
    ///
    /// Never in practice: callers only pass hardcoded, always-valid role-name
    /// constants (`DEFAULT_DATE_CREATED_FIELD`/`DEFAULT_DATE_MODIFIED_FIELD`).
    #[inline]
    #[expect(
        clippy::expect_used,
        reason = "callers only pass hardcoded role-name constants; failure \
                  here means a constant itself is malformed, not a \
                  recoverable caller error"
    )]
    fn default_for(name: &str) -> Self {
        Self {
            name: FieldName::try_from(name)
                .expect("role-name constant is a valid field key"),
            format: DEFAULT_DATE_FORMAT.to_owned(),
        }
    }

    /// Resolves a raw date-role table into a concrete config, filling missing
    /// `name`/`format` from role-aware defaults.
    ///
    /// # Errors
    ///
    /// - [`FieldNameError`] if a configured `raw.name` fails validation.
    #[inline]
    fn from_raw_or_default(
        raw: Option<RawDateFieldConfig>,
        default_name: &str,
    ) -> Result<Self, FieldNameError> {
        Ok(match raw {
            None => Self::default_for(default_name),
            Some(raw) => Self {
                name: FieldName::try_from(
                    raw.name.unwrap_or_else(|| default_name.to_owned()),
                )?,
                format: raw
                    .format
                    .unwrap_or_else(|| DEFAULT_DATE_FORMAT.to_owned()),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::TaskError;

    mod root_arc {
        use super::*;

        #[test]
        fn repeated_calls_share_the_cached_allocation() {
            let config = Config::test_default(PathBuf::from("/vault"));
            let first = config.root_arc();
            let second = config.root_arc();

            assert!(Arc::ptr_eq(&first, &second));
        }
    }

    mod frontmatter_for_test {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn sets_the_expected_title_and_aliases() {
            let config = FrontmatterConfig::for_test("heading", "also_known");

            assert_eq!(config.title_name(), "heading");
            assert_eq!(config.aliases_name(), "also_known");
        }

        #[test]
        fn defaults_tags_to_tags() {
            let config = FrontmatterConfig::for_test("heading", "also_known");

            assert_eq!(config.tags_name(), "tags");
        }
    }
    mod schemas_for_test {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn sets_the_expected_class_field() {
            let config = SchemasConfig::for_test("kind");

            assert_eq!(config.class_field_name(), "kind");
        }
    }
    mod config_sub_dir {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn accepts_safe_relative_path() {
            let subdir = ConfigSubDir::try_from("schemas/dir")
                .expect("valid safe relative path");
            assert_eq!(subdir.as_path(), Path::new("schemas/dir"));
        }

        #[test]
        fn rejects_absolute_and_parent_paths() {
            assert!(ConfigSubDir::try_from("/absolute/path").is_err());
            assert!(ConfigSubDir::try_from("../parent").is_err());
            assert!(ConfigSubDir::try_from("dir/../../escaped").is_err());
        }

        #[test]
        fn resolve_against_joins_subdir_onto_root() {
            let temp = tempfile::tempdir().expect("create temp directory");
            let root = temp.path().join("vault");
            let schemas = root.join(".traces/schemas");
            std::fs::create_dir_all(&schemas).expect("create schema directory");
            let subdir = ConfigSubDir::try_from(DEFAULT_LOCAL_SCHEMAS_DIR)
                .expect("valid safe relative path");

            assert_eq!(
                subdir.resolve_against(&root).expect("resolves"),
                schemas
            );
        }

        #[cfg(unix)]
        #[test]
        fn resolve_against_rejects_subdir_that_escapes_root() {
            let temp = tempfile::tempdir().expect("create temp directory");
            let root = temp.path().join("root");
            let external = temp.path().join("external");
            std::fs::create_dir_all(&root).expect("create root directory");
            std::fs::create_dir_all(&external)
                .expect("create external directory");
            std::os::unix::fs::symlink(&external, root.join(".traces"))
                .expect("create escaping schema symlink");
            let subdir = ConfigSubDir::try_from(DEFAULT_LOCAL_SCHEMAS_DIR)
                .expect("valid safe relative path");

            let error = subdir
                .resolve_against(&root)
                .expect_err("schema directory escapes root through symlink");

            assert!(matches!(error, ConfigFileError::InvalidSubDir {
                source: crate::path::PathError::OutsideRoot,
                ..
            }));
        }
    }

    mod task_config {
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::config::raw::RawTaskStatus;

        #[test]
        fn defaults_to_empty_tag_filters_and_default_statuses() {
            let config = TaskConfig::default();

            assert_eq!(config.tag_filters(), []);
            assert!(config.statuses().by_symbol(' '.into()).is_some());
        }

        #[test]
        fn normalizes_entries_with_and_without_a_leading_hash() {
            let raw = RawTaskConfig {
                tag_filters: vec!["task".to_owned(), "#todo".to_owned()],
                statuses: Vec::new(),
            };

            let config = TaskConfig::try_from(raw).expect("valid tag filters");

            assert_eq!(config.tag_filters(), [
                Tag::parse("#task").unwrap(),
                Tag::parse("#todo").unwrap(),
            ]);
        }

        #[test]
        fn allows_an_empty_tag_filters_list() {
            let raw = RawTaskConfig {
                tag_filters: Vec::new(),
                statuses: Vec::new(),
            };

            let config = TaskConfig::try_from(raw).expect("empty is valid");

            assert_eq!(config.tag_filters(), []);
        }

        #[test]
        fn adds_a_custom_status_with_its_configured_kind() {
            let raw = RawTaskConfig {
                statuses: vec![RawTaskStatus {
                    symbol: '?',
                    name: "Blocked".to_owned(),
                    kind: RawTaskStatusKind::OnHold,
                }],
                ..RawTaskConfig::default()
            };

            let config = TaskConfig::try_from(raw).expect("valid status");

            let status =
                config.statuses().by_symbol('?'.into()).expect("custom status");
            assert_eq!(status.name(), "Blocked");
            assert_eq!(status.kind(), TaskStatusType::OnHold);
        }

        #[test]
        fn overriding_a_symbol_removes_the_replaced_name() {
            let raw = RawTaskConfig {
                statuses: vec![RawTaskStatus {
                    symbol: '!',
                    name: "Waiting".to_owned(),
                    kind: RawTaskStatusKind::Todo,
                }],
                ..RawTaskConfig::default()
            };

            let config = TaskConfig::try_from(raw).expect("valid status");

            let status = config
                .statuses()
                .by_symbol('!'.into())
                .expect("overridden status");
            assert_eq!(status.name(), "Waiting");
            assert_eq!(status.kind(), TaskStatusType::Todo);
            assert!(config.statuses().by_name("On Hold").is_none());
        }

        #[test]
        fn rejects_an_invalid_tag_filter_entry() {
            let raw = RawTaskConfig {
                tag_filters: vec!["1invalid".to_owned()],
                statuses: Vec::new(),
            };

            let error = TaskConfig::try_from(raw).expect_err("invalid entry");

            assert!(matches!(error, ConfigFileError::InvalidTagFilter {
                entry,
                ..
            } if entry == "1invalid"));
        }

        #[test]
        fn rejects_a_whitespace_only_tag_filter_entry() {
            let raw = RawTaskConfig {
                tag_filters: vec!["   ".to_owned()],
                statuses: Vec::new(),
            };

            let error = TaskConfig::try_from(raw).expect_err("blank entry");

            assert!(matches!(error, ConfigFileError::InvalidTagFilter { .. }));
        }

        #[test]
        fn rejects_a_prohibited_status_symbol() {
            let raw = RawTaskConfig {
                statuses: vec![RawTaskStatus {
                    symbol: '[',
                    name: "Square".to_owned(),
                    kind: RawTaskStatusKind::Todo,
                }],
                ..RawTaskConfig::default()
            };

            let error =
                TaskConfig::try_from(raw).expect_err("prohibited delimiter");

            assert!(matches!(
                error,
                ConfigFileError::Task(TaskError::ProhibitedStatusSymbol {
                    symbol: '['
                })
            ));
        }
    }
}
