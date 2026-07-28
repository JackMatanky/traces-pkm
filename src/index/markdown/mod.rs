//! Markdown Note Metadata parsing and domain types.

mod parser;
mod types;

pub(crate) use parser::parse_markdown;
pub(crate) use types::{
    CodeRegion, Frontmatter, List, ListItem, Note, NoteRecord, Outlink,
    OutlinkKind, TaskStatus,
};
