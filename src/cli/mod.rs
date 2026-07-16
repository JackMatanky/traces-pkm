//! Command-line interface: parses arguments and dispatches to command
//! handlers. Each command module (e.g. [`trust`]) is a thin adapter over
//! `config::ConfigService` — see its docs for the actual command logic.
//! Error types from `config` stay `thiserror`-only and unnameable outside
//! that module by design (see `config::mod`'s docs);
//! [`error::ConfigTrustCliError`] is the first (and only) place that adds
//! user-facing help text and error codes, via `miette::Diagnostic`.

mod error;
mod trust;

use clap::{Parser, Subcommand};
pub use error::ConfigTrustCliError;

/// The `traces` command-line tool.
#[derive(Debug, Parser)]
#[command(
    name = "traces",
    version,
    about = "Template-driven personal knowledge management"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Top-level `traces` subcommands.
#[derive(Debug, Subcommand)]
enum Commands {
    /// Manage trusted project roots
    Trust(trust::TrustArgs),
}

/// Parses process arguments and runs the selected command.
///
/// # Errors
///
/// Returns [`ConfigTrustCliError`] when the selected command fails.
#[inline]
pub fn run() -> Result<(), ConfigTrustCliError> {
    let cli = Cli::parse();
    let service = crate::config::ConfigService::new();
    match &cli.command {
        Commands::Trust(args) => trust::run(args, &service),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::*;

    /// Guards the `#[command(subcommand)]` wiring [`run`] depends on:
    /// `trust::run`'s own tests exercise the trust logic once parsed, but
    /// nothing else asserts that real `traces trust ...` argv actually
    /// reaches the [`Commands::Trust`] variant through [`Cli`] at all.
    #[test]
    fn trust_argv_parses_to_the_trust_subcommand() {
        let cli = Cli::try_parse_from(["traces", "trust", "some/path"])
            .expect("parse trust argv");

        assert!(matches!(cli.command, Commands::Trust(_)));
    }
}
