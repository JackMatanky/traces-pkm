//! Outgoing links from Markdown and Obsidian wikilink syntax.
//!
//! [`Link`] pairs a raw target string with its [`LinkType`] (Markdown or
//! wikilink syntax); [`LinkTarget`] splits that raw target into its path and
//! heading-anchor parts for resolution against indexed notes.

use serde::{Deserialize, Serialize};

use super::cursor::SourceText;
use crate::{DelimiterType, find_closing_delimiter};

/// The syntax used by an extracted [`Link`].
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum LinkType {
    /// Standard Markdown `[text](target)` link.
    Markdown,
    /// Obsidian `[[target|alias]]` wikilink.
    Wikilink,
}

/// An outgoing Markdown link or Obsidian wikilink.
///
/// Holds the raw target string, display text (or alias for wikilinks), link
/// syntax kind, and an embedded flag for `![[target]]` embeds.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Link {
    target: String,
    text: String,
    kind: LinkType,
    embedded: bool,
}

impl Link {
    /// Creates a non-embedded [`Link`] with `target`, display `text`, and
    /// `kind`.
    #[inline]
    #[must_use]
    pub(crate) fn new(
        target: impl Into<String>,
        text: impl Into<String>,
        kind: LinkType,
    ) -> Self {
        Self {
            target: target.into(),
            text: text.into(),
            kind,
            embedded: false,
        }
    }

    /// Parses `s` as a complete Obsidian wikilink such as `[[target|text]]`.
    ///
    /// Unlike [`Self::parse_wikilink_prefix`], `s` must contain nothing but the
    /// wikilink itself: any trailing text after the closing `]]` makes this
    /// return `None`. Also returns `None` for any input
    /// [`Self::parse_wikilink_prefix`] rejects.
    #[must_use]
    pub(crate) fn parse_wikilink(s: &str) -> Option<Self> {
        let (link, consumed) = Self::parse_wikilink_prefix(s)?;
        (consumed == s.len()).then_some(link)
    }

    /// Parses an Obsidian wikilink prefix from the start of `s`.
    ///
    /// Allows trailing text after the closing `]]` and returns the parsed link
    /// with the number of bytes consumed. Returns `None` if `s` does not start
    /// with `[[` (after an optional leading `!`), has no unescaped closing
    /// `]]`, or has an empty target.
    #[must_use]
    pub(crate) fn parse_wikilink_prefix(s: &str) -> Option<(Self, usize)> {
        let source = SourceText::new(s);
        let (embedded, raw_start, raw) =
            s.strip_prefix('!').map_or((false, 0, s), |rest| (true, 1, rest));
        let inner_start = source.advance(raw_start, 2);
        let inner = raw.strip_prefix("[[")?;
        let inner_source = SourceText::new(inner);
        let inner_end = find_wikilink_close(inner)?;
        let raw_inner = inner_source.get(0..inner_end)?;
        let consumed =
            source.advance(source.advance(inner_start, inner_end), 2);
        let (target, text) = split_wikilink_text(raw_inner);
        let target = unescape_wikilink_part(target.trim());
        if target.is_empty() {
            return None;
        }
        let text = unescape_wikilink_part(text.trim());
        let text = if text.is_empty() {
            target.clone()
        } else {
            text
        };
        Some((
            Self {
                target,
                text,
                kind: LinkType::Wikilink,
                embedded,
            },
            consumed,
        ))
    }

    /// Returns the link destination.
    #[inline]
    #[must_use]
    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    /// Splits this link's raw target text into [`LinkTarget`] parts at its
    /// first `#`, trimming surrounding whitespace from each part.
    #[must_use]
    pub(crate) fn target_parts(&self) -> LinkTarget<'_> {
        match self.target.split_once('#') {
            Some((path, anchor)) => {
                let path = path.trim();
                let anchor = anchor.trim();
                if path.is_empty() {
                    LinkTarget::AnchorOnly(anchor)
                } else {
                    LinkTarget::PathWithAnchor(path, anchor)
                }
            }
            None => LinkTarget::Path(self.target.trim()),
        }
    }

    /// Returns the display text, or alias text for a wikilink.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; kept for Link accessor \
                      symmetry with its fields"
        )
    )]
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Returns the syntax used by the source link.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; kept for Link accessor \
                      symmetry with its fields"
        )
    )]
    pub(crate) const fn kind(&self) -> LinkType {
        self.kind
    }

    /// Returns `true` for Obsidian wikilinks.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; kept for Link accessor \
                      symmetry with its fields"
        )
    )]
    pub(crate) const fn is_wikilink(&self) -> bool {
        matches!(self.kind, LinkType::Wikilink)
    }

    /// Returns `true` for standard Markdown links.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; kept for Link accessor \
                      symmetry with its fields"
        )
    )]
    pub(crate) const fn is_markdown(&self) -> bool {
        matches!(self.kind, LinkType::Markdown)
    }

    /// Returns `true` for embedded wikilinks (`![[target]]`).
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; kept for Link accessor \
                      symmetry with its fields"
        )
    )]
    pub(crate) const fn is_embedded(&self) -> bool {
        self.embedded
    }
}

