//! Markdown Note Metadata: parses `.md`/`.markdown` source into [`Note`]
//! records for the [`super::FileIndex`].
//!
//! - [`types`]: the [`Note`] domain model — frontmatter, lists, outlinks, and
//!   the [`CodeRegion`] byte ranges #03's Inline Field extraction will exclude.
//! - [`parser`]: [`parse_markdown`] walks `pulldown-cmark` events into that
//!   model with an explicit stack (not recursion) for arbitrarily nested lists.
//!   A link's display text feeds both its [`Outlink`] and the plain text of the
//!   list item containing it.

mod parser;
mod types;

pub(crate) use parser::parse_markdown;
pub(crate) use types::{
    CodeRegion, Frontmatter, LinkType, List, ListItem, Note, Outlink,
    TaskStatus,
};
