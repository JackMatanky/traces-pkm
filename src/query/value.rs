//! Borrowed and owned field value types for query resolution.
//!
//! This module defines [`QueryFieldValueRef`], the zero-copy resolved field
//! value returned by [`super::QueryRecord::resolve_ref`], and
//! [`QueryListValueRef`], the borrowed list variant.
//!
//! # Main Types
//!
//! - [`QueryFieldValueRef`] borrows string and collection data from the
//!   underlying [`super::QueryRecord`] where possible, falling back to
//!   [`QueryFieldValueRef::Owned`] for values that require allocation.
//! - [`QueryListValueRef`] is the list-specific borrowed variant, handling
//!   tags, inlinks, and generic value slices.
//!
//! [`NoteFieldValue`]: crate::note::NoteFieldValue

use std::{cmp::Ordering, fmt::Write as _, path::PathBuf};

use super::sort::compare_field_values;
use crate::note::{Link, NoteFieldValue, Tag, is_nested_under};

/// Borrowed field value resolved from a [`super::QueryRecord`].
pub(super) enum QueryFieldValueRef<'a> {
    Null,
    Bool(bool),
    Number(f64),
    Text(&'a str),
    Link(&'a Link),
    Date(&'a str),
    Duration(&'a str),
    Object(&'a indexmap::IndexMap<String, NoteFieldValue>),
    List(QueryListValueRef<'a>),
    Owned(NoteFieldValue),
}

impl QueryFieldValueRef<'_> {
    pub(super) fn to_owned_value(&self) -> NoteFieldValue {
        match self {
            Self::Null => NoteFieldValue::Null,
            Self::Bool(value) => NoteFieldValue::Bool(*value),
            Self::Number(value) => NoteFieldValue::Number(*value),
            Self::Text(value) => NoteFieldValue::String((*value).to_owned()),
            Self::Link(value) => NoteFieldValue::Link((*value).clone()),
            Self::Date(value) => NoteFieldValue::Date((*value).to_owned()),
            Self::Duration(value) => {
                NoteFieldValue::Duration((*value).to_owned())
            }
            Self::Object(value) => NoteFieldValue::Object((*value).clone()),
            Self::List(value) => value.to_owned_value(),
            Self::Owned(value) => value.clone(),
        }
    }

    pub(super) fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(value) | Self::Date(value) | Self::Duration(value) => {
                Some(value)
            }
            Self::Owned(value) => value.as_str(),
            _ => None,
        }
    }
}

impl<'a> From<&'a NoteFieldValue> for QueryFieldValueRef<'a> {
    fn from(value: &'a NoteFieldValue) -> Self {
        match value {
            NoteFieldValue::Null => Self::Null,
            NoteFieldValue::Bool(value) => Self::Bool(*value),
            NoteFieldValue::Number(value) => Self::Number(*value),
            NoteFieldValue::String(value) => Self::Text(value),
            NoteFieldValue::Date(value) => Self::Date(value),
            NoteFieldValue::Duration(value) => Self::Duration(value),
            NoteFieldValue::Link(value) => Self::Link(value),
            NoteFieldValue::List(value) => {
                Self::List(QueryListValueRef::Values(value))
            }
            NoteFieldValue::Object(value) => Self::Object(value),
        }
    }
}

/// Returns whether two resolved [`NoteFieldValue`] instances represent equal
/// values under filter comparison (`==` and `!=`).
///
/// Returns `true` when structural equality (`a == b`) holds, or when
/// [`compare_field_values`] returns `Some(Ordering::Equal)`. This cross-kind
/// text normalization allows string literals to match date or duration fields.
pub(super) fn fields_equal(a: &NoteFieldValue, b: &NoteFieldValue) -> bool {
    a == b || compare_field_values(a, b) == Some(Ordering::Equal)
}

fn is_tag_str_matching(item: &str, target_str: &str) -> bool {
    item == target_str
        || item.starts_with('#')
            && target_str.starts_with('#')
            && is_nested_under(item, target_str)
}

fn is_tag_or_value_matching(
    item: &NoteFieldValue,
    target: &NoteFieldValue,
    target_str: Option<&str>,
) -> bool {
    if fields_equal(item, target) {
        return true;
    }
    let (Some(item_str), Some(target_str)) = (item.as_str(), target_str) else {
        return false;
    };
    item_str.starts_with('#')
        && target_str.starts_with('#')
        && is_nested_under(item_str, target_str)
}

pub(super) fn is_list_containing(
    items: &QueryListValueRef<'_>,
    target: &NoteFieldValue,
) -> bool {
    let target_str = target.as_str();
    match items {
        QueryListValueRef::Values(items) => items
            .iter()
            .any(|item| is_tag_or_value_matching(item, target, target_str)),
        QueryListValueRef::Tags(tags) => {
            let Some(target_str) = target_str else {
                return false;
            };
            tags.iter().any(|tag| is_tag_str_matching(tag.as_str(), target_str))
        }
        QueryListValueRef::Inlinks(paths) => {
            let Some(target_str) = target_str else {
                return false;
            };
            paths.iter().any(|path| {
                let path = path.to_string_lossy();
                is_tag_str_matching(&path, target_str)
            })
        }
    }
}

pub(super) fn escape_table_text(text: &str) -> String {
    text.replace('\n', " ").replace('|', "\\|")
}

fn append_joined<T>(
    out: &mut String,
    values: &[T],
    mut append: impl FnMut(&mut String, &T),
) {
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        append(out, value);
    }
}

