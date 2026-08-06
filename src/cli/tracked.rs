//! Tracked-config store inspection and cleanup.
//!
//! Handles `traces tracked` by listing or cleaning the best-effort store of
//! local config files [`crate::config::ConfigService::load`] has seen, kept
//! separate from [`traces trust`](super::trust)'s trust decisions. See ADR
//! 0002 (config trust and tracking) for the tracked/trusted split.

use std::path::PathBuf;

use clap::{Args, Subcommand};

use super::error::CliError;
use crate::config::ConfigService;

/// Command-line arguments for `traces tracked`.
#[derive(Debug, Args)]
pub(super) struct Tracked {
    #[command(subcommand)]
    action: TrackedAction,
}

/// Nested `traces tracked` subcommands.
#[derive(Debug, Subcommand)]
enum TrackedAction {
    /// List all tracked local config paths.
    List,
    /// Remove stale tracked config entries.
    Clean,
}

impl Tracked {
    /// Runs `traces tracked` with the selected action.
    ///
    /// # Errors
    ///
    /// - [`CliError`] if the dispatched action fails.
    #[inline]
    pub(super) fn run(self, service: &ConfigService) -> Result<(), CliError> {
        match self.action {
            TrackedAction::List => Self::list(service),
            TrackedAction::Clean => Self::clean(service),
        }
    }

    /// Prints all tracked config paths to stdout, one per line.
    ///
    /// # Errors
    ///
    /// - [`CliError::TrackedList`] if reading the tracked-config store fails.
    #[expect(
        clippy::print_stdout,
        reason = "tracked list's output is data meant to be piped, not \
                  diagnostic text; mirrors the precedent in crate::cli::trust"
    )]
    fn list(service: &ConfigService) -> Result<(), CliError> {
        let paths: Vec<PathBuf> =
            service.list_tracked().map_err(|source| CliError::TrackedList {
                source,
            })?;
        for path in &paths {
            println!("{}", path.display());
        }
        Ok(())
    }

    /// Removes dangling entries from the tracked-config store.
    ///
    /// # Errors
    ///
    /// - [`CliError::TrackedClean`] if cleaning the tracked-config store fails.
    fn clean(service: &ConfigService) -> Result<(), CliError> {
        let removed = service.clean_tracked_store().map_err(|source| {
            CliError::TrackedClean {
                source,
            }
        })?;
        match removed {
            1 => eprintln!("removed 1 stale tracked entry"),
            n => eprintln!("removed {n} stale tracked entries"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod fixtures {
        use clap::Parser;

        use super::*;

        /// Wraps [`Tracked`] in a minimal top-level parser for clap tests.
        #[derive(Debug, Parser)]
        pub(super) struct TestCli {
            #[command(flatten)]
            pub(super) tracked: Tracked,
        }

        pub(super) fn parse(args: &[&str]) -> Tracked {
            TestCli::try_parse_from(
                std::iter::once("test").chain(args.iter().copied()),
            )
            .expect("parse tracked args")
            .tracked
        }

        pub(super) fn action_args(action: TrackedAction) -> Tracked {
            Tracked {
                action,
            }
        }
    }
    use fixtures::*;

    use crate::cli::tests::fixtures::{create_trusted_project, service};

    mod parsing {
        use super::*;

        #[test]
        fn list_argv_maps_to_list_action() {
            let args = parse(&["list"]);
            assert!(matches!(args.action, TrackedAction::List));
        }

        #[test]
        fn clean_argv_maps_to_clean_action() {
            let args = parse(&["clean"]);
            assert!(matches!(args.action, TrackedAction::Clean));
        }
    }

    mod list {
        use super::*;

        #[test]
        fn succeeds_against_an_empty_tracked_store() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let service = service(temp.path());

            action_args(TrackedAction::List)
                .run(&service)
                .expect("list empty tracked store");
        }
    }

    mod clean {
        use super::*;

        #[test]
        fn removes_a_stale_tracked_entry() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            let service = service(temp.path());
            create_trusted_project(&service, &root);
            service.load(&root).expect("load config to record tracking");
            std::fs::remove_dir_all(&root).expect("delete project dir");

            action_args(TrackedAction::Clean)
                .run(&service)
                .expect("clean tracked store");

            assert!(service.list_tracked().expect("list tracked").is_empty());
        }

        #[test]
        fn on_an_empty_tracked_store_does_not_error() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let service = service(temp.path());

            action_args(TrackedAction::Clean)
                .run(&service)
                .expect("clean empty tracked store");
        }
    }
}
