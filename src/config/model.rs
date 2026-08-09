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

use std::path::{Path, PathBuf};

use super::raw::{RawDateFieldConfig, RawFrontmatterConfig, RawSchemasConfig};
use crate::field::{FieldKey, FieldKeyError};

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
}

impl Config {
    /// Creates a resolved config from builder-owned parts.
    #[inline]
    #[must_use]
    pub(super) fn new(
        root: PathBuf,
        templates: TemplateConfig,
        schemas: SchemasConfig,
        frontmatter: FrontmatterConfig,
    ) -> Self {
        Self {
            root,
            templates,
            schemas,
            frontmatter,
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
    pub fn schemas(&self) -> &SchemasConfig {
        &self.schemas
    }

    /// Returns the resolved `[frontmatter]` settings.
    #[inline]
    #[must_use]
    pub fn frontmatter(&self) -> &FrontmatterConfig {
        &self.frontmatter
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
    pub(super) fn new(
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
#[derive(Clone, Debug)]
pub struct SchemasConfig {
    class_field: FieldKey,
    directory: PathBuf,
}

impl SchemasConfig {
    /// Returns the frontmatter key naming a Note's File Class(es) as a
    /// validated [`FieldKey`].
    ///
    /// Used for canonical-form matching against Note frontmatter.
    /// Defaults to `class`.
    #[inline]
    #[must_use]
    pub(crate) fn class_field(&self) -> &FieldKey {
        &self.class_field
    }

    /// Returns the frontmatter key naming a Note's File Class(es).
    ///
    /// Defaults to `class` when unconfigured.
    #[inline]
    #[must_use]
    pub fn class_field_name(&self) -> &str {
        self.class_field.name()
    }

    /// Returns the Schema registry directory as configured, unresolved against
    /// [`Config::root`].
    ///
    /// Defaults to `.traces/schemas/` when unconfigured.
    #[inline]
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
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
            class_field: FieldKey::try_new(DEFAULT_CLASS_FIELD)
                .expect("DEFAULT_CLASS_FIELD is a valid field key"),
            directory: PathBuf::from(DEFAULT_SCHEMAS_DIR),
        }
    }
}

impl TryFrom<RawSchemasConfig> for SchemasConfig {
    type Error = FieldKeyError;

    /// # Errors
    ///
    /// See [`FieldKey::try_new`] for when `raw.class_field` fails validation.
    #[inline]
    fn try_from(raw: RawSchemasConfig) -> Result<Self, Self::Error> {
        Ok(Self {
            class_field: FieldKey::try_new(
                raw.class_field
                    .unwrap_or_else(|| DEFAULT_CLASS_FIELD.to_owned()),
            )?,
            directory: raw
                .directory
                .unwrap_or_else(|| PathBuf::from(DEFAULT_SCHEMAS_DIR)),
        })
    }
}

/// Resolved `[frontmatter]` settings mapping key names for title, aliases,
/// and date roles.
#[derive(Clone, Debug)]
pub struct FrontmatterConfig {
    title: FieldKey,
    aliases: FieldKey,
    date_created: DateFieldConfig,
    date_modified: DateFieldConfig,
}

impl FrontmatterConfig {
    /// Returns the frontmatter key holding a Note's display title as a
    /// validated [`FieldKey`].
    ///
    /// Used for canonical-form matching against Note frontmatter.
    #[inline]
    #[must_use]
    pub(crate) fn title(&self) -> &FieldKey {
        &self.title
    }

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
        self.title.name()
    }

    /// Returns the frontmatter key holding a Note's aliases as a validated
    /// [`FieldKey`].
    ///
    /// Used for canonical-form matching against Note frontmatter.
    #[inline]
    #[must_use]
    pub(crate) fn aliases(&self) -> &FieldKey {
        &self.aliases
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
        self.aliases.name()
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
    pub fn date_created(&self) -> &DateFieldConfig {
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
    pub fn date_modified(&self) -> &DateFieldConfig {
        &self.date_modified
    }

    /// Builds a frontmatter config with custom title/aliases keys for tests
    /// that exercise non-default label resolution.
    ///
    /// # Panics
    ///
    /// If `title` or `aliases` fails `FieldKey` validation (empty or
    /// whitespace-only) — a test-fixture bug, not a runtime error path.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "test-only constructor; an invalid literal here is a test \
                  fixture bug, not a recoverable caller error"
    )]
    pub fn for_test(
        title: impl Into<String>,
        aliases: impl Into<String>,
    ) -> Self {
        Self {
            title: FieldKey::try_new(title.into())
                .expect("test fixture title is a valid field key"),
            aliases: FieldKey::try_new(aliases.into())
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
            title: FieldKey::try_new(DEFAULT_TITLE_FIELD)
                .expect("DEFAULT_TITLE_FIELD is a valid field key"),
            aliases: FieldKey::try_new(DEFAULT_ALIASES_FIELD)
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
    type Error = FieldKeyError;

    /// # Errors
    ///
    /// See [`FieldKey::try_new`] for when `raw.title` or `raw.aliases` fails
    /// validation.
    #[inline]
    fn try_from(raw: RawFrontmatterConfig) -> Result<Self, Self::Error> {
        Ok(Self {
            title: FieldKey::try_new(
                raw.title.unwrap_or_else(|| DEFAULT_TITLE_FIELD.to_owned()),
            )?,
            aliases: FieldKey::try_new(
                raw.aliases.unwrap_or_else(|| DEFAULT_ALIASES_FIELD.to_owned()),
            )?,
            date_created: DateFieldConfig::from_raw_or_default(
                raw.date_created,
                DEFAULT_DATE_CREATED_FIELD,
            )?,
            date_modified: DateFieldConfig::from_raw_or_default(
                raw.date_modified,
                DEFAULT_DATE_MODIFIED_FIELD,
            )?,
        })
    }
}

/// A frontmatter key name and its date format string.
#[derive(Clone, Debug)]
pub struct DateFieldConfig {
    name: FieldKey,
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
        self.name.name()
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
            name: FieldKey::try_new(name)
                .expect("role-name constant is a valid field key"),
            format: DEFAULT_DATE_FORMAT.to_owned(),
        }
    }

    /// Resolves a raw date-role table into a concrete config, filling missing
    /// `name`/`format` from role-aware defaults.
    ///
    /// # Errors
    ///
    /// See [`FieldKey::try_new`] for when a configured `raw.name` fails
    /// validation.
    #[inline]
    fn from_raw_or_default(
        raw: Option<RawDateFieldConfig>,
        default_name: &str,
    ) -> Result<Self, FieldKeyError> {
        Ok(match raw {
            None => Self::default_for(default_name),
            Some(raw) => Self {
                name: FieldKey::try_new(
                    raw.name.unwrap_or_else(|| default_name.to_owned()),
                )?,
                format: raw
                    .format
                    .unwrap_or_else(|| DEFAULT_DATE_FORMAT.to_owned()),
            },
        })
    }
}
