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

/// Resolved config ready for consumers after discovery, trust checks, and
/// merging.
#[derive(Clone, Debug)]
pub struct Config {
    root: PathBuf,
    templates: TemplateConfig,
    schemas: SchemasConfig,
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "declared by the config-surface ticket; read by later \
                      frontmatter-aware tickets \
                      (.scratch/metadata-schemas/issues/01-config-surface.md)"
        )
    )]
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
    /// May be relative (preserved unresolved from the config file) or absolute
    /// (the [`root`] fallback); callers needing an absolute path resolve a
    /// relative result against [`root`] themselves.
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
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "declared by the config-surface ticket; read by later \
                      frontmatter-aware tickets \
                      (.scratch/metadata-schemas/issues/01-config-surface.md)"
        )
    )]
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
}

/// Template directories and output path from merged config.
///
/// Keeps local and global directories separate so template lookup can preserve
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
    /// Returns the frontmatter key naming a Note's File Class(es). Defaults
    /// to `class`.
    #[inline]
    #[must_use]
    pub fn class_field(&self) -> &str {
        &self.class_field
    }

    /// Returns the Schema registry directory, as configured (unresolved
    /// against [`Config::root`]). Defaults to `.traces/schemas/`.
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

/// Resolved `[frontmatter]` settings mapping key names for title, aliases,
/// and date roles.
#[derive(Clone, Debug, Default)]
pub struct FrontmatterConfig {
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "declared by the config-surface ticket; read by later \
                      frontmatter-aware tickets \
                      (.scratch/metadata-schemas/issues/01-config-surface.md)"
        )
    )]
    title: Option<String>,
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "declared by the config-surface ticket; read by later \
                      frontmatter-aware tickets \
                      (.scratch/metadata-schemas/issues/01-config-surface.md)"
        )
    )]
    aliases: Option<String>,
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "declared by the config-surface ticket; read by later \
                      frontmatter-aware tickets \
                      (.scratch/metadata-schemas/issues/01-config-surface.md)"
        )
    )]
    date_created: Option<DateFieldConfig>,
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "declared by the config-surface ticket; read by later \
                      frontmatter-aware tickets \
                      (.scratch/metadata-schemas/issues/01-config-surface.md)"
        )
    )]
    date_modified: Option<DateFieldConfig>,
}

impl FrontmatterConfig {
    /// Returns the frontmatter key holding a Note's display title, if
    /// configured.
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
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Returns the frontmatter key holding a Note's aliases, if configured.
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
    pub fn aliases(&self) -> Option<&str> {
        self.aliases.as_deref()
    }

    /// Returns the creation-timestamp frontmatter key and date format, if
    /// configured.
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
    pub fn date_created(&self) -> Option<&DateFieldConfig> {
        self.date_created.as_ref()
    }

    /// Returns the modification-timestamp frontmatter key and date format,
    /// if configured.
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
    pub fn date_modified(&self) -> Option<&DateFieldConfig> {
        self.date_modified.as_ref()
    }
}

impl From<RawFrontmatterConfig> for FrontmatterConfig {
    #[inline]
    fn from(raw: RawFrontmatterConfig) -> Self {
        Self {
            title: raw.title,
            aliases: raw.aliases,
            date_created: raw.date_created.map(DateFieldConfig::from),
            date_modified: raw.date_modified.map(DateFieldConfig::from),
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
}

impl From<RawDateFieldConfig> for DateFieldConfig {
    #[inline]
    fn from(raw: RawDateFieldConfig) -> Self {
        Self {
            name: raw.name,
            format: raw.format,
        }
    }
}
