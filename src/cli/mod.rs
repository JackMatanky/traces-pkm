//! CLI entry point and command dispatch.
//!
//! Parses process arguments via [`clap`], routes to the selected subcommand (or
//! the default `-i` template dispatch), and translates domain errors into
//! [`CliError`] diagnostics.
//!
//! Key types:
//!
//! - [`CommandOutcome`]: top-level result of a successful command.
//! - [`UserAbort`]: deliberate user cancellation or interruption.
//! - [`CliError`]: unified diagnostic error for all CLI operations.
//!
//! Submodules contain command-specific logic; this module stays limited to
//! argument flow and shared helpers.

mod completions;
mod cwd;
mod error;
mod index;
pub mod init;
mod list;
mod table;
mod task;
mod template;
mod tracked;
mod trust;
mod untrust;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use clap::{Parser, Subcommand};
use cwd::Cwd;
#[cfg(test)]
pub(crate) use cwd::CwdGuard;
pub use error::{CliError, CliResult};

use crate::{
    DialogProvider,
    config::{Config, ConfigService, DiscoveryScope, TrustRequests},
    index::{FileIndex, IndexerService},
    query::{
        QueryError, QueryRecordSet, QueryRequest, QueryService, SourceSelector,
    },
    schema::{SchemaService, warn_schema_construction_diagnostics},
};

/// Top-level result of a successful CLI command.
///
/// Returned by [`run`] to distinguish normal completion from deliberate user
/// cancellation or interruption. The process exit code is `0` for both
/// variants; callers that need different behavior can match on the variant.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CommandOutcome {
    /// The command completed its work without interruption.
    Completed,
    /// The user cancelled or interrupted an interactive prompt before the
    /// command could finish.
    Aborted(UserAbort),
}

/// User gesture that ended an interactive command.
///
/// Extracted from dialog-layer cancellation or interruption errors in the error
/// source chain.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum UserAbort {
    /// The user pressed Escape to cancel an interactive prompt.
    Cancelled,
    /// The user pressed Ctrl-C to interrupt an interactive prompt.
    Interrupted,
}

/// Root command-line parser for `traces`.
///
/// Dispatches to a subcommand when one is present, or to the default
/// `-i`/`--input` template instantiation path when the flag is given. Returns
/// [`CliError::NoCommand`] when neither is provided.
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
    /// Initialize local traces configuration in the current directory.
    Init(init::Init),
    /// Grant trust to one or more project roots.
    Trust(trust::Trust),
    /// Revoke trust from one or more project roots.
    Untrust(untrust::Untrust),
    /// Build or rebuild the persisted [`FileIndex`].
    Index(index::Index),
    /// Query pages and print matching file paths as a Markdown bullet list.
    List(list::List),
    /// Query pages and print matching records as a Markdown table.
    Table(table::Table),
    /// Query tasks and print matching checkbox lines.
    Task(task::Task),
    /// Render a template and write it to disk, or list available templates.
    #[command(alias = "tmpl")]
    Template(template::Template),
    /// Generate shell completion scripts or list available template names.
    #[command(alias = "completion")]
    Completions(completions::Completions),
    /// Inspect or clean the tracked-config store.
    Tracked(tracked::Tracked),
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
    ) -> CliResult {
        match self {
            Self::Init(args) => args.run(provider.as_ref()),
            Self::Trust(args) => args.run(service),
            Self::Untrust(args) => args.run(service),
            Self::Index(args) => args.run(service),
            Self::List(args) => args.run(service),
            Self::Table(args) => args.run(service),
            Self::Task(args) => args.run(service),
            Self::Template(args) => args.run(service, provider),
            Self::Completions(args) => args.run(service),
            Self::Tracked(args) => args.run(service),
        }
    }
}

