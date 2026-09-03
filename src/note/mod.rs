//! Parse Obsidian-style Markdown notes into structured records.
//!
//! [`parse_markdown`] walks a `pulldown-cmark` event stream once, building a
//! [`Note`] that holds YAML frontmatter, lists, outgoing links, inline fields,
//! and tags.
//!
//! # Architecture and Parsing Pipeline
//!
//! 1. `pulldown-cmark` tokenizes raw Markdown with YAML metadata blocks and
//!    wikilinks enabled. Task markers are recognized at item-leading positions
//!    using pulldown-cmark-compatible whitespace rules.
//! 2. `ParserContext` accumulates block-level state, including frontmatter
//!    text, list nesting, link targets, and plain-text scan buffers that
//!    exclude fenced code blocks, indented code blocks, and inline code spans.
//! 3. When a text block closes, inline token lexing scans its buffer for `Key::
//!    Value`, `[Key:: Value]`, `(Key:: Value)`, and `#tag` tokens.
//!    Status-marked list items also recognize task emoji shorthands (`🗓️`,
//!    `➕`, `🛫`, `⏳`, `✅`).
//! 4. The assembled [`Note`] stores all extracted data in document order.
//!
//! # Key Types
//!
//! - [`Note`]: Parsed record for one Markdown file.
//! - [`List`], [`ListItem`], [`ListItemType`], [`TaskListItem`]: Ordered and
//!   unordered lists, including classified task items, checkboxes, and nested
//!   child lists.
//! - [`ListText`], [`TaskDates`], [`TaskPriority`]: Normalized list text,
//!   extracted task dates, and priority levels.
//! - [`TaskIter`]: Depth-first task iterators across list trees.
//! - [`Link`], [`LinkType`], [`LinkTarget`]: Outgoing links from Markdown
//!   `[text](target)` and Obsidian `[[target|alias]]` syntax.
//! - [`Frontmatter`], [`RawFrontmatter`]: YAML frontmatter as structured fields
//!   or raw text.
//! - [`NoteFieldValue`]: Body metadata values parsed from `Key:: Value` syntax.
//! - [`Tag`](crate::Tag): Markdown tags such as `#book` and `#projects/active`.
//!
//! # Examples
//!
//! ```rust
//! # #[cfg(feature = "test-utils")]
//! # {
//! use std::path::Path;
//!
//! use traces_pkm::{MarkdownParserInput, parse_markdown};
//!
//! let input = MarkdownParserInput::for_test(
//!     Path::new("todo.md"),
//!     "- [ ] Action item 📅 2025-01-15\n- Plain bullet",
//! );
//! let note = parse_markdown(&input);
//! assert_eq!(note.tasks().count(), 1);
//! # }
//! ```

mod cursor;
mod field;
mod links;
mod lists;
mod metadata;
mod model;
mod parser;

pub use field::NoteFieldValue;
pub use links::{Link, LinkTarget, LinkType};
pub use lists::{
    List, ListItem, ListItemType, ListText, TaskDates, TaskIter, TaskListItem,
    TaskPriority,
};
pub use metadata::Frontmatter;
pub(crate) use metadata::RawFrontmatter;
pub use model::Note;
pub use parser::{MarkdownParserInput, parse_markdown};
