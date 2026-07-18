//! Traces: template-driven personal knowledge management.

pub mod cli;
mod cwd;
// `dialog` stays `pub`: its doctests (`traces_pkm::dialog::...`) compile as
// external crates, so they need real crate-external reachability, not just
// `cli`/`main.rs`'s crate-internal access.
// `config`'s tracked-config-store admin surface (`ConfigService::list_tracked`/
// `clean_tracked_store`, and the `store`/`tracker` internals they delegate
// to) has no CLI consumer yet — no `traces config list`/`clean` command
// exists, unlike trust's equivalent (`cli::trust`'s `list`/`clean`, wired to
// `list_trusted`/`clean_trusted_store`). `TrustTarget::Directory` and
// `TrustTarget::for_root`/`resolve_trust_target` (the pre-config-exists
// trust path) are similarly unused: `cli::trust` always resolves a config
// file first. The re-exported error types (`ConfigBuilderError`,
// `DiscoveryError`, `TrustError`, `TrustState`, `TrustTarget`,
// `TrustTargetError`) are never named directly even by `cli::template`,
// which — like `cli::trust`/`cli::init` — type-erases sources behind
// `Box<dyn StdError>` (see `cli::error`'s module docs); kept in case a
// future consumer needs the concrete type.
#[expect(
    dead_code,
    unused_imports,
    reason = "config admin surface (tracked-store list/clean, directory-only \
              trust) and several re-exported error types await a CLI consumer \
              that names them directly — warns when no longer needed"
)]
mod config;
pub mod dialog;
mod hash;
mod template;

pub(crate) use cwd::Cwd;
#[cfg(test)]
pub(crate) use cwd::CwdGuard;
pub(crate) use hash::{Blake3FileHash, Blake3PathHash};
