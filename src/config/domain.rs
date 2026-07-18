//! Domain types: resolved `Config` and `TemplateConfig`.
//!
//! Template resolution — matching a template name against these
//! directories — is `crate::template::resolve`'s concern, not this
//! module's (moved out in issue tmpl-01): this module only holds
//! parsed/merged config data and the read-only accessors template-service
//! resolves through.

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
        self.templates.local.as_deref()
    }

    /// The global template directory, if set.
    #[inline]
    #[must_use]
    pub(crate) fn global_template_dir(&self) -> Option<&Path> {
        self.templates.global.as_deref()
    }

    /// The configured output path, or [`root`](Self::root) when not
    /// configured.
    ///
    /// Not read by the render pipeline tracer (issue tmpl-01): its default
    /// output path is always `./<template-stem>.md`, computed by
    /// `crate::template::service` from the resolved template itself, not
    /// from this setting. Issue tmpl-02's `-o`/`set_output()` handling is
    /// the intended consumer.
    #[inline]
    #[must_use]
    pub(crate) fn output_dir(&self) -> &Path {
        &self.templates.output
    }

    /// Test-only constructor that builds a `Config` directly, bypassing
    /// discovery and the builder pipeline.
    ///
    /// Lets `crate::template::resolve`'s tests exercise resolution logic
    /// against arbitrary directory layouts without standing up a full
    /// [`super::builder::ConfigBuilder`] pipeline, mirroring
    /// [`super::service::ConfigService::at`]'s test-only role for its own
    /// module.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn for_test(
        root: PathBuf,
        local: Option<PathBuf>,
        global: Option<PathBuf>,
    ) -> Self {
        Self {
            templates: TemplateConfig {
                local,
                global,
                output: root.clone(),
            },
            root,
        }
    }
}

/// Template directories and output path from merged config.
///
/// Keeps local and global directories separately. Resolution (try local
/// first, fall back to global) lives in `crate::template::resolve`.
#[derive(Clone, Debug)]
pub(super) struct TemplateConfig {
    /// Local project template directory (from `.traces/config.toml`).
    pub(super) local: Option<PathBuf>,
    /// Global template directory (from `~/.config/traces/config.toml`).
    pub(super) global: Option<PathBuf>,
    /// Configured `output_dir`, or the config root when absent.
    pub(super) output: PathBuf,
}
