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
    /// May be relative (a literal `output_dir = "…"` from a config file is
    /// preserved unresolved — see [`super::builder::ConfigBuilder::merge`]'s
    /// docs) or absolute (the [`root`](Self::root) fallback). Consumers
    /// that need an absolute path — `crate::template::service`'s default
    /// output path, currently the only consumer — resolve a relative
    /// result against [`root`](Self::root) themselves.
    #[inline]
    #[must_use]
    pub(crate) fn output_dir(&self) -> &Path {
        self.templates.output()
    }

    /// Test-only constructor that builds a `Config` directly, bypassing
    /// discovery, trust-gating, and the builder pipeline.
    ///
    /// Lets `crate::template`'s tests exercise resolution/rendering logic
    /// against arbitrary directory layouts without standing up a full
    /// [`super::builder::ConfigBuilder`] pipeline, mirroring
    /// [`super::service::ConfigService::at`]'s test-only role for its own
    /// module.
    ///
    /// TODO(remove): this exists only because `template::`'s tests need
    /// arbitrary `Config` values without a real TOML fixture per test —
    /// every `template::` unit test currently uses this instead of the
    /// real pipeline; `cli::template`'s tests are the only ones that
    /// exercise `ConfigService::discover`/`build` directly (see
    /// `cli::template::tests::create_config` for the TOML-fixture
    /// pattern to mirror). To remove: rewrite `template::`'s tests to
    /// build real TOML fixtures and go through
    /// `ConfigService::at(...).discover(...).build(...)`, then delete
    /// this constructor and its `#[cfg(test)]` gate.
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
/// Keeps local and global directories separately. Resolution (try local
/// first, fall back to global) lives in `crate::template::resolve`.
/// Fields are private with accessors — [`super::builder::ConfigBuilder`]
/// builds this through [`Self::new`] rather than a struct literal, so
/// this type alone owns what "unset" and "relative vs. resolved" mean for
/// each field.
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
    /// Creates a `TemplateConfig` from builder-owned parts.
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
