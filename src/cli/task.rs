//! Task query CLI command implementation.
//!
//! Handles `traces task` by refreshing the trusted project root's
//! [`WorkspaceIndex`], selecting task rows via optional source and filter
//! expressions, and formatting matching tasks as Markdown checkbox lines or a
//! table.
//!
//! [`WorkspaceIndex`]: crate::index::WorkspaceIndex

use clap::Args;

use super::{
    SortArgs,
    error::{CliError, CliResult},
};
use crate::{Config, ConfigService, query::TaskPathStyle};

/// Default table column headers when `--table` is passed without `--column`.
const DEFAULT_TABLE_HEADERS: &[&str] =
    &["Task", "Status", "Due", "Priority", "File"];

/// Default table field paths when `--table` is passed without `--column`.
const DEFAULT_TABLE_COLUMNS: &[&str] =
    &["list.text", "list.status", "list.due", "list.priority", "file.path"];

/// Filter options for task completion and status.
#[derive(Clone, Debug, Default, Args, PartialEq, Eq)]
pub(super) struct TaskFilterArgs {
    /// Filter to incomplete tasks (`list.completed == false`). Conflicts with
    /// `--done`.
    #[arg(long, conflicts_with = "done")]
    todo: bool,
    /// Filter to completed tasks (`list.completed == true`). Conflicts with
    /// `--todo`.
    #[arg(long, conflicts_with = "todo")]
    done: bool,
    /// Filter by status character via `list.status_symbol == "<char>"`.
    #[arg(long, value_name = "CHAR")]
    status: Option<char>,
}

impl TaskFilterArgs {
    /// Formats an exact status filter expression when `--status` is specified.
    fn status_filter(&self) -> Option<String> {
        self.status.map(|status| {
            format!("list.status_symbol == \"{}\"", status.escape_default())
        })
    }

    /// Collects synthesized filter expressions for `--todo`, `--done`, and
    /// `--status`.
    fn shortcuts<'a>(&self, status_filter: Option<&'a str>) -> Vec<&'a str> {
        let mut filters = Vec::with_capacity(2);
        if self.todo {
            filters.push("list.completed == false");
        }
        if self.done {
            filters.push("list.completed == true");
        }
        if let Some(filter) = status_filter {
            filters.push(filter);
        }
        filters
    }
}

/// Table presentation configuration for task output.
#[derive(Clone, Debug, Default, Args, PartialEq, Eq)]
pub(super) struct TaskTableArgs {
    /// Render output as a Markdown table.
    #[arg(long)]
    table: bool,
    /// Field path to render as a table column. Custom overrides default table
    /// columns.
    #[arg(long = "column", requires = "table")]
    columns: Vec<String>,
}

impl TaskTableArgs {
    /// Formats query rows as a Markdown table.
    ///
    /// # Errors
    ///
    /// - [`CliError::Query`] if table column validation or rendering fails.
    fn format(
        &self,
        root: &std::path::Path,
        rows: &crate::query::QuerySet,
    ) -> Result<String, CliError> {
        if self.columns.is_empty() {
            rows.table(DEFAULT_TABLE_HEADERS, DEFAULT_TABLE_COLUMNS)
        } else {
            let columns: Vec<&str> =
                self.columns.iter().map(String::as_str).collect();
            rows.table(&columns, &columns)
        }
        .map_err(|source| super::query_error(root, source))
    }
}

/// Output presentation configuration for task lists, tables, and counts.
#[derive(Clone, Debug, Default, Args, PartialEq, Eq)]
pub(super) struct TaskPresentationArgs {
    /// Display clickable editor line coordinates `({path}:{line})`.
    #[arg(short = 'l', long = "line-numbers")]
    line_numbers: bool,
    /// Table configuration options.
    #[command(flatten)]
    table: TaskTableArgs,
    /// Output only the matching row count.
    #[arg(long)]
    count: bool,
}

impl TaskPresentationArgs {
    /// Formats query rows according to active presentation flags.
    ///
    /// # Errors
    ///
    /// - [`CliError::Query`] if table or task list rendering fails.
    fn format_rows(
        &self,
        root: &std::path::Path,
        rows: &crate::query::QuerySet,
    ) -> Result<String, CliError> {
        if self.count {
            return Ok(format!("{}\n", rows.len()));
        }
        if self.table.table {
            return self.table.format(root, rows);
        }
        let path_style = if self.line_numbers {
            TaskPathStyle::Coordinates
        } else {
            TaskPathStyle::Suffix
        };
        rows.task_list(path_style)
            .map_err(|source| super::query_error(root, source))
    }
}

