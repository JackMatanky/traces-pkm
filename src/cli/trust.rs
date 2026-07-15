//! `traces trust` command: default action (`traces trust [PATH]`) plus
//! `list`/`clean` nested subcommands.
//!
//! Thin adapter over [`ConfigService`]: this module only parses args,
//! derives `config_file` from the given path, and formats output — trust
//! decisions live in `config::trust::ConfigTrust` (see its docs).

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};

use super::error::CliError;
use crate::config::{ConfigService, LOCAL_CONFIG_FILE};

/// `traces trust [PATH]` / `traces trust list` / `traces trust clean`.
///
/// `args_conflicts_with_subcommands` is what lets `list`/`clean` disambiguate
/// from a positional `path`: clap tries to match the first free-standing
/// argument against a [`TrustAction`] variant name before falling back to
/// treating it as `path`, and rejects combining the two (`traces trust list
/// some/path` is a clap usage error, not "trust the path some/path while
/// also listing").
#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub(super) struct TrustArgs {
    #[command(subcommand)]
    action: Option<TrustAction>,
    /// Directory to trust (defaults to the current directory)
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

/// Dispatches `traces trust` to its default action or a nested subcommand.
///
/// # Errors
///
/// Returns [`CliError`] when the selected action fails.
#[inline]
pub(super) fn run(
    args: &TrustArgs,
    service: &ConfigService,
) -> Result<(), CliError> {
    match &args.action {
        Some(TrustAction::List) => list(service),
        Some(TrustAction::Clean) => clean(service),
        None => trust(args.path.as_deref(), service),
    }
}

/// Trusts `path` (or the current directory when `None`).
///
/// Derives `config_file` as `<path>/.traces/config.toml`. That file doesn't
/// need to exist yet — trusting a directory before `traces init` has
/// created its config file is a valid flow (see
/// [`crate::config::ConfigService::trust`]'s docs).
fn trust(path: Option<&Path>, service: &ConfigService) -> Result<(), CliError> {
    let root = path.map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let config_file = root.join(LOCAL_CONFIG_FILE);
    service.trust(&root, &config_file).map_err(|source| CliError::Trust {
        root: root.clone(),
        source: Box::new(source),
    })?;
    eprintln!("trusted {}", root.display());
    Ok(())
}

/// Prints every currently trusted directory, one per line, to stdout.
#[allow(
    clippy::print_stdout,
    reason = "trust list's output is data meant to be piped, not diagnostic \
              text — see the print_stderr precedent this mirrors"
)]
fn list(service: &ConfigService) -> Result<(), CliError> {
    let roots = service.list_trusted().map_err(|source| CliError::List {
        source: Box::new(source),
    })?;
    for root in &roots {
        println!("{}", root.display());
    }
    Ok(())
}

/// Removes dangling trust entries and reports how many were removed.
fn clean(service: &ConfigService) -> Result<(), CliError> {
    let removed =
        service.clean_trusted_store().map_err(|source| CliError::Clean {
            source: Box::new(source),
        })?;
    let suffix = if removed == 1 {
        "y"
    } else {
        "ies"
    };
    eprintln!("removed {removed} stale trust entr{suffix}");
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
        fn combining_an_action_with_a_path_is_rejected() {
            let result = TestCli::try_parse_from(["test", "list", "some/path"]);

            assert!(result.is_err());
        }
    }

    mod handlers {
        use super::*;

        fn service(temp: &Path) -> ConfigService {
            ConfigService::at(
                temp.join("tracked-store"),
                temp.join("trust-store"),
            )
        }

        #[test]
        fn trust_with_no_path_trusts_the_current_directory() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let service = service(temp.path());

            run(
                &TrustArgs {
                    action: None,
                    path: None,
                },
                &service,
            )
            .expect("trust cwd");

            assert_eq!(service.list_trusted().expect("list trusted").len(), 1);
        }

        #[test]
        fn trust_a_path_before_its_config_file_exists_does_not_error() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            fs::create_dir_all(&root).expect("create project dir");
            let service = service(temp.path());

            run(
                &TrustArgs {
                    action: None,
                    path: Some(root.clone()),
                },
                &service,
            )
            .expect("trust root before config exists");

            assert_eq!(service.list_trusted().expect("list trusted"), vec![
                root.canonicalize().expect("canonicalize root")
            ]);
        }

        #[test]
        fn trusting_again_after_the_config_file_appears_clears_staleness() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            fs::create_dir_all(&root).expect("create project dir");
            let service = service(temp.path());
            run(
                &TrustArgs {
                    action: None,
                    path: Some(root.clone()),
                },
                &service,
            )
            .expect("trust root before config exists");
            let config_file = root.join(LOCAL_CONFIG_FILE);
            fs::create_dir_all(
                config_file.parent().expect("config file parent"),
            )
            .expect("create .traces dir");
            fs::write(&config_file, "").expect("write config file");

            run(
                &TrustArgs {
                    action: None,
                    path: Some(root.clone()),
                },
                &service,
            )
            .expect("re-trust root once config exists");

            assert_eq!(
                service.is_trusted(&root, &config_file).expect("check trust"),
                crate::config::TrustState::Trusted
            );
        }

        #[test]
        fn list_succeeds_against_an_empty_trust_store() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let service = service(temp.path());

            run(
                &TrustArgs {
                    action: Some(TrustAction::List),
                    path: None,
                },
                &service,
            )
            .expect("list empty trust store");
        }

        #[test]
        fn clean_removes_a_stale_root_and_its_companion() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            fs::create_dir_all(&root).expect("create project dir");
            let config_file = root.join(LOCAL_CONFIG_FILE);
            fs::create_dir_all(
                config_file.parent().expect("config file parent"),
            )
            .expect("create .traces dir");
            fs::write(&config_file, "").expect("write config file");
            let service = service(temp.path());
            run(
                &TrustArgs {
                    action: None,
                    path: Some(root.clone()),
                },
                &service,
            )
            .expect("trust root");
            fs::remove_dir_all(&root).expect("delete project dir");

            run(
                &TrustArgs {
                    action: Some(TrustAction::Clean),
                    path: None,
                },
                &service,
            )
            .expect("clean trust store");

            assert!(service.list_trusted().expect("list trusted").is_empty());
        }

        #[test]
        fn clean_on_an_empty_trust_store_does_not_error() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let service = service(temp.path());

            run(
                &TrustArgs {
                    action: Some(TrustAction::Clean),
                    path: None,
                },
                &service,
            )
            .expect("clean empty trust store");
        }
    }
}
