//! Generic hashing: file *content* ([`hash_file`]) and path *strings*
//! ([`hash_path_to_str`]), both BLAKE3-based.
//!
//! Not config-specific: these are plain utilities any module can reach
//! for. [`HashError`] is deliberately `thiserror`-only, no
//! `miette::Diagnostic` — a raw hashing I/O failure is never shown to a
//! user or agent directly; callers wrap it in their own domain error
//! before it reaches anything CLI-facing.

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

/// Hashes `path`'s bytes (not its contents) to a hex string, for use as a
/// hash-keyed store filename (see `config::store::ConfigFileStore`).
/// Distinct from [`hash_file`], which hashes a file's *content* — this
/// hashes the *path string itself*, and never fails (there's no I/O).
#[inline]
#[must_use]
pub(crate) fn hash_path_to_str(path: &Path) -> String {
    let hash = blake3::hash(path.as_os_str().as_encoded_bytes());
    hash.to_hex().to_string()
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

    #[test]
    fn hash_path_to_str_is_deterministic_for_the_same_path() {
        let path = Path::new("/some/project");

        assert_eq!(hash_path_to_str(path), hash_path_to_str(path));
    }

    #[test]
    fn hash_path_to_str_differs_for_different_paths() {
        assert_ne!(
            hash_path_to_str(Path::new("/some/project")),
            hash_path_to_str(Path::new("/some/other-project"))
        );
    }

    #[test]
    fn hash_path_to_str_matches_the_blake3_hex_formula() {
        let path = Path::new("/some/project");

        assert_eq!(
            hash_path_to_str(path),
            blake3::hash(path.as_os_str().as_encoded_bytes())
                .to_hex()
                .to_string()
        );
    }
}
