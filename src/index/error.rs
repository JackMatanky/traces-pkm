//! Errors from index scanning, persistence, and loading.
//!
//! [`FileIndexError`] preserves path context for filesystem, redb, and postcard
//! encoding failures so CLI diagnostics can name the affected record or
//! database.

use std::{io, path::PathBuf};

use thiserror::Error;

/// Error type for [`super::FileIndex`] operations.
///
/// Variants distinguish filesystem access, database storage, and postcard
/// encoding failures.
#[derive(Debug, Error)]
pub(crate) enum FileIndexError {
    /// A filesystem operation failed during a scan or directory setup.
    ///
    /// Occurs while scanning a project root or preparing the index database's
    /// parent directory.
    #[error("failed to access {path}")]
    Io {
        /// The path that could not be accessed.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// Opening, reading, or writing the redb-backed index database failed.
    #[error("failed to access the index database at {path}")]
    Store {
        /// The index database file.
        path: PathBuf,
        /// Source redb error, boxed to keep this enum and `CliError` small.
        ///
        /// `redb::Error` is a large, many-variant enum, and `Store` is by far
        /// the rarest path here.
        #[source]
        source: Box<redb::Error>,
    },
    /// A [`super::FileRecord`] or [`super::Note`] could not be serialized.
    #[error("failed to serialize the record for {path}")]
    Serialize {
        /// The record's project-relative path.
        path: PathBuf,
        /// Source postcard serialization error.
        #[source]
        source: postcard::Error,
    },
    /// A stored record could not be deserialized.
    ///
    /// Occurs when a stored [`super::FileRecord`] or [`super::Note`]'s bytes
    /// are corrupt or were written by an incompatible encoding.
    #[error("failed to deserialize the record for {path}")]
    Deserialize {
        /// The record's project-relative path (its key in the index database).
        path: PathBuf,
        /// Source postcard deserialization error. Not boxed: `postcard::Error`
        /// is a small, fieldless, non-exhaustive enum with no parse-diagnostic
        /// payload, unlike the `toml::de::Error` this replaces.
        #[source]
        source: postcard::Error,
    },
}
