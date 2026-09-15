//! Zero-copy and fallback resolved field values for query evaluation.
//!
//! [`QueryFieldValueRef`] borrows directly from note metadata, tags, and
//! inlinks without allocation in the common case, with an owned fallback for
//! non-UTF8 file paths.

use std::path::PathBuf;

use crate::{
    Tag,
    note::{NoteFieldValue, NoteFieldValueRef},
};

/// Borrowed field value resolved from a [`super::QueryRow`].
pub(super) enum QueryFieldValueRef<'a> {
    /// Borrowed metadata field from note-domain storage.
    Note(NoteFieldValueRef<'a>),
    /// Freshly allocated fallback for non-UTF8 paths, which have no
    /// long-lived backing store to borrow from.
    Owned(NoteFieldValue),
    /// Borrowed note tags.
    Tags(&'a [Tag]),
    /// Borrowed incoming links.
    Inlinks(&'a [PathBuf]),
}

impl QueryFieldValueRef<'_> {
    /// Converts this resolved reference into an owned [`NoteFieldValue`].
    pub(super) fn to_owned_value(&self) -> NoteFieldValue {
        match self {
            Self::Note(note_ref) => note_ref.to_owned_value(),
            Self::Owned(value) => value.clone(),
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

    /// Appends the shared query-display text used by list joins, table
    /// cells, and text output.
    pub(super) fn append_text(&self, out: &mut String) {
        match self {
            Self::Note(note_ref) => note_ref.append_text(out),
            Self::Owned(value) => value.as_ref().append_text(out),
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

    /// Materializes the full display text into a newly allocated [`String`].
    pub(super) fn text(&self) -> String {
        let mut out = String::new();
        self.append_text(&mut out);
        out
    }

    /// Returns the borrowed string for string/duration-typed fields, or
    /// `None` for non-textual variants.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current production caller; exercised in unit tests"
        )
    )]
    pub(super) fn as_str(&self) -> Option<&str> {
        match self {
            Self::Note(note_ref) => note_ref.as_str(),
            Self::Owned(value) => value.as_str(),
            Self::Tags(_) | Self::Inlinks(_) => None,
        }
    }

    /// Borrows this value as a [`NoteFieldValueRef`], when it resolves to
    /// note-domain metadata rather than a query-only `Tags`/`Inlinks` list.
    pub(super) fn as_note_ref(&self) -> Option<NoteFieldValueRef<'_>> {
        match self {
            Self::Note(note_ref) => Some(*note_ref),
            Self::Owned(value) => Some(value.as_ref()),
            Self::Tags(_) | Self::Inlinks(_) => None,
        }
    }

    /// Applies filter equality semantics (`==`, `!=`) to `literal`.
    pub(super) fn is_equal_to_literal(&self, literal: &NoteFieldValue) -> bool {
        match self {
            Self::Note(note_ref) => note_ref.is_equal_to_literal(literal),
            Self::Owned(value) => value.as_ref().is_equal_to_literal(literal),
            Self::Tags(tags) => match literal {
                NoteFieldValue::List(items) => {
                    tags.len() == items.len()
                        && tags.iter().zip(items.iter()).all(|(tag, item)| {
                            item.as_str() == Some(tag.as_str())
                        })
                }
                _ => false,
            },
            Self::Inlinks(inlinks) => match literal {
                NoteFieldValue::List(items) => {
                    inlinks.len() == items.len()
                        && inlinks.iter().zip(items.iter()).all(
                            |(path, item)| {
                                item.as_str()
                                    == Some(path.to_string_lossy().as_ref())
                            },
                        )
                }
                _ => false,
            },
        }
    }

    /// Evaluates `contains(field_val, target)`.
    ///
    /// Lists match by exact value or descendant tags; string-like fields use
    /// substring containment.
    pub(super) fn is_containing(&self, target: &NoteFieldValue) -> bool {
        match self {
            Self::Note(note_ref) => note_ref.is_containing(target),
            Self::Owned(value) => value.as_ref().is_containing(target),
            Self::Tags(tags) => {
                let Some(target_str) = target.as_str() else {
                    return false;
                };
                tags.iter().any(|tag| tag.is_contained_in(target_str))
            }
            Self::Inlinks(paths) => {
                let Some(target_str) = target.as_str() else {
                    return false;
                };
                paths.iter().any(|path| path.to_string_lossy() == target_str)
            }
        }
    }
}

impl<'a> From<&'a NoteFieldValue> for QueryFieldValueRef<'a> {
    fn from(value: &'a NoteFieldValue) -> Self {
        Self::Note(value.as_ref())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DateTimeValue, DateValue, DurationValue, note::NoteFieldValueRef,
    };

    mod comparison {
        use super::*;
        #[test]
        fn returns_true_when_comparing_null_to_null_literal() {
            assert!(
                QueryFieldValueRef::Note(NoteFieldValueRef::Null)
                    .is_equal_to_literal(&NoteFieldValue::Null)
            );
        }

        #[test]
        fn returns_false_when_comparing_null_to_number_literal() {
            assert!(
                !QueryFieldValueRef::Note(NoteFieldValueRef::Null)
                    .is_equal_to_literal(&NoteFieldValue::Number(1.0))
            );
        }

        #[test]
        fn returns_true_when_comparing_text_ref_to_matching_string_literal() {
            assert!(
                QueryFieldValueRef::Note(NoteFieldValueRef::String("hello"))
                    .is_equal_to_literal(&NoteFieldValue::String(
                        "hello".into()
                    ))
            );
        }

        #[test]
        fn returns_false_when_comparing_text_ref_to_mismatched_string_literal()
        {
            assert!(
                !QueryFieldValueRef::Note(NoteFieldValueRef::String("hello"))
                    .is_equal_to_literal(&NoteFieldValue::String(
                        "world".into()
                    ))
            );
        }

        #[test]
        fn returns_true_when_comparing_number_ref_to_matching_number_literal() {
            assert!(
                QueryFieldValueRef::Note(NoteFieldValueRef::Number(5.0))
                    .is_equal_to_literal(&NoteFieldValue::Number(5.0))
            );
        }

        #[test]
        fn returns_true_when_comparing_date_ref_to_matching_date_literal() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            assert!(
                QueryFieldValueRef::Note(NoteFieldValueRef::Date(date))
                    .is_equal_to_literal(&NoteFieldValue::Date(date))
            );
        }

        #[test]
        fn returns_true_when_comparing_date_ref_to_midnight_datetime_literal() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let midnight_datetime = DateTimeValue::from(date);
            assert!(
                QueryFieldValueRef::Note(NoteFieldValueRef::Date(date))
                    .is_equal_to_literal(&NoteFieldValue::DateTime(
                        midnight_datetime
                    ))
            );
        }

        #[test]
        fn returns_false_when_comparing_date_ref_to_non_midnight_datetime_literal()
         {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let non_midnight = DateTimeValue::parse_iso("2026-07-29T14:30:00")
                .expect("valid datetime");
            assert!(
                !QueryFieldValueRef::Note(NoteFieldValueRef::Date(date))
                    .is_equal_to_literal(&NoteFieldValue::DateTime(
                        non_midnight
                    ))
            );
        }

        #[test]
        fn returns_true_when_comparing_datetime_ref_to_midnight_date_literal() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let midnight_datetime = DateTimeValue::from(date);
            assert!(
                QueryFieldValueRef::Note(NoteFieldValueRef::DateTime(
                    midnight_datetime
                ))
                .is_equal_to_literal(&NoteFieldValue::Date(date))
            );
        }

        #[test]
        fn returns_false_when_comparing_date_ref_to_a_non_date_literal() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            assert!(
                !QueryFieldValueRef::Note(NoteFieldValueRef::Date(date))
                    .is_equal_to_literal(&NoteFieldValue::Number(1.0))
            );
        }

        #[test]
        fn returns_true_when_comparing_datetime_ref_to_matching_datetime_literal()
         {
            let value = DateTimeValue::parse_iso("2026-07-29T14:30:00")
                .expect("valid datetime");
            assert!(
                QueryFieldValueRef::Note(NoteFieldValueRef::DateTime(value))
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
                !QueryFieldValueRef::Note(NoteFieldValueRef::DateTime(value))
                    .is_equal_to_literal(&NoteFieldValue::DateTime(other))
            );
        }

        #[test]
        fn returns_false_when_comparing_datetime_ref_to_a_non_date_literal() {
            let value = DateTimeValue::parse_iso("2026-07-29T14:30:00")
                .expect("valid datetime");
            assert!(
                !QueryFieldValueRef::Note(NoteFieldValueRef::DateTime(value))
                    .is_equal_to_literal(&NoteFieldValue::Number(1.0))
            );
        }
        #[test]
        fn matches_query_field_value_ref_duration_with_equivalent_literal() {
            let dv = DurationValue::parse("1h 30m").expect("valid duration");
            let qval =
                QueryFieldValueRef::Note(NoteFieldValueRef::Duration(&dv));
            let literal = NoteFieldValue::Duration(
                DurationValue::parse("90m").expect("valid duration"),
            );
            assert!(qval.is_equal_to_literal(&literal));
        }

        #[test]
        fn returns_true_when_text_ref_contains_substring() {
            assert!(
                QueryFieldValueRef::Note(NoteFieldValueRef::String(
                    "hello world"
                ))
                .is_containing(&NoteFieldValue::String("world".into()))
            );
        }

        #[test]
        fn returns_false_when_text_ref_does_not_contain_substring() {
            assert!(
                !QueryFieldValueRef::Note(NoteFieldValueRef::String("hello"))
                    .is_containing(&NoteFieldValue::String("xyz".into()))
            );
        }

        #[test]
        fn returns_true_when_list_ref_contains_tag_prefix() {
            let items = [NoteFieldValue::String("#book/fiction".into())];
            let list =
                QueryFieldValueRef::Note(NoteFieldValueRef::List(&items));
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

        #[test]
        fn matches_identically_for_borrowed_and_owned_list_values() {
            // Regression: Owned(List) must delegate to the same list
            // containment logic as borrowed lists.
            let items =
                vec![NoteFieldValue::String("#book/fiction".to_owned())];
            let target = NoteFieldValue::String("#book".to_owned());

            let borrowed =
                QueryFieldValueRef::Note(NoteFieldValueRef::List(&items))
                    .is_containing(&target);
            let owned =
                QueryFieldValueRef::Owned(NoteFieldValue::List(items.into()))
                    .is_containing(&target);

            assert_eq!(borrowed, owned);
            assert!(borrowed, "#book must match #book/fiction by tag prefix");
        }
    }

    mod formatting {
        use pretty_assertions::assert_eq;

        use super::*;
        #[test]
        fn formats_empty_string_for_null_value() {
            let mut out = String::new();
            QueryFieldValueRef::Note(NoteFieldValueRef::Null)
                .append_text(&mut out);
            assert_eq!(out, "");
        }

        #[test]
        fn formats_boolean_as_lowercase_literal() {
            let mut out = String::new();
            QueryFieldValueRef::Note(NoteFieldValueRef::Bool(true))
                .append_text(&mut out);
            assert_eq!(out, "true");
        }

        #[test]
        fn formats_number_without_unnecessary_decimals() {
            let mut out = String::new();
            QueryFieldValueRef::Note(NoteFieldValueRef::Number(42.0))
                .append_text(&mut out);
            assert_eq!(out, "42");
        }

        #[test]
        fn formats_text_verbatim() {
            let mut out = String::new();
            QueryFieldValueRef::Note(NoteFieldValueRef::String("hello"))
                .append_text(&mut out);
            assert_eq!(out, "hello");
        }

        #[test]
        fn formats_date_as_yyyy_mm_dd() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let mut out = String::new();
            QueryFieldValueRef::Note(NoteFieldValueRef::Date(date))
                .append_text(&mut out);
            assert_eq!(out, "2026-07-29");
        }

        #[test]
        fn formats_datetime_without_offset() {
            let value = DateTimeValue::parse_iso("2026-07-29T14:30:00")
                .expect("valid datetime");
            let mut out = String::new();
            QueryFieldValueRef::Note(NoteFieldValueRef::DateTime(value))
                .append_text(&mut out);
            assert_eq!(out, "2026-07-29T14:30:00");
        }
    }

    mod conversions {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn to_owned_value_preserves_a_typed_date() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            let owned = QueryFieldValueRef::Note(NoteFieldValueRef::Date(date))
                .to_owned_value();
            assert_eq!(owned, NoteFieldValue::Date(date));
        }

        #[test]
        fn to_owned_value_preserves_a_typed_datetime() {
            let value = DateTimeValue::parse_iso("2026-07-29T14:30:00")
                .expect("valid datetime");
            let owned =
                QueryFieldValueRef::Note(NoteFieldValueRef::DateTime(value))
                    .to_owned_value();
            assert_eq!(owned, NoteFieldValue::DateTime(value));
        }

        #[test]
        fn as_str_returns_none_for_the_date_variant() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            assert_eq!(
                QueryFieldValueRef::Note(NoteFieldValueRef::Date(date))
                    .as_str(),
                None
            );
        }

        #[test]
        fn as_str_returns_none_for_the_datetime_variant() {
            let datetime = DateTimeValue::parse_iso("2026-07-29T14:30:00")
                .expect("valid datetime");
            assert_eq!(
                QueryFieldValueRef::Note(NoteFieldValueRef::DateTime(datetime))
                    .as_str(),
                None
            );
        }

        #[test]
        fn from_note_field_value_preserves_a_typed_date() {
            let date = DateValue::parse_iso("2026-07-29").expect("valid date");
            assert!(matches!(
                QueryFieldValueRef::from(&NoteFieldValue::Date(date)),
                QueryFieldValueRef::Note(NoteFieldValueRef::Date(d)) if d == date
            ));
        }

        #[test]
        fn from_note_field_value_preserves_a_typed_datetime() {
            let datetime = DateTimeValue::parse_iso("2026-07-29T14:30:00")
                .expect("valid datetime");
            assert!(matches!(
                QueryFieldValueRef::from(&NoteFieldValue::DateTime(datetime)),
                QueryFieldValueRef::Note(NoteFieldValueRef::DateTime(d))
                    if d == datetime
            ));
        }

        #[test]
        fn to_owned_value_converts_tags_into_a_string_list() {
            let tags = [Tag::parse("#book").expect("valid tag")];
            let owned = QueryFieldValueRef::Tags(&tags).to_owned_value();
            assert_eq!(
                owned,
                NoteFieldValue::List(Box::from([NoteFieldValue::String(
                    "#book".to_owned()
                )]))
            );
        }

        #[test]
        fn to_owned_value_converts_inlinks_into_a_string_list() {
            let inlinks = [PathBuf::from("notes/a.md")];
            let owned = QueryFieldValueRef::Inlinks(&inlinks).to_owned_value();
            assert_eq!(
                owned,
                NoteFieldValue::List(Box::from([NoteFieldValue::String(
                    "notes/a.md".to_owned()
                )]))
            );
        }

        #[test]
        fn as_note_ref_returns_none_for_tags() {
            let tags = [Tag::parse("#book").expect("valid tag")];
            assert!(QueryFieldValueRef::Tags(&tags).as_note_ref().is_none());
        }

        #[test]
        fn as_note_ref_returns_none_for_inlinks() {
            let inlinks = [PathBuf::from("notes/a.md")];
            assert!(
                QueryFieldValueRef::Inlinks(&inlinks).as_note_ref().is_none()
            );
        }

        #[test]
        fn as_note_ref_returns_some_for_note() {
            assert!(
                QueryFieldValueRef::Note(NoteFieldValueRef::Number(1.0))
                    .as_note_ref()
                    .is_some()
            );
        }

        #[test]
        fn as_note_ref_returns_some_for_owned() {
            let value = NoteFieldValue::Number(1.0);
            assert!(QueryFieldValueRef::Owned(value).as_note_ref().is_some());
        }
    }
}
