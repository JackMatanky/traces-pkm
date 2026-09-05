//! Encoding, decoding, and path reconstruction for the index store.
//!
//! [`encode_row`] and [`decode_row`] wrap (de)serialization with [`DbError`]
//! mapping. [`path_from_bytes`] recovers a [`PathBuf`] from raw bytes, trying
//! UTF-8 first and falling back to lossy decoding. The [`path`] module provides
//! serde support for [`std::path::Path`] and [`std::path::PathBuf`] using
//! platform-specific raw byte representations.

use std::{
    path::{Path, PathBuf},
    str,
};

use serde::{Serialize, de::DeserializeOwned};

use super::error::{DbError, DbResult};

/// Encodes `value` for a row keyed by `path`, wrapping a failure as
/// [`DbError::Serialize`].
pub(super) fn encode_row<T: Serialize>(
    path: &Path,
    value: &T,
) -> DbResult<Vec<u8>> {
    postcard::to_allocvec(value).map_err(|source| DbError::Serialize {
        path: path.to_path_buf(),
        source,
    })
}

/// Decodes `bytes` for a row keyed by `path`, wrapping a failure as
/// [`DbError::Deserialize`].
pub(super) fn decode_row<T: DeserializeOwned>(
    path: &Path,
    bytes: &[u8],
) -> DbResult<T> {
    postcard::from_bytes(bytes).map_err(|source| DbError::Deserialize {
        path: path.to_path_buf(),
        source,
    })
}

/// Recovers a [`PathBuf`] from raw bytes, trying UTF-8 first and falling back
/// to lossy decoding for non-Unicode paths.
///
/// The lossy fallback affects only refresh-diff link paths;
/// [`IndexStore::read_all`] resolves stored link bytes against loaded notes for
/// byte-exact query output. `LISTS` keys are always valid UTF-8 by
/// construction (see [`IndexStore::write_lists_for_note`]), so the fallback
/// never triggers for [`IndexStore::read_lists`] or
/// [`IndexStore::read_lists_for_path`].
///
/// Used by [`IndexStore::read_table`]'s deserialization-error path,
/// [`IndexStore::read_files_and_links_via`]'s link reconstruction, and the
/// `LISTS` table readers.
///
/// [`IndexStore::read_all`]: super::store::IndexStore::read_all
/// [`IndexStore::read_table`]: super::store::IndexStore::read_table
/// [`IndexStore::read_files_and_links_via`]: super::store::IndexStore::read_files_and_links_via
/// [`IndexStore::read_lists`]: super::store::IndexStore::read_lists
/// [`IndexStore::read_lists_for_path`]: super::store::IndexStore::read_lists_for_path
/// [`IndexStore::write_lists_for_note`]: super::store::IndexStore::write_lists_for_note
pub(super) fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    str::from_utf8(bytes).map_or_else(
        |_| PathBuf::from(String::from_utf8_lossy(bytes).into_owned()),
        PathBuf::from,
    )
}

/// Serde support for [`std::path::Path`] and [`std::path::PathBuf`] using
/// platform-specific byte representations.
pub mod path {
    use std::path::{Path, PathBuf};

    use serde::{Deserialize, Deserializer, Serializer};
    /// Serializes a [`Path`] as raw bytes.
    ///
    /// # Errors
    ///
    /// Returns the serializer's error if encoding fails.
    #[cfg(unix)]
    #[inline]
    pub fn serialize<S>(path: &Path, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use std::os::unix::ffi::OsStrExt as _;
        serializer.serialize_bytes(path.as_os_str().as_bytes())
    }

    /// Deserializes a [`PathBuf`] from raw bytes.
    ///
    /// # Errors
    ///
    /// Returns the deserializer's error if decoding fails.
    #[cfg(unix)]
    #[inline]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<PathBuf, D::Error>
    where
        D: Deserializer<'de>,
    {
        use std::os::unix::ffi::OsStringExt as _;
        let bytes = <Vec<u8>>::deserialize(deserializer)?;
        Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
    }

    /// Serializes a [`Path`] as wide characters.
    ///
    /// # Errors
    ///
    /// Returns the serializer's error if encoding fails.
    #[cfg(windows)]
    #[inline]
    pub fn serialize<S>(path: &Path, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use std::os::windows::ffi::OsStrExt as _;
        let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        wide.serialize(serializer)
    }

    /// Deserializes a [`PathBuf`] from wide characters.
    ///
    /// # Errors
    ///
    /// Returns the deserializer's error if decoding fails.
    #[cfg(windows)]
    #[inline]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<PathBuf, D::Error>
    where
        D: Deserializer<'de>,
    {
        use std::os::windows::ffi::OsStringExt as _;
        let wide = <Vec<u16>>::deserialize(deserializer)?;
        Ok(PathBuf::from(std::ffi::OsString::from_wide(&wide)))
    }

