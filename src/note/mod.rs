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
//! - [`InlineField`], [`InlineFieldForm`], [`MetadataField`], and
//!   [`FieldValue`] - Represent Dataview-compatible metadata.
//! - [`Tag`] - Stores Markdown tags.
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

pub(crate) use links::{Link, LinkTarget, LinkType};
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
