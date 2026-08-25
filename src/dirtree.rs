//! Directory-tree traversal: flat listings and recursive walks with classified,
//! path-contextualized errors.
//!
//! [`DirTree::children`] lists a directory's immediate entries;
//! [`DirTree::descendants`] walks a whole tree, with
//! [`DirTree::filter`] pruning subtrees and
//! [`DirTree::sorted_by`] imposing per-directory order. Both yield
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
//! hidden files) stays with callers, who see every [`DirNode`] and decide what
//! matches.
//!
//! Verified against walkdir 2.5.0: loop detection cannot fire while
//! `follow_links` remains unset (the only configuration these constructors
//! use), so loop errors never reach [`DirTreeError`].

use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;
use walkdir::{DirEntry, WalkDir};

/// Applies `configure` to a pending builder, leaving a started walk alone.
fn configure(
    builder: &mut Option<WalkDir>,
    f: impl FnOnce(WalkDir) -> WalkDir,
) {
    if let Some(pending) = builder.take() {
        *builder = Some(f(pending));
    }
}

/// A failure raised while traversing a directory tree.
///
/// Variants are classified inside this module where walkdir's depth information
/// is still known; callers match to state their missing-root policy and convert
/// everything else via [`into_parts`](Self::into_parts).
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
    /// Something beneath the root failed: a directory could not be listed, a
    /// mid-stream read glitched, or one node's metadata could not be read.
    #[error("failed to access node {path}")]
    NodeInaccessible {
        /// The failing node's path, falling back to the walk root when walkdir
        /// supplies none (mid-readdir stream errors carry no path).
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
/// (mid-readdir stream errors), `fallback` (the walk root) is used so the path
/// is never lost.
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

/// One node of a directory tree: a file, directory, or symlink yielded by
/// [`DirTree::children`] or [`DirTree::descendants`].
///
/// Wraps walkdir's entry so callers never touch walkdir types — including
/// [`DirNode::metadata`]'s failure mode, which walkdir reports outside the
/// iteration stream; here it flows through the same [`DirTreeError`] as
/// every other failure.
#[derive(Clone, Debug)]
pub(crate) struct DirNode(DirEntry);

impl DirNode {
    /// Adapts one raw walkdir item into this module's interface: entries become
    /// nodes, failures are classified against the walk's root.
    ///
    /// # Errors
    ///
    /// - [`DirTreeError::MissingRoot`] for a depth-0 `NotFound`
    /// - [`DirTreeError::RootInaccessible`] for any other depth-0 failure
    /// - [`DirTreeError::NodeInaccessible`] for anything deeper
    fn try_new(
        root: &Path,
        result: walkdir::Result<DirEntry>,
    ) -> Result<Self, DirTreeError> {
        match result {
            Ok(entry) => Ok(Self(entry)),
            Err(source) => Err(classify(root, source)),
        }
    }

    /// Returns the node's full path, including the walk root prefix.
    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        self.0.path()
    }

    /// Returns the node's final path component.
    #[must_use]
    pub(crate) fn file_name(&self) -> &OsStr {
        self.0.file_name()
    }

    /// Returns the node's type without following symlinks: a symlinked file
    /// reports [`FileType::is_symlink`](std::fs::FileType::is_symlink), never
    /// its target's type.
    #[must_use]
    pub(crate) fn file_type(&self) -> fs::FileType {
        self.0.file_type()
    }

    /// Reads the node's filesystem metadata.
    ///
    /// # Errors
    ///
    /// - [`DirTreeError::NodeInaccessible`] if the node's metadata cannot be
    ///   read (for example, the entry vanished between listing and this call).
    pub(crate) fn metadata(&self) -> Result<fs::Metadata, DirTreeError> {
        self.0.metadata().map_err(|source| {
            let path = self.0.path().to_path_buf();
            let source = io::Error::from(source);
            DirTreeError::NodeInaccessible {
                path,
                source,
            }
        })
    }
}

/// Type-erased pruner: the caller's node predicate wrapped so
/// [`walkdir::FilterEntry`] can apply it to raw entries.
type PrunePredicate = Box<dyn FnMut(&DirEntry) -> bool>;

/// Iterator over a directory tree — flat or recursive — with classified,
/// path-contextualized errors.
///
/// Created by [`DirTree::children`] (flat) or [`DirTree::descendants`]
/// (recursive); configured with [`DirTree::filter`] and [`DirTree::sorted_by`]
/// before iterating; yields [`Result<DirNode, DirTreeError>`].
pub(crate) struct DirTree {
    builder: Option<WalkDir>,
    root: PathBuf,
    prune: Option<PrunePredicate>,
    inner: Option<Box<dyn Iterator<Item = walkdir::Result<DirEntry>>>>,
}

