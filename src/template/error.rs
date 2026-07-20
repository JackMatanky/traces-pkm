//! Errors from the resolve -> render -> write pipeline.
//!
//! `thiserror`-only, no `miette::Diagnostic` — `crate::cli::error` is where
//! user-facing help text and error codes get added, matching
//! `crate::config`'s convention.

use std::{io, path::PathBuf};

use thiserror::Error;

use super::path::TemplatePathError;

/// Errors from [`super::TemplateService::render_to_file`].
#[derive(Debug, Error)]
pub(crate) enum TemplateError {
    /// Resolving a template name against the configured template
    /// directories failed. Transparent: `TemplatePathError`'s own
    /// `Display` already names the template and what went wrong, so
    /// this variant adds no field of its own to duplicate it.
    #[error(transparent)]
    Resolve(#[from] TemplatePathError),

    /// Reading the resolved template file failed.
    #[error("failed to read template file {path}")]
    Read {
        /// The template file that could not be read.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },

    /// Rendering the template source failed.
    #[error("failed to render template {path}")]
    Render {
        /// The template file whose source failed to render.
        path: PathBuf,
        /// Source minijinja error.
        #[source]
        source: minijinja::Error,
    },

    /// Writing the rendered output failed.
    #[error("failed to write rendered output to {path}")]
    Write {
        /// The output path that could not be written.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
}
