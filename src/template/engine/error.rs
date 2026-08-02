//! Maps path-confinement failures into minijinja errors.

use minijinja::{Error, ErrorKind};

use crate::path::PathError;

/// Builds the error for a template `path` argument rejected by root
/// confinement.
///
/// [`PathError::NotRelative`] and [`PathError::EscapesRoot`] share the
/// "escapes the project root" message because template authors see both as the
/// same failed containment check. [`PathError::Verify`] gets a separate message
/// because containment could not be confirmed, usually because `root` or one of
/// its ancestors could not be canonicalized.
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
