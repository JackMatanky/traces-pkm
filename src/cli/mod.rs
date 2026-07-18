//! Command-line interface: parses arguments and dispatches to command
//! handlers. Each command module is a thin adapter over library services.
//! Error types from those services stay `thiserror`-only and unnameable
//! outside their modules by design; [`error`] is the first place that adds
//! user-facing help text and error codes, via `miette::Diagnostic`.

mod error;
pub mod init;
mod trust;

use clap::{Parser, Subcommand};
pub use error::{ConfigCliError, ConfigInitCliError, ConfigTrustCliError};

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
    /// Initialise local traces configuration
    Init(init::Init),
    /// Manage trusted project roots
    Trust(trust::TrustArgs),
}

/// Parses process arguments and runs the selected command.
///
/// # Errors
///
/// Returns [`ConfigCliError`] when the selected command fails.
#[inline]
pub fn run() -> Result<(), ConfigCliError> {
    let cli = Cli::parse();
    let service = crate::config::ConfigService::new();
    let provider = crate::dialog::TerminalDialogProvider::new();
    match cli.command {
        Commands::Init(args) => args.run(&provider).map_err(Into::into),
        Commands::Trust(args) => args.run(&service).map_err(Into::into),
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

    #[test]
    fn init_argv_parses_to_the_init_subcommand() {
        let cli =
            Cli::try_parse_from(["traces", "init"]).expect("parse init argv");

        assert!(matches!(cli.command, Commands::Init(_)));
    }
}