fn append_owned_field_text(out: &mut String, value: &NoteFieldValue) {
    match value {
        NoteFieldValue::Null => {}
        NoteFieldValue::Bool(value) => out.push_str(&value.to_string()),
        NoteFieldValue::Number(value) => out.push_str(&value.to_string()),
        NoteFieldValue::String(value)
        | NoteFieldValue::Date(value)
        | NoteFieldValue::Duration(value) => out.push_str(value),
        NoteFieldValue::Link(link) => out.push_str(link.target()),
        NoteFieldValue::List(items) => {
            append_joined(out, items, append_owned_field_text);
        }
        NoteFieldValue::Object(fields) => {
            for (idx, (key, field)) in fields.iter().enumerate() {
                if idx > 0 {
                    out.push_str(", ");
                }
                out.push_str(key);
                out.push_str(": ");
                append_owned_field_text(out, field);
            }
        }
    }
}

impl QueryFieldValueRef<'_> {
    pub(super) fn append_text(&self, out: &mut String) {
        match self {
            Self::Null => {}
            Self::Bool(value) => out.push_str(if *value {
                "true"
            } else {
                "false"
            }),
            Self::Number(value) => {
                let _ = write!(out, "{value}");
            }
            Self::Text(value) | Self::Date(value) | Self::Duration(value) => {
                out.push_str(value);
            }
            Self::Link(link) => out.push_str(link.target()),
            Self::Object(fields) => {
                for (idx, (key, field)) in fields.iter().enumerate() {
                    if idx > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(key);
                    out.push_str(": ");
                    Self::from(field).append_text(out);
                }
            }
            Self::List(list) => list.append_text(out),
            Self::Owned(value) => append_owned_field_text(out, value),
        }
    }

    pub(super) fn text(&self) -> String {
        let mut out = String::new();
        self.append_text(&mut out);
        out
    }

    pub(super) fn table_cell_text(&self) -> String {
        escape_table_text(&self.text())
    }
}