impl DirTree {
    /// Lists a directory's immediate entries (non-recursive).
    ///
    /// Yields every direct child of `dir` — files, directories, and symlinks
    /// alike; filtering stays with the caller. A missing directory yields
    /// exactly one [`DirTreeError::MissingRoot`] and then stops; a *file* root
    /// yields nothing at all.
    ///
    /// An unreadable *subdirectory* is still yielded as a plain entry without
    /// an error: walkdir records the failed open but discards it when
    /// `max_depth` cuts the stack. Recursive walks surface such failures as
    /// [`DirTreeError::NodeInaccessible`].
    ///
    /// Entry order follows the OS directory read and is unspecified — pass
    /// [`Self::sorted_by`] to impose deterministic order.
    #[must_use]
    pub(crate) fn children(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        Self {
            builder: Some(WalkDir::new(dir).min_depth(1).max_depth(1)),
            root: dir.to_path_buf(),
            prune: None,
            inner: None,
        }
    }

    /// Walks a directory tree recursively, starting at the root itself.
    ///
    /// Yields the root node first, then every descendant — files,
    /// directories, and symlinks alike; filtering stays with the caller.
    /// Symlinks are never followed. A missing root yields exactly one
    /// [`DirTreeError::MissingRoot`] and then stops.
    ///
    /// Entry order follows the OS directory read and is unspecified — pass
    /// [`Self::sorted_by`] to impose deterministic order.
    #[must_use]
    pub(crate) fn descendants(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        Self {
            builder: Some(WalkDir::new(dir)),
            root: dir.to_path_buf(),
            prune: None,
            inner: None,
        }
    }

    /// Removes every subtree whose directory satisfies `predicate`.
    ///
    /// `predicate` runs on directories only; returning `true` removes that
    /// directory *and* everything beneath it from the walk. Non-matching
    /// entries — including files whose name satisfies the predicate — are
    /// yielded unchanged. A predicate matching the walk root itself empties
    /// the whole walk. Has no effect once iteration has begun.
    ///
    /// Works on both flat and recursive walks: for [`Self::children`],
    /// filtered directories are simply omitted from the listing.
    #[must_use]
    pub(crate) fn filter<F>(mut self, mut predicate: F) -> Self
    where
        F: FnMut(&DirNode) -> bool + 'static,
    {
        self.prune = Some(Box::new(move |entry: &DirEntry| {
            !(entry.file_type().is_dir() && predicate(&DirNode(entry.clone())))
        }));
        self
    }

    /// Orders the entries of every listed directory with `compare`.
    ///
    /// walkdir merges each directory separately, so ordering is
    /// per-directory; there is no global ordering across hierarchy levels.
    /// Comparators run on cloned node views, costing one allocation per
    /// comparison, and must be [`Send`] + [`Sync`] (walkdir's requirement).
    /// Has no effect once iteration has begun.
    #[must_use]
    pub(crate) fn sorted_by<F>(mut self, mut compare: F) -> Self
    where
        F: FnMut(&DirNode, &DirNode) -> std::cmp::Ordering
            + Send
            + Sync
            + 'static,
    {
        configure(&mut self.builder, |builder| {
            builder.sort_by(move |a, b| {
                compare(&DirNode(a.clone()), &DirNode(b.clone()))
            })
        });
        self
    }

    fn start(&mut self) -> &mut dyn Iterator<Item = walkdir::Result<DirEntry>> {
        let Self {
            builder,
            prune,
            inner,
            ..
        } = self;
        inner
            .get_or_insert_with(|| {
                // ponytail: `builder` is always present here — it starts set
                // and only this closure takes it. The empty-walk fallback is
                // unreachable; swap for an enum state if that ever changes.
                let iter = builder.take().map_or_else(
                    || WalkDir::new("").into_iter(),
                    WalkDir::into_iter,
                );
                match prune.take() {
                    Some(predicate) => Box::new(iter.filter_entry(predicate)),
                    None => Box::new(iter),
                }
            })
            .as_mut()
    }
}

impl Iterator for DirTree {
    type Item = Result<DirNode, DirTreeError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.start().next().map(|result| DirNode::try_new(&self.root, result))
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
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn yields_only_immediate_entries() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            write(root, "a.md");
            write(root, "sub/nested.md");

            // Act
            let mut names: Vec<String> = DirTree::children(root)
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
            let collected: Vec<_> = DirTree::children(&missing).collect();

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
        fn sorted_by_orders_immediate_entries() {
            // Arrange — created deliberately out of lexicographic order.
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            write(root, "z.md");
            write(root, "a.md");

            // Act
            let names: Vec<String> = DirTree::children(root)
                .sorted_by(|a, b| a.file_name().cmp(b.file_name()))
                .map(|entry| entry.expect("entry is ok"))
                .map(|node| node.file_name().to_string_lossy().into_owned())
                .collect();

            // Assert — yielded already ordered; nothing sorted after the walk.
            assert_eq!(names, vec!["a.md", "z.md"]);
        }

