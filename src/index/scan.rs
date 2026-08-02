//! Filesystem scan for regular project files.

use std::path::Path;

use walkdir::WalkDir;

use super::{INDEX_FILE, error::FileIndexError, file::FileRecord};

/// Recursively scans `root` for regular files and returns sorted records.
///
/// Skips `.git` directories, the index database itself, and symlinks. Symlink
/// support can be added when PKM projects need linked note trees.
///
/// # Errors
///
/// Returns [`FileIndexError::Io`] if a directory cannot be read or a file's
/// metadata cannot be inspected.
pub(super) fn scan_root(
    root: &Path,
) -> Result<Vec<FileRecord>, FileIndexError> {
    let index_db = root.join(INDEX_FILE);
    let mut records = Vec::new();

    let entries = WalkDir::new(root).into_iter().filter_entry(|entry| {
        !(entry.file_type().is_dir() && is_git_dir(entry.path()))
    });
    for entry in entries {
        let entry = entry.map_err(|source| io_error(root, source))?;
        let path = entry.path();
        if !entry.file_type().is_file() || path == index_db {
            continue;
        }
        let metadata =
            entry.metadata().map_err(|source| io_error(root, source))?;
        records.push(FileRecord::from_metadata(path, root, &metadata)?);
    }

    records.sort_by(|a, b| a.path().cmp(b.path()));
    Ok(records)
}

/// Wraps a [`walkdir::Error`] with path context as a [`FileIndexError::Io`].
///
/// Falls back to `root` if the underlying error provides no path (such as
/// rare symlink loop errors).
fn io_error(root: &Path, source: walkdir::Error) -> FileIndexError {
    let path = source.path().unwrap_or(root).to_path_buf();
    FileIndexError::Io {
        path,
        source: source.into(),
    }
}

/// Returns `true` if `path` names a `.git` directory.
fn is_git_dir(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == ".git")
}

#[cfg(test)]
mod tests {
    use super::*;

    mod scan_root {
        use std::fs;

        use pretty_assertions::assert_eq;

        use super::*;

        fn names(records: &[FileRecord]) -> Vec<&Path> {
            records.iter().map(FileRecord::path).collect()
        }

        #[test]
        fn scans_nested_files_in_sorted_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::create_dir_all(root.join("b")).expect("mkdir b");
            fs::write(root.join("b/one.md"), "1").expect("write b/one.md");
            fs::write(root.join("a.md"), "2").expect("write a.md");

            let records = scan_root(root).expect("scan root");

            assert_eq!(names(&records), vec![
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

            let records = scan_root(root).expect("scan root");

            assert_eq!(names(&records), vec![Path::new("note.md")]);
        }

        #[test]
        fn skips_its_own_index_database_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::create_dir_all(root.join(".traces")).expect("mkdir .traces");
            fs::write(root.join(INDEX_FILE), b"redb-bytes")
                .expect("write index db");
            fs::write(root.join("note.md"), "content").expect("write note.md");

            let records = scan_root(root).expect("scan root");

            assert_eq!(names(&records), vec![Path::new("note.md")]);
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

            let records = scan_root(root).expect("scan root");

            assert_eq!(names(&records), vec![Path::new("note.md")]);
        }

        #[test]
        fn empty_root_yields_no_records() {
            let temp = tempfile::tempdir().expect("create temp dir");

            let records = scan_root(temp.path()).expect("scan root");

            assert_eq!(records.len(), 0);
        }

        #[cfg(unix)]
        #[test]
        fn returns_an_io_error_when_a_directory_is_unreadable() {
            use std::os::unix::fs::PermissionsExt as _;

            /// Restores a locked directory's permissions on drop, even if
            /// the test panics. Otherwise a `0o000` directory blocks the
            /// tempdir's own cleanup.
            struct RestorePermissions<'a>(&'a Path);

            impl Drop for RestorePermissions<'_> {
                fn drop(&mut self) {
                    let _ = fs::set_permissions(
                        self.0,
                        fs::Permissions::from_mode(0o700),
                    );
                }
            }

            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            let locked = root.join("locked");
            fs::create_dir(&locked).expect("create locked dir");
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o000))
                .expect("revoke read permission");
            let _restore = RestorePermissions(&locked);

            let error = scan_root(root).expect_err("unreadable dir fails");

            assert!(matches!(error, FileIndexError::Io { .. }));
        }
    }
}
