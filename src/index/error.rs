//! Errors from [`super::FileIndex`] operations: scanning, persisting, and
//! loading File Records.

use std::{io, path::PathBuf, str::Utf8Error};

use thiserror::Error;

/// Errors from building, persisting, or loading a [`super::FileIndex`].
#[derive(Debug, Error)]
pub(crate) enum FileIndexError {
    /// A filesystem operation failed while scanning a project root or preparing
    /// the index database's parent directory.
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
        /// Source redb error, boxed to keep this enum (and `CliError`, which
        /// wraps it) small — `redb::Error` is a large, many-variant enum, and
        /// `Store` is by far the rarest path here.
        #[source]
        source: Box<redb::Error>,
    },
    /// A File Record could not be serialized for storage.
    #[error("failed to serialize the File Record for {path}")]
    Serialize {
        /// The record's project-relative path.
        path: PathBuf,
        /// Source TOML serialization error.
        #[source]
        source: toml::ser::Error,
    },
    /// A stored File Record's bytes were not valid UTF-8 — the index
    /// database is corrupted.
    #[error("corrupted File Record bytes in the index database for {path}")]
    Corrupt {
        /// The corrupted record's project-relative path (its key in the
        /// index database).
        path: PathBuf,
        /// Source UTF-8 decoding error.
        #[source]
        source: Utf8Error,
    },
    /// A stored File Record's text could not be parsed back into a
    /// [`super::FileRecord`].
    #[error("failed to deserialize the File Record for {path}")]
    Deserialize {
        /// The record's project-relative path (its key in the index database).
        path: PathBuf,
        /// Source TOML deserialization error, boxed to keep this enum (and
        /// `CliError`, which wraps it) small — `toml::de::Error` carries a
        /// full parse diagnostic and is large relative to the other variants
        /// here.
        #[source]
        source: Box<toml::de::Error>,
    },
}
