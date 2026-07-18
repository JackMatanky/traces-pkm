//! `traces trust` command: trust, untrust, show, list, and clean local config
//! trust state.
//!
//! Thin adapter over [`ConfigService`]: this module only parses args, resolves
//! the current directory, and formats output — trust decisions live in
//! `config::trust::ConfigTrust` (see its docs).

use std::path::{Path, PathBuf};

use clap::{ArgGroup, Args, Subcommand};

use super::error::ConfigTrustCliError;
use crate::{
    Cwd,
    config::{ConfigService, ResolvedTrustTarget},
};

/// `traces trust [PATH]` / `traces trust --show` / `traces trust --untrust`.
///
/// `args_conflicts_with_subcommands` is what lets `list`/`clean` disambiguate
/// from a positional `path`: clap tries to match the first free-standing
/// argument against a [`TrustAction`] variant name before falling back to
/// treating it as `path`, and rejects combining the two (`traces trust list
/// some/path` is a clap usage error, not "trust the path some/path while
/// also listing").
#[derive(Debug, Args)]
#[command(
    args_conflicts_with_subcommands = true,
    group(ArgGroup::new("mode").args(["show", "untrust"]).multiple(false))
)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "clap flag structs model independent CLI switches directly"
)]
pub(super) struct TrustArgs {
    #[command(subcommand)]
    action: Option<TrustAction>,
    /// Show resolved config trust status instead of changing it
    #[arg(long)]
    show: bool,
    /// Remove the resolved config from the trust store
    #[arg(long)]
    untrust: bool,
    /// Apply trust, untrust, or show to descendant configs too
    #[arg(long)]
    all: bool,
    /// Directory or .traces/config.toml to trust (defaults to the current
    /// directory)
    path: Option<PathBuf>,
}

/// Nested `traces trust` subcommands.
#[derive(Debug, Subcommand)]
enum TrustAction {
    /// List all trusted directories
    List,
    /// Remove stale trust entries
    Clean,
}

impl TrustArgs {
    /// Dispatches `traces trust` to its default action, flag mode, or a
    /// nested subcommand.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigTrustCliError`] when the selected action fails.
    #[inline]
    pub(super) fn run(
        self,
        service: &ConfigService,
    ) -> Result<(), ConfigTrustCliError> {
        match self.action {
            Some(TrustAction::List) => list(service),
            Some(TrustAction::Clean) => clean(service),
            None if self.show => self.show(service),
            None if self.untrust => self.untrust(service),
            None => self.trust(service),
        }
    }

    /// Trusts the resolved config target or targets.
    fn trust(
        &self,
        service: &ConfigService,
    ) -> Result<(), ConfigTrustCliError> {
        for target in self.targets()? {
            service.trust(target.as_trust_target()).map_err(|source| {
                ConfigTrustCliError::Trust {
                    root: target.root().to_path_buf(),
                    source: Box::new(source),
                }
            })?;
            eprintln!("trusted {}", target.root().display());
        }
        Ok(())
    }

    /// Removes the resolved config target or targets from the trust store.
    fn untrust(
        &self,
        service: &ConfigService,
    ) -> Result<(), ConfigTrustCliError> {
        for target in self.targets()? {
            service.untrust(target.root()).map_err(|source| {
                ConfigTrustCliError::Untrust {
                    root: target.root().to_path_buf(),
                    source: Box::new(source),
                }
            })?;
            eprintln!("untrusted {}", target.root().display());
        }
        Ok(())
    }

    /// Prints the resolved config target statuses, one per line, to stdout.
    #[allow(
        clippy::print_stdout,
        reason = "trust --show's output is data meant to be piped, not \
                  diagnostic text — see the print_stderr precedent this \
                  mirrors"
    )]
    fn show(&self, service: &ConfigService) -> Result<(), ConfigTrustCliError> {
        for target in self.targets()? {
            let state = service
                .is_trusted(target.root(), target.config_file())
                .map_err(|source| ConfigTrustCliError::Show {
                    root: target.root().to_path_buf(),
                    source: Box::new(source),
                })?;
            println!("{}\t{}", target.config_file().display(), state.as_str());
        }
        Ok(())
    }

    /// Resolve one or many trust targets from the current directory and
    /// optional user-provided path.
    fn targets(&self) -> Result<Vec<ResolvedTrustTarget>, ConfigTrustCliError> {
        let cwd = current_dir()?;
        ConfigService::resolve_trust_targets(
            &cwd,
            self.path.as_deref(),
            self.all,
        )
        .map_err(|source| ConfigTrustCliError::TargetResolve {
            path: self.path.as_deref().unwrap_or(Path::new(".")).to_path_buf(),
            source: Box::new(source),
        })
    }
}

