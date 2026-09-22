//! Index store row codec: bytes-of-a-row framing for redb payloads.
//!
//! Rows use postcard with [`StoreError`] context, and [`path_from_bytes`] has
//! a lossy fallback for non-Unicode paths read from byte-oriented stores. The
//! bytes-of-a-path codec for serde fields lives at [`crate::path::codec`].

use std::{
    mem,
    path::{Path, PathBuf},
    str,
};

use serde::{Serialize, de::DeserializeOwned};

use super::error::{StoreError, StoreResult};

/// Serializes `value` into `buf`, reusing its existing allocation.
///
/// Clears `buf` before writing. The returned slice borrows from `buf` and is
/// valid until the next call to this function or `buf.clear()`.
///
/// # Errors
///
/// - [`StoreError::Serialize`] when postcard serialization fails
///
/// [`StoreError::Serialize`]: StoreError::Serialize
pub(super) fn encode_row<'a, T: Serialize>(
    path: &Path,
    value: &T,
    buf: &'a mut Vec<u8>,
) -> StoreResult<&'a [u8]> {
    buf.clear();
    *buf = postcard::to_extend(value, mem::take(buf)).map_err(|source| {
        StoreError::Serialize {
            path: path.to_path_buf(),
            source,
        }
    })?;
    Ok(buf)
}

/// Deserializes `bytes`, capturing `path` in any storage error.
///
/// # Errors
///
/// - [`StoreError::Deserialize`] when postcard deserialization fails
pub(super) fn decode_row<T: DeserializeOwned>(
    path: &Path,
    bytes: &[u8],
) -> StoreResult<T> {
    postcard::from_bytes(bytes).map_err(|source| StoreError::Deserialize {
        path: path.to_path_buf(),
        source,
    })
}

/// Borrowed redb key carrying a project-relative path's native bytes.
///
/// Keys are written with [`Path::as_encoded_bytes`] and read back through the
/// lossy [`path_from_bytes`] fallback; serde row payloads go through
/// [`crate::path::codec`] instead.
///
/// Wraps the path rather than the encoded bytes: error construction and row
/// payloads borrow the same `&Path`, and converting bytes back to an `OsStr`
/// would require the unsafe `from_encoded_bytes_unchecked`.
#[derive(Copy, Clone)]
pub(super) struct IndexPathKey<'a>(&'a Path);

impl<'a> IndexPathKey<'a> {
    #[inline]
    pub(super) fn new(path: &'a Path) -> Self {
        Self(path)
    }

    #[inline]
    pub(super) fn as_bytes(&self) -> &'a [u8] {
        self.0.as_os_str().as_encoded_bytes()
    }

    #[inline]
    pub(super) fn path(&self) -> &'a Path {
        self.0
    }
}

/// Builds a path from byte-oriented store data.
///
/// Tries UTF-8 first and falls back to lossy decoding for non-Unicode paths.
/// The fallback can only surface in row-key decode context and query-output
/// paths (tag, class, and folder lookups); stored `LINKS` edges resolve
/// byte-exactly through loaded rows instead, never through this fallback.
pub(super) fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    str::from_utf8(bytes).map_or_else(
        |_| PathBuf::from(String::from_utf8_lossy(bytes).into_owned()),
        PathBuf::from,
    )
}

#[cfg(test)]
mod tests {
    mod encode {
        use pretty_assertions::assert_eq;
        use serde::{Deserialize, Serialize};

        use super::super::encode_row;
        use crate::index::error::StoreError;

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

            let mut buf = Vec::new();
            let bytes =
                encode_row(path, &item, &mut buf).expect("encode succeeds");
            let decoded: Dummy =
                postcard::from_bytes(bytes).expect("decode succeeds");
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

            let mut buf = Vec::new();
            let result = encode_row(path, &Failing, &mut buf);

            assert!(matches!(result, Err(StoreError::Serialize { .. })));
        }
    }

    mod decode {
        use pretty_assertions::assert_eq;
        use serde::{Deserialize, Serialize};

        use super::super::decode_row;
        use crate::index::error::StoreError;

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
            assert!(matches!(result, Err(StoreError::Deserialize { .. })));
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
}
