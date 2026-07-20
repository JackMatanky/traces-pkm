//! BLAKE3-based hashing: file content hashing ([`Blake3FileHash`]) and path
//! string hashing ([`Blake3PathHash`]).
//!
//! Not config-specific: these are plain utilities any module can reach
//! for. [`HashError`] is deliberately `thiserror`-only, no
//! `miette::Diagnostic` — a raw hashing I/O failure is never shown to a
//! user or agent directly; callers wrap it in their own domain error
//! before it reaches anything CLI-facing.

use std::{
    fmt::{self, Display, Formatter},
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

/// Errors from hashing file contents.
///
/// Public (not `pub(crate)`) because config-facing error types carry it as a
/// `#[from]` source, and a `pub` field can't have a private type.
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

/// The BLAKE3 hash of a file's *contents*.
///
/// Distinct from [`Blake3PathHash`], which hashes a path string.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Blake3FileHash(blake3::Hash);

impl Blake3FileHash {
    /// Computes the BLAKE3 hash of `path`'s current contents.
    ///
    /// # Errors
    ///
    /// Returns [`HashError::Read`] when `path` cannot be read.
    #[inline]
    pub(crate) fn new(path: &Path) -> Result<Self, HashError> {
        let contents = fs::read(path).map_err(|source| HashError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(Self(blake3::hash(&contents)))
    }
}

impl Display for Blake3FileHash {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// The BLAKE3 hex hash of a path string (not its contents).
///
/// Used as a hash-keyed store filename (see
/// [`crate::file_store::FileStateStore`]). Callers that need canonical keys
/// must canonicalize before constructing this value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Blake3PathHash(String);

impl Blake3PathHash {
    /// Hashes `path`'s bytes to a hex string.
    #[inline]
    #[must_use]
    pub(crate) fn new(path: &Path) -> Self {
        let hash = blake3::hash(path.as_os_str().as_encoded_bytes());
        Self(hash.to_hex().to_string())
    }

    /// The hash string to use as a store entry filename.
    #[inline]
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_ne};

    use super::*;

    #[test]
    fn hash_file_contents_is_deterministic_for_the_same_content() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("file.txt");
        fs::write(&path, "hello").expect("write file");

        let first = Blake3FileHash::new(&path).expect("hash file");
        let second = Blake3FileHash::new(&path).expect("hash file again");

        assert_eq!(first, second);
    }

    #[test]
    fn hash_file_contents_differs_when_content_changes() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("file.txt");
        fs::write(&path, "hello").expect("write file");
        let original = Blake3FileHash::new(&path).expect("hash file");

        fs::write(&path, "goodbye").expect("rewrite file");
        let updated = Blake3FileHash::new(&path).expect("hash updated file");

        assert_ne!(original, updated);
    }

    #[test]
    fn hash_file_contents_of_a_missing_file_errors() {
        let temp = tempfile::tempdir().expect("create temp dir");

        assert!(matches!(
            Blake3FileHash::new(&temp.path().join("missing.txt")),
            Err(HashError::Read { .. })
        ));
    }

    #[test]
    fn hash_path_is_deterministic_for_the_same_path_string() {
        let path = Path::new("/project/.traces/config.toml");

        assert_eq!(Blake3PathHash::new(path), Blake3PathHash::new(path));
    }

    #[test]
    fn hash_path_differs_for_different_path_strings() {
        assert_ne!(
            Blake3PathHash::new(Path::new("/project/first")),
            Blake3PathHash::new(Path::new("/project/second")),
        );
    }

    #[test]
    fn hash_path_matches_the_blake3_hex_formula_for_path_bytes() {
        let path = Path::new("/project/.traces/config.toml");

        assert_eq!(
            Blake3PathHash::new(path),
            Blake3PathHash(
                blake3::hash(path.as_os_str().as_encoded_bytes())
                    .to_hex()
                    .to_string()
            ),
        );
    }
}
