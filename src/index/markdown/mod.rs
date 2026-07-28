//! Markdown Note Metadata parsing and domain types.

mod parser;
mod types;

pub(crate) use parser::parse_markdown;
pub(crate) use types::{
    CodeRegion, Frontmatter, LinkType, List, ListItem, Note, NoteRecord,
    Outlink, TaskStatus,
};