impl QueryFieldValueRef<'_> {
    /// Compares this resolved field against an owned literal to establish
    /// ordering for `<`, `<=`, `>`, `>=` filter comparisons.
    pub(super) fn compare_to_literal(
        &self,
        literal: &NoteFieldValue,
    ) -> Option<Ordering> {
        match (self, literal) {
            (Self::Number(x), NoteFieldValue::Number(y)) => x.partial_cmp(y),
            (Self::Bool(x), NoteFieldValue::Bool(y)) => Some(x.cmp(y)),
            (Self::Date(x), NoteFieldValue::Date(y))
            | (Self::Duration(x), NoteFieldValue::Duration(y)) => {
                Some(x.cmp(&y.as_str()))
            }
            (Self::Object(_), NoteFieldValue::Object(_)) => None,
            (Self::Owned(value), literal) => {
                compare_field_values(value, literal)
            }
            _ => match (self.as_str(), literal.as_str()) {
                (Some(x), Some(y)) => Some(x.cmp(y)),
                _ => None,
            },
        }
    }

    /// Returns whether this resolved field equals an owned literal under
    /// filter comparison rules (`==`, `!=`).
    #[expect(
        clippy::float_cmp,
        reason = "query numeric equality intentionally uses exact parsed \
                  metadata equality; ordering still uses total_cmp"
    )]
    pub(super) fn is_equal_to_literal(&self, literal: &NoteFieldValue) -> bool {
        match self {
            Self::Null => matches!(literal, NoteFieldValue::Null),
            Self::Bool(value) => {
                matches!(literal, NoteFieldValue::Bool(other) if value == other)
            }
            Self::Number(value) => {
                matches!(literal, NoteFieldValue::Number(other) if value == other)
            }
            Self::Text(value) => literal.as_str() == Some(value),
            Self::Link(value) => {
                matches!(literal, NoteFieldValue::Link(other) if *value == other)
            }
            Self::Date(value) | Self::Duration(value) => {
                literal.as_str() == Some(value)
            }
            Self::Object(value) => {
                matches!(literal, NoteFieldValue::Object(other) if *value == other)
            }
            Self::List(_) | Self::Owned(_) => {
                fields_equal(&self.to_owned_value(), literal)
            }
        }
    }

    /// Evaluates a `contains(field_val, target)` call.
    ///
    /// For list fields, matches by exact value or tag prefix (for example,
    /// `#book` matching `#book/fiction`). For other field kinds, falls back
    /// to substring containment on stringified values.
    pub(super) fn is_containing(&self, target: &NoteFieldValue) -> bool {
        match self {
            Self::List(items) => is_list_containing(items, target),
            Self::Owned(NoteFieldValue::List(items)) => {
                is_list_containing(&QueryListValueRef::Values(items), target)
            }
            _ => match (self.as_str(), target.as_str()) {
                (Some(haystack), Some(needle)) => haystack.contains(needle),
                _ => false,
            },
        }
    }
}

