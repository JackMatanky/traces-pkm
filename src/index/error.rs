//! Errors from index scanning, persistence, and loading.
//!
//! [`FileIndexError`] preserves path context for filesystem, redb, UTF-8, and
//! TOML failures so CLI diagnostics can name the affected record or database.

use std::{io, path::PathBuf, str::Utf8Error};

use thiserror::Error;

/// Error type for [`super::FileIndex`] operations.
///
/// Variants distinguish filesystem access, database storage, record
/// corruption, and TOML encoding failures.
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
    /// A stored record was not valid UTF-8, indicating database corruption.
    ///
    /// Occurs when stored [`super::FileRecord`] or [`super::Note`] bytes fail
    /// UTF-8 decoding.
    #[error("corrupted record bytes in the index database for {path}")]
    Corrupt {
        /// The corrupted record's project-relative path (its key in the
        /// index database).
        path: PathBuf,
        /// Source UTF-8 decoding error.
        #[source]
        source: Utf8Error,
    },
    /// A [`super::FileRecord`] or [`super::Note`] could not be serialized.
    #[error("failed to serialize the record for {path}")]
    Serialize {
        /// The record's project-relative path.
        path: PathBuf,
        /// Source TOML serialization error.
        #[source]
        source: toml::ser::Error,
    },
    /// A stored record could not be deserialized from TOML.
    ///
    /// Occurs when a stored [`super::FileRecord`] or [`super::Note`] cannot be
    /// deserialized.
    #[error("failed to deserialize the record for {path}")]
    Deserialize {
        /// The record's project-relative path (its key in the index database).
        path: PathBuf,
        /// Source TOML deserialization error, boxed to keep this enum and
        /// `CliError` small.
        ///
        /// `toml::de::Error` carries a full parse diagnostic and is large
        /// relative to the other variants here.
        #[source]
        source: Box<toml::de::Error>,
    },
}
