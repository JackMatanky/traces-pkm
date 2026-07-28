//! Command-line interface: parses arguments and dispatches to command handlers.
//! Each command module is a thin adapter over library services; [`CliError`]
//! adds user-facing help text and error codes via `miette::Diagnostic`.

mod completions;
mod error;
pub mod init;
mod template;
mod trust;

use std::{path::PathBuf, sync::Arc};

use clap::{Parser, Subcommand};
pub use error::CliError;

use crate::{
    Cwd,
    config::{Config, ConfigService},
};

/// Reads the process current directory, wrapped for callers that need to
/// retain [`Cwd`]'s guarantees (e.g. path confinement).
///
/// # Errors
///
/// Returns [`CliError::CurrentDirectory`] when the current directory cannot
/// be read.
fn current_dir() -> Result<Cwd, CliError> {
    Cwd::new().map_err(|source| CliError::CurrentDirectory {
        source,
    })
}

/// Reads the current directory and loads its effective configuration.
///
/// # Errors
///
/// Returns [`CliError::CurrentDirectory`] when the process working
/// directory cannot be read. Returns [`CliError::ConfigLoad`] when
/// configuration discovery or construction fails.
fn load_config(service: &ConfigService) -> Result<Config, CliError> {
    let cwd = current_dir()?.into_inner();
    service.load(&cwd).map_err(|source| CliError::ConfigLoad {
        cwd,
        source,
    })
}

/// The result of a command that did not fail.
///
/// User abandonment is control flow, not a diagnostic failure. The outer CLI
/// seam maps these outcomes to process exit status.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CommandOutcome {
    /// The command completed normally.
    Completed,
    /// The user deliberately ended an interactive command.
    Aborted(UserAbort),
}

/// The user gesture that deliberately ended an interactive command.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum UserAbort {
    /// Escape cancelled the command.
    Cancelled,
    /// Ctrl-C interrupted the command.
    Interrupted,
}

/// The `traces` command-line tool.
///
/// `-i`/`--input` alone (no subcommand) is the default template dispatch,
/// handled in [`run`]; combining it with a subcommand is a clap usage error.
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
    /// Template name to instantiate — the default `traces -i <name>` dispatch,
    /// equivalent to `traces template -i <name>`. Pass with no value to
    /// trigger the interactive fuzzy picker instead.
    //
    // `Option<Option<_>>`, not `Option<PathBuf>`, so parsing can distinguish
    // "flag absent" (`None`, -> `CliError::NoCommand`) from "flag present, no
    // value" (`Some(None)`, -> the interactive picker) from "flag present,
    // value given" (`Some(Some(name))`, -> the ordinary `-i <name>` dispatch) —
    // `Option<PathBuf>` alone collapses the first two into the same `None`.
    #[arg(short = 'i', long = "input", value_name = "NAME", num_args = 0..=1)]
    #[expect(
        clippy::option_option,
        reason = "the three states are load-bearing: None (flag absent) is \
                  CliError::NoCommand, Some(None) (flag present, no value) is \
                  the interactive picker, Some(Some(name)) is the ordinary -i \
                  <name> dispatch — Option<PathBuf> alone can't distinguish \
                  the first two"
    )]
    input: Option<Option<PathBuf>>,
}

