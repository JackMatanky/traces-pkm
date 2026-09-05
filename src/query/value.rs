//! Zero-copy resolved field value types for query resolution.

use std::{fmt::Write as _, path::PathBuf};

use crate::{
    Tag,
    file::Timestamp,
    note::{Link, NoteFieldValue},
};
/// Borrowed field value resolved from a [`super::QueryRow`].
pub(super) enum QueryFieldValueRef<'a> {
    Null,
    Bool(bool),
    Number(f64),
    Text(&'a str),
    Link(&'a Link),
    Date(&'a str),
    Duration(&'a str),
    Timestamp(Timestamp),
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
            Self::Timestamp(ts) => {
                NoteFieldValue::Date(ts.to_conditional_string())
            }
            Self::Object(value) => NoteFieldValue::Object((*value).clone()),
            Self::List(value) => value.to_owned_value(),
            Self::Owned(value) => value.clone(),
        }
    }

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
            Self::Timestamp(ts) => ts.append_conditional(out),
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
        self.text().replace('\n', " ").replace('|', "\\|")
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

    /// Returns whether this resolved field equals an owned literal under filter
    /// comparison rules (`==`, `!=`).
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
            Self::Timestamp(ts) => {
                if let NoteFieldValue::Date(lit_s) = literal
                    && let Some(lit_ts) = Timestamp::parse_iso(lit_s)
                {
                    return ts == &lit_ts;
                }
                false
            }
            Self::Date(value) => {
                if let NoteFieldValue::Date(other) = literal {
                    match (
                        Timestamp::parse_iso(value),
                        Timestamp::parse_iso(other),
                    ) {
                        (Some(tx), Some(ty)) => tx == ty,
                        _ => *value == other,
                    }
                } else {
                    false
                }
            }
            Self::Duration(value) => {
                if let NoteFieldValue::Duration(other) = literal {
                    match (
                        crate::note::duration_seconds(value),
                        crate::note::duration_seconds(other),
                    ) {
                        (Some(sx), Some(sy)) => sx == sy,
                        _ => *value == other,
                    }
                } else {
                    false
                }
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

/// Borrowed list value resolved from a [`super::QueryRow`].
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns whether two resolved [`NoteFieldValue`] instances represent equal
/// values under filter comparison (`==` and `!=`).
///
/// Returns `true` when structural equality (`a == b`) holds, or when both
/// values stringify (via [`NoteFieldValue::as_str`], which covers `String`,
/// `Date`, and `Duration`) to the same text. This cross-kind text
/// normalization allows string literals to match date or duration fields.
fn fields_equal(a: &NoteFieldValue, b: &NoteFieldValue) -> bool {
    a == b || matches!((a.as_str(), b.as_str()), (Some(x), Some(y)) if x == y)
}

fn is_tag_str_matching(item: &str, target_str: &str) -> bool {
    item == target_str
        || (item.starts_with('#') && target_str.starts_with('#'))
            && Tag::parse(item).is_ok_and(|tag| tag.is_contained_in(target_str))
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
        && Tag::parse(item_str).is_ok_and(|tag| tag.is_contained_in(target_str))
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

#[cfg(test)]
mod tests {
    use super::*;

    mod comparison {
        use super::*;
        #[test]
        fn returns_true_when_comparing_null_to_null_literal() {
            assert!(
                QueryFieldValueRef::Null
                    .is_equal_to_literal(&NoteFieldValue::Null)
            );
        }

        #[test]
        fn returns_false_when_comparing_null_to_number_literal() {
            assert!(
                !QueryFieldValueRef::Null
                    .is_equal_to_literal(&NoteFieldValue::Number(1.0))
            );
        }

        #[test]
        fn returns_true_when_comparing_text_ref_to_matching_string_literal() {
            assert!(
                QueryFieldValueRef::Text("hello").is_equal_to_literal(
                    &NoteFieldValue::String("hello".into())
                )
            );
        }

        #[test]
        fn returns_false_when_comparing_text_ref_to_mismatched_string_literal()
        {
            assert!(
                !QueryFieldValueRef::Text("hello").is_equal_to_literal(
                    &NoteFieldValue::String("world".into())
                )
            );
        }

        #[test]
        fn returns_true_when_comparing_number_ref_to_matching_number_literal() {
            assert!(
                QueryFieldValueRef::Number(5.0)
                    .is_equal_to_literal(&NoteFieldValue::Number(5.0))
            );
        }

        #[test]
        fn returns_true_when_text_ref_contains_substring() {
            assert!(
                QueryFieldValueRef::Text("hello world")
                    .is_containing(&NoteFieldValue::String("world".into()))
            );
        }

        #[test]
        fn returns_false_when_text_ref_does_not_contain_substring() {
            assert!(
                !QueryFieldValueRef::Text("hello")
                    .is_containing(&NoteFieldValue::String("xyz".into()))
            );
        }

        #[test]
        fn returns_true_when_list_ref_contains_tag_prefix() {
            let items = [NoteFieldValue::String("#book/fiction".into())];
            let list =
                QueryFieldValueRef::List(QueryListValueRef::Values(&items));
            assert!(
                list.is_containing(&NoteFieldValue::String("#book".into()))
            );
        }

        #[test]
        fn returns_true_when_owned_list_ref_contains_matching_element() {
            let list = QueryFieldValueRef::Owned(NoteFieldValue::List(vec![
                NoteFieldValue::String("a".into()),
            ]));
            assert!(list.is_containing(&NoteFieldValue::String("a".into())));
        }
    }

    mod formatting {
        use pretty_assertions::assert_eq;

        use super::*;
        #[test]
        fn formats_empty_string_for_null_value() {
            let mut out = String::new();
            QueryFieldValueRef::Null.append_text(&mut out);
            assert_eq!(out, "");
        }

        #[test]
        fn formats_boolean_as_lowercase_literal() {
            let mut out = String::new();
            QueryFieldValueRef::Bool(true).append_text(&mut out);
            assert_eq!(out, "true");
        }

        #[test]
        fn formats_number_without_unnecessary_decimals() {
            let mut out = String::new();
            QueryFieldValueRef::Number(42.0).append_text(&mut out);
            assert_eq!(out, "42");
        }

        #[test]
        fn formats_text_verbatim() {
            let mut out = String::new();
            QueryFieldValueRef::Text("hello").append_text(&mut out);
            assert_eq!(out, "hello");
        }
    }

    mod equality {

        use super::*;
        #[test]
        fn returns_true_for_identical_note_field_values() {
            assert!(fields_equal(
                &NoteFieldValue::Number(1.0),
                &NoteFieldValue::Number(1.0)
            ));
        }

        #[test]
        fn returns_false_for_different_note_field_values() {
            assert!(!fields_equal(
                &NoteFieldValue::Number(1.0),
                &NoteFieldValue::Number(2.0)
            ));
        }

        #[test]
        fn returns_true_for_cross_kind_string_and_date_equality() {
            assert!(fields_equal(
                &NoteFieldValue::String("2024-01-01".into()),
                &NoteFieldValue::Date("2024-01-01".into())
            ));
        }
    }
}
