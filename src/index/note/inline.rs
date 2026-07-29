//! Dataview-compatible Inline Field and markdown tag lexer.
//!
//! Runs over plain-text buffers assembled by [`super::parser`]: one per
//! top-level text block (paragraph or heading), one per list item. Both
//! already exclude fenced code blocks, indented code blocks, and inline
//! code, so this module scans plain text only and never touches
//! [`super::types::CodeRegion`] ranges directly.

use std::sync::LazyLock;

use regex::{Captures, Regex};

use super::{FieldSource, FieldValue, InlineFieldForm, MetadataField, Tag};

/// Matches a full-line `Key:: Value` body field: a letter-led key token
/// (no whitespace — an unambiguous single word, unlike the bracket/paren
/// forms below, which are safely delimited even with a multi-word key)
/// followed by `::` and a value running to the end of its line.
#[expect(
    clippy::expect_used,
    reason = "static regex pattern is valid at compile time"
)]
static BODY_FIELD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*([A-Za-z][A-Za-z0-9_-]*)::[ \t]*(.*)$")
        .expect("BODY_FIELD_RE pattern is valid")
});

/// Matches a `[Key:: Value]` visible-key inline field.
#[expect(
    clippy::expect_used,
    reason = "static regex pattern is valid at compile time"
)]
static VISIBLE_FIELD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[([A-Za-z][A-Za-z0-9 _-]*?)::[ \t]*([^\]\n]*)\]")
        .expect("VISIBLE_FIELD_RE pattern is valid")
});

/// Matches a `(Key:: Value)` hidden-key inline field.
#[expect(
    clippy::expect_used,
    reason = "static regex pattern is valid at compile time"
)]
static HIDDEN_FIELD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\(([A-Za-z][A-Za-z0-9 _-]*?)::[ \t]*([^)\n]*)\)")
        .expect("HIDDEN_FIELD_RE pattern is valid")
});

/// Matches a markdown tag token: `#` immediately followed by a letter, then
/// word characters, hyphens, or `/` for nested tags (`#projects/active`).
/// [`extract_tags`] separately checks the character preceding each match to
/// reject mid-word occurrences like the `#` in `foo#bar` — see there for why
/// that check isn't folded into this pattern.
#[expect(
    clippy::expect_used,
    reason = "static regex pattern is valid at compile time"
)]
static TAG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"#[[:alpha:]][[:alnum:]_/-]*").expect("TAG_RE pattern is valid")
});

/// Extracts every Dataview-compatible Inline Field from `text`, sorted by
/// byte position. `text` is a pre-composed plain-text buffer that has already
/// excluded code spans/blocks — see the module docs.
pub(super) fn extract_inline_fields(text: &str) -> Vec<MetadataField> {
    let mut matches: Vec<(usize, MetadataField)> = Vec::new();
    for caps in BODY_FIELD_RE.captures_iter(text) {
        push_field(&mut matches, &caps, InlineFieldForm::Body);
    }
    for caps in VISIBLE_FIELD_RE.captures_iter(text) {
        push_field(&mut matches, &caps, InlineFieldForm::VisibleKey);
    }
    for caps in HIDDEN_FIELD_RE.captures_iter(text) {
        push_field(&mut matches, &caps, InlineFieldForm::HiddenKey);
    }
    matches.sort_by_key(|(start, _)| *start);
    matches.into_iter().map(|(_, field)| field).collect()
}
/// Extracts every markdown tag from `text`, in encounter order, keeping the
/// leading `#`. A tag is rejected when the byte immediately before its `#`
/// is alphanumeric or `_` (e.g. the `#` in `foo#bar`), checked directly on
/// `text` after a boundary-agnostic [`TAG_RE`] match — the `regex` crate has
/// no lookbehind, and folding the check into a consuming leading alternative
/// (`(?:\A|[^alnum])`) would eat a character other matches might need.
pub(super) fn extract_tags(text: &str) -> Vec<Tag> {
    TAG_RE
        .find_iter(text)
        .filter(|found| {
            text[..found.start()]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_alphanumeric() && c != '_')
        })
        .map(|found| Tag::new(found.as_str()))
        .collect()
}

/// Pushes the field captured by `caps`, together with its match start
/// offset for later sorting, onto `matches`. No-op if `caps` is missing the
/// mandatory key/value capture groups.
fn push_field(
    matches: &mut Vec<(usize, MetadataField)>,
    caps: &Captures<'_>,
    form: InlineFieldForm,
) {
    let (Some(whole), Some(key), Some(value)) =
        (caps.get(0), caps.get(1), caps.get(2))
    else {
        return;
    };
    matches.push((
        whole.start(),
        MetadataField::new(
            key.as_str().trim(),
            parse_inline_value_str(value.as_str()),
            FieldSource::Body(form),
        ),
    ));
}

/// Parses a raw string value into a [`FieldValue`].
fn parse_inline_value_str(raw: &str) -> FieldValue {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return FieldValue::Null;
    }
    if trimmed.eq_ignore_ascii_case("true") {
        return FieldValue::Bool(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return FieldValue::Bool(false);
    }
    if let Ok(num) = trimmed.parse::<f64>()
        && num.is_finite()
    {
        return FieldValue::Number(num);
    }
    if is_iso_date(trimmed) {
        return FieldValue::Date(trimmed.to_owned());
    }
    FieldValue::String(trimmed.to_owned())
}