/// Borrowed list value resolved from a [`super::QueryRecord`].
pub(super) enum QueryListValueRef<'a> {
    Values(&'a [NoteFieldValue]),
    Tags(&'a [Tag]),
    Inlinks(&'a [PathBuf]),
}

impl QueryListValueRef<'_> {
    pub(super) fn append_text(&self, out: &mut String) {
        match self {
            Self::Values(values) => {
                append_joined(out, values, append_owned_field_text);
            }
            Self::Tags(tags) => {
                append_joined(out, tags, |out, tag| out.push_str(tag.as_str()));
            }
            Self::Inlinks(inlinks) => {
                append_joined(out, inlinks, |out, path| {
                    out.push_str(&path.to_string_lossy());
                });
            }
        }
    }

    fn to_owned_value(&self) -> NoteFieldValue {
        match self {
            Self::Values(values) => NoteFieldValue::List((*values).to_vec()),
            Self::Tags(tags) => NoteFieldValue::List(
                tags.iter()
                    .map(|tag| NoteFieldValue::String(tag.as_str().to_owned()))
                    .collect(),
            ),
            Self::Inlinks(inlinks) => NoteFieldValue::List(
                inlinks
                    .iter()
                    .map(|path| {
                        NoteFieldValue::String(
                            path.to_string_lossy().into_owned(),
                        )
                    })
                    .collect(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use pretty_assertions::assert_eq;

    use super::*;

    // -- compare_to_literal --

    #[test]
    fn compare_number_greater() {
        assert_eq!(
            QueryFieldValueRef::Number(5.0)
                .compare_to_literal(&NoteFieldValue::Number(3.0)),
            Some(Ordering::Greater)
        );
    }

    #[test]
    fn compare_string_less() {
        assert_eq!(
            QueryFieldValueRef::Text("abc")
                .compare_to_literal(&NoteFieldValue::String("abd".into())),
            Some(Ordering::Less)
        );
    }

    #[test]
    fn compare_bool_vs_number_is_none() {
        assert_eq!(
            QueryFieldValueRef::Bool(true)
                .compare_to_literal(&NoteFieldValue::Number(1.0)),
            None
        );
    }

    #[test]
    fn compare_owned_fallback() {
        assert_eq!(
            QueryFieldValueRef::Owned(NoteFieldValue::Number(5.0))
                .compare_to_literal(&NoteFieldValue::Number(5.0)),
            Some(Ordering::Equal)
        );
    }

    // -- is_equal_to_literal --

    #[test]
    fn equal_null_null() {
        assert!(
            QueryFieldValueRef::Null.is_equal_to_literal(&NoteFieldValue::Null)
        );
    }

    #[test]
    fn equal_null_vs_number() {
        assert!(
            !QueryFieldValueRef::Null
                .is_equal_to_literal(&NoteFieldValue::Number(1.0))
        );
    }

    #[test]
    fn equal_text_string_match() {
        assert!(
            QueryFieldValueRef::Text("hello")
                .is_equal_to_literal(&NoteFieldValue::String("hello".into()))
        );
    }

    #[test]
    fn equal_text_string_mismatch() {
        assert!(
            !QueryFieldValueRef::Text("hello")
                .is_equal_to_literal(&NoteFieldValue::String("world".into()))
        );
    }

    #[test]
    fn equal_number_exact() {
        assert!(
            QueryFieldValueRef::Number(5.0)
                .is_equal_to_literal(&NoteFieldValue::Number(5.0))
        );
    }

    // -- is_containing --

    #[test]
    fn containing_substring() {
        assert!(
            QueryFieldValueRef::Text("hello world")
                .is_containing(&NoteFieldValue::String("world".into()))
        );
    }

    #[test]
    fn containing_no_match() {
        assert!(
            !QueryFieldValueRef::Text("hello")
                .is_containing(&NoteFieldValue::String("xyz".into()))
        );
    }

    #[test]
    fn containing_list_tag_prefix() {
        let items = [NoteFieldValue::String("#book/fiction".into())];
        let list = QueryFieldValueRef::List(QueryListValueRef::Values(&items));
        assert!(list.is_containing(&NoteFieldValue::String("#book".into())));
    }

    #[test]
    fn containing_owned_list() {
        let list = QueryFieldValueRef::Owned(NoteFieldValue::List(vec![
            NoteFieldValue::String("a".into()),
        ]));
        assert!(list.is_containing(&NoteFieldValue::String("a".into())));
    }

    // -- append_text --

    #[test]
    fn append_text_null() {
        let mut out = String::new();
        QueryFieldValueRef::Null.append_text(&mut out);
        assert_eq!(out, "");
    }

    #[test]
    fn append_text_bool() {
        let mut out = String::new();
        QueryFieldValueRef::Bool(true).append_text(&mut out);
        assert_eq!(out, "true");
    }

    #[test]
    fn append_text_number() {
        let mut out = String::new();
        QueryFieldValueRef::Number(42.0).append_text(&mut out);
        assert_eq!(out, "42");
    }

    #[test]
    fn append_text_text() {
        let mut out = String::new();
        QueryFieldValueRef::Text("hello").append_text(&mut out);
        assert_eq!(out, "hello");
    }

    // -- fields_equal --

    #[test]
    fn fields_equal_same_values() {
        assert!(fields_equal(
            &NoteFieldValue::Number(1.0),
            &NoteFieldValue::Number(1.0)
        ));
    }

    #[test]
    fn fields_equal_different_values() {
        assert!(!fields_equal(
            &NoteFieldValue::Number(1.0),
            &NoteFieldValue::Number(2.0)
        ));
    }

    #[test]
    fn fields_equal_string_date_cross_kind() {
        assert!(fields_equal(
            &NoteFieldValue::String("2024-01-01".into()),
            &NoteFieldValue::Date("2024-01-01".into())
        ));
    }
}
