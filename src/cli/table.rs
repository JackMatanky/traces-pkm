//! Page-level table query command.
//!
//! Handles `traces table` by refreshing the trusted root's [`FileIndex`],
//! selecting a source scope, applying the optional filter, and printing
//! matching pages as a Markdown table.

use std::path::Path;

use clap::Args;

use super::error::CliError;
use crate::{
    config::ConfigService,
    index::{FileIndex, QueryError, SortOrder, Source},
};

/// Command-line arguments for `traces table`.
#[derive(Debug, Args)]
pub(super) struct Table {
    /// Page source: a `#tag` (including nested sub-tags) or a folder path.
    /// Omit to query every indexed page.
    #[arg(long)]
    from: Option<String>,
    /// Filter expression narrowing results, e.g. `"file.folder == \"books\""`.
    #[arg(long = "where")]
    filter: Option<String>,
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
    #[expect(
        clippy::print_stdout,
        reason = "table output is primary command output, not diagnostic \
                  text; mirrors the precedent in crate::cli::task"
    )]
    pub(super) fn run(&self, service: &ConfigService) -> Result<(), CliError> {
        let config = super::load_config(service)?;
        let root = config.root();
        let (rendered, count) = self.render(root)?;
        print!("{rendered}");
        eprintln!("{count} page(s) from {}", root.display());
        Ok(())
    }

    /// Renders matching pages from `root`'s [`FileIndex`] as a Markdown
    /// table, alongside the matched row count.
    ///
    /// Split from [`Self::run`] so tests can assert on rendered content
    /// without capturing process stdout.
    ///
    /// # Errors
    ///
    /// - [`CliError::Index`] if refreshing the [`FileIndex`] fails.
    /// - [`CliError::Query`] if `--where` is an unparsable filter expression,
    ///   `--sort` names a malformed field path, or `--column` names a malformed
    ///   field path.
    fn render(&self, root: &Path) -> Result<(String, usize), CliError> {
        let index =
            FileIndex::refresh(root).map_err(|source| CliError::Index {
                root: root.to_path_buf(),
                source,
            })?;
        let mut outcome = index.query(&Source::from_flag(self.from.as_deref()));
        if let Some(expr) = self.filter.as_deref() {
            outcome = outcome
                .filter(expr)
                .map_err(|source| query_error(root, source))?;
        }
        if let Some(path) = self.sort.as_deref() {
            outcome = outcome
                .sort(path, self.order.is_descending())
                .map_err(|source| query_error(root, source))?;
        }
        let count = outcome.len();
        let columns =
            self.columns.iter().map(String::as_str).collect::<Vec<_>>();
        // `QueryOutcome::table` escapes both headers and cell values, so
        // a `--column` value doubling as its header stays row-safe without
        // the CLI escaping it a second time.
        let rendered = outcome
            .table(&columns, &columns)
            .map_err(|source| query_error(root, source))?;
        Ok((rendered, count))
    }
}

/// Wraps a [`QueryError`] as a [`CliError::Query`] against `root`.
///
/// Shared by every `outcome.*` call in [`Table::render`] so each call site
/// is a single `.map_err(|source| query_error(root, source))?`.
fn query_error(root: &Path, source: QueryError) -> CliError {
    CliError::Query {
        root: root.to_path_buf(),
        source,
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

    mod render {
        use std::fs;

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::index::QueryError;

        #[test]
        fn renders_a_table_row_per_page_in_sorted_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A\n").expect("write a.md");
            fs::write(temp.path().join("b.md"), "# B\n").expect("write b.md");
            let table = Table {
                from: None,
                filter: None,
                sort: None,
                order: SortOrder::Ascending,
                columns: vec!["file.path".to_owned()],
            };

            let (rendered, count) =
                table.render(temp.path()).expect("valid query");

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
                filter: None,
                sort: Some("rating".to_owned()),
                order: SortOrder::Ascending,
                columns: vec!["file.path".to_owned()],
            };

            let (rendered, count) =
                table.render(temp.path()).expect("valid query");

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
                filter: None,
                sort: Some("rating".to_owned()),
                order: SortOrder::Descending,
                columns: vec!["file.path".to_owned()],
            };

            let (rendered, count) =
                table.render(temp.path()).expect("valid query");

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
                filter: None,
                sort: Some("file..bad".to_owned()),
                order: SortOrder::Ascending,
                columns: vec!["file.path".to_owned()],
            };

            let error =
                table.render(temp.path()).expect_err("malformed sort fails");

            assert!(matches!(error, CliError::Query {
                source: QueryError::UnknownFieldPath { .. },
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
                filter: None,
                sort: None,
                order: SortOrder::Ascending,
                columns: vec!["file.path".to_owned(), "rating".to_owned()],
            };

            let (rendered, count) =
                table.render(temp.path()).expect("valid query");

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
                filter: None,
                sort: None,
                order: SortOrder::Ascending,
                columns: vec!["a|b".to_owned()],
            };

            let (rendered, count) =
                table.render(temp.path()).expect("valid query");

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
                filter: None,
                sort: None,
                order: SortOrder::Ascending,
                columns: vec!["file.path".to_owned()],
            };

            let (rendered, count) =
                table.render(temp.path()).expect("valid query");

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
                filter: Some("rating > 5".to_owned()),
                sort: None,
                order: SortOrder::Ascending,
                columns: vec!["file.path".to_owned()],
            };

            let (rendered, count) =
                table.render(temp.path()).expect("valid query");

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
                filter: Some("not a valid expression".to_owned()),
                sort: None,
                order: SortOrder::Ascending,
                columns: vec!["file.path".to_owned()],
            };

            let error =
                table.render(temp.path()).expect_err("unparsable filter fails");

            assert!(matches!(error, CliError::Query {
                source: QueryError::UnparsableFilterExpression { .. },
                ..
            }));
        }

        #[test]
        fn rejects_malformed_column_field_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A\n").expect("write a.md");
            let table = Table {
                from: None,
                filter: None,
                sort: None,
                order: SortOrder::Ascending,
                columns: vec!["file..bad".to_owned()],
            };

            let error =
                table.render(temp.path()).expect_err("malformed column fails");

            assert!(matches!(error, CliError::Query {
                source: QueryError::UnknownFieldPath { .. },
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
            fs::write(root.join("a.md"), "# A\n").expect("write a.md");
            let _guard = CwdGuard::enter(&root);
            let table = Table {
                from: None,
                filter: None,
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
                filter: None,
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
