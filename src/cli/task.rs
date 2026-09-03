//! Task query CLI command implementation.
//!
//! Handles `traces task` by refreshing the trusted project root's
//! [`FileIndex`], selecting task rows via optional source and filter
//! expressions, and formatting matching tasks as Markdown checkbox lines.
//!
//! [`FileIndex`]: crate::index::FileIndex

use clap::Args;

use super::error::{CliError, CliResult};
use crate::{
    config::{Config, ConfigService},
    query::TaskPathStyle,
};

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
    /// Repeatable; multiple `--where` flags compose as AND.
    #[arg(long = "where")]
    filter: Vec<String>,
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
                  mirrors the precedent in crate::cli::list and \
                  crate::cli::table"
    )]
    pub(super) fn run(&self, service: &ConfigService) -> CliResult {
        let config = super::load_config(service)?;
        let root = config.root();
        let (rendered, count) = self.render(&config)?;
        print!("{rendered}");
        eprintln!("{count} task(s) from {}", root.display());
        Ok(())
    }

    /// Renders matching tasks from `root`'s [`FileIndex`] as a Markdown task
    /// list with each row's file path appended, alongside the matched row
    /// count.
    ///
    /// Split from [`Self::run`] so tests can assert on rendered content
    /// without capturing process stdout.
    ///
    /// # Errors
    ///
    /// - [`CliError::Index`] if refreshing the [`FileIndex`] fails.
    /// - [`CliError::Query`] if `--where` is an unparsable filter expression.
    ///
    /// [`FileIndex`]: crate::index::FileIndex
    fn render(&self, config: &Config) -> Result<(String, usize), CliError> {
        let root = config.root();
        let outcome = super::refresh_task_query(
            config,
            self.from.as_deref(),
            &self.filter,
        )?;
        let count = outcome.len();
        let rendered = outcome
            .task_list(TaskPathStyle::Suffix)
            .map_err(|source| super::query_error(root, source))?;
        Ok((rendered, count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::tests::fixtures::{create_trusted_project, service};

    mod render {
        use std::{fs, path::Path};

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::query::{QueryBuilderError, QueryError};

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
                filter: vec![],
            };

            let (rendered, count) =
                task.render(&config(temp.path())).expect("valid query");

            assert_eq!(
                rendered,
                "- [ ] buy milk (todo.md)\n- [x] pay rent (todo.md)\n"
            );
            assert_eq!(count, 2);
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
                filter: vec![],
            };

            let (rendered, count) =
                task.render(&config(temp.path())).expect("valid query");

            assert_eq!(rendered, "- [ ] a task (a.md)\n");
            assert_eq!(count, 1);
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
                filter: vec![],
            };

            let (rendered, count) =
                task.render(&config(temp.path())).expect("valid query");

            assert_eq!(rendered, "- [ ] project task (projects/a.md)\n");
            assert_eq!(count, 1);
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
                filter: vec!["task.completed == false".to_owned()],
            };

            let (rendered, count) =
                task.render(&config(temp.path())).expect("valid query");

            // The Note has one matching and one non-matching task: filtering
            // keeps only the matching task row, not every task on the page.
            assert_eq!(rendered, "- [ ] buy milk (todo.md)\n");
            assert_eq!(count, 1);
        }

        #[test]
        fn renders_a_dash_checkbox_for_a_cancelled_task() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("todo.md"),
                "- [x] done\n- [ ] pending\n- [-] cancelled\n",
            )
            .expect("write note");
            let task = Task {
                from: None,
                filter: vec![],
            };

            let (rendered, count) =
                task.render(&config(temp.path())).expect("valid query");

            assert_eq!(
                rendered,
                "- [x] done (todo.md)\n- [ ] pending (todo.md)\n- [-] \
                 cancelled (todo.md)\n"
            );
            assert_eq!(count, 3);
        }

        #[test]
        fn rejects_unparsable_filter_expression() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("todo.md"), "- [ ] buy milk\n")
                .expect("write note");
            let task = Task {
                from: None,
                filter: vec!["not a valid expression".to_owned()],
            };

            let error = task
                .render(&config(temp.path()))
                .expect_err("unparsable filter fails");

            assert!(matches!(error, CliError::Query {
                source: QueryError::Builder(QueryBuilderError::Syntax(_)),
                ..
            }));
        }
    }

    mod argv {
        use clap::Parser as _;
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::cli::{Cli, Commands};

        /// Extracts the parsed `Task` args from `cli`.
        fn task_args(cli: &Cli) -> &Task {
            match &cli.command {
                Some(Commands::Task(task)) => Some(task),
                _ => None,
            }
            .expect("expected the Task subcommand")
        }

        #[test]
        fn parses_from_and_where_flags() {
            let cli = Cli::try_parse_from([
                "traces",
                "task",
                "--from",
                "#tag",
                "--where",
                "task.completed == false",
            ])
            .expect("parse task argv");

            let task = task_args(&cli);

            assert_eq!(task.from.as_deref(), Some("#tag"));
            assert_eq!(task.filter, vec!["task.completed == false".to_owned()]);
        }

        #[test]
        fn defaults_from_and_where_to_empty() {
            let cli = Cli::try_parse_from(["traces", "task"])
                .expect("parse task argv");

            let task = task_args(&cli);

            assert_eq!(task.from, None);
            assert_eq!(task.filter, Vec::<String>::new());
        }

        #[test]
        fn repeated_where_flags_collect_into_one_vec_per_occurrence() {
            let cli = Cli::try_parse_from([
                "traces",
                "task",
                "--where",
                "task.completed == false",
                "--where",
                "rating > 2",
            ])
            .expect("parse task argv");

            let task = task_args(&cli);

            assert_eq!(task.filter, vec![
                "task.completed == false".to_owned(),
                "rating > 2".to_owned()
            ]);
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
                filter: vec![],
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
                filter: vec![],
            };

            let error = task.run(&service).expect_err("untrusted root fails");

            assert!(matches!(error, CliError::ConfigLoad {
                source: ConfigLoadError::Build(_),
                ..
            }));
        }
    }
}