/// Parses process arguments and runs the selected `traces` command.
///
/// Yields [`CommandOutcome::Completed`] on success, or
/// [`CommandOutcome::Aborted`] if the user cancelled or interrupted an
/// interactive prompt.
///
/// # Errors
///
/// - [`CliError::NoCommand`] when neither a subcommand nor `-i`/`--input` is
///   provided.
/// - [`CliError`] for any command-level failure (config load, indexing, query,
///   template rendering, trust store, etc.).
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
/// Returns [`CliError::CurrentDirectory`] if the directory does not exist or
/// cannot be accessed.
fn current_dir() -> Result<Cwd, CliError> {
    Cwd::new().map_err(|source| CliError::CurrentDirectory {
        source,
    })
}

/// Reads the current directory and loads its effective configuration.
///
/// # Errors
///
/// - [`CliError::CurrentDirectory`] if reading the current directory fails.
/// - [`CliError::ConfigLoad`] if configuration discovery or loading fails.
fn load_config(service: &ConfigService) -> Result<Config, CliError> {
    let cwd = current_dir()?.into_inner();
    service.load(&cwd).map_err(|source| CliError::ConfigLoad {
        cwd,
        source,
    })
}

/// Refreshes `root`'s [`FileIndex`] and returns page-level records selected by
/// `from`, filtered by `filters` (composed as AND) and optionally sorted.
///
/// Shared by [`list::List`] and [`table::Table`].
///
/// # Errors
///
/// Returns [`CliError::Index`] if refreshing the [`FileIndex`] fails, or
/// [`CliError::Query`] if any filter expression or the sort field path is
/// malformed.
fn refresh_page_query(
    config: &Config,
    from: Option<&str>,
    filters: &[String],
    sort: Option<&str>,
    descending: bool,
) -> Result<QueryRecordSet, CliError> {
    let root = config.root();
    let index = Arc::new(
        IndexerService::new(root).with_config(config).refresh().map_err(
            |source| CliError::Index {
                root: root.to_path_buf(),
                source,
            },
        )?,
    );
    let source = parse_source(config, from)?;
    let has_classes = source.has_classes();
    let mut request = QueryRequest::pages(source);
    for expr in filters {
        request = request
            .filter(expr)
            .map_err(|error| query_error(root, error.into()))?;
    }
    if let Some(path) = sort {
        request = request
            .sort(path, descending)
            .map_err(|error| query_error(root, error.into()))?;
    }
    execute_query_request(config, &index, request, has_classes)
}

/// Refreshes `root`'s [`FileIndex`] and returns task-level records selected by
/// `from`, filtered by `filters` (composed as AND).
///
/// Shared by [`task::Task`].
///
/// # Errors
///
/// Returns [`CliError::Index`] if refreshing the [`FileIndex`] fails, or
/// [`CliError::Query`] if any filter expression is malformed.
fn refresh_task_query(
    config: &Config,
    from: Option<&str>,
    filters: &[String],
) -> Result<QueryRecordSet, CliError> {
    let root = config.root();
    let index = Arc::new(
        IndexerService::new(root).with_config(config).refresh().map_err(
            |source| CliError::Index {
                root: root.to_path_buf(),
                source,
            },
        )?,
    );
    let source = parse_source(config, from)?;
    let has_classes = source.has_classes();
    let mut request = QueryRequest::tasks(source);
    for expr in filters {
        request = request
            .filter(expr)
            .map_err(|error| query_error(root, error.into()))?;
    }
    execute_query_request(config, &index, request, has_classes)
}

fn parse_source(
    config: &Config,
    from: Option<&str>,
) -> Result<SourceSelector, CliError> {
    let root = config.root();
    SourceSelector::parse(from.unwrap_or_default())
        .map_err(|source| query_error(root, source))
}

fn execute_query_request(
    config: &Config,
    index: &Arc<FileIndex>,
    request: QueryRequest,
    has_classes: bool,
) -> Result<QueryRecordSet, CliError> {
    let service = QueryService::new(config.schemas().class_field_name());
    if has_classes {
        let schema_service = load_schema_service(config)?;
        Ok(service.with_class_expander(&schema_service).execute(index, request))
    } else {
        Ok(service.execute(index, request))
    }
}

