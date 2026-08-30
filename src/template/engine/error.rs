//! Map helper failures into minijinja errors.
//!
//! Engine helpers return [`minijinja::Error`] so template diagnostics keep
//! name, line, and column context. This module centralizes common conversions
//! for path-confinement and source-preserving invalid-operation errors.

use minijinja::{Error, ErrorKind};

use crate::path::PathError;

/// Convenience alias for template engine helper return types.
pub(super) type TemplateEngineResult<T> = std::result::Result<T, Error>;

/// Builds the error for a template `path` argument rejected by root
/// confinement.
///
/// [`PathError::Absolute`], [`PathError::UnsafeComponent`], and
/// [`PathError::EscapesRoot`] share the "escapes the project root" message
/// because template authors see all three as the same failed containment
/// check. [`PathError::Verify`] gets a separate message because containment
/// could not be confirmed, usually because `root` or one of its ancestors
/// could not be canonicalized.
pub(super) fn confine_error(path: &str, source: PathError) -> Error {
    source.fold_confinement(
        || {
            Error::new(
                ErrorKind::InvalidOperation,
                format!("path {path} escapes the project root"),
            )
        },
        |inner| {
            Error::new(
                ErrorKind::InvalidOperation,
                format!(
                    "failed to verify path {path} is inside the project root"
                ),
            )
            .with_source(inner)
        },
    )
}

/// Builds an [`ErrorKind::InvalidOperation`] [`minijinja::Error`] carrying
/// `source` as its error-chain cause.
///
/// Shared by every `template::engine` submodule that maps a domain error
/// (I/O, index, query, regex, dialog) into minijinja's error type with the
/// same "generic message plus preserved source" shape.
pub(super) fn invalid_operation<E>(
    message: impl Into<String>,
    source: E,
) -> Error
where
    E: std::error::Error + Send + Sync + 'static,
{
    Error::new(ErrorKind::InvalidOperation, message.into()).with_source(source)
}
