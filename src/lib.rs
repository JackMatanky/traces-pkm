//! Traces: template-driven personal knowledge management.

pub mod cli;
mod config;
mod cwd;
mod dialog;
mod dirs;
mod hash;
pub use dialog::{
    DialogError, DialogProvider, PresetDialogProvider, TerminalDialogProvider,
};
pub(crate) mod file_store;
pub(crate) use cwd::Cwd;
#[cfg(test)]
pub(crate) use cwd::CwdGuard;
pub(crate) use file_store::{
    FileStateStore, FileStateStoreError, FileStoreCleanMode,
};
pub(crate) use hash::{Blake3FileHash, Blake3PathHash};