    /// Serializes a [`Path`] as a lossless string representation.
    ///
    /// # Errors
    ///
    /// Returns the serializer's error if encoding fails.
    #[cfg(not(any(unix, windows)))]
    #[inline]
    pub fn serialize<S>(path: &Path, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&path.to_string_lossy())
    }

    /// Deserializes a [`PathBuf`] from a string representation.
    ///
    /// # Errors
    ///
    /// Returns the deserializer's error if decoding fails.
    #[cfg(not(any(unix, windows)))]
    #[inline]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<PathBuf, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = <String>::deserialize(deserializer)?;
        Ok(PathBuf::from(s))
    }
}

#[cfg(test)]
mod tests {
    mod encode {
        use pretty_assertions::assert_eq;
        use serde::{Deserialize, Serialize};

        use super::super::encode_row;
        use crate::index::error::DbError;

        #[derive(Debug, PartialEq, Deserialize, Serialize)]
        struct Dummy {
            value: String,
        }

        #[test]
        fn serializes_successfully() {
            let path = std::path::Path::new("test.md");
            let item = Dummy {
                value: "hello".to_owned(),
            };

            let bytes = encode_row(path, &item).expect("encode succeeds");
            let decoded: Dummy =
                postcard::from_bytes(&bytes).expect("decode succeeds");
            assert_eq!(decoded, item);
        }

        #[test]
        fn fails_on_serialization_error() {
            struct Failing;
            impl Serialize for Failing {
                fn serialize<S>(
                    &self,
                    _serializer: S,
                ) -> Result<S::Ok, S::Error>
                where
                    S: serde::Serializer,
                {
                    Err(serde::ser::Error::custom("forced error"))
                }
            }
            let path = std::path::Path::new("test.md");

            let result = encode_row(path, &Failing);

            assert!(matches!(result, Err(DbError::Serialize { .. })));
        }
    }

    mod decode {
        use pretty_assertions::assert_eq;
        use serde::{Deserialize, Serialize};

        use super::super::decode_row;
        use crate::index::error::DbError;

        #[derive(Debug, PartialEq, Deserialize, Serialize)]
        struct Dummy {
            value: String,
        }

        #[test]
        fn deserializes_successfully() {
            let path = std::path::Path::new("test.md");
            let item = Dummy {
                value: "hello".to_owned(),
            };
            let bytes = postcard::to_allocvec(&item).unwrap();

            let decoded: Dummy =
                decode_row(path, &bytes).expect("decode succeeds");

            assert_eq!(decoded, item);
        }

        #[test]
        fn fails_on_corrupt_bytes() {
            let path = std::path::Path::new("test.md");
            let result: Result<Dummy, _> = decode_row(path, &[0xFF, 0x00]);
            assert!(matches!(result, Err(DbError::Deserialize { .. })));
        }
    }

    mod path_from_bytes {
        use std::path::PathBuf;

        use pretty_assertions::assert_eq;

        use super::super::path_from_bytes;

        #[test]
        fn decodes_valid_utf8() {
            let bytes = b"hello.md";
            let path = path_from_bytes(bytes);
            assert_eq!(path, PathBuf::from("hello.md"));
        }

        #[test]
        fn falls_back_to_lossy_on_invalid_utf8() {
            let bytes = b"hello\xFF.md";
            let path = path_from_bytes(bytes);
            assert_eq!(path, PathBuf::from("hello\u{FFFD}.md"));
        }
    }

    mod path_codec {
        use std::path::PathBuf;

        use pretty_assertions::assert_eq;
        use serde::{Deserialize, Serialize};

        use super::super::path;

        #[derive(Debug, PartialEq, Deserialize, Serialize)]
        struct PathWrapper {
            #[serde(with = "path")]
            path: PathBuf,
        }

        #[test]
        fn round_trips_valid_utf8() {
            let path = PathBuf::from("hello.md");
            let item = PathWrapper {
                path,
            };
            let bytes = postcard::to_allocvec(&item).expect("serialize path");
            let decoded: PathWrapper =
                postcard::from_bytes(&bytes).expect("deserialize path");

            assert_eq!(decoded, item);
        }

        #[cfg(unix)]
        #[test]
        fn round_trips_non_unicode_on_unix() {
            use std::os::unix::ffi::OsStringExt as _;
            let weird_os =
                std::ffi::OsString::from_vec(b"weird\xFF.md".to_vec());
            let path = PathBuf::from(weird_os);
            let item = PathWrapper {
                path,
            };
            let bytes = postcard::to_allocvec(&item).expect("serialize path");
            let decoded: PathWrapper =
                postcard::from_bytes(&bytes).expect("deserialize path");

            assert_eq!(decoded, item);
        }

        #[cfg(windows)]
        #[test]
        fn round_trips_non_unicode_on_windows() {
            use std::os::windows::ffi::OsStringExt as _;
            let weird_os = std::ffi::OsString::from_wide(&[
                119, 101, 105, 114, 100, 0xD800, 46, 109, 100,
            ]);
            let path = PathBuf::from(weird_os);
            let item = PathWrapper {
                path,
            };
            let bytes = postcard::to_allocvec(&item).expect("serialize path");
            let decoded: PathWrapper =
                postcard::from_bytes(&bytes).expect("deserialize path");

            assert_eq!(decoded, item);
        }
    }
}