/// The parts a [`Link`]'s raw target text splits into at its first `#`.
///
/// Obsidian and Markdown links can point to three mutually exclusive shapes:
///
/// - A note, by path ([`Self::Path`]).
/// - A heading anchor within a note ([`Self::PathWithAnchor`]).
/// - An in-page anchor with no note path at all, such as `[[#Heading]]`
///   ([`Self::AnchorOnly`]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum LinkTarget<'a> {
    /// A path with no `#` suffix.
    Path(&'a str),
    /// A path followed by a `#heading` anchor.
    PathWithAnchor(&'a str, &'a str),
    /// An in-page anchor with no path segment, such as `[[#Heading]]`.
    AnchorOnly(&'a str),
}

impl<'a> LinkTarget<'a> {
    /// Returns this target's path segment, or `None` for [`Self::AnchorOnly`].
    #[must_use]
    pub(crate) const fn path(self) -> Option<&'a str> {
        match self {
            Self::Path(path) | Self::PathWithAnchor(path, _) => Some(path),
            Self::AnchorOnly(_) => None,
        }
    }

    /// Returns this target's `#heading` anchor, or `None` when it has none.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; kept for LinkTarget \
                      accessor symmetry with its variants"
        )
    )]
    pub(crate) const fn anchor(self) -> Option<&'a str> {
        match self {
            Self::PathWithAnchor(_, anchor) | Self::AnchorOnly(anchor) => {
                Some(anchor)
            }
            Self::Path(_) => None,
        }
    }

    /// Returns whether this target has a non-empty path segment.
    ///
    /// `false` for [`Self::AnchorOnly`] and for the degenerate case of an
    /// entirely empty target. Corresponds to whether an exact-path lookup is
    /// worth attempting at all.
    #[must_use]
    pub(crate) fn has_path(self) -> bool {
        self.path().is_some_and(|path| !path.is_empty())
    }

    /// Returns whether this target's path segment is a bare name with no
    /// directory prefix, such as `Project Alpha` rather than `archive/Project
    /// Alpha`.
    ///
    /// This is the shape Obsidian's wikilink-by-name search resolves, so it
    /// determines whether a whole-index stem search is eligible as a fallback.
    /// Returns `false` for:
    ///
    /// - [`Self::AnchorOnly`], which has no path.
    /// - Paths with an explicit directory component. A qualified path that
    ///   fails to match exactly stays unresolved rather than falling back to a
    ///   whole-index name search that could match an unrelated note.
    #[must_use]
    pub(crate) fn is_basename(self) -> bool {
        self.has_path() && self.path().is_some_and(|path| !path.contains('/'))
    }
}

/// Finds the byte offset of the first unescaped character in `s` for which
/// `is_close` returns `true`, treating a `\` as escaping the character that
/// follows it.
///
/// Shared by [`find_wikilink_close`] and [`split_wikilink_text`], which need
/// identical escape tracking with different terminal predicates.
fn find_unescaped(
    s: &str,
    mut is_close: impl FnMut(usize, char) -> bool,
) -> Option<usize> {
    let mut escaped = false;
    for (index, ch) in s.char_indices() {
        if ch == '\\' {
            escaped = !escaped;
            continue;
        }
        if escaped {
            escaped = false;
            continue;
        }
        if is_close(index, ch) {
            return Some(index);
        }
    }
    None
}

/// Finds the byte offset of the first unescaped `]]` in `s`, marking the
/// wikilink's closing double brackets.
fn find_wikilink_close(s: &str) -> Option<usize> {
    find_closing_delimiter(s, DelimiterType::DoubleBracket)
}

/// Splits wikilink inner text at the first unescaped `|` into a `(target,
/// alias)` pair, or `(s, s)` when there is no `|` separator, so callers can
/// treat the alias as identical to the target.
fn split_wikilink_text(s: &str) -> (&str, &str) {
    let source = SourceText::new(s);
    find_unescaped(s, |_, ch| ch == '|').map_or((s, s), |index| {
        (&s[..index], &s[source.advance_char(index, '|')..])
    })
}

