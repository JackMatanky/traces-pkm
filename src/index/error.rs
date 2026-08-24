//! Errors from index scanning, persistence, and loading.
//!
//! [`IndexError`] covers persistence failures (database, serialization).
//! [`IndexBuilderError`] covers build-pipeline failures (scan, parse).
//! The [`From`] impl converts builder errors into index errors for
//! callers that use the unified [`super::FileIndex`] API.

use std::{io, path::PathBuf};

use thiserror::Error;

/// Generic error type for low-level redb persistence operations.
#[derive(Debug, Error)]
pub(crate) enum DbError {
    /// A filesystem operation failed during directory creation or file setup.
    #[error("failed to access {path}")]
    Io {
        /// The path that could not be accessed.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Opening, reading, or writing the redb-backed database failed.
    #[error("failed to access the database at {path}")]
    Redb {
        /// The database file path.
        path: PathBuf,
        /// Source redb error.
        #[source]
        source: Box<redb::Error>,
    },
    /// A record could not be serialized.
    #[error("failed to serialize the record for {path}")]
    Serialize {
        /// The record's project-relative path.
        path: PathBuf,
        /// Source postcard serialization error.
        #[source]
        source: postcard::Error,
    },
    /// A stored record could not be deserialized.
    #[error("failed to deserialize the record for {path}")]
    Deserialize {
        /// The record's project-relative path.
        path: PathBuf,
        /// Source postcard deserialization error.
        #[source]
        source: postcard::Error,
    },
}

/// Error type for [`super::FileIndex`] persistence operations.
///
/// Variants distinguish database access, and postcard encoding failures.
/// Build-time failures are covered by [`IndexBuilderError`].
#[derive(Debug, Error)]
pub enum IndexError {
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
    ///
    /// `redb::Error` is boxed to keep this enum small; `Store` is by far the
    /// rarest variant.
    #[error("failed to access the index database at {path}")]
    Store {
        /// The index database file.
        path: PathBuf,
        /// Source redb error.
        #[source]
        source: Box<redb::Error>,
    },
    /// A [`super::FileBase`] or [`super::Note`] could not be serialized.
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
    /// Occurs when stored bytes are corrupt or were written by an incompatible
    /// encoding.
    #[error("failed to deserialize the record for {path}")]
    Deserialize {
        /// The record's project-relative path (its key in the index database).
        path: PathBuf,
        /// Source postcard deserialization error. Not boxed: `postcard::Error`
        /// is a small, fieldless, non-exhaustive enum with no parse-diagnostic
        /// payload.
        #[source]
        source: postcard::Error,
    },
}

impl From<DbError> for IndexError {
    #[inline]
    fn from(err: DbError) -> Self {
        match err {
            DbError::Io {
                path,
                source,
            } => Self::Io {
                path,
                source,
            },
            DbError::Redb {
                path,
                source,
            } => Self::Store {
                path,
                source,
            },
            DbError::Serialize {
                path,
                source,
            } => Self::Serialize {
                path,
                source,
            },
            DbError::Deserialize {
                path,
                source,
            } => Self::Deserialize {
                path,
                source,
            },
        }
    }
}

/// Error type for the [`super::builder::IndexBuilder`] build pipeline.
///
/// Distinct from [`IndexError`] to separate build-time failures
/// (filesystem scan, markdown parse) from persistence failures (database,
/// serialization).
#[derive(Debug, Error)]
pub enum IndexBuilderError {
    /// Filesystem error during directory scan or file metadata read.
    #[error("failed to scan {path}")]
    Scan {
        /// The path that could not be read.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// Markdown file could not be read or parsed into a [`crate::note::Note`].
    #[error("failed to parse note {path}")]
    NoteParse {
        /// The markdown file that failed to parse.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// Record metadata matched the previous index, but the corresponding note
    /// was not found in the moved notes map.
    ///
    /// Indicates a logic bug in the reconciliation pipeline: the record's
    /// metadata said "unchanged", so the builder tried to reuse its note, but
    /// the note was never moved into the reuse map.
    #[error("note missing for record at {path}")]
    MissingNote {
        /// The record path whose expected note was absent.
        path: PathBuf,
    },
}

impl From<IndexBuilderError> for IndexError {
    #[inline]
    fn from(err: IndexBuilderError) -> Self {
        match err {
            IndexBuilderError::Scan {
                path,
                source,
            }
            | IndexBuilderError::NoteParse {
                path,
                source,
            } => Self::Io {
                path,
                source,
            },
            IndexBuilderError::MissingNote {
                path,
            } => Self::Io {
                path,
                source: io::Error::new(
                    io::ErrorKind::NotFound,
                    "note missing for matched record",
                ),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod index_builder_error_display {
        use super::*;

        #[test]
        fn scan_includes_path_in_message() {
            let err = IndexBuilderError::Scan {
                path: PathBuf::from("src/main.rs"),
                source: io::Error::new(io::ErrorKind::NotFound, "missing"),
            };

            assert!(err.to_string().contains("src/main.rs"));
        }

        #[test]
        fn note_parse_includes_path_in_message() {
            let err = IndexBuilderError::NoteParse {
                path: PathBuf::from("notes").join("bad.md"),
                source: io::Error::new(io::ErrorKind::InvalidData, "not utf8"),
            };

            assert!(err.to_string().contains("bad.md"));
        }

        #[test]
        fn missing_note_includes_path_in_message() {
            let err = IndexBuilderError::MissingNote {
                path: PathBuf::from("orphan.md"),
            };

            assert!(err.to_string().contains("orphan.md"));
        }
    }

    mod index_error_display {
        use super::*;

        #[test]
        fn io_includes_path_in_message() {
            let err = IndexError::Io {
                path: PathBuf::from("data.csv"),
                source: io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "denied",
                ),
            };

            assert!(err.to_string().contains("data.csv"));
        }

        #[test]
        fn store_includes_path_in_message() {
            let err = IndexError::Store {
                path: PathBuf::from(".traces/index.redb"),
                source: Box::new(redb::Error::DatabaseAlreadyOpen),
            };

            assert!(err.to_string().contains(".traces/index.redb"));
        }

        #[test]
        fn serialize_includes_path_in_message() {
            let err = IndexError::Serialize {
                path: PathBuf::from("note.md"),
                source: postcard::Error::DeserializeUnexpectedEnd,
            };

            assert!(err.to_string().contains("note.md"));
        }

        #[test]
        fn deserialize_includes_path_in_message() {
            let err = IndexError::Deserialize {
                path: PathBuf::from("note.md"),
                source: postcard::Error::DeserializeUnexpectedEnd,
            };

            assert!(err.to_string().contains("note.md"));
        }
    }

    mod from_index_builder_error {
        use std::path::Path;

        use super::*;

        #[test]
        fn scan_converts_to_io() {
            let source =
                io::Error::new(io::ErrorKind::NotFound, "no such file");
            let err = IndexBuilderError::Scan {
                path: PathBuf::from("missing.rs"),
                source,
            };

            let converted: IndexError = err.into();

            assert!(
                matches!(converted, IndexError::Io { path, .. } if path == Path::new("missing.rs"))
            );
        }

        #[test]
        fn note_parse_converts_to_io() {
            let source = io::Error::new(io::ErrorKind::InvalidData, "bad utf8");
            let err = IndexBuilderError::NoteParse {
                path: PathBuf::from("notes").join("bad.md"),
                source,
            };

            let converted: IndexError = err.into();

            assert!(
                matches!(converted, IndexError::Io { path, .. } if path == Path::new("notes/bad.md"))
            );
        }

        #[test]
        fn missing_note_converts_to_io_with_not_found() {
            let err = IndexBuilderError::MissingNote {
                path: PathBuf::from("orphan.md"),
            };

            let converted: IndexError = err.into();

            assert!(matches!(converted, IndexError::Io { path, source }
                if path == Path::new("orphan.md")
                    && source.kind() == io::ErrorKind::NotFound));
        }
    }

    mod error_source_chains {
        use std::error::Error as StdError;

        use super::*;

        #[test]
        fn io_preserves_source() {
            let source = io::Error::new(io::ErrorKind::BrokenPipe, "pipe");
            let err = IndexError::Io {
                path: PathBuf::from("x"),
                source,
            };

            assert!(err.source().is_some());
            assert_eq!(
                err.source()
                    .unwrap()
                    .downcast_ref::<io::Error>()
                    .unwrap()
                    .kind(),
                io::ErrorKind::BrokenPipe,
            );
        }

        #[test]
        fn store_preserves_source() {
            let err = IndexError::Store {
                path: PathBuf::from("db"),
                source: Box::new(redb::Error::DatabaseAlreadyOpen),
            };

            assert!(err.source().is_some());
        }

        #[test]
        fn serialize_preserves_source() {
            let err = IndexError::Serialize {
                path: PathBuf::from("x"),
                source: postcard::Error::DeserializeUnexpectedEnd,
            };

            assert!(err.source().is_some());
        }

        #[test]
        fn deserialize_preserves_source() {
            let err = IndexError::Deserialize {
                path: PathBuf::from("x"),
                source: postcard::Error::DeserializeUnexpectedEnd,
            };

            assert!(err.source().is_some());
        }
    }
}
