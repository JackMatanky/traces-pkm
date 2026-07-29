//! Dataview-compatible Inline Field and markdown tag lexer.
//!
//! Runs over plain-text buffers assembled by [`super::parser`]: one per body
//! paragraph, one per list item. Both buffers already exclude fenced code
//! blocks, indented code blocks, and inline code, so this module scans
//! plain text only and never touches [`super::types::CodeRegion`] ranges
//! directly.

use std::sync::LazyLock;

use regex::{Captures, Regex};

use super::types::{InlineField, InlineFieldForm, Tag};

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
pub(super) fn extract_inline_fields(text: &str) -> Vec<InlineField> {
    let mut matches: Vec<(usize, InlineField)> = Vec::new();
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

/// Pushes the field captured by `caps`, together with its match start
/// offset for later sorting, onto `matches`. No-op if `caps` is missing the
/// mandatory key/value capture groups.
fn push_field(
    matches: &mut Vec<(usize, InlineField)>,
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
        InlineField::new(key.as_str().trim(), value.as_str().trim(), form),
    ));
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
            assert_eq!(field.value(), expected_value);
            assert_eq!(field.form(), expected_form);
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
            assert_eq!(field.value(), "2024-01-01");
        }

        #[test]
        fn extracts_a_bare_field_from_each_line_of_a_multiline_buffer() {
            let fields =
                extract_inline_fields("Status:: Draft\nAuthor:: Jane Doe");

            let keys: Vec<&str> = fields.iter().map(InlineField::key).collect();
            assert_eq!(keys, ["Status", "Author"]);
        }

        #[test]
        fn trims_surrounding_whitespace_from_the_value() {
            let fields = extract_inline_fields("Status::    Draft   ");

            let field = fields.first().expect("field present");
            assert_eq!(field.value(), "Draft");
        }

        #[test]
        fn extracts_an_empty_value_when_nothing_follows_the_double_colon() {
            let fields = extract_inline_fields("Status::");

            let field = fields.first().expect("field present");
            assert_eq!(field.value(), "");
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

            let keys: Vec<&str> = fields.iter().map(InlineField::key).collect();
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
