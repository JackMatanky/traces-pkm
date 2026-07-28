//! Markdown Note Metadata: parses `.md`/`.markdown` source into [`Note`]
//! records for the [`super::FileIndex`].
//!
//! - [`types`]: the [`Note`] domain model — frontmatter, lists, outlinks, code
//!   regions, Inline Fields, and tags.
//! - [`parser`]: [`parse_markdown`] walks `pulldown-cmark` events into that
//!   model with an explicit stack (not recursion) for arbitrarily nested lists.
//!   A link's display text feeds both its [`Outlink`] and the plain text of the
//!   list item containing it.
//! - [`inline`]: the Dataview-compatible Inline Field and markdown tag lexer
//!   [`parser`] runs over each body paragraph and list item's plain text.

mod inline;
mod parser;
mod types;

pub(crate) use parser::parse_markdown;
pub(crate) use types::{
    CodeRegion, Frontmatter, InlineField, InlineFieldForm, LinkType, List,
    ListItem, Note, Outlink, TaskStatus,
};
