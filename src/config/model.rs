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

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use super::{
    error::ConfigFileError,
    raw::{
        RawDateFieldConfig, RawFrontmatterConfig, RawSchemasConfig,
        RawTaskConfig,
    },
};
use crate::{
    field::{FieldName, FieldNameError},
    path::{PathError, RootConfinedPath, SafeRelativePath},
    tag::Tag,
    task::TaskStatusMap,
};

/// Default `[schemas] class_field` when unconfigured.
const DEFAULT_CLASS_FIELD: &str = "class";

/// Default `[schemas] directory` when unconfigured.
const DEFAULT_SCHEMAS_DIR: &str = ".traces/schemas/";

/// Default `[frontmatter] title` key when unconfigured.
const DEFAULT_TITLE_FIELD: &str = "title";

/// Default `[frontmatter] aliases` key when unconfigured.
const DEFAULT_ALIASES_FIELD: &str = "aliases";

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
    root: PathBuf,
    templates: TemplateConfig,
    schemas: SchemasConfig,
    frontmatter: FrontmatterConfig,
    tasks: TaskConfig,
}

impl Config {
    /// Creates a resolved config from builder-owned parts.
    #[inline]
    #[must_use]
    pub(super) const fn new(
        root: PathBuf,
        templates: TemplateConfig,
        schemas: SchemasConfig,
        frontmatter: FrontmatterConfig,
        tasks: TaskConfig,
    ) -> Self {
        Self {
            root,
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

    /// Returns the configured output directory, or [`root`] when not
    /// configured.
    ///
    /// The value is preserved as-is from the config file:
    ///
    /// - **Relative**: the caller resolves it against [`root`].
    /// - **Absolute**: used as-is; [`root`] is the fallback only when no output
    ///   directory is configured.
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
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "public accessor surface kept for API stability; only \
                      config/service.rs's config-resolution test suite reads \
                      it directly (title/aliases/date-field assertions), \
                      production code reaches [`Config`]'s per-field \
                      projection methods instead"
        )
    )]
    pub const fn frontmatter(&self) -> &FrontmatterConfig {
        &self.frontmatter
    }

    /// Returns the resolved `[tasks]` settings.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "public accessor surface kept for API stability; no \
                      production consumer until the task-system parser and \
                      classification issues land"
        )
    )]
    pub const fn tasks(&self) -> &TaskConfig {
        &self.tasks
    }

    /// Returns the project root as a cheaply shareable `'static` path, for
    /// consumers (minijinja namespace objects) that cannot borrow `&Config`.
    #[inline]
    #[must_use]
    pub(crate) fn root_arc(&self) -> Arc<Path> {
        Arc::from(self.root())
    }

    /// Returns the `[schemas] class_field` name as a cheaply shareable
    /// `'static` string, for consumers that cannot borrow `&Config`.
    #[inline]
    #[must_use]
    pub(crate) fn class_field_arc(&self) -> Arc<str> {
        Arc::from(self.schemas().class_field_name())
    }

    /// Returns the Schema registry directory resolved against the project
    /// root.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigFileError::InvalidSubDir`] when the Schema directory
    /// escapes the config root or cannot be verified.
    #[inline]
    pub(crate) fn resolved_schema_directory(
        &self,
    ) -> Result<PathBuf, ConfigFileError> {
        self.schemas.directory.resolve_against(self.root())
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
        Self {
            templates: TemplateConfig::new(local, global, output),
            schemas: SchemasConfig::default(),
            frontmatter: FrontmatterConfig::default(),
            tasks: TaskConfig::default(),
            root,
        }
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

/// Resolved `[schemas]` settings providing the class field name and registry
/// directory for template lookup.
/// A safe, root-relative subdirectory path configured in TOML (e.g. `[schemas]
/// directory`).
///
/// Guaranteed to be relative, contain only normal components (no `..`), and be
/// non-empty.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConfigSubDir(SafeRelativePath);

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
    /// Returns [`ConfigFileError::InvalidSubDir`] if the resolved path escapes
    /// `root` or fails verification.
    pub(crate) fn resolve_against(
        &self,
        root: &Path,
    ) -> Result<PathBuf, ConfigFileError> {
        RootConfinedPath::parse(root, self.as_path())
            .map(RootConfinedPath::into_path_buf)
            .map_err(|source| ConfigFileError::InvalidSubDir {
                path: self.as_path().to_path_buf(),
                source,
            })
    }
}

impl Default for ConfigSubDir {
    /// # Panics
    ///
    /// Never in practice: [`DEFAULT_SCHEMAS_DIR`] is a hardcoded, safe relative
    /// path.
    #[inline]
    #[expect(
        clippy::expect_used,
        reason = "DEFAULT_SCHEMAS_DIR is a hardcoded constant"
    )]
    fn default() -> Self {
        Self::try_from(DEFAULT_SCHEMAS_DIR)
            .expect("DEFAULT_SCHEMAS_DIR is a valid safe relative path")
    }
}

