//! BLAKE3-based hashing: file content hashing ([`Blake3FileHash`]) and path
//! string hashing ([`Blake3PathHash`]).
//!
//! [`HashError`] is `thiserror`-only, no `miette::Diagnostic`: callers wrap
//! it in their own domain error before it reaches anything CLI-facing.

use std::{
    fmt::{self, Display, Formatter},
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

/// Errors from hashing file contents.
#[derive(Debug, Error)]
pub(crate) enum HashError {
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

impl TryFrom<&Path> for Blake3FileHash {
    type Error = HashError;

    /// Computes the BLAKE3 hash of `path`'s current contents.
    ///
    /// Reads `path` fully into memory before hashing — acceptable for the
    /// small template/config files this crate hashes, not a streaming read.
    ///
    /// # Errors
    ///
    /// Returns [`HashError::Read`] when `path` cannot be read.
    #[inline]
    fn try_from(path: &Path) -> Result<Self, HashError> {
        let contents = fs::read(path).map_err(|source| HashError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(Self(blake3::hash(&contents)))
    }
}

impl From<&str> for Blake3FileHash {
    /// Computes the BLAKE3 hash of `content` already read into memory.
    ///
    /// Prefer this over hashing from a `&Path` (`TryFrom<&Path>`)
    /// whenever the caller already holds the file's content in hand —
    /// hashing from a path and then reading that same path again for use
    /// opens a TOCTOU window between the two reads.
    #[inline]
    fn from(content: &str) -> Self {
        Self(blake3::hash(content.as_bytes()))
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
///
/// Stores the hex digest as a fixed-size byte array rather than a heap
/// `String` — BLAKE3's hex encoding is always exactly 64 ASCII bytes, so a
/// stack array avoids an allocation this type constructs often (once per
/// tracked/trusted store entry).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Blake3PathHash([u8; 64]);

impl From<&Path> for Blake3PathHash {
    /// Hashes `path`'s bytes to a hex string.
    #[inline]
    fn from(path: &Path) -> Self {
        let hash = blake3::hash(path.as_os_str().as_encoded_bytes());
        let mut bytes = [0_u8; 64];
        bytes.copy_from_slice(hash.to_hex().as_bytes());
        Self(bytes)
    }
}

impl Blake3PathHash {
    /// The hash string to use as a store entry filename.
    #[inline]
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "blake3's hex digest is always ASCII; failure here means \
                  blake3 itself violated its documented output format, not a \
                  recoverable caller error"
    )]
    pub(crate) fn as_str(&self) -> &str {
        str::from_utf8(&self.0)
            .expect("blake3 hex digest is always valid ASCII/UTF-8")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod file_hash {
        use std::fs;

        use pretty_assertions::{assert_eq, assert_ne};

        use super::*;

        #[test]
        fn deterministic_for_same_content() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("file.txt");
            fs::write(&path, "hello").expect("write file");

            // Act
            let first = Blake3FileHash::try_from(path.as_path());
            let second = Blake3FileHash::try_from(path.as_path());

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
            let first = Blake3FileHash::try_from(path1.as_path());
            let second = Blake3FileHash::try_from(path2.as_path());

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
            let result = Blake3FileHash::try_from(path.as_path());

            // Assert
            assert!(matches!(result, Err(HashError::Read { .. })));
        }

        #[test]
        fn implements_display_formatting() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("file.txt");
            fs::write(&path, "hello").expect("write file");
            let hash =
                Blake3FileHash::try_from(path.as_path()).expect("hash file");

            // Act
            let display_string = format!("{hash}");

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
            let first = Blake3PathHash::from(path);
            let second = Blake3PathHash::from(path);

            // Assert
            assert_eq!(first, second);
        }

        #[test]
        fn differs_for_different_path_strings() {
            // Arrange
            let first_path = Path::new("/project/first");
            let second_path = Path::new("/project/second");

            // Act
            let first = Blake3PathHash::from(first_path);
            let second = Blake3PathHash::from(second_path);

            // Assert
            assert_ne!(first, second);
        }

        #[test]
        fn matches_blake3_hex_formula_for_path_bytes() {
            // Arrange
            let path = Path::new("/project/.traces/config.toml");

            // Act
            let hash = Blake3PathHash::from(path);

            // Assert
            let expected_hex =
                blake3::hash(path.as_os_str().as_encoded_bytes())
                    .to_hex()
                    .to_string();
            assert_eq!(hash.as_str(), expected_hex.as_str());
        }

        #[test]
        fn exposes_as_str_for_inner_hash() {
            // Arrange
            let path = Path::new("/project/.traces/config.toml");
            let hash = Blake3PathHash::from(path);

            // Act
            let hash_str = hash.as_str();

            // Assert
            let expected_hex =
                blake3::hash(path.as_os_str().as_encoded_bytes())
                    .to_hex()
                    .to_string();
            assert_eq!(hash_str, expected_hex);
        }
    }
}
