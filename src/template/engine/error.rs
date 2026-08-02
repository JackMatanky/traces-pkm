//! Shared path-confinement error mapping for template primitives.

use minijinja::{Error, ErrorKind};

use crate::path::PathError;

/// Builds the error for a template `path` argument that fails root confinement.
///
/// Unsafe lexical paths and symlink escapes
/// ([`PathError::NotRelative`]/[`PathError::EscapesRoot`]) share the "escapes
/// the project root" message because template authors see both as the same
/// failure. [`PathError::Verify`] gets its own message because that case is not
/// known to escape, only unconfirmed because the root or an ancestor could not
/// be canonicalized.
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