impl TryFrom<PathBuf> for ConfigSubDir {
    type Error = PathError;

    #[inline]
    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        SafeRelativePath::parse(&path).map(Self)
    }
}

impl TryFrom<&str> for ConfigSubDir {
    type Error = PathError;

    #[inline]
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        SafeRelativePath::parse(Path::new(s)).map(Self)
    }
}

/// Resolved `[schemas]` settings providing the class field name and registry
/// directory for template lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemasConfig {
    class_field: FieldName,
    directory: ConfigSubDir,
}

impl SchemasConfig {
    /// Returns the frontmatter key naming a Note's File Class(es).
    ///
    /// Defaults to `class` when unconfigured.
    #[inline]
    #[must_use]
    pub fn class_field_name(&self) -> &str {
        self.class_field.as_str()
    }

    /// Returns the Schema registry directory as configured, unresolved against
    /// [`Config::root`].
    ///
    /// Defaults to `.traces/schemas/` when unconfigured.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "test-utils exposes the configured Schema directory; \
                      production resolves through \
                      Config::resolved_schema_directory"
        )
    )]
    pub fn directory(&self) -> &Path {
        self.directory.as_path()
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
            class_field: FieldName::try_from(DEFAULT_CLASS_FIELD)
                .expect("DEFAULT_CLASS_FIELD is a valid field key"),
            directory: ConfigSubDir::default(),
        }
    }
}

impl TryFrom<RawSchemasConfig> for SchemasConfig {
    type Error = ConfigFileError;

    /// # Errors
    ///
    /// See [`FieldName::try_from`] and [`ConfigSubDir::try_from`] for when
    /// fields fail validation.
    #[inline]
    fn try_from(raw: RawSchemasConfig) -> Result<Self, Self::Error> {
        let class_field = FieldName::try_from(
            raw.class_field.unwrap_or_else(|| DEFAULT_CLASS_FIELD.to_owned()),
        )
        .map_err(|source| {
            ConfigFileError::invalid_field_key("schemas", source)
        })?;

        let directory = match raw.directory {
            Some(dir) => {
                ConfigSubDir::try_from(dir.clone()).map_err(|source| {
                    ConfigFileError::InvalidSubDir {
                        path: dir,
                        source,
                    }
                })?
            }
            None => ConfigSubDir::default(),
        };

        Ok(Self {
            class_field,
            directory,
        })
    }
}

/// Resolved `[frontmatter]` settings mapping key names for title, aliases,
/// and date roles.
#[derive(Clone, Debug)]
pub struct FrontmatterConfig {
    title: FieldName,
    aliases: FieldName,
    date_created: DateFieldConfig,
    date_modified: DateFieldConfig,
}

impl FrontmatterConfig {
    /// Returns the frontmatter key holding a Note's display title.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "public accessor surface kept for API stability; the \
                      render path uses the validated title() instead"
        )
    )]
    pub fn title_name(&self) -> &str {
        self.title.as_str()
    }

    /// Returns the frontmatter key holding a Note's aliases.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "public accessor surface kept for API stability; the \
                      render path uses the validated aliases() instead"
        )
    )]
    pub fn aliases_name(&self) -> &str {
        self.aliases.as_str()
    }

    /// Returns the creation-timestamp frontmatter key and date format.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "canonical-metadata-role declaration only; \
                      metadata-schemas spec.md User Story 24 specifies no \
                      consumer for these values, so nothing reads them yet"
        )
    )]
    pub const fn date_created(&self) -> &DateFieldConfig {
        &self.date_created
    }

    /// Returns the modification-timestamp frontmatter key and date format.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "canonical-metadata-role declaration only; \
                      metadata-schemas spec.md User Story 24 specifies no \
                      consumer for these values, so nothing reads them yet"
        )
    )]
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
}

impl Default for FrontmatterConfig {
    /// # Panics
    ///
    /// Never in practice: [`DEFAULT_TITLE_FIELD`]/[`DEFAULT_ALIASES_FIELD`]
    /// are hardcoded, always-valid field keys.
    #[inline]
    #[expect(
        clippy::expect_used,
        reason = "DEFAULT_TITLE_FIELD/DEFAULT_ALIASES_FIELD are hardcoded \
                  constants; failure here means a constant itself is \
                  malformed, not a recoverable caller error"
    )]
    fn default() -> Self {
        Self {
            title: FieldName::try_from(DEFAULT_TITLE_FIELD)
                .expect("DEFAULT_TITLE_FIELD is a valid field key"),
            aliases: FieldName::try_from(DEFAULT_ALIASES_FIELD)
                .expect("DEFAULT_ALIASES_FIELD is a valid field key"),
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
    /// See [`FieldName::try_from`] for when `raw.title` or `raw.aliases`
    /// fails validation.
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
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "canonical-metadata-role declaration only; \
                      metadata-schemas spec.md User Story 24 specifies no \
                      consumer for these values, so nothing reads them yet"
        )
    )]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Returns the date format string applied to the key's value.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "canonical-metadata-role declaration only; \
                      metadata-schemas spec.md User Story 24 specifies no \
                      consumer for these values, so nothing reads them yet"
        )
    )]
    pub fn format(&self) -> &str {
        &self.format
    }

    /// Builds a default date field config for `name` using the shared
    /// default date format.
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
    /// See [`FieldName::try_from`] for when a configured `raw.name` fails
    /// validation.
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

