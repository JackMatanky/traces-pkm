//! Traces: template-driven personal knowledge management.

pub mod cli;
mod config;
mod cwd;
mod dialog;
pub use dialog::{
    DialogError, DialogProvider, PresetDialogProvider, TerminalDialogProvider,
};
mod hash;
pub(crate) use cwd::Cwd;
#[cfg(test)]
pub(crate) use cwd::CwdGuard;
pub(crate) use hash::{Blake3FileHash, Blake3PathHash};
