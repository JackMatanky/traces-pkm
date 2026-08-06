//! Template-driven personal knowledge management.
//!
//! `traces-pkm` provides CLI workflow dispatch, configuration resolution and
//! trust verification, note indexing and querying, template execution, and
//! root-confined filesystem writes.
//!
//! # Core Subsystems
//!
//! - [`cli`] - Command-line interface definitions, argument parsing, and
//!   command execution flow.
//! - `config` - Project configuration loading, discovery, TOML parsing, and
//!   trust verification.
//! - `index` - Persistent file index, note parsing, link graph construction,
//!   and query execution.
//! - `note` - Markdown note parsing, YAML frontmatter extraction, and task
//!   processing.
//! - `template` - Template loading, path expansion, custom engine bindings, and
//!   note rendering.

mod config;
mod dialog;
mod dirs;
mod file_name;
mod file_store;
mod hash;
mod index;
mod note;
mod path;
mod template;

pub mod cli;

#[cfg(any(test, feature = "test-utils"))]
pub use config::{Config, ConfigService};
pub use dialog::{
    DialogError, DialogProvider, PresetDialogProvider, TerminalDialogProvider,
};
pub(crate) use file_store::{
    FileStateStore, FileStateStoreError, FileStoreCleanMode,
};
pub(crate) use hash::{Blake3FileHash, Blake3PathHash};
#[cfg(any(test, feature = "test-utils"))]
pub use index::{
    FileIndex, FileIndexError, FileRecord, IndexRecord, QueryOutcome,
    QuerySource,
};
#[cfg(any(test, feature = "test-utils"))]
pub use note::{
    FieldValue, Frontmatter, InlineField, InlineFieldForm, Link, LinkTarget,
    LinkType, List, ListItem, Note, RawFrontmatter, Tag, TaskStatus,
    parse_markdown,
};
#[cfg(any(test, feature = "test-utils"))]
pub use template::{
    RenderFailureKind, TemplateError, TemplatePathError, TemplatePathInput,
    TemplateService, WriteMode, WriteOutcome, classify_render_error,
};