/// Arguments for `traces task`.
///
/// Queries tasks from the trusted project root and prints matching checkbox
/// lines or a formatted table.
#[derive(Debug, Default, Args)]
pub(super) struct Task {
    /// Source expression, e.g. `#tag`, `folder/`, `@Class*`, or a boolean
    /// combination. Omit to query every indexed note's tasks.
    #[arg(long)]
    from: Option<String>,
    /// Filter expression narrowing results, e.g. `"list.completed == false"`.
    /// Repeatable; multiple `--where` flags compose as AND.
    #[arg(long = "where")]
    filter: Vec<String>,
    /// Filter shortcuts for task completion and status.
    #[command(flatten)]
    filters: TaskFilterArgs,
    /// Sort configuration. See [`SortArgs`].
    #[command(flatten)]
    sort: SortArgs,
    /// Output presentation configuration.
    #[command(flatten)]
    presentation: TaskPresentationArgs,
}

impl Task {
    /// Runs `traces task` for the trusted project root.
    ///
    /// Refreshes the root's [`WorkspaceIndex`] and writes each matching task to
    /// stdout as a Markdown checkbox line or formatted table.
    ///
    /// # Errors
    ///
    /// - [`CliError::CurrentDirectory`] if the current directory cannot be
    ///   read.
    /// - [`CliError::ConfigLoad`] if loading configuration fails, including an
    ///   untrusted project root.
    /// - [`CliError::Index`] if refreshing the [`WorkspaceIndex`] fails.
    /// - [`CliError::Query`] if `--where` is an unparsable filter expression,
    ///   `--sort` names a malformed field path, or `--column` names a malformed
    ///   field path.
    ///
    /// [`WorkspaceIndex`]: crate::index::WorkspaceIndex
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
        if !self.presentation.count {
            eprintln!("{count} task(s) from {}", root.display());
        }
        Ok(())
    }

    /// Renders matching tasks from `root`'s [`WorkspaceIndex`] as a Markdown
    /// task list, table, or count-only output, alongside the matched row
    /// count.
    ///
    /// Split from [`Self::run`] so tests can assert on rendered content
    /// without capturing process stdout.
    /// # Errors
    ///
    /// - [`CliError::Index`] if refreshing the [`WorkspaceIndex`] fails.
    /// - [`CliError::Query`] if `--where` is an unparsable filter expression,
    ///   `--sort` names a malformed field path, or `--column` names a malformed
    ///   field path.
    ///
    /// [`WorkspaceIndex`]: crate::index::WorkspaceIndex
    fn render(&self, config: &Config) -> Result<(String, usize), CliError> {
        let root = config.root();
        let status_filter = self.filters.status_filter();
        let extra_filters = self.filters.shortcuts(status_filter.as_deref());
        let all_filters =
            self.filter.iter().map(String::as_str).chain(extra_filters);
        let order = self.sort.resolve(root)?;
        let rows = super::refresh_task_query(
            config,
            self.from.as_deref(),
            all_filters,
            order,
        )?;
        let count = rows.len();
        let rendered = self.presentation.format_rows(root, &rows)?;
        Ok((rendered, count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TestProject;

    mod render {
        use std::{fs, path::Path};

        use pretty_assertions::assert_eq;
        use rstest::rstest;

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
                ..Default::default()
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
        fn selects_tasks_from_tag_source() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "#projects\n- [ ] a task\n")
                .expect("write a.md");
            fs::write(temp.path().join("b.md"), "#books\n- [ ] b task\n")
                .expect("write b.md");
            let task = Task {
                from: Some("#projects".to_owned()),
                filter: vec![],
                ..Default::default()
            };

            let (rendered, count) =
                task.render(&config(temp.path())).expect("valid query");

            assert_eq!(rendered, "- [ ] a task (a.md)\n");
            assert_eq!(count, 1);
        }

        #[test]
        fn selects_tasks_from_folder_source() {
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
                ..Default::default()
            };

            let (rendered, count) =
                task.render(&config(temp.path())).expect("valid query");

            assert_eq!(rendered, "- [ ] project task (projects/a.md)\n");
            assert_eq!(count, 1);
        }

        #[test]
        fn filters_by_task_completion_instead_of_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("todo.md"),
                "- [ ] buy milk\n- [x] pay rent\n",
            )
            .expect("write note");
            let task = Task {
                from: None,
                filter: vec!["list.completed == false".to_owned()],
                ..Default::default()
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
                ..Default::default()
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
                ..Default::default()
            };

            let error = task
                .render(&config(temp.path()))
                .expect_err("unparsable filter fails");

            assert!(matches!(error, CliError::Query {
                source: QueryError::Builder(QueryBuilderError::Syntax(_)),
                ..
            }));
        }

        #[test]
        fn renders_line_number_coordinates_with_line_numbers_flag() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("todo.md"),
                "# Heading\n\n- [ ] buy milk\n- [x] pay rent\n",
            )
            .expect("write note");
            let task = Task {
                presentation: TaskPresentationArgs {
                    line_numbers: true,
                    ..Default::default()
                },
                ..Default::default()
            };

            let (rendered, count) =
                task.render(&config(temp.path())).expect("valid query");

            assert_eq!(
                rendered,
                "- [ ] buy milk (todo.md:3)\n- [x] pay rent (todo.md:4)\n"
            );
            assert_eq!(count, 2);
        }

        #[test]
        fn filters_incomplete_tasks_with_todo_flag() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("todo.md"),
                "- [ ] buy milk\n- [x] pay rent\n- [-] cancelled\n",
            )
            .expect("write note");
            let task = Task {
                filters: TaskFilterArgs {
                    todo: true,
                    ..Default::default()
                },
                ..Default::default()
            };

            let (rendered, count) =
                task.render(&config(temp.path())).expect("valid query");

            assert_eq!(rendered, "- [ ] buy milk (todo.md)\n");
            assert_eq!(count, 1);
        }

        #[test]
        fn filters_completed_tasks_with_done_flag() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("todo.md"),
                "- [ ] buy milk\n- [x] pay rent\n- [-] cancelled\n",
            )
            .expect("write note");
            let task = Task {
                filters: TaskFilterArgs {
                    done: true,
                    ..Default::default()
                },
                ..Default::default()
            };

            let (rendered, count) =
                task.render(&config(temp.path())).expect("valid query");

            assert_eq!(rendered, "- [x] pay rent (todo.md)\n");
            assert_eq!(count, 1);
        }

        #[rstest]
        #[case('/', "- [/] in progress (todo.md)\n")]
        #[case('!', "- [!] urgent (todo.md)\n")]
        fn filters_by_status_symbol_character(
            #[case] symbol: char,
            #[case] expected: &str,
        ) {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("todo.md"),
                "- [ ] buy milk\n- [/] in progress\n- [!] urgent\n- [x] done\n",
            )
            .expect("write note");
            let task = Task {
                filters: TaskFilterArgs {
                    status: Some(symbol),
                    ..Default::default()
                },
                ..Default::default()
            };

            let (rendered, count) =
                task.render(&config(temp.path())).expect("valid query");

            assert_eq!(rendered, expected);
            assert_eq!(count, 1);
        }

        #[test]
        fn sorts_tasks_by_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("todo.md"),
                "- [ ] zebra\n- [ ] apple\n- [ ] mango\n",
            )
            .expect("write note");
            let task = Task {
                sort: SortArgs {
                    sort: vec!["list.text".to_owned()],
                    asc: true,
                    desc: false,
                },
                ..Default::default()
            };

            let (rendered, count) =
                task.render(&config(temp.path())).expect("valid query");

            assert_eq!(
                rendered,
                "- [ ] apple (todo.md)\n- [ ] mango (todo.md)\n- [ ] zebra \
                 (todo.md)\n"
            );
            assert_eq!(count, 3);
        }

        #[test]
        fn renders_table_with_default_columns() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("todo.md"),
                "- [ ] buy milk ➕ 2025-01-01 📅 2025-01-05 🔺\n",
            )
            .expect("write note");
            let task = Task {
                presentation: TaskPresentationArgs {
                    table: TaskTableArgs {
                        table: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            };

            let (rendered, count) =
                task.render(&config(temp.path())).expect("valid query");

            assert_eq!(count, 1);
            assert!(rendered.contains("| Task"));
            assert!(rendered.contains("| Status"));
            assert!(rendered.contains("| Due"));
            assert!(rendered.contains("| Priority"));
            assert!(rendered.contains("| File"));
            assert!(rendered.contains("buy milk"));
            assert!(rendered.contains("todo.md"));
        }

        #[test]
        fn renders_table_with_custom_columns() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("todo.md"), "- [x] buy milk\n")
                .expect("write note");
            let task = Task {
                presentation: TaskPresentationArgs {
                    table: TaskTableArgs {
                        table: true,
                        columns: vec![
                            "list.text".to_owned(),
                            "file.path".to_owned(),
                        ],
                    },
                    ..Default::default()
                },
                ..Default::default()
            };

            let (rendered, count) =
                task.render(&config(temp.path())).expect("valid query");

            assert_eq!(count, 1);
            assert!(rendered.contains("| list.text"));
            assert!(rendered.contains("| file.path"));
            assert!(rendered.contains("buy milk"));
            assert!(rendered.contains("todo.md"));
        }

        #[test]
        fn outputs_only_count_when_count_flag_enabled() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("todo.md"), "- [ ] one\n- [x] two\n")
                .expect("write note");
            let task = Task {
                presentation: TaskPresentationArgs {
                    count: true,
                    ..Default::default()
                },
                ..Default::default()
            };

            let (rendered, count) =
                task.render(&config(temp.path())).expect("valid query");

            assert_eq!(rendered, "2\n");
            assert_eq!(count, 2);
        }

        #[test]
        fn selects_tasks_from_direct_markdown_file_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("notes")).expect("mkdir");
            fs::write(temp.path().join("notes/todo.md"), "- [ ] notes task\n")
                .expect("write notes/todo.md");
            fs::write(temp.path().join("other.md"), "- [ ] other task\n")
                .expect("write other.md");
            let task = Task {
                from: Some("notes/todo.md".to_owned()),
                ..Default::default()
            };

            let (rendered, count) =
                task.render(&config(temp.path())).expect("valid query");

            assert_eq!(rendered, "- [ ] notes task (notes/todo.md)\n");
            assert_eq!(count, 1);
        }

        #[test]
        fn selects_tasks_from_unadorned_path_without_extension() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("notes")).expect("mkdir");
            fs::write(
                temp.path().join("notes/todo.md"),
                "- [ ] unadorned task\n",
            )
            .expect("write notes/todo.md");
            fs::write(temp.path().join("other.md"), "- [ ] other task\n")
                .expect("write other.md");
            let task = Task {
                from: Some("notes/todo".to_owned()),
                ..Default::default()
            };

            let (rendered, count) =
                task.render(&config(temp.path())).expect("valid query");

            assert_eq!(rendered, "- [ ] unadorned task (notes/todo.md)\n");
            assert_eq!(count, 1);
        }
    }

    mod argv {
        use clap::Parser as _;
        use pretty_assertions::assert_eq;
        use rstest::rstest;

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
                "list.completed == false",
            ])
            .expect("parse task argv");

            let task = task_args(&cli);

            assert_eq!(task.from.as_deref(), Some("#tag"));
            assert_eq!(task.filter, vec!["list.completed == false".to_owned()]);
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
                "list.completed == false",
                "--where",
                "rating > 2",
            ])
            .expect("parse task argv");

            let task = task_args(&cli);

            assert_eq!(task.filter, vec![
                "list.completed == false".to_owned(),
                "rating > 2".to_owned()
            ]);
        }

        #[rstest]
        #[case("-l")]
        #[case("--line-numbers")]
        fn parses_line_numbers_flag(#[case] flag: &str) {
            let cli = Cli::try_parse_from(["traces", "task", flag])
                .expect("parse line numbers");
            assert!(task_args(&cli).presentation.line_numbers);
        }

        #[test]
        fn parses_todo_flag() {
            let cli = Cli::try_parse_from(["traces", "task", "--todo"])
                .expect("parse todo flag");
            assert!(task_args(&cli).filters.todo);
            assert!(!task_args(&cli).filters.done);
        }

        #[test]
        fn parses_done_flag() {
            let cli = Cli::try_parse_from(["traces", "task", "--done"])
                .expect("parse done flag");
            assert!(task_args(&cli).filters.done);
            assert!(!task_args(&cli).filters.todo);
        }
        #[test]
        fn rejects_conflicting_todo_and_done_flags() {
            let result =
                Cli::try_parse_from(["traces", "task", "--todo", "--done"]);
            assert!(
                result.is_err(),
                "passing both --todo and --done must fail"
            );
        }

        #[test]
        fn parses_status_flag() {
            let cli = Cli::try_parse_from(["traces", "task", "--status", "/"])
                .expect("parse status flag");
            assert_eq!(task_args(&cli).filters.status, Some('/'));
        }

        #[test]
        fn parses_sort_flags() {
            let cli = Cli::try_parse_from([
                "traces", "task", "--sort", "list.due", "--asc",
            ])
            .expect("parse sort flag");
            let task = task_args(&cli);
            assert_eq!(task.sort.sort, vec!["list.due".to_owned()]);
            assert!(task.sort.asc);
        }

        #[test]
        fn parses_table_flag_with_default_empty_columns() {
            let cli = Cli::try_parse_from(["traces", "task", "--table"])
                .expect("parse table flag");
            let task = task_args(&cli);
            assert!(task.presentation.table.table);
            assert_eq!(task.presentation.table.columns, Vec::<String>::new());
        }

        #[test]
        fn parses_table_columns() {
            let custom_cli = Cli::try_parse_from([
                "traces",
                "task",
                "--table",
                "--column",
                "list.text",
                "--column",
                "file.path",
            ])
            .expect("parse custom columns");
            let custom_task = task_args(&custom_cli);
            assert!(custom_task.presentation.table.table);
            assert_eq!(custom_task.presentation.table.columns, vec![
                "list.text".to_owned(),
                "file.path".to_owned()
            ]);
        }
        #[test]
        fn rejects_column_without_table() {
            let result = Cli::try_parse_from([
                "traces",
                "task",
                "--column",
                "list.text",
            ]);
            assert!(
                result.is_err(),
                "--column requires --table, so passing --column alone must \
                 fail"
            );
        }

        #[test]
        fn parses_count_flag() {
            let cli = Cli::try_parse_from(["traces", "task", "--count"])
                .expect("parse count flag");
            assert!(task_args(&cli).presentation.count);
        }
    }

    mod run {
        use super::*;
        use crate::{cli::CwdGuard, config::ConfigLoadError};

        #[test]
        fn runs_successfully_for_a_trusted_project_root() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let project = TestProject::trusted(temp.path().join("project"));
            project.write_note("todo.md", "- [ ] buy milk\n");
            let _guard = CwdGuard::enter(project.root());
            let task = Task::default();

            task.run(project.service()).expect("run task command");
        }

        #[test]
        fn fails_when_project_root_is_not_trusted() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let project = TestProject::untrusted(temp.path().join("project"));
            let _guard = CwdGuard::enter(project.root());
            let task = Task::default();

            let error =
                task.run(project.service()).expect_err("untrusted root fails");

            assert!(matches!(error, CliError::ConfigLoad {
                source: ConfigLoadError::Build(_),
                ..
            }));
        }

        #[test]
        fn omits_summary_stderr_when_run_with_count() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let project = TestProject::trusted(temp.path().join("project"));
            project.write_note("todo.md", "- [ ] buy milk\n");
            let _guard = CwdGuard::enter(project.root());
            let task = Task {
                presentation: TaskPresentationArgs {
                    count: true,
                    ..Default::default()
                },
                ..Default::default()
            };

            task.run(project.service()).expect("run task command with count");
        }
    }

    mod filter_args {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn escapes_special_characters_in_status_filter() {
            let quote_args = TaskFilterArgs {
                status: Some('"'),
                ..Default::default()
            };
            assert_eq!(
                quote_args.status_filter(),
                Some("list.status_symbol == \"\\\"\"".to_owned())
            );

            let backslash_args = TaskFilterArgs {
                status: Some('\\'),
                ..Default::default()
            };
            assert_eq!(
                backslash_args.status_filter(),
                Some("list.status_symbol == \"\\\\\"".to_owned())
            );
        }

        #[test]
        fn returns_none_when_status_is_omitted() {
            let args = TaskFilterArgs::default();
            assert_eq!(args.status_filter(), None);
        }

        #[test]
        fn collects_shortcuts_for_todo_and_status() {
            let args = TaskFilterArgs {
                todo: true,
                done: false,
                status: Some('/'),
            };
            let status_expr = args.status_filter();
            let shortcuts = args.shortcuts(status_expr.as_deref());
            assert_eq!(shortcuts, vec![
                "list.completed == false",
                "list.status_symbol == \"/\""
            ]);
        }
    }

    mod presentation_args {
        use super::*;

        #[test]
        fn defaults_to_standard_formatting() {
            let args = TaskPresentationArgs::default();
            assert!(!args.line_numbers);
            assert!(!args.table.table);
            assert_eq!(args.table.columns, Vec::<String>::new());
            assert!(!args.count);
        }
    }
}