impl Cli {
    /// Parse [`Cli`] from [`std::env::args`] and run the selected command.
    ///
    /// Accepts pre-built `service`/`provider` so tests can drive real argv
    /// through a real handler with isolated stores. `provider` is `Arc`, not
    /// `&dyn`, since [`template::Template::run`] must clone it into `'static`
    /// minijinja closures.
    ///
    /// # Errors
    ///
    /// Returns [`CliError`] when the selected command fails, or
    /// [`CliError::NoCommand`] when neither a subcommand nor `-i`/`--input` was
    /// given.
    fn run(
        self,
        service: &crate::config::ConfigService,
        provider: &Arc<dyn crate::DialogProvider>,
    ) -> Result<CommandOutcome, CliError> {
        let result = match self.command {
            Some(command) => command.run(service, provider),
            None => match self.input {
                Some(Some(name)) => {
                    template::Template::new(name).run(service, provider)
                }
                Some(None) => {
                    template::Template::interactive().run(service, provider)
                }
                None => Err(CliError::NoCommand),
            },
        };
        match result {
            Ok(()) => Ok(CommandOutcome::Completed),
            Err(error) => error
                .user_abort()
                .map_or(Err(error), |abort| Ok(CommandOutcome::Aborted(abort))),
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
    /// Generate shell completions, or list available template names
    #[command(alias = "completion")]
    Completions(completions::Completions),
}

impl Commands {
    /// Route a parsed subcommand to its handler, normalising the error type.
    fn run(
        self,
        service: &crate::config::ConfigService,
        provider: &Arc<dyn crate::DialogProvider>,
    ) -> Result<(), CliError> {
        match self {
            Self::Init(args) => args.run(provider.as_ref()),
            Self::Trust(args) => args.run(service),
            Self::Template(args) => args.run(service, provider),
            Self::Completions(args) => args.run(service),
        }
    }
}

/// Entry point: parse process arguments, wire up real service implementations,
/// and run the selected command.
///
/// # Errors
///
/// Returns [`CliError`] when the selected command fails, or
/// [`CliError::NoCommand`] when neither a subcommand nor `-i`/`--input` was
/// given. Otherwise returns a [`CommandOutcome`].
#[inline]
pub fn run() -> Result<CommandOutcome, CliError> {
    let provider: Arc<dyn crate::DialogProvider> =
        Arc::new(crate::TerminalDialogProvider::new());
    Cli::parse().run(&crate::config::ConfigService::new(), &provider)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

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
            Some(Commands::Template(args)) if args.name.as_deref().and_then(Path::to_str) == Some("daily")
        ));
    }

    #[test]
    fn tmpl_alias_parses_to_the_template_subcommand() {
        let cli = Cli::try_parse_from(["traces", "tmpl", "-i", "daily"])
            .expect("parse tmpl argv");

        assert!(matches!(
            &cli.command,
            Some(Commands::Template(args)) if args.name.as_deref().and_then(Path::to_str) == Some("daily")
        ));
    }

    #[test]
    fn completion_alias_parses_to_the_completions_subcommand() {
        let cli =
            Cli::try_parse_from(["traces", "completion", "--shell", "zsh"])
                .expect("parse completion alias argv");

        assert!(matches!(&cli.command, Some(Commands::Completions(_))));
    }

    #[test]
    fn bare_input_flag_defaults_to_no_subcommand_dispatch() {
        let cli = Cli::try_parse_from(["traces", "-i", "daily"])
            .expect("parse default -i argv");

        assert!(cli.command.is_none());
        assert_eq!(cli.input, Some(Some(PathBuf::from("daily"))));
    }

    #[test]
    fn top_level_input_alongside_a_subcommand_is_rejected() {
        let result = Cli::try_parse_from(["traces", "init", "-i", "daily"]);

        assert!(result.is_err());
    }

    #[test]
    fn template_list_flag_parses_with_no_name() {
        let cli = Cli::try_parse_from(["traces", "template", "--list"])
            .expect("parse template --list argv");

        assert!(matches!(
            &cli.command,
            Some(Commands::Template(args)) if args.list && args.name.is_none()
        ));
    }

    #[test]
    fn template_list_flag_conflicts_with_input_name() {
        let result = Cli::try_parse_from([
            "traces", "template", "-i", "daily", "--list",
        ]);

        assert!(result.is_err());
    }

    mod dispatch_end_to_end {
        use std::{fs, path::Path};

        use pretty_assertions::assert_eq;
        use rstest::rstest;

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

            let provider: Arc<dyn crate::DialogProvider> =
                Arc::new(PresetDialogProvider::new());
            cli.run(&service, &provider).expect("run succeeds");

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

        #[rstest]
        #[case::bare_dash_i(vec!["traces", "-i"])]
        #[case::bare_template_subcommand(vec!["traces", "template"])]
        fn bare_input_and_bare_template_both_reach_the_interactive_picker(
            #[case] argv: Vec<&str>,
        ) {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            fs::create_dir_all(root.join(".traces")).expect("create dir");
            fs::create_dir_all(root.join("templates"))
                .expect("create empty templates dir");
            fs::write(
                root.join(".traces/config.toml"),
                "[templates]\ndirectory = \"templates\"\n",
            )
            .expect("write config file");
            let config = crate::config::LocalConfigFile::<
                crate::config::Discovered,
            >::try_new(
                root.join(".traces/config.toml")
            )
            .expect("valid local config");
            let service = ConfigService::at(
                temp.path().join("tracked-store"),
                temp.path().join("trust-store"),
            );
            service
                .trust(&TrustRequest::from(&config))
                .expect("trust project root");
            let _guard = CwdGuard::enter(&root);
            let provider: Arc<dyn crate::DialogProvider> =
                Arc::new(PresetDialogProvider::new());
            let cli = Cli::try_parse_from(argv).expect("parse argv");

            let error = cli
                .run(&service, &provider)
                .expect_err("no templates to pick from");

            assert!(matches!(error, CliError::NoTemplates));
        }
    }
}
