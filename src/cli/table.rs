//! Page-level table query command.
//!
//! Handles `traces table` by refreshing the trusted root's [`FileIndex`],
//! selecting a source scope, applying the optional filter, and printing
//! matching pages as a Markdown table.
//!
//! [`FileIndex`]: crate::index::FileIndex

use clap::Args;

use super::error::CliError;
use crate::{
    config::{Config, ConfigService},
    query::SortOrder,
};

/// Arguments for `traces table`.
///
/// Queries pages from the trusted project root and prints matching records as
/// a Markdown table with one column per `--column` flag.
#[derive(Debug, Args)]
pub(super) struct Table {
    /// Source expression, e.g. `#tag`, `folder/`, `@Class*`, or a boolean
    /// combination. Omit to query every indexed page.
    #[arg(long)]
    from: Option<String>,
    /// Filter expression narrowing results, e.g. `"file.folder == \"books\""`.
    /// Repeatable; multiple `--where` flags compose as AND.
    #[arg(long = "where")]
    filter: Vec<String>,
    /// Field path to sort by. Omit to leave results in `FileIndex` order.
    #[arg(long)]
    sort: Option<String>,
    /// Sort direction for `--sort`. Has no effect without `--sort`.
    #[arg(long, value_enum, default_value = "asc")]
    order: SortOrder,
    /// Field path to render as a table column, e.g. `file.name`. Repeat for
    /// multiple columns; each path also becomes its column's header.
    #[arg(long = "column", required = true)]
    columns: Vec<String>,
}