/// Returns `true` if `s` starts with an ISO date format `YYYY-MM-DD`.
fn is_iso_date(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() >= 10
        && bytes.get(0..4).is_some_and(|b| b.iter().all(u8::is_ascii_digit))
        && bytes.get(4) == Some(&b'-')
        && bytes.get(5..7).is_some_and(|b| b.iter().all(u8::is_ascii_digit))
        && bytes.get(7) == Some(&b'-')
        && bytes.get(8..10).is_some_and(|b| b.iter().all(u8::is_ascii_digit))
}

#[cfg(test)]
mod tests {
    use super::*;

    mod inline_fields {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::body(
            "Author:: Jane Doe",
            "Author",
            "Jane Doe",
            InlineFieldForm::Body
        )]
        #[case::visible_key(
            "See the [Status:: Draft] note.",
            "Status",
            "Draft",
            InlineFieldForm::VisibleKey
        )]
        #[case::hidden_key(
            "See the (Status:: Draft) note.",
            "Status",
            "Draft",
            InlineFieldForm::HiddenKey
        )]
        fn extracts_a_field_in_its_declared_form(
            #[case] input: &str,
            #[case] expected_key: &str,
            #[case] expected_value: &str,
            #[case] expected_form: InlineFieldForm,
        ) {
            let fields = extract_inline_fields(input);

            assert_eq!(fields.len(), 1);
            let field = fields.first().expect("field present");
            assert_eq!(field.key(), expected_key);
            assert_eq!(field.value().as_str(), Some(expected_value));
            assert_eq!(field.form(), Some(expected_form));
        }

        #[test]
        fn rejects_a_multi_word_bare_key() {
            let fields =
                extract_inline_fields("This sentence has a :: but no key.");

            assert_eq!(fields.len(), 0);
        }

        #[rstest]
        #[case::visible_key("[Due Date:: 2024-01-01]", "Due Date")]
        #[case::hidden_key("(Due Date:: 2024-01-01)", "Due Date")]
        fn accepts_a_multi_word_key_when_delimiter_bounded(
            #[case] input: &str,
            #[case] expected_key: &str,
        ) {
            let fields = extract_inline_fields(input);

            let field = fields.first().expect("field present");
            assert_eq!(field.key(), expected_key);
            assert_eq!(field.value().as_str(), Some("2024-01-01"));
        }

        #[test]
        fn extracts_a_bare_field_from_each_line_of_a_multiline_buffer() {
            let fields =
                extract_inline_fields("Status:: Draft\nAuthor:: Jane Doe");

            let keys: Vec<&str> =
                fields.iter().map(MetadataField::key).collect();
            assert_eq!(keys, ["Status", "Author"]);
        }

        #[test]
        fn trims_surrounding_whitespace_from_the_value() {
            let fields = extract_inline_fields("Status::    Draft   ");

            let field = fields.first().expect("field present");
            assert_eq!(field.value().as_str(), Some("Draft"));
        }

        #[test]
        fn extracts_an_empty_value_when_nothing_follows_the_double_colon() {
            let fields = extract_inline_fields("Status::");

            let field = fields.first().expect("field present");
            assert_eq!(field.value(), &FieldValue::Null);
        }

        #[test]
        fn accepts_a_bare_key_preceded_by_leading_whitespace() {
            let fields = extract_inline_fields("  Status:: Draft");

            let field = fields.first().expect("field present");
            assert_eq!(field.key(), "Status");
        }

        #[test]
        fn orders_matches_by_position_across_forms() {
            let fields = extract_inline_fields(
                "Status:: Draft\nSee [Reviewer:: Jane] and (Editor:: Sam).",
            );

            let keys: Vec<&str> =
                fields.iter().map(MetadataField::key).collect();
            assert_eq!(keys, ["Status", "Reviewer", "Editor"]);
        }
    }

    mod tags {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::standalone(
            "Filed under #book for later.",
            &["#book"]
        )]
        #[case::nested_path(
            "#projects/active needs review.",
            &["#projects/active"]
        )]
        #[case::multiple_space_separated(
            "#book #fiction favorites.",
            &["#book", "#fiction"]
        )]
        #[case::hash_embedded_in_a_word(
            "The issue is foo#bar, not a tag.",
            &[]
        )]
        #[case::adjacent_separated_by_punctuation("(#a)(#b)", &["#a", "#b"])]
        #[case::glued_directly_onto_another_tag("#a#b", &["#a"])]
        #[case::preceded_by_multibyte_punctuation("café—#book", &["#book"])]
        #[case::glued_onto_a_multibyte_letter("café#book", &[])]
        fn extracts_tags_matching_the_expected_set(
            #[case] input: &str,
            #[case] expected: &[&str],
        ) {
            let tags = extract_tags(input);

            let expected: Vec<Tag> =
                expected.iter().map(|tag| Tag::new(*tag)).collect();
            assert_eq!(tags, expected);
        }
    }
}
