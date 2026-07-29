//! Markdown and Obsidian link values extracted from notes.

use serde::{Deserialize, Serialize};

use super::byte::ByteSource;

/// Link syntax for an extracted [`Outlink`].
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) enum LinkType {
    /// Standard Markdown `[text](target)` link.
    Markdown,
    /// Obsidian `[[target|alias]]` wikilink.
    Wikilink,
}

/// Outgoing Markdown link or Obsidian wikilink.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct Outlink {
    target: String,
    text: String,
    kind: LinkType,
    embedded: bool,
}

impl Outlink {
    /// Creates a non-embedded outlink.
    ///
    /// # Arguments
    ///
    /// * `target` - Link destination.
    /// * `text` - Display text.
    /// * `kind` - Markdown or wikilink syntax.
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

    /// Parses a complete Obsidian wikilink such as `[[target|text]]`.
    #[must_use]
    pub(crate) fn parse_wikilink(s: &str) -> Option<Self> {
        let (link, consumed) = Self::parse_wikilink_prefix(s)?;
        (consumed == s.len()).then_some(link)
    }

    /// Parses an Obsidian wikilink prefix and returns its byte length.
    #[must_use]
    pub(crate) fn parse_wikilink_prefix(s: &str) -> Option<(Self, usize)> {
        let source = ByteSource::new(s);
        let (embedded, raw_start, raw) =
            s.strip_prefix('!').map_or((false, 0, s), |rest| (true, 1, rest));
        let inner_start = source.advance(raw_start, 2);
        let inner = raw.strip_prefix("[[")?;
        let inner_source = ByteSource::new(inner);
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

    /// Link target.
    #[inline]
    #[must_use]
    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    /// Display text, or alias text for a wikilink.
    #[inline]
    #[must_use]
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Link syntax.
    #[inline]
    #[must_use]
    pub(crate) fn kind(&self) -> LinkType {
        self.kind
    }

    /// Returns `true` for Obsidian wikilinks.
    #[inline]
    #[must_use]
    pub(crate) fn is_wikilink(&self) -> bool {
        matches!(self.kind, LinkType::Wikilink)
    }

    /// Returns `true` for standard Markdown links.
    #[inline]
    #[must_use]
    pub(crate) fn is_markdown(&self) -> bool {
        matches!(self.kind, LinkType::Markdown)
    }

    /// Returns `true` for embedded wikilinks.
    #[inline]
    #[must_use]
    pub(crate) fn is_embedded(&self) -> bool {
        self.embedded
    }
}

fn find_wikilink_close(s: &str) -> Option<usize> {
    let source = ByteSource::new(s);
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
        if ch == ']' && source.starts_with(source.advance_char(index, ch), "]")
        {
            return Some(index);
        }
    }
    None
}

fn split_wikilink_text(s: &str) -> (&str, &str) {
    let source = ByteSource::new(s);
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
        if ch == '|' {
            return (&s[..index], &s[source.advance_char(index, ch)..]);
        }
    }
    (s, s)
}

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
        let link = Outlink::new(target, text, kind);
        assert_eq!(link.target(), target);
        assert_eq!(link.text(), text);
        assert_eq!(link.kind(), kind);
        assert_eq!(link.is_wikilink(), expected_wikilink);
        assert_eq!(link.is_markdown(), expected_markdown);
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
        let link = Outlink::parse_wikilink(raw).expect("valid wikilink");

        assert_eq!(link.target(), expected_target);
        assert_eq!(link.text(), expected_text);
        assert_eq!(link.kind(), LinkType::Wikilink);
    }

    #[test]
    fn parses_embedded_wikilink_values() {
        let link = Outlink::parse_wikilink("![[hello]]")
            .expect("valid embedded wikilink");

        assert_eq!(link.target(), "hello");
        assert_eq!(link.text(), "hello");
        assert_eq!(link.kind(), LinkType::Wikilink);
        assert_eq!(link.is_embedded(), true);
    }

    #[test]
    fn parses_escaped_pipe_in_wikilink_target() {
        let link = Outlink::parse_wikilink(r"[[Hello \| There]]")
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
        assert_eq!(Outlink::parse_wikilink(raw), None);
    }
}
