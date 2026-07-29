//! Markdown note domain model and parser.
//!
//! Owns parsed Note structure, Dataview-compatible metadata values, Markdown
//! links, lists, tags, code ranges, and conversion from Markdown source into a
//! [`Note`].
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "crate-internal API surface for note metadata, consumed by \
                  later tickets (#06 QueryOps template namespace)"
    )
)]

mod byte;
mod code;
mod domain;
mod inline;
mod links;
mod lists;
mod metadata;
mod parser;
mod tag;

pub(crate) use code::CodeRegion;
pub(crate) use domain::Note;
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
pub(crate) use parser::parse_markdown;
pub(crate) use tag::Tag;
