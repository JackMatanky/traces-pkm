//! Markdown note domain model and parser.
//!
//! [`parse_markdown`] converts Markdown source into a [`Note`] containing the
//! frontmatter, lists, outgoing links, inline fields, and tags used by the
//! index and query layers.
//!
//! # Main Types
//!
//! - [`Note`] - Represents a parsed note record.
//! - [`List`], [`ListItem`], and [`TaskStatus`] - Represent Markdown lists and
//!   task items.
//! - [`Link`], [`LinkType`], and [`LinkTarget`] - Represent Markdown links,
//!   Obsidian wikilinks, and a link's split path/anchor target.
//! - [`Frontmatter`] and [`RawFrontmatter`] - Preserve YAML metadata.
//! - [`InlineField`], [`InlineFieldForm`], and [`FieldValue`] - Represent
//!   inline-field metadata parsed from note body text.
//! - [`Tag`] - Stores Markdown tags.
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
pub(crate) use metadata::FieldKey;
pub use metadata::{
    FieldValue, Frontmatter, InlineField, InlineFieldForm, RawFrontmatter,
};
pub use model::Note;
pub use parser::parse_markdown;
pub use tag::Tag;
