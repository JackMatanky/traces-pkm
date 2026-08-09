//! Resolved configuration model produced by the builder pipeline.
//!
//! # Types
//!
//! - [`Config`] merges local/global settings into read-only resolved values.
//! - [`TemplateConfig`] preserves local and global template directories.
//! - [`SchemasConfig`] resolves the `[schemas]` class field and registry path.
//! - [`FrontmatterConfig`] resolves `[frontmatter]` key names.
//! - [`DateFieldConfig`] pairs a date frontmatter key with its format.

use std::path::{Path, PathBuf};

use super::raw::{RawDateFieldConfig, RawFrontmatterConfig, RawSchemasConfig};

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
    /// - **Absolute**: this is the [`root`] fallback.
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

    /// Builds config directly for tests that need a non-default `[frontmatter]`
    /// resolution.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn for_test_with_frontmatter(
        root: PathBuf,
        local: Option<PathBuf>,
        global: Option<PathBuf>,
        output: PathBuf,
        frontmatter: FrontmatterConfig,
    ) -> Self {
        Self {
            templates: TemplateConfig::new(local, global, output),
            schemas: SchemasConfig::default(),
            frontmatter,
            root,
        }
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
    class_field: String,
    directory: PathBuf,
}

impl SchemasConfig {
    /// Returns the frontmatter key naming a Note's File Class(es).
    /// Defaults to `class`.
    #[inline]
    #[must_use]
    pub fn class_field(&self) -> &str {
        &self.class_field
    }

    /// Returns the Schema registry directory, as configured (unresolved against
    /// [`Config::root`]). Defaults to `.traces/schemas/`.
    #[inline]
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

impl Default for SchemasConfig {
    #[inline]
    fn default() -> Self {
        Self {
            class_field: DEFAULT_CLASS_FIELD.to_owned(),
            directory: PathBuf::from(DEFAULT_SCHEMAS_DIR),
        }
    }
}

impl From<RawSchemasConfig> for SchemasConfig {
    #[inline]
    fn from(raw: RawSchemasConfig) -> Self {
        Self {
            class_field: raw
                .class_field
                .unwrap_or_else(|| DEFAULT_CLASS_FIELD.to_owned()),
            directory: raw
                .directory
                .unwrap_or_else(|| PathBuf::from(DEFAULT_SCHEMAS_DIR)),
        }
    }
}

/// Resolved `[frontmatter]` settings mapping key names for title, aliases, and
/// date roles.
#[derive(Clone, Debug)]
pub struct FrontmatterConfig {
    title: String,
    aliases: String,
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "declared by the config-surface ticket; read by later \
                      frontmatter-aware tickets \
                      (.scratch/metadata-schemas/issues/01-config-surface.md)"
        )
    )]
    date_created: DateFieldConfig,
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "declared by the config-surface ticket; read by later \
                      frontmatter-aware tickets \
                      (.scratch/metadata-schemas/issues/01-config-surface.md)"
        )
    )]
    date_modified: DateFieldConfig,
}

impl FrontmatterConfig {
    /// Returns the frontmatter key holding a Note's display title.
    #[inline]
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns the frontmatter key holding a Note's aliases.
    #[inline]
    #[must_use]
    pub fn aliases(&self) -> &str {
        &self.aliases
    }

    /// Returns the creation-timestamp frontmatter key and date format.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "declared by the config-surface ticket; read by later \
                      frontmatter-aware tickets \
                      (.scratch/metadata-schemas/issues/01-config-surface.md)"
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
            reason = "declared by the config-surface ticket; read by later \
                      frontmatter-aware tickets \
                      (.scratch/metadata-schemas/issues/01-config-surface.md)"
        )
    )]
    pub fn date_modified(&self) -> &DateFieldConfig {
        &self.date_modified
    }

    /// Builds a frontmatter config with custom title/aliases keys for tests
    /// that exercise non-default label resolution.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn for_test(
        title: impl Into<String>,
        aliases: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            aliases: aliases.into(),
            ..Self::default()
        }
    }
}

impl Default for FrontmatterConfig {
    #[inline]
    fn default() -> Self {
        Self {
            title: DEFAULT_TITLE_FIELD.to_owned(),
            aliases: DEFAULT_ALIASES_FIELD.to_owned(),
            date_created: DateFieldConfig::default_for(
                DEFAULT_DATE_CREATED_FIELD,
            ),
            date_modified: DateFieldConfig::default_for(
                DEFAULT_DATE_MODIFIED_FIELD,
            ),
        }
    }
}

impl From<RawFrontmatterConfig> for FrontmatterConfig {
    #[inline]
    fn from(raw: RawFrontmatterConfig) -> Self {
        Self {
            title: raw.title.unwrap_or_else(|| DEFAULT_TITLE_FIELD.to_owned()),
            aliases: raw
                .aliases
                .unwrap_or_else(|| DEFAULT_ALIASES_FIELD.to_owned()),
            date_created: DateFieldConfig::from_raw_or_default(
                raw.date_created,
                DEFAULT_DATE_CREATED_FIELD,
            ),
            date_modified: DateFieldConfig::from_raw_or_default(
                raw.date_modified,
                DEFAULT_DATE_MODIFIED_FIELD,
            ),
        }
    }
}

/// A frontmatter key name and its date format string.
#[derive(Clone, Debug)]
pub struct DateFieldConfig {
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "declared by the config-surface ticket; read by later \
                      frontmatter-aware tickets \
                      (.scratch/metadata-schemas/issues/01-config-surface.md)"
        )
    )]
    name: String,
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "declared by the config-surface ticket; read by later \
                      frontmatter-aware tickets \
                      (.scratch/metadata-schemas/issues/01-config-surface.md)"
        )
    )]
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
            reason = "declared by the config-surface ticket; read by later \
                      frontmatter-aware tickets \
                      (.scratch/metadata-schemas/issues/01-config-surface.md)"
        )
    )]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the date format string applied to the key's value.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "declared by the config-surface ticket; read by later \
                      frontmatter-aware tickets \
                      (.scratch/metadata-schemas/issues/01-config-surface.md)"
        )
    )]
    pub fn format(&self) -> &str {
        &self.format
    }

    /// Builds a default date field config for `name` using the shared
    /// default date format.
    #[inline]
    fn default_for(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            format: DEFAULT_DATE_FORMAT.to_owned(),
        }
    }

    /// Resolves a raw date-role table into a concrete config, filling missing
    /// `name`/`format` from role-aware defaults.
    #[inline]
    fn from_raw_or_default(
        raw: Option<RawDateFieldConfig>,
        default_name: &str,
    ) -> Self {
        raw.map_or_else(
            || Self::default_for(default_name),
            |raw| Self {
                name: raw.name.unwrap_or_else(|| default_name.to_owned()),
                format: raw
                    .format
                    .unwrap_or_else(|| DEFAULT_DATE_FORMAT.to_owned()),
            },
        )
    }
}
