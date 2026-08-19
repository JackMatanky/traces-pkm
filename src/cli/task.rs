//! Task query command.
//!
//! Handles `traces task` by refreshing the trusted root's [`FileIndex`],
//! selecting a source scope, applying the optional filter, and printing
//! matching tasks as Markdown checkbox lines.
//!
//! [`FileIndex`]: crate::index::FileIndex

use clap::Args;

use super::error::CliError;
use crate::config::{Config, ConfigService};

/// Arguments for `traces task`.
///
/// Queries tasks from the trusted project root and prints matching checkbox
/// lines.
#[derive(Debug, Args)]
pub(super) struct Task {
    /// Source expression, e.g. `#tag`, `folder/`, `@Class*`, or a boolean
    /// combination. Omit to query every indexed note's tasks.
    #[arg(long)]
    from: Option<String>,
    /// Filter expression narrowing results, e.g. `"task.completed == false"`.
    #[arg(long = "where")]
    filter: Option<String>,
}

impl Task {
    /// Runs `traces task` for the trusted project root.
    ///
    /// Refreshes the root's [`FileIndex`] and writes each matching task to
    /// stdout as a Markdown checkbox line.
    ///
    /// # Errors
    ///
    /// - [`CliError::CurrentDirectory`] if the current directory cannot be
    ///   read.
    /// - [`CliError::ConfigLoad`] if loading configuration fails, including an
    ///   untrusted project root.
    /// - [`CliError::Index`] if refreshing the [`FileIndex`] fails.
    /// - [`CliError::Query`] if `--where` is an unparsable filter expression.
    ///
    /// [`FileIndex`]: crate::index::FileIndex
    #[expect(
        clippy::print_stdout,
        reason = "task rows are primary command output, not diagnostic text; \
                  mirrors the dry-run precedent in crate::cli::template and \
                  crate::cli::completions"
    )]
    pub(super) fn run(&self, service: &ConfigService) -> Result<(), CliError> {
        let config = super::load_config(service)?;
        let root = config.root();
        let lines = self.lines(&config)?;
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
    /// Split from [`Self::run`] so tests can assert on rendered content without
    /// capturing process stdout.
    ///
    /// # Errors
    ///
    /// - [`CliError::Index`] if refreshing the [`FileIndex`] fails.
    /// - [`CliError::Query`] if `--where` is an unparsable filter expression.
    ///
    /// [`FileIndex`]: crate::index::FileIndex
    fn lines(&self, config: &Config) -> Result<Vec<String>, CliError> {
        let root = config.root();
        let outcome = super::refresh_task_query(config, self.from.as_deref())?;
        let outcome =
            super::apply_filter(outcome, root, self.filter.as_deref())?;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::tests::fixtures::{create_trusted_project, service};

    mod lines {
        use std::{fs, path::Path};

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::query::QueryError;

        fn config(root: &Path) -> Config {
            Config::for_test(root.to_path_buf(), None, None, root.to_path_buf())
        }

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

            let lines = task.lines(&config(temp.path())).expect("valid query");

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

            let lines = task.lines(&config(temp.path())).expect("valid query");

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
                from: Some("projects/".to_owned()),
                filter: None,
            };

            let lines = task.lines(&config(temp.path())).expect("valid query");

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

            let lines = task.lines(&config(temp.path())).expect("valid query");

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

            let error = task
                .lines(&config(temp.path()))
                .expect_err("unparsable filter fails");

            assert!(matches!(error, CliError::Query {
                source: QueryError::Syntax(_),
                ..
            }));
        }
    }

    mod run {
        use std::fs;

        use super::*;
        use crate::{cli::CwdGuard, config::ConfigLoadError};

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
