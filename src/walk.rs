//! Shared directory-walk error adaptation for `walkdir` consumers.
//!
//! `walkdir` provides no reusable path-context extraction from its errors
//! and no built-in way to treat a missing root directory as an empty walk
//! (verified against 2.5.0, the current latest release). Every domain that
//! walks a directory tree re-derives both by hand; this module is the one
//! place that logic lives. Traversal policy (depth bounds, `filter_entry`
//! pruning, symlink handling) stays with each caller — this module owns
//! nothing about *how* a walk is shaped, only how its errors are reported.

use std::path::PathBuf;

use walkdir::DirEntry;

/// A `walkdir::Error` paired with resolved path context.
///
/// `walkdir::Error::path()` returns `None` for some I/O errors that carry no
/// `DirEntry`; `path` here always falls back to the walk's root in that case.
pub(crate) struct WalkError {
    pub(crate) path: PathBuf,
    pub(crate) source: walkdir::Error,
}

/// Returns `true` if `error` reports that the walk's root itself does not
/// exist, so a caller can degrade to "no entries" instead of a hard error.
pub(crate) fn is_missing_root(error: &walkdir::Error) -> bool {
    error.depth() == 0
        && error
            .io_error()
            .is_some_and(|source| source.kind() == std::io::ErrorKind::NotFound)
}

/// Wraps any `walkdir` iterator, reporting errors as [`WalkError`] instead of
/// bare `walkdir::Error`. Generic over `I` so it accepts a plain
/// `walkdir::IntoIter` or a `filter_entry`-composed `FilterEntry<IntoIter, P>`
/// unchanged — this type owns no traversal configuration of its own.
pub(crate) struct DirWalk<I> {
    inner: I,
    root: PathBuf,
}

impl<I: Iterator<Item = walkdir::Result<DirEntry>>> DirWalk<I> {
    /// Wraps `inner` (an already-configured walkdir iterator), resolving
    /// its errors' path context against `root`.
    pub(crate) fn new(root: impl Into<PathBuf>, inner: I) -> Self {
        Self {
            inner,
            root: root.into(),
        }
    }
}

impl<I: Iterator<Item = walkdir::Result<DirEntry>>> Iterator for DirWalk<I> {
    type Item = Result<DirEntry, WalkError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.inner.next()? {
            Ok(entry) => Some(Ok(entry)),
            Err(source) => {
                let path = source.path().unwrap_or(&self.root).to_path_buf();
                Some(Err(WalkError {
                    path,
                    source,
                }))
            }
        }
    }
}