fn load_schema_service(config: &Config) -> Result<SchemaService, CliError> {
    let root = config.root();
    let schema_directory =
        config.resolved_schema_directory().map_err(|source| {
            CliError::SchemaDirectory {
                root: root.to_path_buf(),
                source,
            }
        })?;
    let construction =
        SchemaService::load_verbose(&schema_directory).map_err(|error| {
            CliError::SchemaQuery {
                root: root.to_path_buf(),
                source: error,
            }
        })?;
    warn_schema_construction_diagnostics(&construction);
    Ok(construction.service)
}

/// Wraps a [`QueryError`] as a [`CliError::Query`] against `root`.
fn query_error(root: &Path, source: QueryError) -> CliError {
    CliError::Query {
        root: root.to_path_buf(),
        source,
    }
}

/// Resolves trust subjects for `path` (or the current directory when absent) at
/// `all`'s scope.
///
/// Shared by [`trust::Trust`] and [`untrust::Untrust`].
///
/// # Errors
///
/// - [`CliError::CurrentDirectory`] if no path was provided and reading the
///   current directory fails.
/// - [`CliError::TrustTargetResolve`] if resolving trust targets fails.
fn resolve_trust_subjects(
    service: &ConfigService,
    path: Option<&Path>,
    all: bool,
) -> Result<TrustRequests, CliError> {
    let cwd;
    let path = if let Some(path) = path {
        path
    } else {
        cwd = current_dir()?;
        cwd.as_ref()
    };
    let scope = if all {
        DiscoveryScope::LocalSubtree
    } else {
        DiscoveryScope::NearestLocal
    };
    service.trust_requests(path, scope).map_err(|source| {
        CliError::TrustTargetResolve {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) mod fixtures {
        use std::{
            fs,
            path::{Path, PathBuf},
            sync::Arc,
        };

        use super::*;
        use crate::{
            cli::CwdGuard,
            config::{
                ConfigService, Discovered, LocalConfigFile, TrustRequest,
            },
            dialog::PresetDialogProvider,
        };

        pub(crate) fn service(temp: &Path) -> ConfigService {
            ConfigService::at(
                temp.join("tracked-store"),
                temp.join("trust-store"),
            )
        }

        pub(crate) fn create_config(root: &Path, directory: &str) -> PathBuf {
            let config_file = root.join(".traces/config.toml");
            fs::create_dir_all(config_file.parent().expect("config parent"))
                .expect("create config parent");
            fs::write(
                &config_file,
                format!("[templates]\ndirectory = \"{directory}\"\n"),
            )
            .expect("write config file");
            config_file
        }

        pub(crate) fn create_empty_config(root: &Path) -> PathBuf {
            let config_file = root.join(".traces/config.toml");
            fs::create_dir_all(config_file.parent().expect("config parent"))
                .expect("create config parent");
            fs::write(&config_file, "").expect("write config file");
            config_file
        }

        pub(crate) fn trust_config(
            service: &ConfigService,
            config_path: &Path,
        ) {
            let config = LocalConfigFile::<Discovered>::try_new(
                config_path.to_path_buf(),
            )
            .expect("valid local config");
            service
                .trust(&TrustRequest::from(&config))
                .expect("trust project config");
        }

        pub(crate) fn create_trusted_project(
            service: &ConfigService,
            root: &Path,
        ) {
            fs::create_dir_all(root).expect("create project dir");
            let config_file = create_config(root, "templates");
            trust_config(service, &config_file);
        }

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
        fn list_argv_maps_to_list_subcommand() {
            let cli = Cli::try_parse_from(["traces", "list"])
                .expect("parse list argv");

            assert!(matches!(cli.command, Some(Commands::List(_))));
        }

        #[test]
        fn table_argv_maps_to_table_subcommand() {
            let cli = Cli::try_parse_from([
                "traces",
                "table",
                "--column",
                "file.path",
            ])
            .expect("parse table argv");

            assert!(matches!(cli.command, Some(Commands::Table(_))));
        }

        #[test]
        fn table_argv_without_a_column_flag_fails_to_parse() {
            let error = Cli::try_parse_from(["traces", "table"])
                .expect_err("table requires at least one --column");

            assert_eq!(
                error.kind(),
                clap::error::ErrorKind::MissingRequiredArgument
            );
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
            cli::CwdGuard,
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

    /// End-to-end coverage exercised together against one shared project
    /// instead of each command's isolated per-behavior tests:
    ///
    /// - Indexing.
    /// - Page and task CLI queries.
    /// - Template `query`/`tasks` `QueryOps`.
    /// - Derived inlinks.
    /// - Diagnostics.
    ///
    /// See `cli::list`, `cli::table`, `cli::task`, and
    /// `template::engine::query` for exhaustive per-feature coverage.
    ///
    /// `list`/`table`/`task` write their primary output to stdout, which
    /// this module doesn't capture (see [`super::list::List::render`]'s
    /// docs for why). Their CLI-equivalent assertions below drive
    /// [`FileIndex`] directly instead, the same shared interface those
    /// commands' `render`/`lines` methods call. [`Cli::run`] dispatch is
    /// still exercised directly wherever the observable is on the
    /// [`Result`] itself, in the diagnostics tests below and every
    /// `parse`/`dispatch_end_to_end` test above.
    mod query_workflows {
        use std::{
            fs,
            path::{Path, PathBuf},
            sync::Arc,
        };

        use miette::Diagnostic as _;
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::{
            cli::CwdGuard,
            config::{
                Config, ConfigService, Discovered, LocalConfigFile,
                TrustRequest,
            },
            dialog::PresetDialogProvider,
            query::{QueryError, QueryRequestError, SourceSelector},
            template::{
                TemplateError, TemplatePathInput, TemplateService, WriteMode,
                WriteOutcome,
            },
        };

        /// Writes a trusted project under `root/project` with two Notes
        /// exercising every seam this suite covers: `#book` tags, a
        /// `rating` frontmatter field, one markdown task, and a wikilink
        /// from `hyperion.md` to `dune.md` so `dune.md` gets a derived
        /// inlink. `dune`'s stem is unique in the project, so the wikilink
        /// resolves unambiguously regardless of proximity tie-breaking.
        ///
        /// Returns the trusted [`ConfigService`] (for [`Cli::run`]
        /// dispatch) and the project root (for direct [`FileIndex`]/
        /// [`TemplateService`] calls).
        fn seed_book_project(root: &Path) -> (ConfigService, PathBuf) {
            let project = root.join("project");
            fs::create_dir_all(project.join(".traces"))
                .expect("create .traces dir");
            fs::create_dir_all(project.join("templates"))
                .expect("create templates dir");
            fs::create_dir_all(project.join("books"))
                .expect("create books dir");
            fs::write(
                project.join(".traces/config.toml"),
                "[templates]\ndirectory = \"templates\"\n",
            )
            .expect("write config file");
            fs::write(
                project.join("books/dune.md"),
                "---\nrating: 9\n---\n#book\n\n- [ ] read part two\n",
            )
            .expect("write dune.md");
            fs::write(
                project.join("books/hyperion.md"),
                "---\nrating: 7\n---\n#book\n\nSee [[dune]] for comparison.\n",
            )
            .expect("write hyperion.md");
            let config = LocalConfigFile::<Discovered>::try_new(
                project.join(".traces/config.toml"),
            )
            .expect("valid local config");
            let service = ConfigService::at(
                root.join("tracked-store"),
                root.join("trust-store"),
            );
            service
                .trust(&TrustRequest::from(&config))
                .expect("trust project root");
            (service, project)
        }

        /// Renders `source` as a one-off template under `project`'s
        /// `templates` directory and returns its preview content, mirroring
        /// `traces template -i report --dry-run` without going through
        /// [`ConfigService`]/trust (this only needs [`Config::for_test`],
        /// matching [`crate::template::service`]'s own render tests).
        fn render_query_template(project: &Path, source: &str) -> String {
            let templates_dir = project.join("templates");
            fs::write(templates_dir.join("report.md"), source)
                .expect("write report.md");
            let config = Config::for_test(
                project.to_path_buf(),
                Some(templates_dir),
                None,
                project.to_path_buf(),
            );
            let service = TemplateService::new(
                &config,
                Arc::new(PresetDialogProvider::new()),
            )
            .expect("valid test schema directory");
            let input = TemplatePathInput::parse(Path::new("report"))
                .expect("valid template input");
            let outcome = service
                .render_to_file(&input, None, WriteMode::DryRun)
                .expect("template renders");
            let previewed = match outcome {
                WriteOutcome::Previewed(content) => Some(content),
                WriteOutcome::Written(_) => None,
            };
            previewed.expect("dry run always previews, never writes")
        }

        #[test]
        fn indexing_then_page_and_task_queries_observe_the_same_project_state()
        {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (service, project) = seed_book_project(temp.path());
            let _guard = CwdGuard::enter(&project);

            let index_outcome = Cli::try_parse_from(["traces", "index"])
                .expect("parse index argv")
                .run(&service, Arc::new(PresetDialogProvider::new()))
                .expect("index succeeds");
            assert_eq!(index_outcome, CommandOutcome::Completed);

            let list_outcome = Cli::try_parse_from([
                "traces", "list", "--from", "#book", "--sort", "rating",
                "--order", "desc",
            ])
            .expect("parse list argv")
            .run(&service, Arc::new(PresetDialogProvider::new()))
            .expect("list succeeds");
            assert_eq!(list_outcome, CommandOutcome::Completed);
            let list_index = Arc::new(
                IndexerService::new(&project).refresh().expect("refresh index"),
            );
            let _list = QueryService::new("class")
                .execute(
                    &list_index,
                    QueryRequest::pages(
                        SourceSelector::parse("#book").expect("valid source"),
                    ),
                )
                .sort("rating", true)
                .expect("valid sort")
                .list("file.path")
                .expect("valid list");

            let table_outcome = Cli::try_parse_from([
                "traces",
                "table",
                "--column",
                "file.name",
                "--column",
                "rating",
            ])
            .expect("parse table argv")
            .run(&service, Arc::new(PresetDialogProvider::new()))
            .expect("table succeeds");
            assert_eq!(table_outcome, CommandOutcome::Completed);
            let table_index = Arc::new(
                IndexerService::new(&project).refresh().expect("refresh index"),
            );
            let _table = QueryService::new("class")
                .execute(&table_index, QueryRequest::pages(SourceSelector::All))
                .table(&["Name", "Rating"], &["file.name", "rating"])
                .expect("valid table");

            let task_outcome = Cli::try_parse_from(["traces", "task"])
                .expect("parse task argv")
                .run(&service, Arc::new(PresetDialogProvider::new()))
                .expect("task succeeds");
            assert_eq!(task_outcome, CommandOutcome::Completed);
            let task_index = Arc::new(
                IndexerService::new(&project).refresh().expect("refresh index"),
            );
            let _tasks = QueryService::new("class")
                .execute(&task_index, QueryRequest::tasks(SourceSelector::All))
                .task_list()
                .expect("valid task_list");
        }

        #[test]
        fn template_query_ops_render_identically_to_the_equivalent_file_index_query()
         {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (_service, project) = seed_book_project(temp.path());
            let indexer = IndexerService::new(&project);
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            let rendered = render_query_template(
                &project,
                "{{ query.from(\"#book\").sort(\"rating\", \
                 true).table([\"Name\", \"Rating\"], [\"file.name\", \
                 \"rating\"]) }}",
            );

            let index = Arc::new(
                IndexerService::new(&project).refresh().expect("refresh index"),
            );
            let expected = QueryService::new("class")
                .execute(
                    &index,
                    QueryRequest::pages(
                        SourceSelector::parse("#book").expect("valid source"),
                    ),
                )
                .sort("rating", true)
                .expect("valid sort")
                .table(&["Name", "Rating"], &["file.name", "rating"])
                .expect("valid table");

            assert_eq!(rendered, expected);
        }

        #[test]
        fn derived_inlinks_are_queryable_from_page_queries_and_templates() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (_service, project) = seed_book_project(temp.path());
            let indexer = IndexerService::new(&project);
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            let index = Arc::new(
                IndexerService::new(&project).refresh().expect("refresh index"),
            );
            let inlinks = QueryService::new("class")
                .execute(
                    &index,
                    QueryRequest::pages(
                        SourceSelector::parse("books/").expect("valid source"),
                    ),
                )
                .sort("file.name", false)
                .expect("valid sort")
                .list("inlinks")
                .expect("valid list");
            assert_eq!(inlinks, "- books/hyperion.md\n- \n");

            let rendered = render_query_template(
                &project,
                "{{ query.from(\"books/\").sort(\"file.name\", \
                 false).list(\"inlinks\") }}",
            );
            assert_eq!(rendered, inlinks);
        }

        #[test]
        fn unknown_field_path_and_unparsable_filter_surface_actionable_cli_diagnostics()
         {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (service, project) = seed_book_project(temp.path());
            let _guard = CwdGuard::enter(&project);

            let bad_field =
                Cli::try_parse_from(["traces", "list", "--sort", "file.nam"])
                    .expect("parse list argv")
                    .run(&service, Arc::new(PresetDialogProvider::new()))
                    .expect_err("unknown field path fails");
            assert!(matches!(
                &bad_field,
                CliError::Query {
                    source: QueryError::Request(QueryRequestError::FieldPath(error)),
                    ..
                } if error.suggestion.as_deref() == Some("file.name")
            ));
            assert_eq!(
                bad_field.code().map(|code| code.to_string()),
                Some("traces::cli::query::failed".to_owned())
            );
            assert!(bad_field.help().is_some());

            let bad_filter = Cli::try_parse_from([
                "traces",
                "list",
                "--where",
                "not a valid expression",
            ])
            .expect("parse list argv")
            .run(&service, Arc::new(PresetDialogProvider::new()))
            .expect_err("unparsable filter fails");
            assert!(matches!(bad_filter, CliError::Query {
                source: QueryError::Request(QueryRequestError::Syntax(_)),
                ..
            }));
        }

        #[test]
        fn template_render_errors_identify_the_failing_template_and_line_through_cli_dispatch()
         {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (service, project) = seed_book_project(temp.path());
            fs::write(
                project.join("templates/report.md"),
                "line one\n{{ query.from().sort(\"nope.bad\") }}\n",
            )
            .expect("write report.md");
            let _guard = CwdGuard::enter(&project);

            let error = Cli::try_parse_from([
                "traces",
                "template",
                "-i",
                "report",
                "--dry-run",
            ])
            .expect("parse template argv")
            .run(&service, Arc::new(PresetDialogProvider::new()))
            .expect_err("malformed query call fails to render");

            assert!(matches!(error, CliError::TemplateInstantiate {
                source: TemplateError::Render { .. },
                ..
            }));
            let help = error.help().map(|h| h.to_string()).unwrap_or_default();
            assert!(
                help.contains("report.md:2:16"),
                "expected the failing template name, line, and column in help \
                 text, got: {help}"
            );
        }
    }

    mod run {
        use std::{fs, sync::Arc};

        use super::*;
        use crate::{
            cli::CwdGuard, config::ConfigService, dialog::PresetDialogProvider,
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

    mod current_dir_and_config {
        use super::*;
        use crate::{cli::CwdGuard, config::ConfigLoadError};

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
