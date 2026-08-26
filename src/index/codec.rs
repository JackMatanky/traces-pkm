//! Serialization codecs and path byte-conversion helpers for the index store.
//!
//! Exposes helper functions for postcard encoding/decoding and path
//! reconstruction from database raw bytes.

use std::{
    path::{Path, PathBuf},
    str,
};

use serde::{Serialize, de::DeserializeOwned};

use super::error::DbError;

/// Postcard-encodes `value` for a row keyed by `path`, wrapping a failure as
/// [`DbError::Serialize`]. Shared by [`IndexStore::write_table`]'s loop and
/// [`IndexStore::upsert_row`], previously duplicated inline.
///
/// [`IndexStore::write_table`]: super::store::IndexStore::write_table
/// [`IndexStore::upsert_row`]: super::store::IndexStore::upsert_row
pub(super) fn encode_row<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<Vec<u8>, DbError> {
    postcard::to_allocvec(value).map_err(|source| DbError::Serialize {
        path: path.to_path_buf(),
        source,
    })
}

/// Postcard-decodes `bytes` for a row keyed by `path`, wrapping a failure as
/// [`DbError::Deserialize`]. Shared by [`IndexStore::read_table`]'s loop and
/// [`IndexStore::read_note`], previously duplicated inline. Not a `redb::Value`
/// impl: see `store.rs`'s module-level correction note.
///
/// [`IndexStore::read_table`]: super::store::IndexStore::read_table
/// [`IndexStore::read_note`]: super::store::IndexStore::read_note
pub(super) fn decode_row<T: DeserializeOwned>(
    path: &Path,
    bytes: &[u8],
) -> Result<T, DbError> {
    postcard::from_bytes(bytes).map_err(|source| DbError::Deserialize {
        path: path.to_path_buf(),
        source,
    })
}

/// Recovers a `PathBuf` from raw key/value bytes: an exact UTF-8 decode,
/// falling back to a lossy decode only for non-Unicode paths. No `unsafe`:
/// `OsStr::from_encoded_bytes_unchecked` would be exact for every input but
/// requires an `unsafe` block this crate avoids; the lossy fallback degrades
/// only non-Unicode filenames, matching this crate's pre-migration
/// `Path::to_string_lossy` behavior for the same edge case. In practice the
/// lossy branch is unreachable for stored records: [`FileBase`]/[`Note`]
/// serialize their paths through serde, which rejects non-Unicode paths at
/// write time, so no non-Unicode path ever reaches a stored key or edge. Used
/// by [`read_table`]'s per-row deserialize-error path and by
/// [`read_files_and_links_via`]'s link reconstruction (the refresh side, whose
/// links only feed `diff_inlinks`); [`read_all`] instead resolves stored link
/// bytes against the loaded notes, dropping orphaned edges.
///
/// [`read_table`]: super::store::IndexStore::read_table
/// [`read_all`]: super::store::IndexStore::read_all
/// [`read_files_and_links_via`]: super::store::IndexStore::read_files_and_links_via
/// [`FileBase`]: crate::file::FileBase
/// [`Note`]: crate::note::Note
pub(super) fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    str::from_utf8(bytes).map_or_else(
        |_| PathBuf::from(String::from_utf8_lossy(bytes).into_owned()),
        PathBuf::from,
    )
}

/// Custom serde serialization module for `std::path::Path` and
/// `std::path::PathBuf`. Serializes paths as target-specific raw byte slices
/// (on Unix) or wide character arrays (on Windows) to allow exact non-Unicode
/// path round-trips.
pub(crate) mod path {
    use std::path::{Path, PathBuf};

    use serde::{Deserialize, Deserializer, Serializer};
    #[cfg(unix)]
    pub(crate) fn serialize<S>(
        path: &Path,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use std::os::unix::ffi::OsStrExt as _;
        serializer.serialize_bytes(path.as_os_str().as_bytes())
    }

    #[cfg(unix)]
    pub(crate) fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<PathBuf, D::Error>
    where
        D: Deserializer<'de>,
    {
        use std::os::unix::ffi::OsStringExt as _;
        let bytes = <Vec<u8>>::deserialize(deserializer)?;
        Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
    }

    #[cfg(windows)]
    pub(crate) fn serialize<S>(
        path: &Path,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use std::os::windows::ffi::OsStrExt as _;
        let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        wide.serialize(serializer)
    }

    #[cfg(windows)]
    pub(crate) fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<PathBuf, D::Error>
    where
        D: Deserializer<'de>,
    {
        use std::os::windows::ffi::OsStringExt as _;
        let wide = <Vec<u16>>::deserialize(deserializer)?;
        Ok(PathBuf::from(std::ffi::OsString::from_wide(&wide)))
    }
}

#[cfg(test)]
mod tests {
    mod encode {
        use pretty_assertions::assert_eq;
        use serde::{Deserialize, Serialize};

        use super::super::encode_row;
        use crate::index::error::DbError;

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
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

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
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

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
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