/// Unescapes a wikilink target or alias segment.
///
/// A backslash escapes only `|`: `\|` becomes a literal `|`. Every other
/// backslash is copied through unchanged.
fn unescape_wikilink_part(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut escaped = false;
    for ch in s.chars() {
        if escaped {
            if ch != '|' {
                out.push('\\');
            }
            out.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            out.push(ch);
        }
    }
    if escaped {
        out.push('\\');
    }
    out
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::wikilink("target", "alias", LinkType::Wikilink, true, false)]
    #[case::markdown(
        "https://example.com",
        "text",
        LinkType::Markdown,
        false,
        true
    )]
    fn evaluates_outlink_kind_predicates(
        #[case] target: &str,
        #[case] text: &str,
        #[case] kind: LinkType,
        #[case] expected_wikilink: bool,
        #[case] expected_markdown: bool,
    ) {
        let link = Link::new(target, text, kind);
        assert_eq!(link.target(), target);
        assert_eq!(link.text(), text);
        assert_eq!(link.kind(), kind);
        assert_eq!(link.is_wikilink(), expected_wikilink);
        assert_eq!(link.is_markdown(), expected_markdown);
    }

    #[rstest]
    #[case::plain_path("notes/other.md", LinkTarget::Path("notes/other.md"))]
    #[case::path_with_anchor(
        "other#Some Heading",
        LinkTarget::PathWithAnchor("other", "Some Heading")
    )]
    #[case::anchor_only(
        "#Some Heading",
        LinkTarget::AnchorOnly("Some Heading")
    )]
    #[case::trims_whitespace_around_parts(
        " other # Some Heading ",
        LinkTarget::PathWithAnchor("other", "Some Heading")
    )]
    fn target_parts_splits_the_raw_target_at_the_first_hash(
        #[case] target: &str,
        #[case] expected: LinkTarget<'_>,
    ) {
        let link = Link::new(target, "text", LinkType::Markdown);
        assert_eq!(link.target_parts(), expected);
    }

    #[rstest]
    #[case::plain_path(LinkTarget::Path("a.md"), Some("a.md"), None)]
    #[case::path_with_anchor(
        LinkTarget::PathWithAnchor("a.md", "Heading"),
        Some("a.md"),
        Some("Heading")
    )]
    #[case::anchor_only(
        LinkTarget::AnchorOnly("Heading"),
        None,
        Some("Heading")
    )]
    fn link_target_path_and_anchor_report_the_matching_parts(
        #[case] target: LinkTarget<'_>,
        #[case] expected_path: Option<&str>,
        #[case] expected_anchor: Option<&str>,
    ) {
        assert_eq!(target.path(), expected_path);
        assert_eq!(target.anchor(), expected_anchor);
    }

    #[rstest]
    #[case::bare_name(LinkTarget::Path("a.md"), true, true)]
    #[case::qualified_path(LinkTarget::Path("archive/a.md"), true, false)]
    #[case::bare_name_with_anchor(
        LinkTarget::PathWithAnchor("a.md", "Heading"),
        true,
        true
    )]
    #[case::qualified_path_with_anchor(
        LinkTarget::PathWithAnchor("archive/a.md", "Heading"),
        true,
        false
    )]
    #[case::anchor_only(LinkTarget::AnchorOnly("Heading"), false, false)]
    #[case::empty_path(LinkTarget::Path(""), false, false)]
    fn link_target_has_path_and_is_basename_classify_the_path_segment(
        #[case] target: LinkTarget<'_>,
        #[case] expected_has_path: bool,
        #[case] expected_is_basename: bool,
    ) {
        assert_eq!(target.has_path(), expected_has_path);
        assert_eq!(target.is_basename(), expected_is_basename);
    }

    #[rstest]
    #[case::target_only("[[target]]", "target", "target")]
    #[case::target_with_alias("[[target|alias]]", "target", "alias")]
    #[case::trims_parts("[[ target | alias ]]", "target", "alias")]
    #[case::empty_alias_uses_target("[[target| ]]", "target", "target")]
    fn parses_wikilink_values(
        #[case] raw: &str,
        #[case] expected_target: &str,
        #[case] expected_text: &str,
    ) {
        let link = Link::parse_wikilink(raw).expect("valid wikilink");

        assert_eq!(link.target(), expected_target);
        assert_eq!(link.text(), expected_text);
        assert_eq!(link.kind(), LinkType::Wikilink);
    }

    #[test]
    fn parses_embedded_wikilink_values() {
        let link = Link::parse_wikilink("![[hello]]")
            .expect("valid embedded wikilink");

        assert_eq!(link.target(), "hello");
        assert_eq!(link.text(), "hello");
        assert_eq!(link.kind(), LinkType::Wikilink);
        assert_eq!(link.is_embedded(), true);
    }

    #[test]
    fn non_embedded_link_reports_false_for_is_embedded() {
        let link = Link::new("target", "text", LinkType::Markdown);

        assert_eq!(link.is_embedded(), false);
    }

    #[test]
    fn parses_escaped_pipe_in_wikilink_target() {
        let link = Link::parse_wikilink(r"[[Hello \| There]]")
            .expect("valid wikilink");

        assert_eq!(link.target(), "Hello | There");
        assert_eq!(link.text(), "Hello | There");
    }

    #[rstest]
    #[case::missing_open("target]]")]
    #[case::missing_close("[[target")]
    #[case::empty_target("[[]]")]
    #[case::blank_target("[[ | alias]]")]
    fn rejects_invalid_wikilink_values(#[case] raw: &str) {
        assert_eq!(Link::parse_wikilink(raw), None);
    }
}
