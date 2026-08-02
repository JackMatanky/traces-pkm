//! Resolved config model for consumers.
//!
//! # Main Types
//!
//! - [`Config`] - Exposes the project root, template directories, and output
//!   directory after discovery, trust checks, and merging.
//! - [`TemplateConfig`] - Preserves local and global template directories so
//!   template lookup can stay local-first.

use std::path::{Path, PathBuf};

/// Merged local/global configuration ready for consumers.
#[derive(Clone, Debug)]
pub(crate) struct Config {
    root: PathBuf,
    templates: TemplateConfig,
}

impl Config {
    /// Creates a resolved config from builder-owned parts.
    #[inline]
    #[must_use]
    pub(super) fn new(root: PathBuf, templates: TemplateConfig) -> Self {
        Self {
            root,
            templates,
        }
    }

    /// Project root directory used as the local resolution base.
    #[inline]
    #[must_use]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Local template directory, if one was configured.
    #[inline]
    #[must_use]
    pub(crate) fn local_template_dir(&self) -> Option<&Path> {
        self.templates.local()
    }

    /// Global template directory, if one was configured.
    #[inline]
    #[must_use]
    pub(crate) fn global_template_dir(&self) -> Option<&Path> {
        self.templates.global()
    }

    /// The configured output directory, or [`root`](Self::root) when not
    /// configured.
    ///
    /// May be relative (preserved unresolved from the config file) or absolute
    /// (the [`root`](Self::root) fallback); callers needing an absolute path
    /// resolve a relative result against [`root`](Self::root) themselves.
    #[inline]
    #[must_use]
    pub(crate) fn output_dir(&self) -> &Path {
        self.templates.output()
    }

    /// Builds config directly for tests that do not exercise discovery.
    ///
    /// Prefer [`super::service::ConfigService::at`] and TOML fixtures for
    /// integration-style tests that need the real loading pipeline.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn for_test(
        root: PathBuf,
        local: Option<PathBuf>,
        global: Option<PathBuf>,
        output: PathBuf,
    ) -> Self {
        Self {
            templates: TemplateConfig::new(local, global, output),
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

    /// The local project template directory, if set.
    #[inline]
    #[must_use]
    pub(super) fn local(&self) -> Option<&Path> {
        self.local.as_deref()
    }

    /// The global template directory, if set.
    #[inline]
    #[must_use]
    pub(super) fn global(&self) -> Option<&Path> {
        self.global.as_deref()
    }

    /// The configured output directory, or the config root when absent.
    #[inline]
    #[must_use]
    pub(super) fn output(&self) -> &Path {
        &self.output
    }
}
