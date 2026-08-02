//! Resolved configuration consumed by CLI and template code.
//!
//! [`Config`] exposes the project root and template settings after discovery,
//! trust checks, and config merging. [`TemplateConfig`] keeps local and global
//! template directories separate so `crate::template` can resolve local first.

use std::path::{Path, PathBuf};

/// Merged configuration ready for consumers.
#[derive(Clone, Debug)]
pub(crate) struct Config {
    /// Project root directory.
    root: PathBuf,
    /// Template directories and output path from merged config.
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

    /// The project root directory.
    #[inline]
    #[must_use]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// The local template directory, if set.
    #[inline]
    #[must_use]
    pub(crate) fn local_template_dir(&self) -> Option<&Path> {
        self.templates.local()
    }

    /// The global template directory, if set.
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
/// Keeps local and global directories separately; resolution (try local first,
/// fall back to global) lives in `crate::template`.
#[derive(Clone, Debug)]
pub(super) struct TemplateConfig {
    /// Local project template directory (from `.traces/config.toml`).
    local: Option<PathBuf>,
    /// Global template directory (from `~/.config/traces/config.toml`).
    global: Option<PathBuf>,
    /// Configured `output_dir`, or the config root when absent.
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
