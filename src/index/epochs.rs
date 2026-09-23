//! Persisted configuration epoch markers for index repair detection.
//!
//! Tracks configuration snapshots that affect note parsing (task statuses,
//! task tag filters) and axis index projection (the File Class field). When a
//! sync pass opens an index whose persisted epochs differ from current
//! configuration, [`super::refresh::RepairScope`] computes the minimal repair
//! action required without requiring file mtime changes.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{
    codec::{decode_row, encode_row},
    error::StoreResult,
};
use crate::{Tag, TaskConfig, TaskStatus};

/// Row key for the parse epoch marker in the `EPOCHS` table.
pub(super) const PARSE_KEY: &[u8] = b"parse";

/// Row key for the class epoch marker in the `EPOCHS` table.
pub(super) const CLASS_KEY: &[u8] = b"class";

const PARSE_PSEUDO_PATH: &str = "epochs/parse";
const CLASS_PSEUDO_PATH: &str = "epochs/class";

/// Persisted epoch markers captured across parse and projection dimensions.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct PersistedEpochs {
    parse: ParseEpoch,
    class: ClassEpoch,
}

impl PersistedEpochs {
    /// Captures the current configuration epochs.
    #[inline]
    #[must_use]
    pub(super) fn current(tasks: &TaskConfig, class_field: &str) -> Self {
        Self {
            parse: ParseEpoch::from_config(tasks),
            class: ClassEpoch::from_class_field(class_field),
        }
    }

    /// Constructs an unknown epoch snapshot representing missing or corrupted
    /// markers.
    ///
    /// Because valid configurations always have non-empty default task
    /// statuses, an unknown epoch compares unequal to any real configuration.
    #[inline]
    #[must_use]
    pub(super) fn unknown() -> Self {
        Self {
            parse: ParseEpoch::empty(),
            class: ClassEpoch::empty(),
        }
    }

    /// Creates an epoch snapshot from its individual dimensions.
    #[inline]
    #[must_use]
    pub(super) const fn new(parse: ParseEpoch, class: ClassEpoch) -> Self {
        Self {
            parse,
            class,
        }
    }

    /// Returns the parse epoch dimension.
    #[inline]
    #[must_use]
    pub(super) const fn parse(&self) -> &ParseEpoch {
        &self.parse
    }

    /// Returns the class epoch dimension.
    #[inline]
    #[must_use]
    pub(super) const fn class(&self) -> &ClassEpoch {
        &self.class
    }

    /// Reports whether this persisted snapshot matches `current` across all
    /// dimensions.
    #[inline]
    #[must_use]
    pub(super) fn is_match(&self, current: &Self) -> bool {
        self == current
    }

    /// Reports whether the parse epoch matches `current`.
    #[inline]
    #[must_use]
    pub(super) fn is_parse_match(&self, current: &Self) -> bool {
        self.parse == current.parse
    }

    /// Reports whether the class epoch matches `current`.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(super) fn is_class_match(&self, current: &Self) -> bool {
        self.class == current.class
    }
}

/// Snapshot of configuration settings that affect Markdown note parsing.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct ParseEpoch {
    statuses: Box<[TaskStatus]>,
    tag_filters: Box<[Tag]>,
}

impl ParseEpoch {
    /// Captures the canonical parse epoch from `tasks`.
    #[must_use]
    pub(super) fn from_config(tasks: &TaskConfig) -> Self {
        let statuses =
            tasks.statuses().statuses_sorted_by_symbol().into_boxed_slice();
        let mut filters: Vec<Tag> = tasks.tag_filters().to_vec();
        filters.sort_unstable();
        filters.dedup();
        Self {
            statuses,
            tag_filters: filters.into_boxed_slice(),
        }
    }

    /// Constructs an empty parse epoch for [`PersistedEpochs::unknown`].
    #[inline]
    #[must_use]
    pub(super) fn empty() -> Self {
        Self {
            statuses: Box::default(),
            tag_filters: Box::default(),
        }
    }

