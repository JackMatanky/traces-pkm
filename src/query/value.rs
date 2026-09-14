//! Zero-copy resolved field values.

use std::{fmt::Write as _, path::PathBuf};

use crate::{
    DateTimeValue, DateValue, DurationValue, Tag,
    note::{Link, NoteFieldValue},
};
/// Borrowed field value resolved from a [`super::QueryRow`].
pub(super) enum QueryFieldValueRef<'a> {
    Null,
    Bool(bool),
    Number(f64),
    Text(&'a str),
    Link(&'a Link),
    Date(DateValue),
    DateTime(DateTimeValue),
    Duration(&'a DurationValue),
    Object(&'a indexmap::IndexMap<String, NoteFieldValue>),
    List(QueryListValueRef<'a>),
    Owned(NoteFieldValue),
}

impl QueryFieldValueRef<'_> {
    /// Materializes this resolved value as metadata.
    pub(super) fn to_owned_value(&self) -> NoteFieldValue {
        match self {
            Self::Null => NoteFieldValue::Null,
            Self::Bool(value) => NoteFieldValue::Bool(*value),
            Self::Number(value) => NoteFieldValue::Number(*value),
            Self::Text(value) => NoteFieldValue::String((*value).to_owned()),
            Self::Link(value) => NoteFieldValue::Link((*value).clone()),
            Self::Date(value) => NoteFieldValue::Date(*value),
            Self::DateTime(value) => NoteFieldValue::DateTime(*value),
            Self::Duration(value) => NoteFieldValue::Duration((*value).clone()),
            Self::Object(value) => NoteFieldValue::Object((*value).clone()),
            Self::List(value) => value.to_owned_value(),
            Self::Owned(value) => value.clone(),
        }
    }

    /// Appends the shared query-display text used by list joins, table cells,
    /// and text output.
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
            Self::Text(value) => out.push_str(value),
            Self::Duration(value) => out.push_str(value.as_str()),
            Self::Date(value) => {
                let _ = write!(out, "{value}");
            }
            Self::DateTime(value) => {
                let _ = write!(out, "{value}");
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
        self.text().replace('\n', " ").replace('|', "\\|")
    }

    pub(super) fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            Self::Duration(value) => Some(value.as_str()),
            Self::Owned(value) => value.as_str(),
            _ => None,
        }
    }

    /// Applies filter equality semantics (`==`, `!=`) to `literal`.
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
            Self::Date(value) => match literal {
                NoteFieldValue::Date(other) => value == other,
                NoteFieldValue::DateTime(other) => {
                    other.is_equal_to_date(*value)
                }
                _ => false,
            },
            Self::DateTime(value) => match literal {
                NoteFieldValue::DateTime(other) => value == other,
                NoteFieldValue::Date(other) => value.is_equal_to_date(*other),
                _ => false,
            },
            Self::Duration(dv) => match literal {
                NoteFieldValue::Duration(other) => *dv == other,
                _ => false,
            },
            Self::Object(value) => {
                matches!(literal, NoteFieldValue::Object(other) if *value == other)
            }
            Self::List(_) | Self::Owned(_) => self.to_owned_value() == *literal,
        }
    }

    /// Evaluates `contains(field_val, target)`.
    ///
    /// Lists match exact values or descendant tags; string-like fields use
    /// substring containment.
    pub(super) fn is_containing(&self, target: &NoteFieldValue) -> bool {
        match self {
            Self::List(items) => items.is_containing(target),
            Self::Owned(NoteFieldValue::List(items)) => {
                QueryListValueRef::Values(items).is_containing(target)
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
            NoteFieldValue::Date(value) => Self::Date(*value),
            NoteFieldValue::DateTime(value) => Self::DateTime(*value),
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

    /// Returns `true` if `target` matches an exact value or descendant tag
    /// in this list.
    pub(super) fn is_containing(&self, target: &NoteFieldValue) -> bool {
        let target_str = target.as_str();
        match self {
            Self::Values(items) => items
                .iter()
                .any(|item| is_tag_or_value_matching(item, target, target_str)),
            Self::Tags(tags) => {
                let Some(target_str) = target_str else {
                    return false;
                };
                tags.iter()
                    .any(|tag| is_tag_str_matching(tag.as_str(), target_str))
            }
            Self::Inlinks(paths) => {
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

    fn to_owned_value(&self) -> NoteFieldValue {
        match self {
            Self::Values(values) => NoteFieldValue::List((*values).into()),
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

/// Matches exact tags and descendants, so `#book/fiction` satisfies `#book`.
fn is_tag_str_matching(item: &str, target_str: &str) -> bool {
    item == target_str
        || (item.starts_with('#') && target_str.starts_with('#'))
            && Tag::parse(item).is_ok_and(|tag| tag.is_contained_in(target_str))
}

/// Matches exact values, or tag descendants when both values stringify to tags.
fn is_tag_or_value_matching(
    item: &NoteFieldValue,
    target: &NoteFieldValue,
    target_str: Option<&str>,
) -> bool {
    if item == target {
        return true;
    }
    let (Some(item_str), Some(target_str)) = (item.as_str(), target_str) else {
        return false;
    };
    item_str.starts_with('#')
        && target_str.starts_with('#')
        && Tag::parse(item_str).is_ok_and(|tag| tag.is_contained_in(target_str))
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

/// Uses the shared query-display text rules for owned metadata.
fn append_owned_field_text(out: &mut String, value: &NoteFieldValue) {
    match value {
        NoteFieldValue::Null => {}
        NoteFieldValue::Bool(value) => {
            out.push_str(if *value {
                "true"
            } else {
                "false"
            });
        }
        NoteFieldValue::Number(value) => {
            let _ = write!(out, "{value}");
        }
        NoteFieldValue::String(value) => {
            out.push_str(value);
        }
        NoteFieldValue::Duration(value) => {
            out.push_str(value.as_str());
        }
        NoteFieldValue::Date(value) => {
            let _ = write!(out, "{value}");
        }
        NoteFieldValue::DateTime(value) => {
            let _ = write!(out, "{value}");
        }
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
        fn returns_true_when_comparing_date_ref_to_matching_date_literal() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            assert!(
                QueryFieldValueRef::Date(date)
                    .is_equal_to_literal(&NoteFieldValue::Date(date))
            );
        }

        #[test]
        fn returns_true_when_comparing_date_ref_to_midnight_datetime_literal() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let midnight_datetime = DateTimeValue::from(date);
            assert!(QueryFieldValueRef::Date(date).is_equal_to_literal(
                &NoteFieldValue::DateTime(midnight_datetime)
            ));
        }

        #[test]
        fn returns_false_when_comparing_date_ref_to_non_midnight_datetime_literal()
         {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let non_midnight = DateTimeValue::parse_iso("2026-07-29T14:30:00")
                .expect("valid datetime");
            assert!(
                !QueryFieldValueRef::Date(date).is_equal_to_literal(
                    &NoteFieldValue::DateTime(non_midnight)
                )
            );
        }

        #[test]
        fn returns_true_when_comparing_datetime_ref_to_midnight_date_literal() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let midnight_datetime = DateTimeValue::from(date);
            assert!(
                QueryFieldValueRef::DateTime(midnight_datetime)
                    .is_equal_to_literal(&NoteFieldValue::Date(date))
            );
        }

        #[test]
        fn returns_false_when_comparing_date_ref_to_a_non_date_literal() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            assert!(
                !QueryFieldValueRef::Date(date)
                    .is_equal_to_literal(&NoteFieldValue::Number(1.0))
            );
        }

        #[test]
        fn returns_true_when_comparing_datetime_ref_to_matching_datetime_literal()
         {
            let value = DateTimeValue::parse_iso("2026-07-29T14:30:00")
                .expect("valid datetime");
            assert!(
                QueryFieldValueRef::DateTime(value)
                    .is_equal_to_literal(&NoteFieldValue::DateTime(value))
            );
        }

        #[test]
        fn returns_false_when_comparing_datetime_ref_to_mismatched_datetime_literal()
         {
            let value = DateTimeValue::parse_iso("2026-07-29T14:30:00")
                .expect("valid datetime");
            let other = DateTimeValue::parse_iso("2026-07-29T09:00:00")
                .expect("valid datetime");
            assert!(
                !QueryFieldValueRef::DateTime(value)
                    .is_equal_to_literal(&NoteFieldValue::DateTime(other))
            );
        }

        #[test]
        fn returns_false_when_comparing_datetime_ref_to_a_non_date_literal() {
            let value = DateTimeValue::parse_iso("2026-07-29T14:30:00")
                .expect("valid datetime");
            assert!(
                !QueryFieldValueRef::DateTime(value)
                    .is_equal_to_literal(&NoteFieldValue::Number(1.0))
            );
        }
        #[test]
        fn matches_query_field_value_ref_duration_with_equivalent_literal() {
            let dv = DurationValue::parse("1h 30m").expect("valid duration");
            let qval = QueryFieldValueRef::Duration(&dv);
            let literal = NoteFieldValue::Duration(
                DurationValue::parse("90m").expect("valid duration"),
            );
            assert!(qval.is_equal_to_literal(&literal));
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
            let list = QueryFieldValueRef::Owned(NoteFieldValue::List(
                vec![NoteFieldValue::String("a".into())].into(),
            ));
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

        #[test]
        fn formats_date_as_yyyy_mm_dd() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let mut out = String::new();
            QueryFieldValueRef::Date(date).append_text(&mut out);
            assert_eq!(out, "2026-07-29");
        }

        #[test]
        fn formats_datetime_without_offset() {
            let value = DateTimeValue::parse_iso("2026-07-29T14:30:00")
                .expect("valid datetime");
            let mut out = String::new();
            QueryFieldValueRef::DateTime(value).append_text(&mut out);
            assert_eq!(out, "2026-07-29T14:30:00");
        }
    }

    mod conversions {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn to_owned_value_preserves_a_typed_date() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let owned = QueryFieldValueRef::Date(date).to_owned_value();
            assert_eq!(owned, NoteFieldValue::Date(date));
        }

        #[test]
        fn to_owned_value_preserves_a_typed_datetime() {
            let value = DateTimeValue::parse_iso("2026-07-29T14:30:00")
                .expect("valid datetime");
            let owned = QueryFieldValueRef::DateTime(value).to_owned_value();
            assert_eq!(owned, NoteFieldValue::DateTime(value));
        }

        #[test]
        fn as_str_returns_none_for_date_and_datetime_variants() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let datetime = DateTimeValue::parse_iso("2026-07-29T14:30:00")
                .expect("valid datetime");
            assert_eq!(QueryFieldValueRef::Date(date).as_str(), None);
            assert_eq!(QueryFieldValueRef::DateTime(datetime).as_str(), None);
        }

        #[test]
        fn from_note_field_value_preserves_typed_date_and_datetime() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let datetime = DateTimeValue::parse_iso("2026-07-29T14:30:00")
                .expect("valid datetime");
            assert!(matches!(
                QueryFieldValueRef::from(&NoteFieldValue::Date(date)),
                QueryFieldValueRef::Date(d) if d == date
            ));
            assert!(matches!(
                QueryFieldValueRef::from(&NoteFieldValue::DateTime(datetime)),
                QueryFieldValueRef::DateTime(d) if d == datetime
            ));
        }
    }
}
