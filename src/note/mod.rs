//! Markdown note domain model and parser.
//!
//! Converts Markdown source into a [`Note`] via [`parse_markdown`], and
//! stores its parsed frontmatter, lists, outgoing links, inline fields, and
//! tags.
//!
//! Main types:
//! - [`Note`]: Parsed note structure.
//! - [`List`] / [`ListItem`] / [`TaskStatus`]: Markdown lists and task items.
//! - [`Outlink`] / [`LinkType`]: Markdown and Obsidian links.
//! - [`Frontmatter`] / [`RawFrontmatter`]: YAML frontmatter.
//! - [`InlineField`] / [`InlineFieldForm`]: Dataview-style inline metadata.
//! - [`MetadataField`] / [`FieldValue`]: Key-value pairs shared by frontmatter
//!   and inline fields.
//! - [`Tag`]: Markdown tags.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "crate-internal API surface for note metadata, consumed by \
                  later tickets (#06 QueryOps template namespace)"
    )
)]

mod cursor;
mod lexer;
mod links;
mod lists;
mod metadata;
mod model;
mod parser;
mod tag;

pub(crate) use links::{LinkType, Outlink};
pub(crate) use lists::{List, ListItem, TaskStatus};
#[expect(
    unused_imports,
    reason = "note domain interface exported for later query callers"
)]
pub(crate) use metadata::{
    FieldValue, Frontmatter, InlineField, InlineFieldForm, MetadataField,
    RawFrontmatter,
};
pub(crate) use model::Note;
pub(crate) use parser::parse_markdown;
pub(crate) use tag::Tag;