    /// Serializes this parse epoch using postcard.
    ///
    /// # Errors
    ///
    /// - [`StoreError::Serialize`] if postcard encoding fails.
    #[inline]
    pub(super) fn encode<'a>(
        &self,
        buf: &'a mut Vec<u8>,
    ) -> StoreResult<&'a [u8]> {
        encode_row(Path::new(PARSE_PSEUDO_PATH), self, buf)
    }

    /// Deserializes a parse epoch from stored bytes.
    ///
    /// # Errors
    ///
    /// - [`StoreError::Deserialize`] if stored bytes cannot be decoded.
    #[inline]
    pub(super) fn decode(bytes: &[u8]) -> StoreResult<Self> {
        decode_row(Path::new(PARSE_PSEUDO_PATH), bytes)
    }
}

/// Snapshot of the File Class frontmatter field name.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct ClassEpoch(Box<str>);

impl ClassEpoch {
    /// Captures the class epoch from `class_field`.
    #[inline]
    #[must_use]
    pub(super) fn from_class_field(class_field: &str) -> Self {
        Self(class_field.into())
    }

    /// Constructs an empty class epoch for [`PersistedEpochs::unknown`].
    #[inline]
    #[must_use]
    pub(super) fn empty() -> Self {
        Self(String::new().into_boxed_str())
    }

    /// Returns the class field name.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }

    /// Serializes this class epoch using postcard.
    ///
    /// # Errors
    ///
    /// - [`StoreError::Serialize`] if postcard encoding fails.
    #[inline]
    pub(super) fn encode<'a>(
        &self,
        buf: &'a mut Vec<u8>,
    ) -> StoreResult<&'a [u8]> {
        encode_row(Path::new(CLASS_PSEUDO_PATH), self, buf)
    }

    /// Deserializes a class epoch from stored bytes.
    ///
    /// # Errors
    ///
    /// - [`StoreError::Deserialize`] if stored bytes cannot be decoded.
    #[inline]
    pub(super) fn decode(bytes: &[u8]) -> StoreResult<Self> {
        decode_row(Path::new(CLASS_PSEUDO_PATH), bytes)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_ne};

    use super::{super::error::StoreError, *};

    #[test]
    fn round_trips_parse_epoch() {
        let tasks = TaskConfig::default();
        let original = ParseEpoch::from_config(&tasks);
        let mut buf = Vec::new();
        let bytes = original.encode(&mut buf).expect("encode parse epoch");
        let decoded = ParseEpoch::decode(bytes).expect("decode parse epoch");
        assert_eq!(original, decoded);
    }

    #[test]
    fn round_trips_class_epoch() {
        let original = ClassEpoch::from_class_field("custom_kind");
        let mut buf = Vec::new();
        let bytes = original.encode(&mut buf).expect("encode class epoch");
        let decoded = ClassEpoch::decode(bytes).expect("decode class epoch");
        assert_eq!(original, decoded);
        assert_eq!(decoded.as_str(), "custom_kind");
    }

    #[test]
    fn garbage_bytes_fail_decode() {
        let parse_err = ParseEpoch::decode(&[0xFF, 0xFF, 0xFF])
            .expect_err("garbage parse bytes fail decode");
        assert!(matches!(parse_err, StoreError::Deserialize { .. }));

        let class_err = ClassEpoch::decode(&[0xFF, 0xFF, 0xFF])
            .expect_err("garbage class bytes fail decode");
        assert!(matches!(class_err, StoreError::Deserialize { .. }));
    }

    #[test]
    fn unknown_epoch_never_matches_real_config() {
        let tasks = TaskConfig::default();
        let current = PersistedEpochs::current(&tasks, "class");
        let unknown = PersistedEpochs::unknown();
        assert_ne!(unknown, current);
        assert!(!unknown.is_match(&current));
        assert!(!unknown.is_parse_match(&current));
        assert!(!unknown.is_class_match(&current));
    }

    #[test]
    fn statuses_sorted_by_symbol_is_order_stable() {
        let tasks = TaskConfig::default();
        let epoch1 = ParseEpoch::from_config(&tasks);
        let epoch2 = ParseEpoch::from_config(&tasks);
        assert_eq!(epoch1, epoch2);
        assert_eq!(epoch1.statuses, epoch2.statuses);
    }
}
