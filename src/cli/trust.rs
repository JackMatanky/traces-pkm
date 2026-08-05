//! Trust-store mutation and inspection.
//!
//! Handles `traces trust` by resolving a directory or `.traces/config.toml` to
//! trust subjects, then listing, cleaning, showing, or recording trusted
//! workspace roots.

use std::path::PathBuf;

use clap::{Args, Subcommand};

use super::error::CliError;
use crate::config::{ConfigService, TrustRequest};

/// Command-line arguments for `traces trust`.
#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub(super) struct Trust {
    #[command(subcommand)]
    action: Option<TrustAction>,
    /// Show resolved configuration trust status instead of changing it.
    #[arg(long)]
    show: bool,
    /// Apply trust or status checks to descendant configurations too.
    #[arg(long)]
    all: bool,
    /// Directory or `.traces/config.toml` to trust.
    ///
    /// Defaults to the current directory.
    path: Option<PathBuf>,
}

/// Nested `traces trust` subcommands.
#[derive(Debug, Subcommand)]
enum TrustAction {
    /// List all trusted directories.
    List,
    /// Remove stale trust entries.
    Clean,
}

impl Trust {
    /// Runs `traces trust` with the selected trust action.
    ///
    /// # Errors
    ///
    /// - [`CliError`] if the dispatched trust action fails.
    #[inline]
    pub(super) fn run(self, service: &ConfigService) -> Result<(), CliError> {
        match self.action {
            Some(TrustAction::List) => Self::list(service),
            Some(TrustAction::Clean) => Self::clean(service),
            None if self.show => self.show(service),
            None => self.trust(service),
        }
    }

