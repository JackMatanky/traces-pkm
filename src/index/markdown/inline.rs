//! Dataview-compatible Inline Field and markdown tag lexer.
//!
//! Operates on already-composed plain-text buffers ([`parser::ParserContext`]
//! assembles one per body paragraph and per list item, skipping fenced code
//! blocks, indented code blocks, and inline code spans as it goes — see
//! [`super::parser`]) rather than re-deriving exclusions from
//! [`super::types::CodeRegion`] byte ranges over raw source. Both approaches
//! observe the same code-block boundaries; scanning the pre-excluded buffer
//! avoids a second raw-source pass and byte-range overlap arithmetic.
//!
//! [`parser::ParserContext`]: super::parser

use std::sync::LazyLock;

use regex::{Captures, Regex};

use super::types::{InlineField, InlineFieldForm};

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

/// Pushes the field captured by `caps` (with its match start offset, for
/// later sorting) onto `matches`. The `let`-`else` reads as a guard rather
/// than an `unwrap`/index (`clippy::indexing_slicing` denies `Captures`
/// indexing too), even though groups 1 and 2 are mandatory in every source
/// pattern and so are always present once `caps` exists.
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
pub(super) fn extract_tags(text: &str) -> Vec<String> {
    TAG_RE
        .find_iter(text)
        .filter(|found| {
            text[..found.start()]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_alphanumeric() && c != '_')
        })
        .map(|found| found.as_str().to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    mod inline_fields {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn extracts_a_bare_body_field() {
            let fields = extract_inline_fields("Author:: Jane Doe");

            assert_eq!(fields.len(), 1);
            let field = fields.first().expect("field present");
            assert_eq!(field.key(), "Author");
            assert_eq!(field.value(), "Jane Doe");
            assert_eq!(field.form(), InlineFieldForm::Body);
        }

        #[test]
        fn extracts_a_visible_key_bracket_field() {
            let fields =
                extract_inline_fields("See the [Status:: Draft] note.");

            assert_eq!(fields.len(), 1);
            let field = fields.first().expect("field present");
            assert_eq!(field.key(), "Status");
            assert_eq!(field.value(), "Draft");
            assert_eq!(field.form(), InlineFieldForm::VisibleKey);
        }

        #[test]
        fn extracts_a_hidden_key_paren_field() {
            let fields =
                extract_inline_fields("See the (Status:: Draft) note.");

            assert_eq!(fields.len(), 1);
            let field = fields.first().expect("field present");
            assert_eq!(field.key(), "Status");
            assert_eq!(field.value(), "Draft");
            assert_eq!(field.form(), InlineFieldForm::HiddenKey);
        }

        #[test]
        fn does_not_match_a_bare_field_mid_sentence() {
            let fields =
                extract_inline_fields("This sentence has a :: but no key.");

            assert_eq!(fields.len(), 0);
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

        use super::*;

        #[test]
        fn extracts_a_standalone_tag() {
            let tags = extract_tags("Filed under #book for later.");

            assert_eq!(tags, ["#book".to_owned()]);
        }

        #[test]
        fn extracts_a_nested_tag() {
            let tags = extract_tags("#projects/active needs review.");

            assert_eq!(tags, ["#projects/active".to_owned()]);
        }

        #[test]
        fn extracts_multiple_tags_in_order() {
            let tags = extract_tags("#book #fiction favorites.");

            assert_eq!(tags, ["#book".to_owned(), "#fiction".to_owned()]);
        }

        #[test]
        fn ignores_a_hash_embedded_in_a_word() {
            let tags = extract_tags("The issue is foo#bar, not a tag.");

            assert_eq!(tags.len(), 0);
        }

        #[test]
        fn extracts_adjacent_tags_separated_only_by_punctuation() {
            let tags = extract_tags("(#a)(#b)");

            assert_eq!(tags, ["#a".to_owned(), "#b".to_owned()]);
        }

        #[test]
        fn rejects_a_second_tag_glued_directly_onto_the_first() {
            let tags = extract_tags("#a#b");

            assert_eq!(tags, ["#a".to_owned()]);
        }
    }
}