/// Resolved `[tasks]` settings: the task status lookup map and the tag
/// filters that classify status-marked list items as Tasks.
#[derive(Clone, Debug)]
pub struct TaskConfig {
    statuses: TaskStatusMap,
    tag_filters: Vec<Tag>,
}

impl TaskConfig {
    /// Returns the resolved task status lookup map.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "no production consumer until the custom marker scanner \
                      added in a later task-system issue"
        )
    )]
    pub const fn statuses(&self) -> &TaskStatusMap {
        &self.statuses
    }

    /// Returns the configured task tag filters.
    ///
    /// Empty means no filter is configured: every status-marked list item
    /// becomes a Task.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "no production consumer until task classification added \
                      in a later task-system issue"
        )
    )]
    pub fn tag_filters(&self) -> &[Tag] {
        &self.tag_filters
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
    /// Returns [`ConfigFileError::InvalidTagFilter`] when a `tag_filters`
    /// entry does not normalize into a valid [`Tag`].
    #[inline]
    fn try_from(raw: RawTaskConfig) -> Result<Self, Self::Error> {
        let tag_filters = raw
            .tag_filters
            .into_iter()
            .map(|entry| {
                let normalized = normalize_tag_filter(&entry);
                Tag::parse(&normalized).map_err(|source| {
                    ConfigFileError::InvalidTagFilter {
                        entry,
                        source,
                    }
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            statuses: TaskStatusMap::default(),
            tag_filters,
        })
    }
}

/// Normalizes a `[tasks] tag_filters` entry by trimming whitespace and
/// prefixing a leading `#` if absent, before constructing a [`Tag`].
///
/// Users may write `"task"` or `"#task"` interchangeably; both normalize to
/// the same internal `Tag`.
fn normalize_tag_filter(entry: &str) -> String {
    let trimmed = entry.trim();
    if trimmed.starts_with('#') {
        trimmed.to_owned()
    } else {
        format!("#{trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod resolved_schema_directory {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn joins_the_configured_schema_directory_onto_root() {
            let temp = tempfile::tempdir().expect("create temp directory");
            let root = temp.path().join("vault");
            let schemas = root.join(".traces/schemas");
            std::fs::create_dir_all(&schemas).expect("create schema directory");
            let config = Config::for_test(root.clone(), None, None, root);

            assert_eq!(
                config
                    .resolved_schema_directory()
                    .expect("schema directory resolves"),
                schemas
            );
        }

        #[cfg(unix)]
        #[test]
        fn rejects_schema_directory_that_resolves_outside_root() {
            let temp = tempfile::tempdir().expect("create temp directory");
            let root = temp.path().join("root");
            let external = temp.path().join("external");
            std::fs::create_dir_all(&root).expect("create root directory");
            std::fs::create_dir_all(&external)
                .expect("create external directory");
            std::os::unix::fs::symlink(&external, root.join(".traces"))
                .expect("create escaping schema symlink");

            let config =
                Config::for_test(root, None, None, temp.path().join("out"));
            let error = config
                .resolved_schema_directory()
                .expect_err("schema directory escapes root through symlink");

            assert!(matches!(error, ConfigFileError::InvalidSubDir {
                source: crate::path::PathError::EscapesRoot,
                ..
            }));
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
    }

    mod task_config {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn defaults_to_empty_tag_filters_and_default_statuses() {
            let config = TaskConfig::default();

            assert!(config.tag_filters().is_empty());
            assert!(config.statuses().by_symbol(' '.into()).is_some());
        }

        #[test]
        fn normalizes_entries_with_and_without_a_leading_hash() {
            let raw = RawTaskConfig {
                tag_filters: vec!["task".to_owned(), "#todo".to_owned()],
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
            };

            let config = TaskConfig::try_from(raw).expect("empty is valid");

            assert!(config.tag_filters().is_empty());
        }

        #[test]
        fn rejects_an_invalid_tag_filter_entry() {
            let raw = RawTaskConfig {
                tag_filters: vec!["1invalid".to_owned()],
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
            };

            let error = TaskConfig::try_from(raw).expect_err("blank entry");

            assert!(matches!(error, ConfigFileError::InvalidTagFilter { .. }));
        }
    }
}
