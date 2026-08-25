//! Directory-tree traversal: flat listings and recursive walks with
//! classified, path-contextualized errors.
//!
//! [`children`] lists a directory's immediate entries; [`descendants`] walks
//! a whole tree, with [`Descendants::skipping`] pruning subtrees. Both yield
//! [`DirNode`] values and report failures as [`DirTreeError`], classified at
//! the point where walkdir's depth information is still known:
//!
//! - [`DirTreeError::MissingRoot`] — the walk root does not exist. Callers pick
//!   their own policy: degrade to empty or fail.
//! - [`DirTreeError::RootInaccessible`] — the root exists but could not be
//!   inspected or opened.
//! - [`DirTreeError::NodeInaccessible`] — something beneath the root failed.
//!
//! Traversal construction lives here; entry filtering (extensions, stems,
//! hidden files) stays with callers, who see every [`DirNode`] and decide
//! what matches.
//!
//! Verified against walkdir 2.5.0: loop detection cannot fire while
//! `follow_links` remains unset (the only configuration these constructors
//! use), so loop errors never reach [`DirTreeError`].
#![expect(
    dead_code,
    reason = "internal API; walk.rs consumers migrate onto dirtree in later \
              tasks"
)]

use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;
use walkdir::{DirEntry, WalkDir};

