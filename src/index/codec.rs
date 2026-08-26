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
    use serde::{Deserialize, Deserializer, Serializer};

    use super::*;

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
