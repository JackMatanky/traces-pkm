//! Error types for the template resolve, read, render, and write pipeline.
//!
//! These errors stay presentation-neutral: `thiserror` records the failing
//! stage and source error, while `crate::cli::error` adds user-facing help text
//! and diagnostic codes.

use std::{error::Error as StdError, io, path::PathBuf};

use thiserror::Error;

use super::path::TemplatePathError;
use crate::{
    DialogError,
    index::{FileIndexError, QueryError},
};

/// Identifies pipeline stage failures, allowing callers to determine which
/// stage failed without inspecting the wrapped source error.
#[derive(Debug, Error)]
pub(crate) enum TemplateError {
    /// `name` failed to resolve to a file. Transparent: [`TemplatePathError`]'s
    /// own [`Display`] already names the template and what went wrong.
    ///
    /// [`Display`]: std::fmt::Display
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

    /// `path` comes from `file.write_to()` or `-o` and is absolute or contains
    /// a `..` component, so it would write outside the project root.
    #[error("output path {path} escapes the project root")]
    OutputPathEscapesRoot {
        /// The rejected candidate, exactly as given.
        path: PathBuf,
    },

    /// `path` comes from `file.write_to()` or `-o` and could not be verified as
    /// staying inside the project root. Canonicalizing the root or the path's
    /// existing ancestor failed for a reason other than nonexistence
    /// (permission denied, a broken symlink loop). The pipeline fails closed
    /// rather than writing an unverified path.
    #[error("failed to verify output path {path} is inside the project root")]
    OutputPathUnverifiable {
        /// The candidate that could not be verified.
        path: PathBuf,
        /// The underlying I/O error from canonicalization.
        #[source]
        source: io::Error,
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

    /// An interactive prompt (picker or collision prompt) failed or was
    /// cancelled.
    #[error(transparent)]
    Prompt(#[from] DialogError),
}

/// Coarse category for a [`TemplateError::Render`] failure.
///
/// Provides just enough detail for `crate::cli::error` to choose a stable
/// diagnostic code and help text. Classification inspects
/// [`minijinja::Error::kind`] and the retained source chain instead of
/// parsing display text, so new custom functions don't need to update
/// string-matching logic in the CLI diagnostic layer.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum RenderFailureKind {
    /// The template's own minijinja syntax is invalid.
    Syntax,
    /// An interactive `ui.*` prompt failed for a reason other than a
    /// deliberate user abort (handled separately, upstream of this
    /// classification).
    Prompt,
    /// A `query.*` call failed on a malformed field path, filter expression,
    /// sort path, or `.table()` header/column mismatch.
    Query,
    /// Refreshing the file index for a `query.*` call failed: a filesystem,
    /// database, or (de)serialization error, not a template authoring
    /// mistake.
    Index,
    /// A `file.include()` (or other Custom Function) I/O operation failed.
    Io,
    /// Anything else: an unknown function/filter/test, a bad argument, an
    /// undefined-value operation, or another engine-level failure.
    Other,
}

/// Classifies `error` per [`RenderFailureKind`].
pub(crate) fn classify_render_error(
    error: &minijinja::Error,
) -> RenderFailureKind {
    if error.kind() == minijinja::ErrorKind::SyntaxError {
        return RenderFailureKind::Syntax;
    }
    let mut cause: Option<&(dyn StdError + 'static)> = StdError::source(error);
    while let Some(err) = cause {
        if err.downcast_ref::<DialogError>().is_some() {
            return RenderFailureKind::Prompt;
        }
        if err.downcast_ref::<QueryError>().is_some() {
            return RenderFailureKind::Query;
        }
        if err.downcast_ref::<FileIndexError>().is_some() {
            return RenderFailureKind::Index;
        }
        if err.downcast_ref::<io::Error>().is_some() {
            return RenderFailureKind::Io;
        }
        cause = err.source();
    }
    RenderFailureKind::Other
}