/// A failure raised while traversing a directory tree.
///
/// Variants are classified inside this module where walkdir's depth
/// information is still known; callers match to state their missing-root
/// policy and convert everything else via [`into_parts`](Self::into_parts).
#[derive(Debug, Error)]
pub(crate) enum DirTreeError {
    /// The walk root does not exist (depth-0 `NotFound`).
    #[error("walk root {path} does not exist")]
    MissingRoot {
        /// The root path passed to the constructor.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// The root exists but could not be inspected or opened.
    #[error("failed to access walk root {path}")]
    RootInaccessible {
        /// The root path passed to the constructor.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// Something beneath the root failed: a directory could not be listed,
    /// a mid-stream read glitched, or one node's metadata could not be read.
    #[error("failed to access node {path}")]
    NodeInaccessible {
        /// The failing node's path, falling back to the walk root when
        /// walkdir supplies none (mid-readdir stream errors carry no path).
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
}

impl DirTreeError {
    /// Splits the error into its resolved path and I/O source.
    ///
    /// Domain errors shaped `{path, io::Error}` convert in one line.
    pub(crate) fn into_parts(self) -> (PathBuf, io::Error) {
        match self {
            Self::MissingRoot {
                path,
                source,
            }
            | Self::RootInaccessible {
                path,
                source,
            }
            | Self::NodeInaccessible {
                path,
                source,
            } => (path, source),
        }
    }
}

/// Classifies one raw walkdir failure against the walk's root.
///
/// Depth 0 + `NotFound` is [`DirTreeError::MissingRoot`]; other depth-0
/// failures are [`DirTreeError::RootInaccessible`]; anything deeper is
/// [`DirTreeError::NodeInaccessible`]. When walkdir carries no path
/// (mid-readdir stream errors), `fallback` (the walk root) is used so the
/// path is never lost.
fn classify(fallback: &Path, source: walkdir::Error) -> DirTreeError {
    let depth = source.depth();
    let path = source.path().unwrap_or(fallback).to_path_buf();
    let source = io::Error::from(source);
    match depth {
        0 if source.kind() == io::ErrorKind::NotFound => {
            DirTreeError::MissingRoot {
                path,
                source,
            }
        }
        0 => DirTreeError::RootInaccessible {
            path,
            source,
        },
        _ => DirTreeError::NodeInaccessible {
            path,
            source,
        },
    }
}

/// Adapts one raw walkdir item into this module's interface.
fn adapt(
    root: &Path,
    result: walkdir::Result<DirEntry>,
) -> Result<DirNode, DirTreeError> {
    match result {
        Ok(entry) => Ok(DirNode::new(entry)),
        Err(source) => Err(classify(root, source)),
    }
}

/// One node of a directory tree: a file, directory, or symlink yielded by
/// [`children`] or [`descendants`].
///
/// Wraps walkdir's entry so callers never touch walkdir types — including
/// [`DirNode::metadata`]'s failure mode, which walkdir reports outside the
/// iteration stream; here it flows through the same [`DirTreeError`] as
/// every other failure.
#[derive(Clone, Debug)]
pub(crate) struct DirNode {
    inner: DirEntry,
}

impl DirNode {
    /// Wraps a raw walkdir entry.
    fn new(inner: DirEntry) -> Self {
        Self {
            inner,
        }
    }

    /// Returns the node's full path, including the walk root prefix.
    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        self.inner.path()
    }

    /// Returns the node's final path component.
    #[must_use]
    pub(crate) fn file_name(&self) -> &OsStr {
        self.inner.file_name()
    }

    /// Returns the node's type without following symlinks: a symlinked file
    /// reports [`FileType::is_symlink`](std::fs::FileType::is_symlink),
    /// never its target's type.
    #[must_use]
    pub(crate) fn file_type(&self) -> fs::FileType {
        self.inner.file_type()
    }

    /// Reads the node's filesystem metadata.
    ///
    /// # Errors
    ///
    /// - [`DirTreeError::NodeInaccessible`] if the node's metadata cannot be
    ///   read (for example, the entry vanished between listing and this call).
    pub(crate) fn metadata(&self) -> Result<fs::Metadata, DirTreeError> {
        self.inner.metadata().map_err(|source| {
            let path = self.inner.path().to_path_buf();
            let source = io::Error::from(source);
            DirTreeError::NodeInaccessible {
                path,
                source,
            }
        })
    }
}

/// Lists a directory's immediate entries (non-recursive).
///
/// Yields every direct child of `directory` — files, directories, and
/// symlinks alike; filtering stays with the caller. A missing directory
/// yields exactly one [`DirTreeError::MissingRoot`] and then stops; a
/// *file* root yields nothing at all.
///
/// Entry order follows the OS directory read and is unspecified — sort if
/// order matters.
pub(crate) fn children(directory: impl AsRef<Path>) -> Children {
    let directory = directory.as_ref();
    Children {
        inner: WalkDir::new(directory).min_depth(1).max_depth(1).into_iter(),
        root: directory.to_path_buf(),
    }
}

/// Iterator over a directory's immediate entries.
///
/// Created by [`children`]; yields [`Result<DirNode, DirTreeError>`].
pub(crate) struct Children {
    inner: walkdir::IntoIter,
    root: PathBuf,
}

impl Iterator for Children {
    type Item = Result<DirNode, DirTreeError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|result| adapt(&self.root, result))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// Writes `rel` under `dir` with placeholder content, creating parent
    /// directories, and returns the absolute path.
    fn write(dir: &Path, rel: &str) -> PathBuf {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dirs");
        }
        fs::write(&path, "content").expect("write fixture file");
        path
    }

    mod children {
        use super::*;

        #[test]
        fn yields_only_immediate_entries() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            write(root, "a.md");
            write(root, "sub/nested.md");

            // Act
            let mut names: Vec<String> = children(root)
                .map(|entry| entry.expect("entry is ok"))
                .map(|node| node.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();

            // Assert
            assert_eq!(names, vec!["a.md", "sub"]);
        }

        #[test]
        fn missing_directory_yields_one_missing_root_error_then_stops() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let missing = temp.path().join("does-not-exist");

            // Act
            let collected: Vec<_> = children(&missing).collect();

            // Assert
            assert_eq!(
                collected.len(),
                1,
                "missing root yields exactly one item"
            );
            let error = collected
                .into_iter()
                .next()
                .expect("one item")
                .expect_err("is an error");
            assert!(matches!(error, DirTreeError::MissingRoot { .. }));
            let (path, source) = error.into_parts();
            assert_eq!(path, missing);
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }

        #[test]
        fn a_file_root_yields_no_entries_and_no_errors() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let file = write(temp.path(), "plain.md");

            // Act
            let collected: Vec<_> = children(&file).collect();

            // Assert
            assert!(collected.is_empty(), "a file root lists nothing");
        }
    }
}
