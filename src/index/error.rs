//! Errors returned while scanning, persisting, or loading a
//! [`super::FileIndex`].

use std::{io, path::PathBuf, str::Utf8Error};

use thiserror::Error;

/// Error type for [`super::FileIndex`] operations.
///
/// [`Self::Io`] comes from filesystem scans and index-directory setup. The
/// remaining variants come from redb persistence and TOML record encoding.
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
    /// A stored [`super::FileRecord`] or [`super::Note`] was not valid UTF-8,
    /// which means the index database is corrupted.
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
    /// A stored [`super::FileRecord`] or [`super::Note`] could not be
    /// deserialized.
    #[error("failed to deserialize the record for {path}")]
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
