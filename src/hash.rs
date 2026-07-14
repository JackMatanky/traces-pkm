//! Generic file-content hashing.
//!
//! Not config-specific: this is a plain BLAKE3-over-file-bytes utility any
//! module can reach for. [`HashError`] is deliberately `thiserror`-only, no
//! `miette::Diagnostic` — a raw hashing I/O failure is never shown to a user
//! or agent directly; callers wrap it in their own domain error before it
//! reaches anything CLI-facing.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

/// Errors from [`hash_file`].
///
/// Public (not `pub(crate)`) because [`crate::config::trust::TrustError`]
/// carries it as a `#[from]` source, and a `pub` field can't have a
/// private type — same reasoning as `config::store::StoreError`.
#[derive(Debug, Error)]
pub enum HashError {
    /// The file's contents could not be read.
    #[error("failed to read {path} for hashing")]
    Read {
        /// Path that could not be read.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
}

/// Computes the BLAKE3 hash of `path`'s current contents.
///
/// # Errors
///
/// Returns [`HashError::Read`] when `path` cannot be read.
#[inline]
pub(crate) fn hash_file(path: &Path) -> Result<blake3::Hash, HashError> {
    let contents = fs::read(path).map_err(|source| HashError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(blake3::hash(&contents))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn hash_file_is_deterministic_for_the_same_content() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("file.txt");
        fs::write(&path, "hello").expect("write file");

        let first = hash_file(&path).expect("hash file");
        let second = hash_file(&path).expect("hash file again");

        assert_eq!(first, second);
    }

    #[test]
    fn hash_file_differs_when_content_changes() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("file.txt");
        fs::write(&path, "hello").expect("write file");
        let original = hash_file(&path).expect("hash file");

        fs::write(&path, "goodbye").expect("rewrite file");
        let updated = hash_file(&path).expect("hash updated file");

        assert_ne!(original, updated);
    }

    #[test]
    fn hash_file_of_a_missing_file_errors() {
        let temp = tempfile::tempdir().expect("create temp dir");

        assert!(matches!(
            hash_file(&temp.path().join("missing.txt")),
            Err(HashError::Read { .. })
        ));
    }
}