        #[test]
        fn a_file_root_yields_no_entries_and_no_errors() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let file = write(temp.path(), "plain.md");

            // Act
            let collected: Vec<_> = DirTree::children(&file).collect();

            // Assert
            assert!(collected.is_empty(), "a file root lists nothing");
        }

        #[test]
        fn filter_removes_matching_directories_from_listing() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            write(root, ".git/HEAD");
            write(root, "note.md");

            // Act
            let mut names: Vec<String> = DirTree::children(root)
                .filter(|node| node.file_name() == ".git")
                .map(|entry| entry.expect("entry is ok"))
                .map(|node| node.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();

            // Assert — .git directory removed, note.md and remaining entries
            // kept.
            assert_eq!(names, vec!["note.md"]);
        }
    }

    mod descendants {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn walks_the_whole_tree_including_the_root_node() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            write(root, "a.md");
            write(root, "b/one.md");

            // Act
            let mut relatives: Vec<String> = DirTree::descendants(root)
                .map(|entry| entry.expect("entry is ok"))
                .map(|node| {
                    node.path()
                        .strip_prefix(root)
                        .expect("under root")
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            relatives.sort();

            // Assert — the root itself is yielded (empty relative path),
            // matching what index scanning and subtree discovery rely on.
            assert_eq!(relatives, vec!["", "a.md", "b", "b/one.md"]);
        }

        #[test]
        fn missing_root_yields_a_missing_root_error() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let missing = temp.path().join("gone");

            // Act
            let collected: Vec<_> = DirTree::descendants(&missing).collect();

            // Assert
            assert_eq!(collected.len(), 1);
            assert!(matches!(
                collected.into_iter().next().expect("one item"),
                Err(DirTreeError::MissingRoot { .. })
            ));
        }

        #[test]
        fn filter_prunes_matching_subtrees_but_keeps_other_entries() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            write(root, ".git/HEAD");
            write(root, "note.md");

            // Act
            let mut names: Vec<String> = DirTree::descendants(root)
                .filter(|node| node.file_name() == ".git")
                .map(|entry| entry.expect("entry is ok"))
                .map(|node| node.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();

            // Assert — pruned subtree absent entirely, surviving entry kept.
            assert_eq!(names.len(), 2);
            assert!(names.contains(&"note.md".to_owned()));
            assert!(!names.contains(&"HEAD".to_owned()));
        }

        #[test]
        fn filter_keeps_files_whose_name_matches_the_predicate() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            write(root, ".git");

            // Act
            let mut names: Vec<String> = DirTree::descendants(root)
                .filter(|node| node.file_name() == ".git")
                .map(|entry| entry.expect("entry is ok"))
                .map(|node| node.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();

            // Assert — the predicate only prunes directories; a file named
            // `.git` passes through untouched (alongside the walk root).
            assert_eq!(names.len(), 2);
            assert!(names.contains(&".git".to_owned()));
        }

        #[test]
        fn sorted_by_orders_entries_within_each_directory() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            write(root, "b/zero.md");
            write(root, "a.md");
            write(root, "b/one.md");

            // Act
            let relatives: Vec<String> = DirTree::descendants(root)
                .sorted_by(|a, b| a.file_name().cmp(b.file_name()))
                .map(|entry| entry.expect("entry is ok"))
                .map(|node| {
                    node.path()
                        .strip_prefix(root)
                        .expect("under root")
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();

            // Assert — depth-first with siblings name-ordered per directory;
            // the root itself still comes first.
            assert_eq!(relatives, vec![
                "",
                "a.md",
                "b",
                "b/one.md",
                "b/zero.md"
            ]);
        }

        #[test]
        fn filter_composes_with_sorted_by() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            write(root, ".git/HEAD");
            write(root, "b/note.md");

            // Act
            let relatives: Vec<String> = DirTree::descendants(root)
                .filter(|node| node.file_name() == ".git")
                .sorted_by(|a, b| a.file_name().cmp(b.file_name()))
                .map(|entry| entry.expect("entry is ok"))
                .map(|node| {
                    node.path()
                        .strip_prefix(root)
                        .expect("under root")
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();

            // Assert — pruned subtree absent AND surviving siblings ordered.
            assert_eq!(relatives, vec!["", "b", "b/note.md"]);
        }
    }

    /// RAII guard that restores permissions on drop so that a `0o000` directory
    /// does not block the tempdir's cleanup.
    #[cfg(unix)]
    struct RestorePermissions<'a>(&'a Path);

    #[cfg(unix)]
    impl Drop for RestorePermissions<'_> {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ =
                fs::set_permissions(self.0, fs::Permissions::from_mode(0o700));
        }
    }

    mod classification {
        use pretty_assertions::assert_eq;

        use super::*;

        #[cfg(unix)]
        #[test]
        fn unreadable_root_reports_root_inaccessible_never_missing_root() {
            use std::os::unix::fs::PermissionsExt;

            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            write(root, "inside.md");
            fs::set_permissions(root, fs::Permissions::from_mode(0o000))
                .expect("revoke root permissions");
            let _restore = RestorePermissions(root);

            // Act
            let collected: Vec<_> = DirTree::children(root).collect();

            // Assert — stat on the root still succeeds (parent grants it),
            // so this is an access failure, not absence.
            assert_eq!(collected.len(), 1);
            let error = collected
                .into_iter()
                .next()
                .expect("one item")
                .expect_err("is an error");
            assert!(
                matches!(error, DirTreeError::RootInaccessible { .. }),
                "expected RootInaccessible, got {error:?}"
            );
        }

        #[cfg(unix)]
        #[test]
        fn children_yields_an_unreadable_subdirectory_without_error() {
            use std::os::unix::fs::PermissionsExt;

            // Arrange — walkdir opens child directories eagerly even under
            // max_depth(1), but a failed open is stored in the child's dir
            // list and popped before it can be polled, so flat listings
            // yield the locked directory as a plain entry with no error.
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            let kid = root.join("locked-kid");
            fs::create_dir(&kid).expect("create locked dir");
            fs::set_permissions(&kid, fs::Permissions::from_mode(0o000))
                .expect("revoke permissions");
            let _restore = RestorePermissions(&kid);

            // Act
            let collected: Vec<_> = DirTree::children(root).collect();

            // Assert
            assert_eq!(collected.len(), 1);
            assert!(
                !collected.iter().any(Result::is_err),
                "flat listing surfaces no error: {collected:?}"
            );
            let node = collected
                .into_iter()
                .next()
                .expect("one item")
                .expect("entry is ok");
            assert_eq!(node.file_name(), std::ffi::OsStr::new("locked-kid"));
        }

        #[cfg(unix)]
        #[test]
        fn descendants_reports_an_unreadable_subdirectory_naming_it() {
            use std::os::unix::fs::PermissionsExt;

            // Arrange — unlike `children`, an unlimited-depth walk polls the
            // stored open-failure and surfaces it.
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            let kid = root.join("locked-kid");
            fs::create_dir(&kid).expect("create locked dir");
            fs::set_permissions(&kid, fs::Permissions::from_mode(0o000))
                .expect("revoke permissions");
            let _restore = RestorePermissions(&kid);

            // Act
            let mut errors = DirTree::descendants(root).filter_map(Result::err);

            // Assert
            let error = errors.next();
            assert!(
                matches!(&error, Some(DirTreeError::NodeInaccessible { .. })),
                "expected NodeInaccessible, got {error:?}"
            );
            let (path, _) = error.expect("present").into_parts();
            assert_eq!(path, kid);
            assert!(errors.next().is_none(), "exactly one error");
        }
    }

    mod dirnode {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn exposes_path_file_name_and_file_type() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            let file = write(root, "daily.md");

            // Act
            let node = DirTree::children(root)
                .next()
                .expect("one entry")
                .expect("entry is ok");

            // Assert
            assert_eq!(node.path(), file);
            assert_eq!(node.file_name(), std::ffi::OsStr::new("daily.md"));
            assert!(node.file_type().is_file());
        }

        #[test]
        fn metadata_reads_size_and_mtime() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            write(root, "daily.md");

            // Act
            let node = DirTree::children(root)
                .next()
                .expect("one entry")
                .expect("entry is ok");
            let metadata = node.metadata().expect("metadata reads");

            let expected_len =
                u64::try_from("content".len()).expect("len fits u64");

            // Assert
            assert_eq!(metadata.len(), expected_len);
            assert!(metadata.modified().is_ok());
        }
    }

    mod display {
        use super::*;

        #[test]
        fn messages_are_lowercase_without_trailing_punctuation() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let missing = temp.path().join("gone");
            let error = DirTree::children(&missing)
                .next()
                .expect("one item")
                .expect_err("missing root");

            // Act
            let message = error.to_string();

            // Assert
            assert!(
                message.starts_with(char::is_lowercase),
                "message starts lowercase: {message}"
            );
            assert!(!message.ends_with('.') && !message.ends_with('!'));
        }
    }
}
