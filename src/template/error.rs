//! Define errors for template resolution, rendering, and writing.
//!
//! [`TemplateError`] is the primary error returned by [`TemplateService`]. It
//! records the failing pipeline stage and preserves source errors. CLI code
//! adds diagnostic codes and help text outside this module.
//!
//! Public API:
//!
//! - [`TemplateError`] - Resolve, read, render, output-path, write, and prompt
//!   failures.
//! - [`RenderFailureKind`] - Coarse render-error category for diagnostics.
//!
//! [`TemplateService`]: super::service::TemplateService

use std::{error::Error as StdError, io, path::PathBuf};

use thiserror::Error;

use super::path::TemplatePathError;
use crate::{DialogError, index::IndexError, query::QueryError};

/// Reports the failed stage of a template operation.
///
/// Variants preserve their source error when one exists. Callers can match the
/// variant instead of parsing display text.
#[derive(Debug, Error)]
pub enum TemplateError {
    /// Resolving the requested template identifier failed.
    ///
    /// Covers validation failures, ambiguous stem matches, unreadable template
    /// directories, and missing templates. The wrapped [`TemplatePathError`]
    /// names the exact condition.
    #[error(transparent)]
    Resolve(#[from] TemplatePathError),

    /// Reading the resolved template source failed.
    #[error("failed to read template {path}")]
    Read {
        /// Absolute path selected by template resolution.
        path: PathBuf,
        /// Filesystem error returned while reading the source file.
        #[source]
        source: io::Error,
    },

    /// Creating a new output file would overwrite an existing file.
    ///
    /// Returned only for [`CommitPolicy::CreateNew`].
    /// [`CommitPolicy::Overwrite`] truncates the file instead.
    ///
    /// [`CommitPolicy::CreateNew`]: super::writer::CommitPolicy::CreateNew
    /// [`CommitPolicy::Overwrite`]: super::writer::CommitPolicy::Overwrite
    #[error("output file already exists at {path}")]
    OutputFileAlreadyExists {
        /// Existing file that blocked the write.
        path: PathBuf,
    },

    /// An explicit output path could escape the project root.
    ///
    /// Covers `-o` and `file.write_to()` values that are absolute, contain
    /// unsafe components such as `..`, or resolve through a symlink outside the
    /// root.
    #[error("output path {path} escapes the project root")]
    OutputPathEscapesRoot {
        /// Rejected output candidate as supplied by the caller or template.
        path: PathBuf,
    },

    /// An explicit output path could not be verified against the project root.
    ///
    /// Returned when canonicalizing the root or an existing ancestor fails, for
    /// example because of permissions or a symlink loop. The writer fails
    /// closed instead of writing to an unverified path.
    #[error("failed to verify output path {path} is inside the project root")]
    OutputPathUnverifiable {
        /// Output candidate whose confinement could not be verified.
        path: PathBuf,
        /// Filesystem error returned while canonicalizing the path.
        #[source]
        source: io::Error,
    },

    /// Rendering the template source failed.
    ///
    /// Covers minijinja syntax errors, missing helper functions, bad helper
    /// arguments, include or extends failures, query/index/schema failures,
    /// file include failures, and dialog failures raised during rendering.
    #[error("failed to render template {path}")]
    Render {
        /// Absolute path used as the minijinja template name.
        path: PathBuf,
        /// minijinja error with template context and source-chain details.
        #[source]
        source: minijinja::Error,
    },

    /// Writing rendered output failed.
    ///
    /// Covers parent directory creation, file creation except existing-file
    /// collisions reported by [`Self::OutputFileAlreadyExists`], and writing
    /// the rendered bytes.
    #[error("failed to write rendered output to {path}")]
    Write {
        /// Target path being written.
        path: PathBuf,
        /// Filesystem error returned while creating or writing the file.
        #[source]
        source: io::Error,
    },

    /// An interactive output-collision prompt failed or was cancelled.
    ///
    /// Covers the writer prompt shown before replacing an existing output file.
    #[error(transparent)]
    Prompt(#[from] DialogError),

    /// Loading the Schema registry failed during service construction.
    ///
    /// Reached only for registry-wide failures: the registry directory could
    /// not be read or listed, a Schema file's TOML syntax is malformed or has
    /// an unknown key, or the `extends` DAG contains a cycle; see the wrapped
    /// [`SchemaError`](crate::schema::SchemaError) for which.
    ///
    /// A field-level defect *within* an otherwise well-formed Schema TOML file
    /// (an invalid attribute value, an out-of-bounds `$ref`, an ambiguous field
    /// name) never reaches this variant: that Schema alone is excluded from the
    /// registry and logged as a construction-time failure instead, while every
    /// other Schema still resolves.
    #[error(transparent)]
    #[expect(
        private_interfaces,
        reason = "SchemaError is deliberately crate-internal; TemplateError \
                  is only pub behind the test-utils/test cfg, and transparent \
                  wrapping only needs Display/Error, never external code \
                  naming SchemaError itself"
    )]
    SchemaLoad(#[from] crate::schema::SchemaError),
}

/// Classifies a [`TemplateError::Render`] failure for diagnostics.
///
/// Provides just enough detail for `crate::cli::error` to choose a stable
/// diagnostic code and help text. Classification inspects
/// [`minijinja::Error::kind`] and the retained source chain instead of parsing
/// display text, so new custom functions don't need to update string-matching
/// logic in the CLI diagnostic layer.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RenderFailureKind {
    /// The template's own minijinja syntax is invalid.
    Syntax,
    /// An interactive `ui.*` prompt failed for a reason other than a deliberate
    /// user abort (handled separately, upstream of this classification).
    Prompt,
    /// A `query.*` call failed on a malformed field path, filter expression,
    /// sort path, or `.table()` header/column mismatch.
    Query,
    /// Refreshing the file index for a `query.*` call failed: a filesystem,
    /// database, or (de)serialization error, not a template authoring mistake.
    Index,
    /// A `file.include()` (or other Custom Function) I/O operation failed.
    Io,
    /// Anything else: an unknown function/filter/test, a bad argument, an
    /// undefined-value operation, or another engine-level failure.
    Other,
}

/// Classifies a minijinja render error.
///
/// Inspects [`minijinja::Error::kind`] and the source chain. It does not parse
/// display text.
///
/// # Examples
///
/// ```rust
/// # #[cfg(feature = "test-utils")]
/// # {
/// use minijinja::{Error, ErrorKind};
/// use traces_pkm::{RenderFailureKind, classify_render_error};
///
/// let error = Error::new(ErrorKind::SyntaxError, "bad syntax");
/// assert_eq!(classify_render_error(&error), RenderFailureKind::Syntax);
/// # }
/// ```
#[inline]
#[must_use]
pub fn classify_render_error(error: &minijinja::Error) -> RenderFailureKind {
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
        if err.downcast_ref::<IndexError>().is_some() {
            return RenderFailureKind::Index;
        }
        if err.downcast_ref::<io::Error>().is_some() {
            return RenderFailureKind::Io;
        }
        cause = err.source();
    }
    RenderFailureKind::Other
}
