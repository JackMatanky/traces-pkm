//! Traces: template-driven personal knowledge management.

pub mod cli;
mod cwd;
mod config;
// `dialog` stays `pub`: its doctests (`traces_pkm::dialog::...`) compile as
// external crates, so they need real crate-external reachability, not just
// `cli`/`main.rs`'s crate-internal access.
pub mod dialog;
mod hash;
mod template;
pub(crate) use cwd::Cwd;

#[cfg(test)]
pub(crate) use cwd::CwdGuard;
pub(crate) use hash::{Blake3FileHash, Blake3PathHash};
