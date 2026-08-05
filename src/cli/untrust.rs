//! Trust-store revocation.
//!
//! Handles `traces untrust` by resolving a directory or `.traces/config.toml`
//! to trust subjects and removing their roots from the store.

use std::path::PathBuf;

use clap::Args;

use super::error::CliError;
use crate::config::ConfigService;

/// Command-line arguments for `traces untrust`.
#[derive(Debug, Args)]
pub(super) struct Untrust {
    /// Apply untrust to descendant configurations too.
    #[arg(long)]
    all: bool,
    /// Directory or `.traces/config.toml` to untrust.
    ///
    /// Defaults to the current directory.
    path: Option<PathBuf>,
}

impl Untrust {
    /// Runs `traces untrust` for the selected target directory or config.
    ///
    /// # Errors
    ///
    /// - [`CliError::CurrentDirectory`] if no path was provided and reading
    ///   current directory fails.
    /// - [`CliError::TrustTargetResolve`] if resolving trust targets fails.
    /// - [`CliError::Untrust`] if updating the trust store fails.
    #[inline]
    pub(super) fn run(self, service: &ConfigService) -> Result<(), CliError> {
        let subjects = super::resolve_trust_subjects(
            service,
            self.path.as_deref(),
            self.all,
        )?;
        for subject in subjects {
            let root = subject.root_path().to_path_buf();
            if let Err(source) = service.untrust(&subject) {
                return Err(CliError::Untrust {
                    root,
                    source,
                });
            }
            eprintln!("untrusted {}", root.display());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use clap::Parser;

    use super::*;
    use crate::config::{ConfigTrustStatus, TrustRequest};

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        untrust: Untrust,
    }

    fn parse(args: &[&str]) -> Untrust {
        TestCli::try_parse_from(
            std::iter::once("test").chain(args.iter().copied()),
        )
        .expect("parse untrust args")
        .untrust
    }

    mod parsing {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn untrust_with_no_path_has_defaults() {
            let args = parse(&[]);

            assert!(args.path.is_none());
            assert!(!args.all);
        }

        #[test]
        fn untrust_with_path_parses_path() {
            let args = parse(&["some/path"]);

            assert_eq!(args.path, Some(PathBuf::from("some/path")));
            assert!(!args.all);
        }

        #[test]
        fn untrust_with_all_flag_parses_all() {
            let args = parse(&["--all", "some/path"]);

            assert!(args.all);
            assert_eq!(args.path, Some(PathBuf::from("some/path")));
        }
    }

    fn service(temp: &Path) -> ConfigService {
        ConfigService::at(temp.join("tracked-store"), temp.join("trust-store"))
    }

    fn untrust_args(path: Option<PathBuf>, all: bool) -> Untrust {
        Untrust {
            all,
            path,
        }
    }

    fn create_config(root: &Path) -> PathBuf {
        let config_file = root.join(".traces/config.toml");
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "").expect("write config file");
        config_file
    }

    fn trust_root(service: &ConfigService, root: &Path) {
        let request = TrustRequest::from(root);
        service.trust(&request).expect("trust root");
    }

    mod untrust {
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::CwdGuard;

        #[test]
        fn removes_the_resolved_root() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            fs::create_dir_all(&root).expect("create project dir");
            super::create_config(&root);
            let service = super::service(temp.path());
            super::trust_root(&service, &root);

            super::untrust_args(Some(root.clone()), false)
                .run(&service)
                .expect("untrust root");

            assert_eq!(
                service
                    .trust_status(&TrustRequest::from(root.as_path()))
                    .expect("check trust"),
                ConfigTrustStatus::Untrusted
            );
        }

        #[test]
        fn with_no_path_untrusts_cwd_project_root() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            let cwd = root.join("notes/daily");
            fs::create_dir_all(&cwd).expect("create nested cwd");
            super::create_config(&root);
            let service = super::service(temp.path());
            super::trust_root(&service, &root);
            let _guard = CwdGuard::enter(&cwd);

            super::untrust_args(None, false)
                .run(&service)
                .expect("untrust cwd");

            assert_eq!(
                service
                    .trust_status(&TrustRequest::from(root.as_path()))
                    .expect("check trust"),
                ConfigTrustStatus::Untrusted
            );
        }

        #[test]
        fn all_mode_untrusts_descendant_configs() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let parent = temp.path().join("parent");
            let child = parent.join("child");
            fs::create_dir_all(&child).expect("create child dir");
            super::create_config(&parent);
            super::create_config(&child);
            let service = super::service(temp.path());
            super::trust_root(&service, &parent);
            super::trust_root(&service, &child);

            super::untrust_args(Some(parent.clone()), true)
                .run(&service)
                .expect("untrust all configs");

            assert!(service.list_trusted().expect("list trusted").is_empty());
        }
    }
}
