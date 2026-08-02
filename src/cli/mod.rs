//! CLI entry point and dispatch.
//!
//! Owns the clap parser, default `-i` template dispatch, command routing, and
//! [`CliError`] export. Submodules contain command-specific behavior so this
//! module stays limited to argument flow and shared helpers.

mod completions;
mod error;
mod index;
pub mod init;
mod task;
mod template;
mod trust;
mod untrust;

use std::{path::PathBuf, sync::Arc};

use clap::{Parser, Subcommand};
pub use error::CliError;

use crate::{
    Cwd, DialogProvider,
    config::{Config, ConfigService},
};

/// Outcome of a successful CLI command.
///
/// Distinguishes normal completion from deliberate user cancellation or
/// interruption.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CommandOutcome {
    /// The command completed normally.
    Completed,
    /// The user deliberately ended an interactive command.
    Aborted(UserAbort),
}

/// User gesture that ended an interactive command.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum UserAbort {
    /// Escape cancelled the command.
    Cancelled,
    /// Ctrl-C interrupted the command.
    Interrupted,
}

/// Root command-line parser for `traces`.
///
/// When no subcommand is specified, `-i`/`--input` dispatches to template
/// instantiation.
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
    /// Optional template name for the default `traces -i <name>` dispatch.
    ///
    /// Passing `-i` with no value opens the interactive fuzzy picker.
    //
    // `Option<Option<_>>`, not `Option<PathBuf>`, so parsing can distinguish
    // "flag absent" (`None`, -> `CliError::NoCommand`) from "flag present, no
    // value" (`Some(None)`, -> the interactive picker) from "flag present,
    // value given" (`Some(Some(name))`, -> the ordinary `-i <name>` dispatch).
    // `Option<PathBuf>` alone collapses the first two into the same `None`.
    #[arg(short = 'i', long = "input", value_name = "NAME", num_args = 0..=1)]
    #[expect(
        clippy::option_option,
        reason = "the three states are load-bearing: None (flag absent) is \
                  CliError::NoCommand, Some(None) (flag present, no value) is \
                  the interactive picker, Some(Some(name)) is the ordinary -i \
                  <name> dispatch; Option<PathBuf> alone can't distinguish \
                  the first two"
    )]
    input: Option<Option<PathBuf>>,
}

