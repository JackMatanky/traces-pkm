//! Recursive filesystem scan for a project root.
//!
//! [`scan_root`] walks the directory tree via [`crate::dirtree`], collects
//! every regular file as a [`FileBase`], and returns them sorted by
//! project-relative path. Skipped:
//!
//! - `.git` directories and their descendants (via `skipping`)
//! - The index database file (`.traces/index.redb`)
//! - Symbolic links
//!
//! The sorted output is a precondition for the merge-join reconciliation in
//! [`super::builder::IndexBuilder`].

use std::path::Path;

use super::{INDEX_FILE, error::IndexBuilderError};
use crate::{DirDescendants, DirTreeError, file::FileBase};

/// Converts any classified walk failure into the builder's scan error.
///
/// Replaces the deleted `io_error` helper: path context and I/O conversion
/// now happen inside `dirtree`, so this is a straight rewrap.
fn scan_error(error: DirTreeError) -> IndexBuilderError {
    let (path, source) = error.into_parts();
    IndexBuilderError::Scan {
        path,
        source,
    }
}

/// Recursively scans `root` for regular files and returns sorted records.
///
/// Skips `.git` directories (and their descendants), the index database
/// itself, and symlinks.
///
/// # Errors
///
/// - [`IndexBuilderError::Scan`] if a directory cannot be read or a file's
///   metadata cannot be inspected.
pub(super) fn scan_root(
    root: &Path,
) -> Result<Vec<FileBase>, IndexBuilderError> {
    let index_db = root.join(INDEX_FILE);
    let mut bases = Vec::new();
    let nodes =
        DirDescendants::new(root).skipping(|node| node.file_name() == ".git");
    for node in nodes {
        let node = node.map_err(scan_error)?;
        let path = node.path();
        if !node.file_type().is_file() || path == index_db {
            continue;
        }
        let metadata = node.metadata().map_err(scan_error)?;
        bases.push(FileBase::from_metadata(path, root, &metadata).map_err(
            |source| IndexBuilderError::Scan {
                path: path.to_path_buf(),
                source,
            },
        )?);
    }

    bases.sort_by(|a, b| a.path().cmp(b.path()));
    Ok(bases)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::index::tests::fixtures::RestorePermissions;

    mod scan_root {
        use std::fs;

        use pretty_assertions::assert_eq;

        use super::*;

        fn names(bases: &[FileBase]) -> Vec<&Path> {
            bases.iter().map(FileBase::path).collect()
        }

        #[test]
        fn scans_nested_files_in_sorted_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::create_dir_all(root.join("b")).expect("mkdir b");
            fs::write(root.join("b/one.md"), "1").expect("write b/one.md");
            fs::write(root.join("a.md"), "2").expect("write a.md");

            let bases = scan_root(root).expect("scan root");

            assert_eq!(names(&bases), vec![
                Path::new("a.md"),
                Path::new("b/one.md")
            ]);
        }

        #[test]
        fn skips_git_directories() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::create_dir_all(root.join(".git")).expect("mkdir .git");
            fs::write(root.join(".git/HEAD"), "ref: refs/heads/main")
                .expect("write .git/HEAD");
            fs::write(root.join("note.md"), "content").expect("write note.md");

            let bases = scan_root(root).expect("scan root");

            assert_eq!(names(&bases), vec![Path::new("note.md")]);
        }

        #[test]
        fn skips_its_own_index_database_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::create_dir_all(root.join(".traces")).expect("mkdir .traces");
            fs::write(root.join(INDEX_FILE), b"redb-bytes")
                .expect("write index db");
            fs::write(root.join("note.md"), "content").expect("write note.md");

            let bases = scan_root(root).expect("scan root");

            assert_eq!(names(&bases), vec![Path::new("note.md")]);
        }

        #[cfg(unix)]
        #[test]
        fn skips_symlinks_entirely() {
            use std::os::unix::fs::symlink;

            let temp = tempfile::tempdir().expect("create temp dir");
            let outside = tempfile::tempdir().expect("create outside dir");
            let root = temp.path();
            let target = outside.path().join("outside.md");
            fs::write(&target, "content").expect("write link target");
            symlink(&target, root.join("link.md")).expect("create symlink");
            fs::write(root.join("note.md"), "content").expect("write note.md");

            let bases = scan_root(root).expect("scan root");

            assert_eq!(names(&bases), vec![Path::new("note.md")]);
        }

        #[test]
        fn empty_root_yields_no_records() {
            let temp = tempfile::tempdir().expect("create temp dir");

            let bases = scan_root(temp.path()).expect("scan root");

            assert_eq!(bases.len(), 0);
        }

        #[cfg(unix)]
        #[test]
        fn returns_an_io_error_when_a_directory_is_unreadable() {
            use std::os::unix::fs::PermissionsExt as _;

            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            let locked = root.join("locked");
            fs::create_dir(&locked).expect("create locked dir");
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o000))
                .expect("revoke read permission");
            let _restore = RestorePermissions(&locked);

            let error = scan_root(root).expect_err("unreadable dir fails");

            assert!(matches!(error, IndexBuilderError::Scan { .. }));
        }
    }
}
