//! Persisted index rebuild command.
//!
//! Handles `traces index` by loading the trusted project root, refreshing its
//! [`FileIndex`], and replacing the stored index. Task queries live in the task
//! command module.

use clap::Args;

use super::error::{CliError, CliResult};
use crate::{config::ConfigService, index::IndexerService};

/// Arguments for `traces index`.
///
/// Takes no positional or optional arguments. Scans the trusted project root
/// and persists a fresh [`FileIndex`].
#[derive(Debug, Args)]
pub(super) struct Index;

impl Index {
    /// Runs `traces index` for the trusted project root.
    ///
    /// Scans the root, persists the fresh [`FileIndex`], and reports the
    /// indexed file count to stderr.
    ///
    /// # Errors
    ///
    /// - [`CliError::CurrentDirectory`] if the current directory cannot be
    ///   read.
    /// - [`CliError::ConfigLoad`] if loading configuration fails, including an
    ///   untrusted project root.
    /// - [`CliError::Index`] if scanning the project root or persisting the
    ///   [`FileIndex`] fails.
    #[inline]
    #[expect(
        clippy::unused_self,
        reason = "keeps the dispatch signature consistent with every other \
                  command handler (all args.run(&self, service)) so the \
                  Commands::run match arms are uniform"
    )]
    pub(super) fn run(&self, service: &ConfigService) -> CliResult {
        let config = super::load_config(service)?;
        let root = config.root();
        let index_error = |source| CliError::Index {
            root: root.to_path_buf(),
            source,
        };
        let indexer = IndexerService::new(root).with_config(&config);
        let index = indexer.build().map_err(index_error)?;
        indexer.persist(&index).map_err(index_error)?;
        eprintln!(
            "indexed {} file(s) under {}",
            index.entries().len(),
            root.display()
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod fixtures {
        use crate::index::FileIndex;

        pub(super) fn record_paths(index: &FileIndex) -> Vec<String> {
            index
                .entries()
                .iter()
                .map(|entry| entry.file().path().to_string_lossy().into_owned())
                .collect()
        }
    }
    use fixtures::*;

    use crate::cli::tests::fixtures::{create_trusted_project, service};

    mod index {
        use std::{fs, path::Path};

        use miette::Diagnostic;
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::{
            cli::CwdGuard,
            config::ConfigLoadError,
            index::{IndexBuilderError, IndexError, IndexerService},
        };

        #[test]
        fn persists_covering_every_project_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            let service = service(temp.path());
            create_trusted_project(&service, &root);
            fs::create_dir_all(root.join("notes")).expect("mkdir notes");
            fs::write(root.join("notes/todo.md"), "- [ ] task")
                .expect("write note");
            let _guard = CwdGuard::enter(&root);

            Index.run(&service).expect("run index command");

            let loaded = IndexerService::new(&root)
                .load()
                .expect("load persisted index");
            let paths = record_paths(&loaded);
            assert!(paths.contains(&"notes/todo.md".to_owned()));
            assert!(paths.contains(&".traces/config.toml".to_owned()));
        }

        #[test]
        fn survives_a_later_process_invocation_via_load() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            let service = service(temp.path());
            create_trusted_project(&service, &root);
            fs::write(root.join("first.md"), "content").expect("write first");
            let _guard = CwdGuard::enter(&root);
            Index.run(&service).expect("first index run");

            let reloaded = IndexerService::new(&root)
                .load()
                .expect("reload persisted index");

            assert_eq!(reloaded.entries().len(), 2);
        }

        #[test]
        fn rebuilds_rather_than_appends() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            let service = service(temp.path());
            create_trusted_project(&service, &root);
            fs::write(root.join("stale.md"), "old").expect("write stale");
            let _guard = CwdGuard::enter(&root);
            Index.run(&service).expect("first index run");
            fs::remove_file(root.join("stale.md")).expect("remove stale");
            fs::write(root.join("fresh.md"), "new").expect("write fresh");

            Index.run(&service).expect("second index run");

            let loaded = IndexerService::new(&root)
                .load()
                .expect("load persisted index");
            let paths = record_paths(&loaded);
            assert!(!paths.contains(&"stale.md".to_owned()));
            assert!(paths.contains(&"fresh.md".to_owned()));
        }

        #[test]
        fn fails_when_project_root_is_not_trusted() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            fs::create_dir_all(&root).expect("create project dir");
            let config_file = root.join(".traces/config.toml");
            fs::create_dir_all(config_file.parent().expect("config parent"))
                .expect("create config parent");
            fs::write(&config_file, "[templates]\ndirectory = \"templates\"\n")
                .expect("write config file");
            let service = service(temp.path());
            let _guard = CwdGuard::enter(&root);

            let error = Index.run(&service).expect_err("untrusted root fails");

            assert!(matches!(error, CliError::ConfigLoad {
                source: ConfigLoadError::Build(_),
                ..
            }));
            assert_eq!(
                error.code().map(|code| code.to_string()),
                Some("traces::cli::config_build_untrusted".to_owned())
            );
        }

        #[cfg(unix)]
        #[test]
        fn wraps_scan_failure_as_cli_index_error() {
            use std::os::unix::fs::PermissionsExt as _;

            struct RestorePermissions<'a>(&'a Path);

            impl Drop for RestorePermissions<'_> {
                fn drop(&mut self) {
                    let _ = fs::set_permissions(
                        self.0,
                        fs::Permissions::from_mode(0o700),
                    );
                }
            }

            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            let service = service(temp.path());
            create_trusted_project(&service, &root);
            let locked = root.join("locked");
            fs::create_dir(&locked).expect("create locked dir");
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o000))
                .expect("revoke read permission");
            let _restore = RestorePermissions(&locked);
            let _guard = CwdGuard::enter(&root);

            let error =
                Index.run(&service).expect_err("unreadable subdirectory fails");

            assert!(matches!(error, CliError::Index {
                source: IndexError::Builder(IndexBuilderError::Scan { .. }),
                ..
            }));
        }
    }
}