fn current_dir() -> Result<PathBuf, ConfigTrustCliError> {
    Cwd::new().map(Cwd::into_inner).map_err(|source| {
        ConfigTrustCliError::TargetResolve {
            path: PathBuf::from("."),
            source: Box::new(source),
        }
    })
}

/// Prints every currently trusted directory, one per line, to stdout.
#[allow(
    clippy::print_stdout,
    reason = "trust list's output is data meant to be piped, not diagnostic \
              text — see the print_stderr precedent this mirrors"
)]
fn list(service: &ConfigService) -> Result<(), ConfigTrustCliError> {
    let roots =
        service.list_trusted().map_err(|source| ConfigTrustCliError::List {
            source: Box::new(source),
        })?;
    for root in &roots {
        println!("{}", root.display());
    }
    Ok(())
}

/// Removes dangling trust entries and reports how many were removed.
fn clean(service: &ConfigService) -> Result<(), ConfigTrustCliError> {
    let removed = service.clean_trusted_store().map_err(|source| {
        ConfigTrustCliError::Clean {
            source: Box::new(source),
        }
    })?;
    match removed {
        1 => eprintln!("removed 1 stale trust entry"),
        n => eprintln!("removed {n} stale trust entries"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use clap::Parser;

    use super::*;

    /// Wraps [`TrustArgs`] in a minimal top-level parser so its
    /// `args_conflicts_with_subcommands` disambiguation can be exercised
    /// with [`Parser::try_parse_from`] — [`clap::Args`] types don't parse
    /// standalone.
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        trust: TrustArgs,
    }

    fn parse(args: &[&str]) -> TrustArgs {
        TestCli::try_parse_from(
            std::iter::once("test").chain(args.iter().copied()),
        )
        .expect("parse trust args")
        .trust
    }

    mod parsing {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn bare_trust_has_no_path_and_no_action() {
            let args = parse(&[]);

            assert!(args.path.is_none());
            assert!(args.action.is_none());
        }

        #[test]
        fn trust_with_a_path_sets_path_and_no_action() {
            let args = parse(&["some/path"]);

            assert_eq!(args.path, Some(PathBuf::from("some/path")));
            assert!(args.action.is_none());
        }

        #[test]
        fn trust_list_is_the_list_action_not_a_path_named_list() {
            let args = parse(&["list"]);

            assert!(matches!(args.action, Some(TrustAction::List)));
            assert!(args.path.is_none());
        }

        #[test]
        fn trust_clean_is_the_clean_action_not_a_path_named_clean() {
            let args = parse(&["clean"]);

            assert!(matches!(args.action, Some(TrustAction::Clean)));
            assert!(args.path.is_none());
        }

        #[test]
        fn show_and_untrust_modes_parse() {
            let show = parse(&["--show", "some/path"]);
            let untrust = parse(&["--untrust", "some/path"]);

            assert!(show.show);
            assert_eq!(show.path, Some(PathBuf::from("some/path")));
            assert!(untrust.untrust);
            assert_eq!(untrust.path, Some(PathBuf::from("some/path")));
        }

        #[test]
        fn all_mode_parse() {
            let args = parse(&["--all", "some/path"]);

            assert!(args.all);
            assert_eq!(args.path, Some(PathBuf::from("some/path")));
        }

        #[test]
        fn show_and_untrust_conflict() {
            let result =
                TestCli::try_parse_from(["test", "--show", "--untrust"]);

            assert!(result.is_err());
        }

        #[test]
        fn combining_an_action_with_a_path_is_rejected() {
            let result = TestCli::try_parse_from(["test", "list", "some/path"]);

            assert!(result.is_err());
        }
    }

    mod handlers {
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::CwdGuard;

        fn service(temp: &Path) -> ConfigService {
            ConfigService::at(
                temp.join("tracked-store"),
                temp.join("trust-store"),
            )
        }

        fn trust_args(path: Option<PathBuf>) -> TrustArgs {
            TrustArgs {
                action: None,
                show: false,
                untrust: false,
                all: false,
                path,
            }
        }

        fn action_args(action: TrustAction) -> TrustArgs {
            TrustArgs {
                action: Some(action),
                show: false,
                untrust: false,
                all: false,
                path: None,
            }
        }

        fn create_config(root: &Path) -> PathBuf {
            let config_file = root.join(".traces/config.toml");
            fs::create_dir_all(config_file.parent().expect("config parent"))
                .expect("create config parent");
            fs::write(&config_file, "").expect("write config file");
            config_file
        }

        #[test]
        fn trust_with_no_path_trusts_the_discovered_project_root() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            let cwd = root.join("notes/daily");
            fs::create_dir_all(&cwd).expect("create nested cwd");
            let config_file = create_config(&root);
            let service = service(temp.path());
            let _guard = CwdGuard::enter(&cwd);

            trust_args(None).run(&service).expect("trust cwd");

            assert_eq!(service.list_trusted().expect("list trusted"), vec![
                root.canonicalize().expect("canonicalize root")
            ]);
            assert_eq!(
                service.is_trusted(&root, &config_file).expect("check trust"),
                crate::config::TrustState::Trusted
            );
        }

        #[test]
        fn trust_accepts_a_config_file_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            fs::create_dir_all(&root).expect("create project dir");
            let config_file = create_config(&root);
            let service = service(temp.path());

            trust_args(Some(config_file.to_path_buf()))
                .run(&service)
                .expect("trust config file");

            assert_eq!(service.list_trusted().expect("list trusted"), vec![
                root.canonicalize().expect("canonicalize root")
            ]);
        }

        #[test]
        fn trust_missing_config_returns_target_resolve_error() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            fs::create_dir_all(&root).expect("create project dir");
            let service = service(temp.path());

            let error = trust_args(Some(root))
                .run(&service)
                .expect_err("missing config fails trust");

            assert!(matches!(error, ConfigTrustCliError::TargetResolve { .. }));
        }

        #[test]
        fn show_checks_status_without_changing_trust_store() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            fs::create_dir_all(&root).expect("create project dir");
            create_config(&root);
            let service = service(temp.path());
            let mut args = trust_args(Some(root));
            args.show = true;

            args.run(&service).expect("show trust status");

            assert!(service.list_trusted().expect("list trusted").is_empty());
        }

        #[test]
        fn untrust_removes_the_resolved_root_and_companion() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            fs::create_dir_all(&root).expect("create project dir");
            let config_file = create_config(&root);
            let service = service(temp.path());
            trust_args(Some(root.to_path_buf()))
                .run(&service)
                .expect("trust root");
            let mut args = trust_args(Some(root.to_path_buf()));
            args.untrust = true;

            args.run(&service).expect("untrust root");

            assert_eq!(
                service.is_trusted(&root, &config_file).expect("check trust"),
                crate::config::TrustState::Untrusted
            );
        }

        #[test]
        fn all_mode_trusts_descendant_configs() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let parent = temp.path().join("parent");
            let child = parent.join("child");
            fs::create_dir_all(&child).expect("create child dir");
            create_config(&parent);
            create_config(&child);
            let service = service(temp.path());
            let mut args = trust_args(Some(parent));
            args.all = true;

            args.run(&service).expect("trust all configs");

            assert_eq!(service.list_trusted().expect("list trusted").len(), 2);
        }

        #[test]
        fn list_succeeds_against_an_empty_trust_store() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let service = service(temp.path());

            action_args(TrustAction::List)
                .run(&service)
                .expect("list empty trust store");
        }

        #[test]
        fn clean_removes_a_stale_root_and_its_companion() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            fs::create_dir_all(&root).expect("create project dir");
            create_config(&root);
            let service = service(temp.path());
            trust_args(Some(root.to_path_buf()))
                .run(&service)
                .expect("trust root");
            fs::remove_dir_all(&root).expect("delete project dir");

            action_args(TrustAction::Clean)
                .run(&service)
                .expect("clean trust store");

            assert!(service.list_trusted().expect("list trusted").is_empty());
        }

        #[test]
        fn clean_on_an_empty_trust_store_does_not_error() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let service = service(temp.path());

            action_args(TrustAction::Clean)
                .run(&service)
                .expect("clean empty trust store");
        }
    }
}
