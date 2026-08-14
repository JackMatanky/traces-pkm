//! File options and borrowed filters produced from the `FileIndex`.

use std::collections::BTreeSet;

use crate::{
    field::FieldKey,
    index::FileRecord,
    note::{FieldValue, Note},
};

/// One selectable file option derived from the current [`FileIndex`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FileOption {
    label: String,
    value: String,
}

impl FileOption {
    /// Builds the option for `record`: label resolution tries `note`'s
    /// configured aliases key, then its title key (via `keys`), falling back to
    /// `record`'s filename stem when neither resolves or `note` is `None`. The
    /// value is always `record`'s project-relative path.
    #[inline]
    #[must_use]
    pub(crate) fn for_record(
        record: &FileRecord,
        note: Option<&Note>,
        keys: &FrontmatterFieldKeys,
    ) -> Self {
        let label = note
            .and_then(|note| Self::frontmatter_label(note, keys))
            .unwrap_or_else(|| record.name().as_str())
            .to_owned();
        Self {
            label,
            value: record.path().to_string_lossy().into_owned(),
        }
    }

    /// Returns the display label for `note`: the first usable configured
    /// aliases value, falling back to a scalar configured title value.
    /// Callers fall back further to the filename stem when this returns
    /// `None`.
    fn frontmatter_label<'a>(
        note: &'a Note,
        keys: &FrontmatterFieldKeys,
    ) -> Option<&'a str> {
        let frontmatter = note.frontmatter()?;
        let alias =
            frontmatter.get(keys.aliases()).and_then(|value| match value {
                FieldValue::List(items) => {
                    items.iter().find_map(FieldValue::as_str)
                }
                value => value.as_str(),
            });
        alias.or_else(|| {
            frontmatter.get(keys.title()).and_then(FieldValue::as_str)
        })
    }

    /// Consumes the option, returning its `(label, value)` text without
    /// re-allocating: the caller (minijinja value construction) needs owned
    /// `String`s anyway, and this option is never read again afterward.
    #[inline]
    #[must_use]
    pub(crate) fn into_parts(self) -> (String, String) {
        (self.label, self.value)
    }
}

/// Borrowed filter values for [`crate::index::FileIndex::file_options`].
#[derive(Copy, Clone)]
pub(crate) struct FileOptionFilter<'a> {
    pub(crate) folders: &'a [String],
    pub(crate) ext: Option<&'a str>,
    pub(crate) classes: Option<&'a BTreeSet<String>>,
    pub(crate) keys: &'a FrontmatterFieldKeys,
}
impl<'a> FileOptionFilter<'a> {
    /// Builds a borrowed file-option filter.
    #[inline]
    #[must_use]
    pub(crate) const fn new(
        folders: &'a [String],
        ext: Option<&'a str>,
        classes: Option<&'a BTreeSet<String>>,
        keys: &'a FrontmatterFieldKeys,
    ) -> Self {
        Self {
            folders,
            ext,
            classes,
            keys,
        }
    }
}

/// The `[frontmatter] title`/`aliases` and `[schemas] class_field` keys used
/// together to resolve `file`-typed Schema field options: label resolution
/// needs `title`+`aliases`, class filtering needs `class`. Bundled because
/// every render-path consumer needs at least two of the three, each already
/// validated once at config load (`FrontmatterConfig`/`SchemasConfig` in
/// `crate::config`) — this just carries that guarantee forward instead of
/// re-threading three loose parameters. Lives here, not in `crate::field`,
/// because it's a config-resolved aggregate specific to file-option
/// matching, not a field-name/key primitive.
#[derive(Clone, Debug)]
pub(crate) struct FrontmatterFieldKeys {
    class: FieldKey,
    title: FieldKey,
    aliases: FieldKey,
}

impl FrontmatterFieldKeys {
    /// Bundles three already-validated field keys. Infallible: validation
    /// happens once, upstream, when each [`FieldKey`] is first constructed
    /// from config.
    #[inline]
    #[must_use]
    pub(crate) const fn new(
        class: FieldKey,
        title: FieldKey,
        aliases: FieldKey,
    ) -> Self {
        Self {
            class,
            title,
            aliases,
        }
    }

    /// Returns the frontmatter key naming a Note's File Class(es).
    #[inline]
    #[must_use]
    pub(crate) fn class(&self) -> &FieldKey {
        &self.class
    }

    /// Returns the frontmatter key holding a Note's display title.
    #[inline]
    #[must_use]
    pub(crate) fn title(&self) -> &FieldKey {
        &self.title
    }

    /// Returns the frontmatter key holding a Note's aliases.
    #[inline]
    #[must_use]
    pub(crate) fn aliases(&self) -> &FieldKey {
        &self.aliases
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::parse_markdown;
    mod frontmatter_label {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::alias_wins_over_title(
            "---\ntitle: Title Value\naliases: [Alias Value]\n---\n",
            Some("Alias Value")
        )]
        #[case::title_wins_over_stem_fallback(
            "---\ntitle: Title Value\n---\n",
            Some("Title Value")
        )]
        #[case::non_string_title_is_ignored("---\ntitle: 42\n---\n", None)]
        #[case::no_frontmatter_falls_through_to_stem(
            "No frontmatter here.",
            None
        )]
        #[case::scalar_aliases_value(
            "---\naliases: Solo Name\n---\n",
            Some("Solo Name")
        )]
        #[case::aliases_list_with_no_strings_falls_through_to_title(
            "---\ntitle: Title Value\naliases: [42, true]\n---\n",
            Some("Title Value")
        )]
        #[case::aliases_list_skips_leading_non_strings(
            "---\naliases: [42, Real Alias]\n---\n",
            Some("Real Alias")
        )]
        #[case::non_string_aliases_scalar_falls_through_to_title(
            "---\ntitle: Title Value\naliases: 42\n---\n",
            Some("Title Value")
        )]
        fn resolves_precedence(
            #[case] source: &str,
            #[case] expected: Option<&str>,
        ) {
            let note = parse_markdown("note.md", source);
            let keys = FrontmatterFieldKeys::new(
                FieldKey::try_new("class").expect("valid field key"),
                FieldKey::try_new("title").expect("valid field key"),
                FieldKey::try_new("aliases").expect("valid field key"),
            );

            assert_eq!(FileOption::frontmatter_label(&note, &keys), expected);
        }
    }
}