impl Cli {
    /// Runs the parsed command with the given services.
    ///
    /// # Errors
    ///
    /// - [`CliError::NoCommand`] if neither a subcommand nor `-i`/`--input` was
    ///   provided.
    /// - [`CliError`] if command execution fails.
    fn run(
        self,
        service: &ConfigService,
        provider: Arc<dyn DialogProvider>,
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
    /// Initialize local traces configuration.
    Init(init::Init),
    /// Manage trusted project roots.
    Trust(trust::Trust),
    /// Revoke trust from project roots.
    Untrust(untrust::Untrust),
    /// Build or rebuild the persisted [`FileIndex`](crate::index::FileIndex).
    Index(index::Index),
    /// Run a task-level query and print task output.
    Task(task::Task),
    /// Render a template and write it to disk.
    #[command(alias = "tmpl")]
    Template(template::Template),
    /// Generate shell completions or list available template names.
    #[command(alias = "completion")]
    Completions(completions::Completions),
}

impl Commands {
    /// Routes a parsed subcommand to its handler.
    ///
    /// # Errors
    ///
    /// - Any [`CliError`] returned by the selected subcommand.
    fn run(
        self,
        service: &ConfigService,
        provider: Arc<dyn DialogProvider>,
    ) -> Result<(), CliError> {
        match self {
            Self::Init(args) => args.run(provider.as_ref()),
            Self::Trust(args) => args.run(service),
            Self::Untrust(args) => args.run(service),
            Self::Index(args) => args.run(service),
            Self::Task(args) => args.run(service),
            Self::Template(args) => args.run(service, provider),
            Self::Completions(args) => args.run(service),
        }
    }
}

/// Parses process arguments and runs the selected `traces` command.
///
/// On success, yields the top-level [`CommandOutcome`].
///
/// # Errors
///
/// - [`CliError::NoCommand`] if neither a subcommand nor `-i`/`--input` was
///   provided.
/// - [`CliError`] if command execution fails.
#[inline]
pub fn run() -> Result<CommandOutcome, CliError> {
    let provider: Arc<dyn DialogProvider> =
        Arc::new(crate::TerminalDialogProvider::new());
    Cli::parse().run(&ConfigService::new(), provider)
}

/// Reads the process current directory as a [`Cwd`].
///
/// # Errors
///
/// - [`CliError::CurrentDirectory`] if the current directory cannot be read.
fn current_dir() -> Result<Cwd, CliError> {
    Cwd::new().map_err(|source| CliError::CurrentDirectory {
        source,
    })
}

/// Reads the current directory and loads its effective configuration.
///
/// # Errors
///
/// - [`CliError::CurrentDirectory`] if the current directory cannot be read.
/// - [`CliError::ConfigLoad`] if configuration discovery or loading fails.
fn load_config(service: &ConfigService) -> Result<Config, CliError> {
    let cwd = current_dir()?.into_inner();
    service.load(&cwd).map_err(|source| CliError::ConfigLoad {
        cwd,
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    mod fixtures {
        use std::{fs, path::Path, sync::Arc};

        use super::*;
        use crate::{
            CwdGuard,
            config::{ConfigService, TrustRequest},
            dialog::PresetDialogProvider,
        };

        pub(super) struct CancellingSelect;
        impl DialogProvider for CancellingSelect {
            fn is_interactive(&self) -> bool {
                true
            }

            fn text(
                &self,
                _: &str,
                _: Option<&str>,
            ) -> Result<String, crate::DialogError> {
                Ok(String::new())
            }

            fn confirm(
                &self,
                _: &str,
                _: Option<bool>,
            ) -> Result<bool, crate::DialogError> {
                Ok(true)
            }

            fn select(
                &self,
                _: &str,
                _: &[String],
            ) -> Result<usize, crate::DialogError> {
                Err(crate::DialogError::UserCancelled)
            }

            fn multi_select(
                &self,
                _: &str,
                _: &[String],
            ) -> Result<Vec<usize>, crate::DialogError> {
                Err(crate::DialogError::UserCancelled)
            }
        }

        /// Parses `argv` and drives it through [`Cli::run`] against an
        /// isolated, trusted project, writing (and returning the contents
        /// of) `daily.md`.
        pub(super) fn dispatch_argv_and_read_output(
            argv: &[&str],
            root: &Path,
        ) -> String {
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

            let provider: Arc<dyn DialogProvider> =
                Arc::new(PresetDialogProvider::new());
            cli.run(&service, provider).expect("run succeeds");

            fs::read_to_string(project.join("daily.md"))
                .expect("read written output")
        }
    }

    mod parse {
        use std::path::Path;

        use pretty_assertions::assert_eq;

        use super::*;

        /// Guards the `#[command(subcommand)]` wiring [`run`] depends on:
        /// `trust::run`'s own tests exercise the trust logic once parsed, but
        /// nothing else asserts that real `traces trust ...` argv actually
        /// reaches the [`Commands::Trust`] variant through [`Cli`] at all.
        #[test]
        fn trust_argv_maps_to_trust_subcommand() {
            let cli = Cli::try_parse_from(["traces", "trust", "some/path"])
                .expect("parse trust argv");

            assert!(matches!(cli.command, Some(Commands::Trust(_))));
        }
        #[test]
        fn untrust_argv_maps_to_untrust_subcommand() {
            let cli = Cli::try_parse_from(["traces", "untrust", "some/path"])
                .expect("parse untrust argv");

            assert!(matches!(cli.command, Some(Commands::Untrust(_))));
        }

        #[test]
        fn init_argv_maps_to_init_subcommand() {
            let cli = Cli::try_parse_from(["traces", "init"])
                .expect("parse init argv");

            assert!(matches!(cli.command, Some(Commands::Init(_))));
        }

        #[test]
        fn index_argv_maps_to_index_subcommand() {
            let cli = Cli::try_parse_from(["traces", "index"])
                .expect("parse index argv");

            assert!(matches!(cli.command, Some(Commands::Index(_))));
        }

        #[test]
        fn template_argv_maps_to_template_subcommand() {
            let cli =
                Cli::try_parse_from(["traces", "template", "-i", "daily"])
                    .expect("parse template argv");

            assert!(matches!(
                &cli.command,
                Some(Commands::Template(args))
                    if args.name().and_then(Path::to_str) == Some("daily")
            ));
        }

        #[test]
        fn template_positional_argv_maps_to_template_subcommand() {
            let cli = Cli::try_parse_from(["traces", "template", "daily"])
                .expect("parse template positional argv");

            assert!(matches!(
                &cli.command,
                Some(Commands::Template(args))
                    if args.name().and_then(Path::to_str) == Some("daily")
            ));
        }

        #[test]
        fn tmpl_alias_maps_to_template_subcommand() {
            let cli = Cli::try_parse_from(["traces", "tmpl", "-i", "daily"])
                .expect("parse tmpl argv");

            assert!(matches!(
                &cli.command,
                Some(Commands::Template(args))
                    if args.name().and_then(Path::to_str) == Some("daily")
            ));
        }

        #[test]
        fn completion_alias_maps_to_completions_subcommand() {
            let cli =
                Cli::try_parse_from(["traces", "completion", "--shell", "zsh"])
                    .expect("parse completion alias argv");

            assert!(matches!(&cli.command, Some(Commands::Completions(_))));
        }

        #[test]
        fn completions_shell_flag_accepts_bash() {
            let cli = Cli::try_parse_from([
                "traces",
                "completions",
                "--shell",
                "bash",
            ])
            .expect("parse completions --shell bash");

            assert!(matches!(&cli.command, Some(Commands::Completions(_))));
        }

        #[test]
        fn bare_input_flag_has_no_subcommand() {
            let cli = Cli::try_parse_from(["traces", "-i", "daily"])
                .expect("parse default -i argv");

            assert!(cli.command.is_none());
            assert_eq!(cli.input, Some(Some(PathBuf::from("daily"))));
        }

        #[test]
        fn bare_input_flag_without_value_sets_input_to_some_none() {
            let cli = Cli::try_parse_from(["traces", "-i"])
                .expect("parse bare -i flag");

            assert!(cli.command.is_none());
            assert_eq!(cli.input, Some(None));
        }

        #[test]
        fn no_args_has_no_command_and_no_input() {
            let cli =
                Cli::try_parse_from(["traces"]).expect("parse with no args");

            assert!(cli.command.is_none());
            assert_eq!(cli.input, None);
        }

        #[test]
        fn rejects_input_alongside_a_subcommand() {
            let result = Cli::try_parse_from(["traces", "init", "-i", "daily"]);

            assert!(result.is_err());
        }

        #[test]
        fn template_list_flag_has_no_name() {
            let cli = Cli::try_parse_from(["traces", "template", "--list"])
                .expect("parse template --list argv");

            assert!(matches!(
                &cli.command,
                Some(Commands::Template(args))
                    if args.list && args.name.is_none()
            ));
        }

        #[test]
        fn template_list_flag_conflicts_with_input_name() {
            let result = Cli::try_parse_from([
                "traces", "template", "-i", "daily", "--list",
            ]);

            assert!(result.is_err());
        }
    }

    mod dispatch_end_to_end {
        use std::fs;

        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;
        use crate::{
            CwdGuard,
            config::{ConfigService, TrustRequest},
            dialog::PresetDialogProvider,
        };

        #[test]
        fn all_three_invocation_forms_produce_identical_output() {
            let form_a = tempfile::tempdir().expect("create temp dir");
            let form_b = tempfile::tempdir().expect("create temp dir");
            let form_c = tempfile::tempdir().expect("create temp dir");

            let via_template = fixtures::dispatch_argv_and_read_output(
                &["traces", "template", "-i", "daily"],
                form_a.path(),
            );
            let via_tmpl = fixtures::dispatch_argv_and_read_output(
                &["traces", "tmpl", "-i", "daily"],
                form_b.path(),
            );
            let via_default = fixtures::dispatch_argv_and_read_output(
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
            let provider: Arc<dyn DialogProvider> =
                Arc::new(PresetDialogProvider::new());
            let cli = Cli::try_parse_from(argv).expect("parse argv");

            let error = cli
                .run(&service, provider)
                .expect_err("no templates to pick from");

            assert!(matches!(error, CliError::NoTemplates));
        }
    }

    mod run {
        use std::{fs, sync::Arc};

        use super::*;
        use crate::{
            CwdGuard, config::ConfigService, dialog::PresetDialogProvider,
        };

        #[test]
        fn no_command_returns_no_command_error() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let service = ConfigService::at(
                temp.path().join("tracked-store"),
                temp.path().join("trust-store"),
            );
            let provider: Arc<dyn DialogProvider> =
                Arc::new(PresetDialogProvider::new());

            let error = Cli {
                command: None,
                input: None,
            }
            .run(&service, provider)
            .expect_err("no command should fail");

            assert!(matches!(error, CliError::NoCommand));
        }

        #[test]
        fn user_cancelled_during_template_picker_returns_aborted() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            let config_file = root.join(".traces/config.toml");
            fs::create_dir_all(config_file.parent().expect("config parent"))
                .expect("create config parent");
            fs::write(&config_file, "[templates]\ndirectory = \"templates\"\n")
                .expect("write config file");
            fs::create_dir_all(root.join("templates"))
                .expect("create templates dir");
            fs::write(root.join("templates/daily.md"), "content")
                .expect("write template");
            let service = ConfigService::at(
                temp.path().join("tracked-store"),
                temp.path().join("trust-store"),
            );
            let config = crate::config::LocalConfigFile::<
                crate::config::Discovered,
            >::try_new(config_file)
            .expect("valid local config");
            service
                .trust(&crate::config::TrustRequest::from(&config))
                .expect("trust project root");
            let _guard = CwdGuard::enter(&root);

            let outcome = Cli {
                command: None,
                input: Some(None),
            }
            .run(&service, Arc::new(fixtures::CancellingSelect))
            .expect("user abort is not an error");

            assert_eq!(outcome, CommandOutcome::Aborted(UserAbort::Cancelled));
        }
    }

    mod helpers {
        use super::*;
        use crate::{CwdGuard, config::ConfigLoadError};

        #[test]
        fn current_dir_reads_process_cwd() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let cwd = temp.path().canonicalize().expect("canonicalize");
            let guard = CwdGuard::enter(&cwd);
            let dir = current_dir().expect("current_dir succeeds");

            assert_eq!(dir.as_ref(), cwd);
            drop(guard);
        }

        #[test]
        fn load_config_fails_without_a_config_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let service = ConfigService::at(
                temp.path().join("tracked-store"),
                temp.path().join("trust-store"),
            );
            let _guard = CwdGuard::enter(temp.path());

            let error =
                load_config(&service).expect_err("no config should fail");

            assert!(matches!(error, CliError::ConfigLoad {
                source: ConfigLoadError::Discovery(_),
                ..
            }));
        }
    }
}
