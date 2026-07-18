//! Errors from the resolve -> render -> write pipeline.
//!
//! `thiserror`-only, no `miette::Diagnostic` — `crate::cli::error` is where
//! user-facing help text and error codes get added, matching
//! `crate::config`'s convention.

use std::{io, path::PathBuf};

use thiserror::Error;

use super::resolve::ResolutionError;

/// Errors from [`super::TemplateService::instantiate`].
#[derive(Debug, Error)]
pub(crate) enum TemplateError {
    /// Resolving `name` against the configured template directories
    /// failed.
    #[error("failed to resolve template {name}")]
    Resolve {
        /// The template name that was searched for.
        name: PathBuf,
        /// Source resolution error.
        #[source]
        source: ResolutionError,
    },

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
