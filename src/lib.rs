//! Traces: template-driven personal knowledge management.

pub mod cli;
pub(crate) mod cwd;
// `dialog` stays `pub`: its doctests (`traces_pkm::dialog::...`) compile as
// external crates, so they need real crate-external reachability, not just
// `cli`/`main.rs`'s crate-internal access.
// `config`'s discover/build/resolve pipeline (the `builder`, `candidate`,
// `tracker`, `domain`, and parts of `discovery`) is not yet wired to any CLI
// command. `cli::trust` only reaches `ConfigService`/`ConfigTrust`/`ConfigFileStore`;
// `cli::init` writes raw config via `RawConfig` directly, bypassing the builder.
// The pipeline is complete and unit-tested but has no production caller yet —
// keeping it rather than delete-re-add when a future `init`/`render` command
// needs it.
#[expect(
    dead_code,
    unused_imports,
    reason = "config build pipeline (discover → builder → merge) awaits its CLI \
              consumer — warns when no longer needed"
)]
mod config;
pub mod dialog;
mod hash;
