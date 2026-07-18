//! Traces: template-driven personal knowledge management.

pub mod cli;
pub(crate) mod cwd;
// `dialog` stays `pub`: its doctests (`traces_pkm::dialog::...`) compile as
// external crates, so they need real crate-external reachability, not just
// `cli`/`main.rs`'s crate-internal access.
pub mod dialog;
// `config`'s discover/build/track/resolve pipeline (and the read side of
// trust, `is_trusted`/`TrustState`) predates its CLI consumer: only
// `traces trust <path>`/`list`/`clean` are wired into `cli::trust` so far
// (see `cli/trust.rs`). The rest — `ConfigService::discover`/`build`/
// `is_trusted`/`list_tracked`/`clean_tracked_store`, the whole `builder`/
// `candidate`/`tracker` subsystems, and most of `discovery`/`domain` — is
// complete, unit-tested groundwork awaiting a future `init`/`render`-style
// command, not dead weight to delete. Tightening visibility to `pub(crate)`
// (see `config/mod.rs`'s doc) is what surfaced this: it was previously
// masked by everything being `pub`, which exempts an item from dead_code
// analysis regardless of whether anything in this crate actually calls it.
#[allow(
    dead_code,
    unused_imports,
    reason = "discover/build/track/resolve pipeline awaits its CLI consumer \
              (future init/render command) — see this mod's doc comment for \
              the full explanation. unused_imports covers config/mod.rs's \
              re-exports of that same pipeline's types, kept ready rather \
              than removed-then-re-added once a consumer needs to name them \
              from cli"
)]
mod config;
mod hash;
