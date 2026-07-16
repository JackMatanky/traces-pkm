//! Binary entry point: parses CLI arguments and dispatches to
//! [`traces_pkm::cli::run`]. Logic lives in the lib crate (see
//! `src/lib.rs`); this stays minimal (`proj-lib-main-split`).
//!
//! Returns `miette::Result<()>` rather than a bare `Result` or
//! `std::process::ExitCode`: this is the standard miette idiom for
//! rendering a returned error's diagnostic (code, help text, source
//! chain) automatically — no hand-rolled printing loop needed, and no
//! `std::process::exit` call (denied by this workspace's lints).

fn main() -> miette::Result<()> {
    traces_pkm::cli::run()?;
    Ok(())
}
