//! Traces: template-driven personal knowledge management.

mod config;
mod cwd;
mod dialog;
mod dirs;
mod file_store;
mod hash;
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
