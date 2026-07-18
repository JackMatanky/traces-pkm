//! Command-line interface: parses arguments and dispatches to command
//! handlers. Each command module is a thin adapter over library services.
//! Error types from those services stay `thiserror`-only and unnameable
//! outside their modules by design; [`error`] is the first place that adds
//! user-facing help text and error codes, via `miette::Diagnostic`.

mod error;
pub mod init;
mod template;
mod trust;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
pub use error::{ConfigCliError, ConfigInitCliError, ConfigTrustCliError};

/// The `traces` command-line tool.
///
/// `args_conflicts_with_subcommands` lets the top-level `-i`/`--input` flag
/// disambiguate from a subcommand: passing a subcommand and `-i` together is
/// a clap usage error, and `-i` alone (no subcommand) is the default
/// template dispatch handled in [`run`].
#[derive(Debug, Parser)]
#[command(
    name = "traces",
    version,
    about = "Template-driven personal knowledge management",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    /// Template name to instantiate — the default `traces -i <name>`
    /// dispatch, equivalent to `traces template -i <name>`.
    #[arg(short = 'i', long = "input", value_name = "NAME")]
    input: Option<PathBuf>,
}

/// Top-level `traces` subcommands.
#[derive(Debug, Subcommand)]
enum Commands {
    /// Initialise local traces configuration
    Init(init::Init),
    /// Manage trusted project roots
    Trust(trust::TrustArgs),
    /// Render a template and write it to disk
    #[command(alias = "tmpl")]
    Template(template::TemplateArgs),
}

/// Parses process arguments and runs the selected command.
///
/// # Errors
///
/// Returns [`ConfigCliError`] when the selected command fails, or
/// [`ConfigCliError::NoCommand`] when neither a subcommand nor `-i`/
/// `--input` was given.
#[inline]
pub fn run() -> Result<(), ConfigCliError> {
    let cli = Cli::parse();
    let service = crate::config::ConfigService::new();
    let provider = crate::dialog::TerminalDialogProvider::new();
    match cli.command {
        Some(Commands::Init(args)) => args.run(&provider).map_err(Into::into),
        Some(Commands::Trust(args)) => args.run(&service).map_err(Into::into),
        Some(Commands::Template(args)) => {
            args.run(&service).map_err(Into::into)
        }
        None => match cli.input {
            Some(name) => template::TemplateArgs::new(name)
                .run(&service)
                .map_err(Into::into),
            None => Err(ConfigCliError::NoCommand),
        },
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;
    use pretty_assertions::assert_eq;

    use super::*;

    /// Guards the `#[command(subcommand)]` wiring [`run`] depends on:
    /// `trust::run`'s own tests exercise the trust logic once parsed, but
    /// nothing else asserts that real `traces trust ...` argv actually
    /// reaches the [`Commands::Trust`] variant through [`Cli`] at all.
    #[test]
    fn trust_argv_parses_to_the_trust_subcommand() {
        let cli = Cli::try_parse_from(["traces", "trust", "some/path"])
            .expect("parse trust argv");

        assert!(matches!(cli.command, Some(Commands::Trust(_))));
    }

    #[test]
    fn init_argv_parses_to_the_init_subcommand() {
        let cli =
            Cli::try_parse_from(["traces", "init"]).expect("parse init argv");

        assert!(matches!(cli.command, Some(Commands::Init(_))));
    }

    #[test]
    fn template_argv_parses_to_the_template_subcommand() {
        let cli = Cli::try_parse_from(["traces", "template", "-i", "daily"])
            .expect("parse template argv");

        assert!(matches!(
            &cli.command,
            Some(Commands::Template(args)) if args.name == PathBuf::from("daily")
        ));
    }

    #[test]
    fn tmpl_alias_parses_to_the_template_subcommand() {
        let cli = Cli::try_parse_from(["traces", "tmpl", "-i", "daily"])
            .expect("parse tmpl argv");

        assert!(matches!(
            &cli.command,
            Some(Commands::Template(args)) if args.name == PathBuf::from("daily")
        ));
    }

    #[test]
    fn bare_input_flag_defaults_to_no_subcommand_dispatch() {
        let cli = Cli::try_parse_from(["traces", "-i", "daily"])
            .expect("parse default -i argv");

        assert!(cli.command.is_none());
        assert_eq!(cli.input, Some(PathBuf::from("daily")));
    }

    #[test]
    fn top_level_input_alongside_a_subcommand_is_rejected() {
        let result = Cli::try_parse_from(["traces", "init", "-i", "daily"]);

        assert!(result.is_err());
    }
}
