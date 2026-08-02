//! Template-driven personal knowledge management.
//!
//! The library owns CLI dispatch, configuration discovery and trust checks,
//! note indexing, template loading, rendering, and root-confined filesystem
//! writes.

mod config;
mod cwd;
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

pub(crate) use cwd::Cwd;
#[cfg(test)]
pub(crate) use cwd::CwdGuard;
pub use dialog::{
    DialogError, DialogProvider, PresetDialogProvider, TerminalDialogProvider,
};
pub(crate) use file_store::{
    FileStateStore, FileStateStoreError, FileStoreCleanMode,
};
pub(crate) use hash::{Blake3FileHash, Blake3PathHash};
