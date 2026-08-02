//! Task query command.
//!
//! Handles `traces task` by refreshing the trusted root's [`FileIndex`],
//! selecting a source scope, applying the optional filter, and printing
//! matching tasks as Markdown checkbox lines.

use std::path::{Path, PathBuf};

use clap::Args;

use super::error::CliError;
use crate::{
    config::ConfigService,
    index::{FileIndex, Source},
};

/// Command-line arguments for `traces task`.
#[derive(Debug, Args)]
pub(super) struct Task {
    /// Task source: a `#tag` (including nested sub-tags) or a folder path.
    /// Omit to query every indexed note's tasks.
    #[arg(long)]
    from: Option<String>,
    /// Filter expression narrowing results, e.g. `"task.completed == false"`.
    #[arg(long = "where")]
    filter: Option<String>,
}

impl Task {
    /// Refreshes the trusted project root's [`FileIndex`] and prints matching
    /// tasks.
    ///
    /// Each matching task is written to stdout as a Markdown checkbox line.
    ///
    /// # Errors
    ///
    /// - [`CliError::CurrentDirectory`] if the current directory cannot be
    ///   read.
    /// - [`CliError::ConfigLoad`] if loading configuration fails, including an
    ///   untrusted project root.
    /// - [`CliError::Index`] if refreshing the [`FileIndex`] fails.
    /// - [`CliError::Query`] if `--where` is an unparsable filter expression.
    #[expect(
        clippy::print_stdout,
        reason = "task rows are primary command output, not diagnostic text; \
                  mirrors the dry-run precedent in crate::cli::template and \
                  crate::cli::completions"
    )]
    pub(super) fn run(&self, service: &ConfigService) -> Result<(), CliError> {
        let config = super::load_config(service)?;
        let root = config.root();
        let lines = self.lines(root)?;
        let count = lines.len();
        for line in &lines {
            println!("{line}");
        }
        eprintln!("{count} task(s) from {}", root.display());
        Ok(())
    }

    /// Renders matching tasks from `root`'s [`FileIndex`].
    ///
    /// Each task becomes one Markdown checkbox line:
    ///
    /// - `- [ ] <text> (<path>)` for an incomplete task.
    /// - `- [x] <text> (<path>)` for a complete task.
    ///
    /// Split from [`Self::run`] so tests can assert on rendered content
    /// without capturing process stdout.
    ///
    /// # Errors
    ///
    /// - [`CliError::Index`] if refreshing the [`FileIndex`] fails.
    /// - [`CliError::Query`] if `--where` is an unparsable filter expression.
    fn lines(&self, root: &Path) -> Result<Vec<String>, CliError> {
        let index =
            FileIndex::refresh(root).map_err(|source| CliError::Index {
                root: root.to_path_buf(),
                source,
            })?;
        let mut outcome = index.query_tasks(&self.source());
        if let Some(expr) = self.filter.as_deref() {
            outcome =
                outcome.filter(expr).map_err(|source| CliError::Query {
                    root: root.to_path_buf(),
                    source,
                })?;
        }
        Ok(outcome
            .iter()
            .map(|record| {
                let checkbox = if record.task_completed() == Some(true) {
                    'x'
                } else {
                    ' '
                };
                let text = record.task_text().unwrap_or_default();
                format!(
                    "- [{checkbox}] {text} ({})",
                    record.file().path().display()
                )
            })
            .collect())
    }

    /// Builds the [`Source`] selected by `--from`.
    ///
    /// Values beginning with `#` become tag queries, other values become folder
    /// queries, and omitted values become [`Source::All`].
    fn source(&self) -> Source {
        match self.from.as_deref() {
            None => Source::All,
            Some(from) if from.starts_with('#') => Source::Tag(from.to_owned()),
            Some(from) => Source::Folder(PathBuf::from(from)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod fixtures {
        use std::{fs, path::Path};

        use super::*;
        use crate::config::{Discovered, LocalConfigFile, TrustRequest};

        pub(super) fn service(temp: &Path) -> ConfigService {
            ConfigService::at(
                temp.join("tracked-store"),
                temp.join("trust-store"),
            )
        }

        pub(super) fn create_trusted_project(
            service: &ConfigService,
            root: &Path,
        ) {
            fs::create_dir_all(root).expect("create project dir");
            let config_file = root.join(".traces/config.toml");
            fs::create_dir_all(config_file.parent().expect("config parent"))
                .expect("create config parent");
            fs::write(&config_file, "[templates]\ndirectory = \"templates\"\n")
                .expect("write config file");
            let config = LocalConfigFile::<Discovered>::try_new(config_file)
                .expect("valid local config");
            service
                .trust(&TrustRequest::from(&config))
                .expect("trust project config");
        }
    }
    use fixtures::*;

    mod lines {
        use std::fs;

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::index::QueryError;

        #[test]
        fn renders_a_checkbox_line_per_task_in_document_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("todo.md"),
                "- [ ] buy milk\n- [x] pay rent\n",
            )
            .expect("write note");
            let task = Task {
                from: None,
                filter: None,
            };

            let lines = task.lines(temp.path()).expect("valid query");

            assert_eq!(lines, [
                "- [ ] buy milk (todo.md)",
                "- [x] pay rent (todo.md)",
            ]);
        }

        #[test]
        fn from_tag_selects_only_matching_notes_tasks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "#projects\n- [ ] a task\n")
                .expect("write a.md");
            fs::write(temp.path().join("b.md"), "#books\n- [ ] b task\n")
                .expect("write b.md");
            let task = Task {
                from: Some("#projects".to_owned()),
                filter: None,
            };

            let lines = task.lines(temp.path()).expect("valid query");

            assert_eq!(lines, ["- [ ] a task (a.md)"]);
        }

        #[test]
        fn from_folder_selects_only_matching_notes_tasks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("projects")).expect("mkdir");
            fs::write(
                temp.path().join("projects/a.md"),
                "- [ ] project task\n",
            )
            .expect("write projects/a.md");
            fs::write(temp.path().join("b.md"), "- [ ] other task\n")
                .expect("write b.md");
            let task = Task {
                from: Some("projects".to_owned()),
                filter: None,
            };

            let lines = task.lines(temp.path()).expect("valid query");

            assert_eq!(lines, ["- [ ] project task (projects/a.md)"]);
        }

        #[test]
        fn where_filters_by_task_completion_not_by_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("todo.md"),
                "- [ ] buy milk\n- [x] pay rent\n",
            )
            .expect("write note");
            let task = Task {
                from: None,
                filter: Some("task.completed == false".to_owned()),
            };

            let lines = task.lines(temp.path()).expect("valid query");

            // The Note has one matching and one non-matching task: filtering
            // keeps only the matching task row, not every task on the page.
            assert_eq!(lines, ["- [ ] buy milk (todo.md)"]);
        }

        #[test]
        fn rejects_unparsable_filter_expression() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("todo.md"), "- [ ] buy milk\n")
                .expect("write note");
            let task = Task {
                from: None,
                filter: Some("not a valid expression".to_owned()),
            };

            let error =
                task.lines(temp.path()).expect_err("unparsable filter fails");

            assert!(matches!(error, CliError::Query {
                source: QueryError::UnparsableFilterExpression { .. },
                ..
            }));
        }
    }

    mod run {
        use std::fs;

        use super::*;
        use crate::{CwdGuard, config::ConfigLoadError};

        #[test]
        fn succeeds_for_a_trusted_project_root() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            let service = service(temp.path());
            create_trusted_project(&service, &root);
            fs::write(root.join("todo.md"), "- [ ] buy milk\n")
                .expect("write note");
            let _guard = CwdGuard::enter(&root);
            let task = Task {
                from: None,
                filter: None,
            };

            task.run(&service).expect("run task command");
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
            let task = Task {
                from: None,
                filter: None,
            };

            let error = task.run(&service).expect_err("untrusted root fails");

            assert!(matches!(error, CliError::ConfigLoad {
                source: ConfigLoadError::Build(_),
                ..
            }));
        }
    }
}
