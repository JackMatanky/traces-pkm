//! Markdown Note Metadata: parses `.md`/`.markdown` source into [`Note`]
//! records for the [`super::FileIndex`].
//!
//! - [`note`]: [`Note`] aggregate domain record.
//! - [`metadata`]: YAML [`RawFrontmatter`], [`Frontmatter`], and
//!   [`MetadataField`]/[`FieldValue`] model.
//! - [`structure`]: markdown lists, tasks, outlinks, tags, and code regions.
//! - [`parser`]: [`parse_markdown`] walks `pulldown-cmark` events into the
//!   domain model.
//! - [`inline`]: the Dataview-compatible Inline Field and markdown tag lexer.

mod inline;
mod metadata;
mod note;
mod parser;
mod structure;

pub(crate) use metadata::{
    FieldSource, FieldValue, Frontmatter, InlineField, InlineFieldForm,
    MetadataField, RawFrontmatter,
};
pub(crate) use note::Note;
pub(crate) use parser::parse_markdown;
pub(crate) use structure::{
    CodeRegion, LinkType, List, ListItem, Outlink, Tag, TaskStatus,
};
