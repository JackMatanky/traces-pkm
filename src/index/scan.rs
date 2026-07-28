//! Filesystem walk building a [`FileRecord`] for every regular file under a
//! project root.

use std::{fs, path::Path};

use super::{INDEX_FILE, domain::FileRecord, error::FileIndexError};

/// Recursively scans `root`, returning a File Record for every regular file,
/// sorted by path for deterministic output.
///
/// Skips `.git` directories (VCS metadata, not project content) and the
/// `FileIndex`'s own database file (avoids the index indexing itself).
/// Symlinks are not followed, so they're skipped rather than resolved —
/// ponytail: revisit if PKM projects turn out to rely on symlinked notes.
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
    let mut pending = vec![root.to_path_buf()];

    while let Some(dir) = pending.pop() {
        for entry in read_dir(&dir)? {
            let entry = entry.map_err(|source| FileIndexError::Io {
                path: dir.clone(),
                source,
            })?;
            let path = entry.path();
            let file_type =
                entry.file_type().map_err(|source| FileIndexError::Io {
                    path: path.clone(),
                    source,
                })?;

            #[expect(
                clippy::else_if_without_else,
                reason = "a symlink or other non-dir/non-file entry falls \
                          through both branches deliberately — nothing to \
                          index, no else case needed"
            )]
            if file_type.is_dir() {
                if !is_git_dir(&path) {
                    pending.push(path);
                }
            } else if file_type.is_file() && path != index_db {
                let metadata =
                    entry.metadata().map_err(|source| FileIndexError::Io {
                        path: path.clone(),
                        source,
                    })?;
                records
                    .push(FileRecord::from_metadata(&path, root, &metadata)?);
            }
        }
    }

    records.sort_by(|a, b| a.path().cmp(b.path()));
    Ok(records)
}

fn read_dir(dir: &Path) -> Result<fs::ReadDir, FileIndexError> {
    fs::read_dir(dir).map_err(|source| FileIndexError::Io {
        path: dir.to_path_buf(),
        source,
    })
}

fn is_git_dir(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == ".git")
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn empty_root_yields_no_records() {
        let temp = tempfile::tempdir().expect("create temp dir");

        let records = scan_root(temp.path()).expect("scan root");

        assert_eq!(records.len(), 0);
    }
}
