//! Markdown Note Metadata domain types: `Note`, `Frontmatter`, `MetadataField`,
//! `FieldValue`, `List`, `ListItem`, `Outlink`, `Tag`, and `CodeRegion`.

mod metadata;
mod note;
mod structure;

pub(crate) use metadata::{
    FieldSource, FieldValue, Frontmatter, InlineField, InlineFieldForm,
    MetadataField, RawFrontmatter,
};
pub(crate) use note::Note;
pub(crate) use structure::{
    CodeRegion, LinkType, List, ListItem, Outlink, Tag, TaskStatus,
};
