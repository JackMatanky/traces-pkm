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
/// [`crate::FileStateStore`]). Callers that need canonical keys
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
    use super::*;

    mod file_hash {
        use pretty_assertions::{assert_eq, assert_ne};
        use std::fs;

        use super::*;

        #[test]
        fn deterministic_for_same_content() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("file.txt");
            fs::write(&path, "hello").expect("write file");

            // Act
            let first = Blake3FileHash::new(&path);
            let second = Blake3FileHash::new(&path);

            // Assert
            assert!(first.is_ok());
            assert!(second.is_ok());
            assert_eq!(first.unwrap(), second.unwrap());
        }

        #[test]
        fn differs_when_content_changes() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let path1 = temp.path().join("file1.txt");
            let path2 = temp.path().join("file2.txt");
            fs::write(&path1, "hello").expect("write file 1");
            fs::write(&path2, "goodbye").expect("write file 2");

            // Act
            let first = Blake3FileHash::new(&path1);
            let second = Blake3FileHash::new(&path2);

            // Assert
            assert!(first.is_ok());
            assert!(second.is_ok());
            assert_ne!(first.unwrap(), second.unwrap());
        }

        #[test]
        fn error_when_file_is_missing() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("missing.txt");

            // Act
            let result = Blake3FileHash::new(&path);

            // Assert
            assert!(matches!(result, Err(HashError::Read { .. })));
        }

        #[test]
        fn implements_display_formatting() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("file.txt");
            fs::write(&path, "hello").expect("write file");
            let hash = Blake3FileHash::new(&path).expect("hash file");

            // Act
            let display_string = format!("{}", hash);

            // Assert
            assert_eq!(display_string.len(), 64);
            let expected_raw_hash = blake3::hash(b"hello").to_hex().to_string();
            assert_eq!(display_string, expected_raw_hash);
        }
    }

    mod path_hash {
        use pretty_assertions::{assert_eq, assert_ne};

        use super::*;

        #[test]
        fn deterministic_for_same_path_string() {
            // Arrange
            let path = Path::new("/project/.traces/config.toml");

            // Act
            let first = Blake3PathHash::new(path);
            let second = Blake3PathHash::new(path);

            // Assert
            assert_eq!(first, second);
        }

        #[test]
        fn differs_for_different_path_strings() {
            // Arrange
            let first_path = Path::new("/project/first");
            let second_path = Path::new("/project/second");

            // Act
            let first = Blake3PathHash::new(first_path);
            let second = Blake3PathHash::new(second_path);

            // Assert
            assert_ne!(first, second);
        }

        #[test]
        fn matches_blake3_hex_formula_for_path_bytes() {
            // Arrange
            let path = Path::new("/project/.traces/config.toml");

            // Act
            let hash = Blake3PathHash::new(path);

            // Assert
            let expected_hex = blake3::hash(path.as_os_str().as_encoded_bytes())
                .to_hex()
                .to_string();
            assert_eq!(hash, Blake3PathHash(expected_hex));
        }

        #[test]
        fn exposes_as_str_for_inner_hash() {
            // Arrange
            let path = Path::new("/project/.traces/config.toml");
            let hash = Blake3PathHash::new(path);

            // Act
            let hash_str = hash.as_str();

            // Assert
            let expected_hex = blake3::hash(path.as_os_str().as_encoded_bytes())
                .to_hex()
                .to_string();
            assert_eq!(hash_str, expected_hex);
        }
    }
}
