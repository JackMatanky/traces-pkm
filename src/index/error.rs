//! Errors from index scanning, persistence, and loading.
//!
//! [`IndexError`] wraps persistence failures ([`DbError`]), directory-walk
//! failures ([`DirTreeError`]), and note-parse failures. [`DbError`] is the
//! lower-level error type for the raw redb/filesystem/serialization operations
//! [`super::store::IndexStore`] performs underneath it.

use std::{io, path::PathBuf};

use thiserror::Error;

use crate::DirTreeError;

/// Convenience alias for high-level index operations.
pub type IndexResult<T> = std::result::Result<T, IndexError>;

/// Error type for [`super::FileIndex`] operations.
#[derive(Debug, Error)]
#[expect(
    private_interfaces,
    reason = "DirTreeError is pub(crate), IndexError is pub"
)]
pub enum IndexError {
    /// Database access or record (de)serialization failed.
    #[error(transparent)]
    Store(#[from] DbError),
    /// Directory traversal failed during scan.
    #[error(transparent)]
    Walk(#[from] DirTreeError),
    /// A markdown file could not be read or parsed into a [`crate::Note`].
    #[error("failed to parse note {path}")]
    NoteParse {
        /// The markdown file that failed to parse.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
}

/// Convenience alias for low-level index persistence operations.
pub type DbResult<T> = std::result::Result<T, DbError>;

/// Generic error type for low-level redb persistence operations.
///
/// Wraps filesystem I/O, redb database access, and serialization failures.
/// [`IndexError::Store`] forwards this type transparently, so callers of
/// high-level index operations see these variants' messages unchanged.
#[derive(Debug, Error)]
pub enum DbError {
    /// A filesystem operation failed.
    #[error("failed to access {path}")]
    Io {
        /// The path that could not be accessed.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
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

#[cfg(test)]
mod tests {
    use std::error::Error as StdError;

    use super::*;

    mod db_error_display {
        use super::*;

        #[test]
        fn io_includes_path_in_message() {
            let err = DbError::Io {
                path: PathBuf::from("data.csv"),
                source: io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "denied",
                ),
            };

            assert!(err.to_string().contains("data.csv"));
        }

        #[test]
        fn redb_includes_path_in_message() {
            let err = DbError::Redb {
                path: PathBuf::from(".traces/index.redb"),
                source: Box::new(redb::Error::DatabaseAlreadyOpen),
            };

            assert!(err.to_string().contains(".traces/index.redb"));
        }

        #[test]
        fn serialize_includes_path_in_message() {
            let err = DbError::Serialize {
                path: PathBuf::from("note.md"),
                source: postcard::Error::DeserializeUnexpectedEnd,
            };

            assert!(err.to_string().contains("note.md"));
        }

        #[test]
        fn deserialize_includes_path_in_message() {
            let err = DbError::Deserialize {
                path: PathBuf::from("note.md"),
                source: postcard::Error::DeserializeUnexpectedEnd,
            };

            assert!(err.to_string().contains("note.md"));
        }
    }

    mod index_error_display {
        use super::*;

        #[test]
        fn note_parse_includes_path_in_message() {
            let err = IndexError::NoteParse {
                path: PathBuf::from("notes/bad.md"),
                source: io::Error::new(io::ErrorKind::InvalidData, "not utf8"),
            };

            assert!(err.to_string().contains("bad.md"));
        }
    }

    mod transparent_forwarding {
        use super::*;

        #[test]
        fn store_display_matches_the_wrapped_db_error_with_no_added_text() {
            let db_error = DbError::Redb {
                path: PathBuf::from(".traces/index.redb"),
                source: Box::new(redb::Error::DatabaseAlreadyOpen),
            };
            let db_message = db_error.to_string();

            let wrapped = IndexError::Store(db_error);

            assert_eq!(wrapped.to_string(), db_message);
        }

        #[test]
        fn walk_display_matches_the_wrapped_dir_tree_error_with_no_added_text()
        {
            let walk_error = DirTreeError::NodeInaccessible {
                path: PathBuf::from("orphan.md"),
                source: io::Error::other("boom"),
            };
            let walk_message = walk_error.to_string();

            let wrapped = IndexError::Walk(walk_error);

            assert_eq!(wrapped.to_string(), walk_message);
        }

        #[test]
        fn store_source_skips_straight_to_the_db_errors_own_source() {
            // `#[error(transparent)]` hides the wrapping variant from the
            // source chain entirely: `.source()` returns what `DbError`'s
            // own `.source()` returns (the io::Error), not the `DbError`
            // itself.
            let err = IndexError::Store(DbError::Io {
                path: PathBuf::from("x"),
                source: io::Error::new(io::ErrorKind::BrokenPipe, "pipe"),
            });

            let source = err.source().expect("source present");
            assert!(source.downcast_ref::<DbError>().is_none());
            assert_eq!(
                source.downcast_ref::<io::Error>().map(io::Error::kind),
                Some(io::ErrorKind::BrokenPipe)
            );
        }

        #[test]
        fn walk_source_skips_straight_to_the_dir_tree_errors_own_source() {
            let err = IndexError::Walk(DirTreeError::NodeInaccessible {
                path: PathBuf::from("x"),
                source: io::Error::new(io::ErrorKind::InvalidData, "bad"),
            });

            let source = err.source().expect("source present");
            assert!(source.downcast_ref::<DirTreeError>().is_none());
            assert_eq!(
                source.downcast_ref::<io::Error>().map(io::Error::kind),
                Some(io::ErrorKind::InvalidData)
            );
        }

        #[test]
        fn note_parse_source_returns_the_io_error() {
            let err = IndexError::NoteParse {
                path: PathBuf::from("x"),
                source: io::Error::new(io::ErrorKind::InvalidData, "bad"),
            };

            let source = err.source().expect("source present");
            assert_eq!(
                source.downcast_ref::<io::Error>().map(io::Error::kind),
                Some(io::ErrorKind::InvalidData)
            );
        }

        #[test]
        fn db_error_converts_to_the_store_variant() {
            let db_error = DbError::Io {
                path: PathBuf::from("x"),
                source: io::Error::other("boom"),
            };

            let converted: IndexError = db_error.into();

            assert!(matches!(converted, IndexError::Store(DbError::Io { .. })));
        }

        #[test]
        fn dir_tree_error_converts_to_the_walk_variant() {
            let walk_error = DirTreeError::NodeInaccessible {
                path: PathBuf::from("x"),
                source: io::Error::other("boom"),
            };

            let converted: IndexError = walk_error.into();

            assert!(matches!(
                converted,
                IndexError::Walk(DirTreeError::NodeInaccessible { .. })
            ));
        }
    }
}
