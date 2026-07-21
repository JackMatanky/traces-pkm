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

impl Cli {
    /// Parse [`Cli`] from [`std::env::args`] and run the selected command.
    ///
    /// Accepts pre-constructed [`ConfigService`] and [`DialogProvider`] so
    /// that tests can drive real argv through to a real handler call with
    /// isolated stores, without touching the process's OS-correct
    /// trust/tracked-config paths.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigCliError`] when the selected command fails, or
    /// [`ConfigCliError::NoCommand`] when neither a subcommand nor
    /// `-i`/`--input` was given.
    fn run(
        self,
        service: &crate::config::ConfigService,
        provider: &dyn crate::DialogProvider,
    ) -> Result<(), ConfigCliError> {
        match self.command {
            Some(cmd) => cmd.run(service, provider),
            None => match self.input {
                Some(name) => template::Template::new(name)
                    .run(service)
                    .map_err(Into::into),
                None => Err(ConfigCliError::NoCommand),
            },
        }
    }
}

/// Top-level `traces` subcommands.
#[derive(Debug, Subcommand)]
enum Commands {
    /// Initialise local traces configuration
    Init(init::Init),
    /// Manage trusted project roots
    Trust(trust::Trust),
    /// Render a template and write it to disk
    #[command(alias = "tmpl")]
    Template(template::Template),
}

impl Commands {
    /// Route a parsed subcommand to its handler.
    ///
    /// Each `Commands` variant wraps a struct that owns the command-specific
    /// args and implements its own `run()`; this method selects the right one
    /// and normalises the error type.  A new subcommand adds one arm here and
    /// one variant in the enum — no other dispatch site needs updating.
    fn run(
        self,
        service: &crate::config::ConfigService,
        provider: &dyn crate::DialogProvider,
    ) -> Result<(), ConfigCliError> {
        match self {
            Self::Init(args) => args.run(provider).map_err(Into::into),
            Self::Trust(args) => args.run(service).map_err(Into::into),
            Self::Template(args) => args.run(service).map_err(Into::into),
        }
    }
}

/// Entry point: parse process arguments, wire up real service
/// implementations, and run the selected command.
///
/// # Errors
///
/// Returns [`ConfigCliError`] when the command fails or no command was given.
#[inline]
pub fn run() -> Result<(), ConfigCliError> {
    Cli::parse().run(
        &crate::config::ConfigService::new(),
        &crate::TerminalDialogProvider::new(),
    )
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
            Some(Commands::Template(args)) if args.name.to_str() == Some("daily")
        ));
    }

    #[test]
    fn tmpl_alias_parses_to_the_template_subcommand() {
        let cli = Cli::try_parse_from(["traces", "tmpl", "-i", "daily"])
            .expect("parse tmpl argv");

        assert!(matches!(
            &cli.command,
            Some(Commands::Template(args)) if args.name.to_str() == Some("daily")
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

    mod dispatch_end_to_end {
        use std::{fs, path::Path};

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::{
            CwdGuard,
            config::{ConfigService, TrustRequest},
            dialog::PresetDialogProvider,
        };

        /// Parses `argv` and drives it through [`Cli::run`] against an
        /// isolated, trusted project, writing (and returning the contents
        /// of) `daily.md`.
        ///
        /// Exercises the exact same path a real `traces` invocation takes
        /// — real argv parsing through to a real handler call — without
        /// touching the process's real OS-correct trust/tracked-config
        /// stores, proving all three invocation forms produce identical
        /// output by construction (same [`Cli::run`] call, same args) and
        /// by observation (the file each writes matches).
        fn dispatch_argv_and_read_output(argv: &[&str], root: &Path) -> String {
            let cli = Cli::try_parse_from(argv).expect("parse argv");
            let service = ConfigService::at(
                root.join("tracked-store"),
                root.join("trust-store"),
            );
            let project = root.join("project");
            fs::create_dir_all(project.join(".traces"))
                .expect("create .traces dir");
            fs::create_dir_all(project.join("templates"))
                .expect("create templates dir");
            fs::write(
                project.join(".traces/config.toml"),
                "[templates]\ndirectory = \"templates\"\n",
            )
            .expect("write config file");
            fs::write(
                project.join("templates/daily.md"),
                "{% for n in [1, 2, 3] %}{{ n }}{% endfor %}",
            )
            .expect("write template");
            let config = crate::config::LocalConfigFile::<
                crate::config::Discovered,
            >::try_new(
                project.join(".traces/config.toml")
            )
            .expect("valid local config");
            service
                .trust(&TrustRequest::from(&config))
                .expect("trust project root");
            let _guard = CwdGuard::enter(&project);

            cli.run(&service, &PresetDialogProvider::new())
                .expect("run succeeds");

            fs::read_to_string(project.join("daily.md"))
                .expect("read written output")
        }

        #[test]
        fn all_three_invocation_forms_produce_identical_output() {
            let form_a = tempfile::tempdir().expect("create temp dir");
            let form_b = tempfile::tempdir().expect("create temp dir");
            let form_c = tempfile::tempdir().expect("create temp dir");

            let via_template = dispatch_argv_and_read_output(
                &["traces", "template", "-i", "daily"],
                form_a.path(),
            );
            let via_tmpl = dispatch_argv_and_read_output(
                &["traces", "tmpl", "-i", "daily"],
                form_b.path(),
            );
            let via_default = dispatch_argv_and_read_output(
                &["traces", "-i", "daily"],
                form_c.path(),
            );

            assert_eq!(via_template, "123");
            assert_eq!(via_tmpl, via_template);
            assert_eq!(via_default, via_template);
        }
    }
}
