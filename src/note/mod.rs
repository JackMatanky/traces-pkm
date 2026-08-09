//! Parse Obsidian-style Markdown notes into structured records.
//!
//! [`parse_markdown`] walks a [`pulldown-cmark`] event stream once, building a
//! [`Note`] that holds frontmatter, lists, outgoing links, inline fields, and
//! tags.
//!
//! # Pipeline
//!
//! 1. `pulldown-cmark` tokenizes raw Markdown with task-list, YAML, and
//!    wikilink extensions enabled.
//! 2. [`ParserContext`](parser::ParserContext) accumulates block-level state:
//!    frontmatter text, list nesting, link targets, and plain-text scan buffers
//!    that exclude fenced code blocks, indented code blocks, and inline code.
//! 3. When a text block closes, the [`lexer`](lexer) scans its buffer for
//!    `Key:: Value`, `[Key:: Value]`, `(Key:: Value)`, and `#tag` tokens. Task
//!    items also recognize date-shorthand emoji (`🗓️`, `➕`, `🛫`, `⏳`, `✅`).
//! 4. The assembled [`Note`] stores all extracted data in document order.
//!
//! # Main Types
//!
//! - [`Note`] -- parsed record for one Markdown file.
//! - [`List`], [`ListItem`], [`TaskStatus`] -- ordered and unordered lists,
//!   including task items and nested child lists.
//! - [`Link`], [`LinkType`], [`LinkTarget`] -- outgoing links from Markdown
//!   `[text](target)` and Obsidian `[[target|alias]]` syntax.
//! - [`Frontmatter`], [`RawFrontmatter`] -- YAML frontmatter as structured
//!   fields or raw text.
//! - [`InlineField`], [`InlineFieldForm`], [`FieldValue`] -- body metadata
//!   parsed from `Key:: Value` syntax.
//! - [`Tag`] -- Markdown tags such as `#book` and `#projects/active`.

mod cursor;
mod lexer;
mod links;
mod lists;
mod metadata;
mod model;
mod parser;
mod tag;

pub use links::{Link, LinkTarget, LinkType};
pub use lists::{List, ListItem, TaskStatus};
pub use metadata::{
    FieldValue, Frontmatter, InlineField, InlineFieldForm, RawFrontmatter,
};
pub use model::Note;
pub use parser::parse_markdown;
pub use tag::Tag;
