//! Parse Obsidian-style Markdown notes into structured records.
//!
//! [`parse_markdown`] walks a `pulldown-cmark` event stream once, building a
//! [`Note`] that holds frontmatter, lists, outgoing links, inline fields, and
//! tags.
//!
//! # Pipeline
//!
//! 1. `pulldown-cmark` tokenizes raw Markdown with YAML and wikilink extensions
//!    enabled. Task markers are not a pulldown-cmark extension:
//!    [`marker::scan_marker`] recognizes them from item-leading text.
//! 2. `ParserContext` accumulates block-level state: frontmatter text, list
//!    nesting, link targets, and plain-text scan buffers that exclude fenced
//!    code blocks, indented code blocks, and inline code.
//! 3. When a text block closes, [`lexer::InlineTokenLexer`] scans its buffer
//!    for `Key:: Value`, `[Key:: Value]`, `(Key:: Value)`, and `#tag` tokens.
//!    Status-marked list items also recognize date-shorthand emoji (`🗓️`, `➕`,
//!    `🛫`, `⏳`, `✅`).
//! 4. The assembled [`Note`] stores all extracted data in document order.
//!
//! # Main Types
//!
//! - [`Note`]: parsed record for one Markdown file.
//! - [`List`], [`ListItem`], [`ListItemType`]: ordered and unordered lists,
//!   including classified task items and nested child lists.
//! - [`Link`], [`LinkType`], [`LinkTarget`]: outgoing links from Markdown
//!   `[text](target)` and Obsidian `[[target|alias]]` syntax.
//! - [`Frontmatter`], `RawFrontmatter`: YAML frontmatter as structured fields
//!   or raw text.
//! - [`NoteFieldValue`]: body metadata values parsed from `Key:: Value` syntax.
//! - [`Tag`]: Markdown tags such as `#book` and `#projects/active`.

mod cursor;
mod lexer;
mod links;
mod lists;
mod marker;
mod metadata;
mod model;
mod parser;

pub use links::{Link, LinkTarget, LinkType};
pub(crate) use lists::ListItemType;
pub use lists::{List, ListItem};
pub(crate) use metadata::RawFrontmatter;
pub use metadata::{Frontmatter, NoteFieldValue};
pub use model::Note;
pub use parser::parse_markdown;
