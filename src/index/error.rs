//! Errors from index scanning, persistence, and loading.
//!
//! [`IndexError`] is the unified type every fallible `index` operation returns.
//! It wraps persistence failures ([`DbError`]) and build-pipeline failures
//! ([`IndexBuilderError`]) via `#[error(transparent)]`, delegating `Display`
//! and `source()` entirely to whichever inner error occurred.

use std::{io, path::PathBuf};

use thiserror::Error;

/// Convenience alias for low-level index persistence operations.
pub type DbResult<T> = std::result::Result<T, DbError>;

/// Convenience alias for high-level index operations.
pub type IndexResult<T> = std::result::Result<T, IndexError>;

/// Error type for [`super::FileIndex`] operations: build, persist, load, and
/// refresh.
///
/// Every fallible `index` operation converts into this type via `?`. A thin
/// `#[error(transparent)]` wrapper that delegates `Display` and `source()`
/// entirely to whichever inner error actually occurred.
#[derive(Debug, Error)]
pub enum IndexError {
    /// Database access or record (de)serialization failed.
    #[error(transparent)]
    Store(#[from] DbError),
    /// Scanning, parsing, or reconciling the build pipeline failed.
    #[error(transparent)]
    Builder(#[from] IndexBuilderError),
}

/// Generic error type for low-level redb persistence operations.
///
/// Wraps filesystem I/O, redb database access, and postcard
/// (de)serialization failures. [`super::IndexerService`] never exposes
/// these directly; callers see [`IndexError`], which delegates to
/// [`DbError`] or [`IndexBuilderError`].
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
    ///
    /// `redb::Error` is boxed to keep this enum small.
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

/// Error type for the [`super::builder::IndexBuilder`] build pipeline:
/// filesystem scan, markdown parse, and refresh reconciliation.
///
/// [`super::IndexerService::refresh`] converts these into [`IndexError`]
/// via `?`; callers see [`IndexError::Builder`], not the raw
/// `IndexBuilderError`.
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
    /// was absent from the persisted index during point-lookup recall.
    ///
    /// Indicates a logic bug in the reconciliation pipeline: the record's
    /// metadata said "unchanged", so the builder tried to reuse its note via
    /// [`super::store::IndexStore::read_note`], but no note was persisted at
    /// that path.
    #[error("note missing for record at {path}")]
    MissingNote {
        /// The record path whose expected note was absent.
        path: PathBuf,
    },
    /// A previously-persisted [`crate::note::Note`] could not be read via a
    /// point lookup during refresh reconciliation.
    ///
    /// `source` is boxed: it breaks the size cycle this variant otherwise
    /// creates ([`IndexError::Builder`] holds a plain, unboxed
    /// `IndexBuilderError`), and it matches
    /// [`super::store::IndexStore::read_note`]'s own return type, so callers
    /// forward its error unchanged instead of re-wrapping it.
    #[error("failed to read persisted note for {path}")]
    NoteLookup {
        /// The path whose previous Note lookup failed.
        path: PathBuf,
        /// Source index-store error.
        #[source]
        source: Box<IndexError>,
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

        #[test]
        fn note_lookup_includes_path_in_message() {
            let err = IndexBuilderError::NoteLookup {
                path: PathBuf::from("recall.md"),
                source: Box::new(IndexError::Store(DbError::Redb {
                    path: PathBuf::from(".traces/index.redb"),
                    source: Box::new(redb::Error::DatabaseAlreadyOpen),
                })),
            };

            assert!(err.to_string().contains("recall.md"));
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
        fn builder_display_matches_the_wrapped_builder_error_with_no_added_text()
         {
            let builder_error = IndexBuilderError::MissingNote {
                path: PathBuf::from("orphan.md"),
            };
            let builder_message = builder_error.to_string();

            let wrapped = IndexError::Builder(builder_error);

            assert_eq!(wrapped.to_string(), builder_message);
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
        fn builder_source_skips_straight_to_the_builder_errors_own_source() {
            let err = IndexError::Builder(IndexBuilderError::NoteParse {
                path: PathBuf::from("x"),
                source: io::Error::new(io::ErrorKind::InvalidData, "bad"),
            });

            let source = err.source().expect("source present");
            assert!(source.downcast_ref::<IndexBuilderError>().is_none());
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
        fn index_builder_error_converts_to_the_builder_variant() {
            let builder_error = IndexBuilderError::MissingNote {
                path: PathBuf::from("orphan.md"),
            };

            let converted: IndexError = builder_error.into();

            assert!(matches!(
                converted,
                IndexError::Builder(IndexBuilderError::MissingNote { .. })
            ));
        }

        #[test]
        fn note_lookup_keeps_its_own_path_distinct_from_the_wrapped_errors_path()
         {
            // `NoteLookup`'s own `path` (the record being recalled) must stay
            // reachable even though its `source` is a full `IndexError` that
            // may carry an unrelated path of its own (e.g. the database
            // file, for a `Store`-caused failure).
            let err = IndexBuilderError::NoteLookup {
                path: PathBuf::from("recall.md"),
                source: Box::new(IndexError::Store(DbError::Redb {
                    path: PathBuf::from(".traces/index.redb"),
                    source: Box::new(redb::Error::DatabaseAlreadyOpen),
                })),
            };

            assert!(matches!(
                &err,
                IndexBuilderError::NoteLookup { path, .. }
                    if path == std::path::Path::new("recall.md")
            ));
            assert!(matches!(
                &err,
                IndexBuilderError::NoteLookup { source, .. }
                    if matches!(
                        &**source,
                        IndexError::Store(DbError::Redb { path, .. })
                            if path == std::path::Path::new(".traces/index.redb")
                    )
            ));
        }
    }
}
