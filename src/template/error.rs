//! Errors from the resolve -> read -> render -> write pipeline behind
//! [`super::TemplateService::render_to_file`].
//!
//! `thiserror`-only, no `miette::Diagnostic` — `crate::cli::error` is
//! where user-facing help text and error codes get added, matching
//! `crate::config`'s convention.

use std::{io, path::PathBuf};

use thiserror::Error;

use super::path::TemplatePathError;

/// One variant per pipeline stage, so a caller can tell which stage
/// failed without inspecting the wrapped source error.
#[derive(Debug, Error)]
pub(crate) enum TemplateError {
    /// `name` failed to resolve to a file. Transparent:
    /// [`TemplatePathError`]'s own [`Display`](std::fmt::Display)
    /// already names the template and what went wrong, so this variant
    /// adds no field of its own.
    #[error(transparent)]
    Resolve(#[from] TemplatePathError),

    /// The resolved template could not be read from disk.
    #[error("failed to read template {path}")]
    Read {
        /// The resolved template's absolute path.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },

    /// The output path already exists and `force` was not passed.
    #[error("output file already exists at {path}")]
    OutputFileAlreadyExists {
        /// The path that would have been overwritten.
        path: PathBuf,
    },

    /// `path` — from `file.write_to()`, `-o`, or (should config ever
    /// allow it) the computed default — is absolute or contains a `..`
    /// component, so it would write outside the project root.
    #[error("output path {path} escapes the project root")]
    OutputPathEscapesRoot {
        /// The rejected candidate, exactly as given.
        path: PathBuf,
    },

    /// The template's source failed to render.
    #[error("failed to render template {path}")]
    Render {
        /// The resolved template's absolute path.
        path: PathBuf,
        /// The underlying minijinja error.
        #[source]
        source: minijinja::Error,
    },

    /// The rendered output could not be written to disk.
    #[error("failed to write rendered output to {path}")]
    Write {
        /// The output path the render was writing to.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },
}
