//! Markdown structure extracted from notes.
//!
//! This module stores links, lists, task state, tags, and code ranges produced
//! by the markdown parser.

mod code_region;
mod links;
mod lists;
mod tag;

pub(crate) use code_region::CodeRegion;
pub(crate) use links::{LinkType, Outlink};
pub(crate) use lists::{List, ListItem, TaskStatus};
pub(crate) use tag::Tag;