impl Table {
    /// Runs `traces table` for the trusted project root.
    ///
    /// Refreshes the root's [`FileIndex`] and writes a Markdown table of
    /// matching pages to stdout.
    ///
    /// # Errors
    ///
    /// - [`CliError::CurrentDirectory`] if the current directory cannot be
    ///   read.
    /// - [`CliError::ConfigLoad`] if loading configuration fails, including an
    ///   untrusted project root.
    /// - [`CliError::Index`] if refreshing the [`FileIndex`] fails.
    /// - [`CliError::Query`] if `--where` is an unparsable filter expression,
    ///   `--sort` names a malformed field path, or `--column` names a malformed
    ///   field path.
    ///
    /// [`FileIndex`]: crate::index::FileIndex
    #[expect(
        clippy::print_stdout,
        reason = "table output is primary command output, not diagnostic \
                  text; mirrors the precedent in crate::cli::task"
    )]
    pub(super) fn run(&self, service: &ConfigService) -> Result<(), CliError> {
        let config = super::load_config(service)?;
        let root = config.root();
        let (rendered, count) = self.render(&config)?;
        print!("{rendered}");
        eprintln!("{count} page(s) from {}", root.display());
        Ok(())
    }

    /// Renders matching pages from `root`'s [`FileIndex`] as a Markdown table,
    /// alongside the matched row count.
    ///
    /// Split from [`Self::run`] so tests can assert on rendered content without
    /// capturing process stdout.
    ///
    /// # Errors
    ///
    /// - [`CliError::Index`] if refreshing the [`FileIndex`] fails.
    /// - [`CliError::Query`] if `--where` is an unparsable filter expression,
    ///   `--sort` names a malformed field path, or `--column` names a malformed
    ///   field path.
    ///
    /// [`FileIndex`]: crate::index::FileIndex
    fn render(&self, config: &Config) -> Result<(String, usize), CliError> {
        let root = config.root();
        let outcome = super::refresh_page_query(
            config,
            self.from.as_deref(),
            &self.filter,
            self.sort.as_deref(),
            self.order.is_descending(),
        )?;
        let count = outcome.len();
        let columns =
            self.columns.iter().map(String::as_str).collect::<Vec<_>>();
        // `QuerySet::table` escapes both headers and cell values, so a
        // `--column` value doubling as its header stays row-safe without the
        // CLI escaping it a second time.
        let rendered = outcome
            .table(&columns, &columns)
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
        use crate::{
            index::{IndexBuilderError, IndexError},
            query::{QueryBuilderError, QueryError},
        };

        fn config(root: &Path) -> Config {
            Config::for_test(root.to_path_buf(), None, None, root.to_path_buf())
        }

        #[test]
        fn renders_a_table_row_per_page_in_sorted_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A\n").expect("write a.md");
            fs::write(temp.path().join("b.md"), "# B\n").expect("write b.md");
            let table = Table {
                from: None,
                filter: vec![],
                sort: None,
                order: SortOrder::Ascending,
                columns: vec!["file.path".to_owned()],
            };

            let (rendered, count) =
                table.render(&config(temp.path())).expect("valid query");

            assert_eq!(
                rendered,
                "| file.path |\n|-----------|\n| a.md      |\n| b.md      |\n"
            );
            assert_eq!(count, 2);
        }

        #[test]
        fn sort_orders_rows_by_the_named_field_ascending_by_default() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "---\nrating: 3\n---\n")
                .expect("write a.md");
            fs::write(temp.path().join("b.md"), "---\nrating: 9\n---\n")
                .expect("write b.md");
            let table = Table {
                from: None,
                filter: vec![],
                sort: Some("rating".to_owned()),
                order: SortOrder::Ascending,
                columns: vec!["file.path".to_owned()],
            };

            let (rendered, count) =
                table.render(&config(temp.path())).expect("valid query");

            assert_eq!(
                rendered,
                "| file.path |\n|-----------|\n| a.md      |\n| b.md      |\n"
            );
            assert_eq!(count, 2);
        }

        #[test]
        fn desc_reverses_the_sort_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "---\nrating: 3\n---\n")
                .expect("write a.md");
            fs::write(temp.path().join("b.md"), "---\nrating: 9\n---\n")
                .expect("write b.md");
            let table = Table {
                from: None,
                filter: vec![],
                sort: Some("rating".to_owned()),
                order: SortOrder::Descending,
                columns: vec!["file.path".to_owned()],
            };

            let (rendered, count) =
                table.render(&config(temp.path())).expect("valid query");

            assert_eq!(
                rendered,
                "| file.path |\n|-----------|\n| b.md      |\n| a.md      |\n"
            );
            assert_eq!(count, 2);
        }

        #[test]
        fn rejects_a_malformed_sort_field_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A\n").expect("write a.md");
            let table = Table {
                from: None,
                filter: vec![],
                sort: Some("file..bad".to_owned()),
                order: SortOrder::Ascending,
                columns: vec!["file.path".to_owned()],
            };

            let error = table
                .render(&config(temp.path()))
                .expect_err("malformed sort fails");

            assert!(matches!(error, CliError::Query {
                source: QueryError::Builder(QueryBuilderError::FieldPath(_)),
                ..
            }));
        }

        #[test]
        fn renders_one_column_per_column_flag() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "---\nrating: 9\n---\n")
                .expect("write a.md");
            let table = Table {
                from: None,
                filter: vec![],
                sort: None,
                order: SortOrder::Ascending,
                columns: vec!["file.path".to_owned(), "rating".to_owned()],
            };

            let (rendered, count) =
                table.render(&config(temp.path())).expect("valid query");

            assert_eq!(
                rendered,
                "| file.path | rating |\n|-----------|--------|\n| a.md      \
                 | 9      |\n"
            );
            assert_eq!(count, 1);
        }

        #[test]
        fn escapes_a_pipe_character_in_a_column_header() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "---\n\"a|b\": 9\n---\n")
                .expect("write a.md");
            let table = Table {
                from: None,
                filter: vec![],
                sort: None,
                order: SortOrder::Ascending,
                columns: vec!["a|b".to_owned()],
            };

            let (rendered, count) =
                table.render(&config(temp.path())).expect("valid query");

            assert_eq!(rendered, "| a\\|b |\n|------|\n| 9    |\n");
            assert_eq!(count, 1);
        }

        #[test]
        fn from_tag_selects_only_matching_pages() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "#projects\n")
                .expect("write a.md");
            fs::write(temp.path().join("b.md"), "#books\n")
                .expect("write b.md");
            let table = Table {
                from: Some("#projects".to_owned()),
                filter: vec![],
                sort: None,
                order: SortOrder::Ascending,
                columns: vec!["file.path".to_owned()],
            };

            let (rendered, count) =
                table.render(&config(temp.path())).expect("valid query");

            assert_eq!(
                rendered,
                "| file.path |\n|-----------|\n| a.md      |\n"
            );
            assert_eq!(count, 1);
        }

        #[test]
        fn where_filters_by_frontmatter_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "---\nrating: 9\n---\n")
                .expect("write a.md");
            fs::write(temp.path().join("b.md"), "---\nrating: 3\n---\n")
                .expect("write b.md");
            let table = Table {
                from: None,
                filter: vec!["rating > 5".to_owned()],
                sort: None,
                order: SortOrder::Ascending,
                columns: vec!["file.path".to_owned()],
            };

            let (rendered, count) =
                table.render(&config(temp.path())).expect("valid query");

            assert_eq!(
                rendered,
                "| file.path |\n|-----------|\n| a.md      |\n"
            );
            assert_eq!(count, 1);
        }

        #[test]
        fn rejects_unparsable_filter_expression() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A\n").expect("write a.md");
            let table = Table {
                from: None,
                filter: vec!["not a valid expression".to_owned()],
                sort: None,
                order: SortOrder::Ascending,
                columns: vec!["file.path".to_owned()],
            };

            let error = table
                .render(&config(temp.path()))
                .expect_err("unparsable filter fails");

            assert!(matches!(error, CliError::Query {
                source: QueryError::Builder(QueryBuilderError::Syntax(_)),
                ..
            }));
        }

        #[test]
        fn rejects_malformed_column_field_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A\n").expect("write a.md");
            let table = Table {
                from: None,
                filter: vec![],
                sort: None,
                order: SortOrder::Ascending,
                columns: vec!["file..bad".to_owned()],
            };

            let error = table
                .render(&config(temp.path()))
                .expect_err("malformed column fails");

            assert!(matches!(error, CliError::Query {
                source: QueryError::FieldPath(_),
                ..
            }));
        }

        #[cfg(unix)]
        #[test]
        fn wraps_refresh_failure_as_a_cli_index_error() {
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
            let locked = temp.path().join("locked");
            fs::create_dir(&locked).expect("create locked dir");
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o000))
                .expect("revoke read permission");
            let _restore = RestorePermissions(&locked);
            let table = Table {
                from: None,
                filter: vec![],
                sort: None,
                order: SortOrder::Ascending,
                columns: vec!["file.path".to_owned()],
            };

            let error = table
                .render(&config(temp.path()))
                .expect_err("unreadable subdirectory fails");

            assert!(matches!(error, CliError::Index {
                source: IndexError::Builder(IndexBuilderError::Scan { .. }),
                ..
            }));
        }
    }

    mod argv {
        use clap::Parser as _;
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::cli::{Cli, Commands};

        /// Extracts the parsed `Table` args from `cli`.
        ///
        /// A dedicated `Option`-returning match so the "this is the Table
        /// subcommand" invariant is asserted through the same `.expect()`
        /// idiom as the rest of this suite, not a manual `panic!`/
        /// `unreachable!`/`assert!(false, ..)` (all denied by this crate's
        /// clippy lint policy, including inside test code).
        fn table_args(cli: &Cli) -> &Table {
            match &cli.command {
                Some(Commands::Table(table)) => Some(table),
                _ => None,
            }
            .expect("expected the Table subcommand")
        }

        #[test]
        fn parses_from_where_sort_order_and_column_flags() {
            let cli = Cli::try_parse_from([
                "traces",
                "table",
                "--from",
                "#tag",
                "--where",
                "rating > 5",
                "--sort",
                "rating",
                "--order",
                "desc",
                "--column",
                "file.path",
                "--column",
                "rating",
            ])
            .expect("parse table argv");

            let table = table_args(&cli);

            assert_eq!(table.from.as_deref(), Some("#tag"));
            assert_eq!(table.filter, vec!["rating > 5".to_owned()]);
            assert_eq!(table.sort.as_deref(), Some("rating"));
            assert_eq!(table.order, SortOrder::Descending);
            assert_eq!(table.columns, ["file.path", "rating"]);
        }

        #[test]
        fn repeated_where_flags_collect_into_one_vec_per_occurrence() {
            let cli = Cli::try_parse_from([
                "traces",
                "table",
                "--where",
                "rating > 2",
                "--where",
                "rating < 9",
                "--column",
                "file.path",
            ])
            .expect("parse table argv");

            let table = table_args(&cli);

            assert_eq!(table.filter, vec![
                "rating > 2".to_owned(),
                "rating < 9".to_owned()
            ]);
        }

        #[test]
        fn defaults_from_where_sort_to_none_and_order_to_ascending() {
            let cli = Cli::try_parse_from([
                "traces",
                "table",
                "--column",
                "file.path",
            ])
            .expect("parse table argv");

            let table = table_args(&cli);

            assert_eq!(table.from, None);
            assert_eq!(table.filter, Vec::<String>::new());
            assert_eq!(table.sort, None);
            assert_eq!(table.order, SortOrder::Ascending);
        }

        #[test]
        fn order_flag_accepts_the_shortened_asc_and_desc_values() {
            for (value, expected) in
                [("asc", SortOrder::Ascending), ("desc", SortOrder::Descending)]
            {
                let cli = Cli::try_parse_from([
                    "traces",
                    "table",
                    "--column",
                    "file.path",
                    "--order",
                    value,
                ])
                .expect("parse table argv");

                assert_eq!(table_args(&cli).order, expected);
            }
        }

        #[test]
        fn order_flag_rejects_the_unshortened_ascending_spelling() {
            let error = Cli::try_parse_from([
                "traces",
                "table",
                "--column",
                "file.path",
                "--order",
                "ascending",
            ])
            .expect_err("--order only accepts the shortened asc/desc values");

            assert_eq!(error.kind(), clap::error::ErrorKind::InvalidValue);
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
            fs::write(root.join("a.md"), "# A\n").expect("write a.md");
            let _guard = CwdGuard::enter(&root);
            let table = Table {
                from: None,
                filter: vec![],
                sort: None,
                order: SortOrder::Ascending,
                columns: vec!["file.path".to_owned()],
            };

            table.run(&service).expect("run table command");
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
            let table = Table {
                from: None,
                filter: vec![],
                sort: None,
                order: SortOrder::Ascending,
                columns: vec!["file.path".to_owned()],
            };

            let error = table.run(&service).expect_err("untrusted root fails");

            assert!(matches!(error, CliError::ConfigLoad {
                source: ConfigLoadError::Build(_),
                ..
            }));
        }
    }
}