    /// Prints all trusted directories to stdout, one per line.
    ///
    /// # Errors
    ///
    /// - [`CliError::TrustList`] if reading the trust store fails.
    #[expect(
        clippy::print_stdout,
        reason = "trust list's output is data meant to be piped, not \
                  diagnostic text; see the print_stderr precedent this mirrors"
    )]
    fn list(service: &ConfigService) -> Result<(), CliError> {
        let roots: Vec<PathBuf> =
            service.list_trusted().map_err(|source| CliError::TrustList {
                source,
            })?;
        for root in &roots {
            println!("{}", root.display());
        }
        Ok(())
    }

    /// Removes dangling trust entries from the trust store.
    ///
    /// # Errors
    ///
    /// - [`CliError::TrustClean`] if cleaning the trust store fails.
    fn clean(service: &ConfigService) -> Result<(), CliError> {
        let removed = service.clean_trusted_store().map_err(|source| {
            CliError::TrustClean {
                source,
            }
        })?;
        match removed {
            1 => eprintln!("removed 1 stale trust entry"),
            n => eprintln!("removed {n} stale trust entries"),
        }
        Ok(())
    }

    /// Displays trust status for the target configuration.
    ///
    /// # Errors
    ///
    /// - [`CliError::TrustTargetResolve`] if resolving trust targets fails.
    /// - [`CliError::TrustShow`] if reading trust status fails.
    #[expect(
        clippy::print_stdout,
        reason = "trust --show's output is data meant to be piped, not \
                  diagnostic text; see the print_stderr precedent this mirrors"
    )]
    fn show(&self, service: &ConfigService) -> Result<(), CliError> {
        self.for_each_subject(service, |subject: TrustRequest| {
            let root = subject.root_path().to_path_buf();
            let path = subject
                .config_file()
                .unwrap_or(subject.root_path())
                .to_path_buf();
            let state = service.trust_status(&subject).map_err(|source| {
                CliError::TrustShow {
                    root,
                    source,
                }
            })?;
            println!("{}\t{}", path.display(), state);
            Ok(())
        })
    }

    /// Grants trust to the specified target directory or configuration.
    ///
    /// # Errors
    ///
    /// - [`CliError::TrustTargetResolve`] if resolving trust targets fails.
    /// - [`CliError::Trust`] if updating the trust store fails.
    fn trust(&self, service: &ConfigService) -> Result<(), CliError> {
        self.for_each_subject(service, |subject: TrustRequest| {
            let root = subject.root_path().to_path_buf();
            if let Err(source) = service.trust(&subject) {
                return Err(CliError::Trust {
                    root,
                    source,
                });
            }
            eprintln!("trusted {}", root.display());
            Ok(())
        })
    }

    /// Applies `visit` to every trust request resolved from arguments.
    ///
    /// # Errors
    ///
    /// - [`CliError::CurrentDirectory`] if no path was provided and reading
    ///   current directory fails.
    /// - [`CliError::TrustTargetResolve`] if resolving trust targets fails.
    /// - Any [`CliError`] returned by `visit`.
    fn for_each_subject(
        &self,
        service: &ConfigService,
        mut visit: impl FnMut(TrustRequest) -> Result<(), CliError>,
    ) -> Result<(), CliError> {
        let subjects = super::resolve_trust_subjects(
            service,
            self.path.as_deref(),
            self.all,
        )?;
        for subject in subjects {
            visit(subject)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use clap::Parser;

    use super::*;
    use crate::config::ConfigTrustStatus;

    mod fixtures {
        use std::{
            fs,
            path::{Path, PathBuf},
        };

        use clap::Parser;

        use super::super::*;

        /// Wraps [`Trust`] in a minimal top-level parser for clap tests.
        ///
        /// This allows [`Parser::try_parse_from`] to exercise
        /// `args_conflicts_with_subcommands`, which [`clap::Args`] types do not
        /// parse standalone.
        #[derive(Debug, Parser)]
        pub(super) struct TestCli {
            #[command(flatten)]
            pub(super) trust: Trust,
        }

        pub(super) fn parse(args: &[&str]) -> Trust {
            TestCli::try_parse_from(
                std::iter::once("test").chain(args.iter().copied()),
            )
            .expect("parse trust args")
            .trust
        }

        pub(super) fn service(temp: &Path) -> ConfigService {
            ConfigService::at(
                temp.join("tracked-store"),
                temp.join("trust-store"),
            )
        }

        pub(super) fn trust_args(path: Option<PathBuf>) -> Trust {
            Trust {
                action: None,
                show: false,
                all: false,
                path,
            }
        }

        pub(super) fn action_args(action: TrustAction) -> Trust {
            Trust {
                action: Some(action),
                show: false,
                all: false,
                path: None,
            }
        }

        pub(super) fn create_config(root: &Path) -> PathBuf {
            let config_file = root.join(".traces/config.toml");
            fs::create_dir_all(config_file.parent().expect("config parent"))
                .expect("create config parent");
            fs::write(&config_file, "").expect("write config file");
            config_file
        }
    }
    use fixtures::*;

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
        fn show_mode_parses() {
            let show = parse(&["--show", "some/path"]);

            assert!(show.show);
            assert_eq!(show.path, Some(PathBuf::from("some/path")));
        }

        #[test]
        fn all_mode_parse() {
            let args = parse(&["--all", "some/path"]);

            assert!(args.all);
            assert_eq!(args.path, Some(PathBuf::from("some/path")));
        }

        #[test]
        fn combining_an_action_with_a_path_is_rejected() {
            let result = fixtures::TestCli::try_parse_from([
                "test",
                "list",
                "some/path",
            ]);

            assert!(result.is_err());
        }
    }

    mod trust {
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::CwdGuard;

        #[test]
        fn with_no_path_trusts_the_discovered_project_root() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            let cwd = root.join("notes/daily");
            fs::create_dir_all(&cwd).expect("create nested cwd");
            super::create_config(&root);
            let service = super::service(temp.path());
            let _guard = CwdGuard::enter(&cwd);

            super::trust_args(None).run(&service).expect("trust cwd");

            assert_eq!(service.list_trusted().expect("list trusted"), vec![
                root.canonicalize().expect("canonicalize root")
            ]);
            assert_eq!(
                service
                    .trust_status(&TrustRequest::from(root.as_path()))
                    .expect("check trust"),
                ConfigTrustStatus::Trusted
            );
        }

        #[test]
        fn accepts_a_config_file_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            fs::create_dir_all(&root).expect("create project dir");
            let config_file = super::create_config(&root);
            let service = super::service(temp.path());

            super::trust_args(Some(config_file.clone()))
                .run(&service)
                .expect("trust config file");

            assert_eq!(service.list_trusted().expect("list trusted"), vec![
                root.canonicalize().expect("canonicalize root")
            ]);
        }

        #[test]
        fn missing_config_trusts_the_directory_target() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            fs::create_dir_all(&root).expect("create project dir");
            let service = super::service(temp.path());

            super::trust_args(Some(root.clone()))
                .run(&service)
                .expect("trust directory");

            assert_eq!(service.list_trusted().expect("list trusted"), vec![
                root.canonicalize().expect("canonicalize root")
            ]);
            assert_eq!(
                service
                    .trust_status(&TrustRequest::from(root.as_path()))
                    .expect("check trust"),
                ConfigTrustStatus::Trusted
            );
        }

        #[test]
        fn all_mode_trusts_descendant_configs() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let parent = temp.path().join("parent");
            let child = parent.join("child");
            fs::create_dir_all(&child).expect("create child dir");
            super::create_config(&parent);
            super::create_config(&child);
            let service = super::service(temp.path());
            let mut args = super::trust_args(Some(parent));
            args.all = true;

            args.run(&service).expect("trust all configs");

            assert_eq!(service.list_trusted().expect("list trusted").len(), 2);
        }
    }

    mod show {
        use super::*;

        #[test]
        fn checks_status_without_changing_trust_store() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            fs::create_dir_all(&root).expect("create project dir");
            super::create_config(&root);
            let service = super::service(temp.path());
            let mut args = super::trust_args(Some(root));
            args.show = true;

            args.run(&service).expect("show trust status");

            assert!(service.list_trusted().expect("list trusted").is_empty());
        }
    }

    mod list {
        use super::*;

        #[test]
        fn succeeds_against_an_empty_trust_store() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let service = super::service(temp.path());

            super::action_args(TrustAction::List)
                .run(&service)
                .expect("list empty trust store");
        }
    }

    mod clean {
        use super::*;

        #[test]
        fn removes_a_stale_root() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            fs::create_dir_all(&root).expect("create project dir");
            super::create_config(&root);
            let service = super::service(temp.path());
            super::trust_args(Some(root.clone()))
                .run(&service)
                .expect("trust root");
            fs::remove_dir_all(&root).expect("delete project dir");

            super::action_args(TrustAction::Clean)
                .run(&service)
                .expect("clean trust store");

            assert!(service.list_trusted().expect("list trusted").is_empty());
        }

        #[test]
        fn on_an_empty_trust_store_does_not_error() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let service = super::service(temp.path());

            super::action_args(TrustAction::Clean)
                .run(&service)
                .expect("clean empty trust store");
        }
    }
}
