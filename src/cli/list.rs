//! Page-level list query command.
//!
//! Handles `traces list` by refreshing the trusted root's [`FileIndex`],
//! selecting a source scope, applying the optional filter, and printing
//! matching pages as a Markdown bullet list.
//!
//! [`FileIndex`]: crate::index::FileIndex

use clap::Args;

use super::error::CliError;
use crate::{
    config::{Config, ConfigService},
    query::SortOrder,
};

/// Field path rendered for each `traces list` bullet.
const LIST_FIELD: &str = "file.path";

/// Arguments for `traces list`.
///
/// Queries pages from the trusted project root and prints matching file paths
/// as a Markdown bullet list.
#[derive(Debug, Args)]
pub(super) struct List {
    /// Source expression, e.g. `#tag`, `folder/`, `@Class*`, or a boolean
    /// combination. Omit to query every indexed page.
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
}

impl List {
    /// Runs `traces list` for the trusted project root.
    ///
    /// Refreshes the root's [`FileIndex`] and writes one Markdown bullet per
    /// matching page to stdout.
    ///
    /// # Errors
    ///
    /// - [`CliError::CurrentDirectory`] if the current directory cannot be
    ///   read.
    /// - [`CliError::ConfigLoad`] if loading configuration fails, including an
    ///   untrusted project root.
    /// - [`CliError::Index`] if refreshing the [`FileIndex`] fails.
    /// - [`CliError::Query`] if `--where` is an unparsable filter expression or
    ///   `--sort` names a malformed field path.
    ///
    /// [`FileIndex`]: crate::index::FileIndex
    #[expect(
        clippy::print_stdout,
        reason = "list output is primary command output, not diagnostic text; \
                  mirrors the precedent in crate::cli::task"
    )]
    pub(super) fn run(&self, service: &ConfigService) -> Result<(), CliError> {
        let config = super::load_config(service)?;
        let root = config.root();
        let (rendered, count) = self.render(&config)?;
        print!("{rendered}");
        eprintln!("{count} page(s) from {}", root.display());
        Ok(())
    }

    /// Renders matching pages from `root`'s [`FileIndex`] as a Markdown bullet
    /// list, alongside the matched row count.
    ///
    /// Split from [`Self::run`] so tests can assert on rendered content without
    /// capturing process stdout.
    ///
    /// # Errors
    ///
    /// - [`CliError::Index`] if refreshing the [`FileIndex`] fails.
    /// - [`CliError::Query`] if `--where` is an unparsable filter expression or
    ///   `--sort` names a malformed field path.
    ///
    /// [`FileIndex`]: crate::index::FileIndex
    fn render(&self, config: &Config) -> Result<(String, usize), CliError> {
        let root = config.root();
        let outcome = super::refresh_page_query(config, self.from.as_deref())?;
        let outcome =
            super::apply_filter(outcome, root, self.filter.as_deref())?;
        let outcome = super::apply_sort(
            outcome,
            root,
            self.sort.as_deref(),
            self.order.is_descending(),
        )?;
        let count = outcome.len();
        let rendered = outcome
            .list(LIST_FIELD)
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
        use crate::{index::FileIndexError, query::QueryError};

        fn config(root: &Path) -> Config {
            Config::for_test(root.to_path_buf(), None, None, root.to_path_buf())
        }

        #[test]
        fn renders_a_bullet_per_page_path_in_sorted_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A\n").expect("write a.md");
            fs::write(temp.path().join("b.md"), "# B\n").expect("write b.md");
            let list = List {
                from: None,
                filter: None,
                sort: None,
                order: SortOrder::Ascending,
            };

            let (rendered, count) =
                list.render(&config(temp.path())).expect("valid query");

            assert_eq!(rendered, "- a.md\n- b.md\n");
            assert_eq!(count, 2);
        }

        #[test]
        fn sort_orders_by_the_named_field_ascending_by_default() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "---\nrating: 3\n---\n")
                .expect("write a.md");
            fs::write(temp.path().join("b.md"), "---\nrating: 9\n---\n")
                .expect("write b.md");
            let list = List {
                from: None,
                filter: None,
                sort: Some("rating".to_owned()),
                order: SortOrder::Ascending,
            };

            let (rendered, count) =
                list.render(&config(temp.path())).expect("valid query");

            assert_eq!(rendered, "- a.md\n- b.md\n");
            assert_eq!(count, 2);
        }

        #[test]
        fn desc_reverses_the_sort_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "---\nrating: 3\n---\n")
                .expect("write a.md");
            fs::write(temp.path().join("b.md"), "---\nrating: 9\n---\n")
                .expect("write b.md");
            let list = List {
                from: None,
                filter: None,
                sort: Some("rating".to_owned()),
                order: SortOrder::Descending,
            };

            let (rendered, count) =
                list.render(&config(temp.path())).expect("valid query");

            assert_eq!(rendered, "- b.md\n- a.md\n");
            assert_eq!(count, 2);
        }

        #[test]
        fn rejects_a_malformed_sort_field_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A\n").expect("write a.md");
            let list = List {
                from: None,
                filter: None,
                sort: Some("file..bad".to_owned()),
                order: SortOrder::Ascending,
            };

            let error = list
                .render(&config(temp.path()))
                .expect_err("malformed sort fails");

            assert!(matches!(error, CliError::Query {
                source: QueryError::FieldPath(_),
                ..
            }));
        }

        #[test]
        fn from_tag_selects_only_matching_pages() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "#projects\n")
                .expect("write a.md");
            fs::write(temp.path().join("b.md"), "#books\n")
                .expect("write b.md");
            let list = List {
                from: Some("#projects".to_owned()),
                filter: None,
                sort: None,
                order: SortOrder::Ascending,
            };

            let (rendered, count) =
                list.render(&config(temp.path())).expect("valid query");

            assert_eq!(rendered, "- a.md\n");
            assert_eq!(count, 1);
        }

        #[test]
        fn from_folder_selects_only_matching_pages() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("projects")).expect("mkdir");
            fs::write(temp.path().join("projects/a.md"), "# A\n")
                .expect("write projects/a.md");
            fs::write(temp.path().join("b.md"), "# B\n").expect("write b.md");
            let list = List {
                from: Some("projects".to_owned()),
                filter: None,
                sort: None,
                order: SortOrder::Ascending,
            };

            let (rendered, count) =
                list.render(&config(temp.path())).expect("valid query");

            assert_eq!(rendered, "- projects/a.md\n");
            assert_eq!(count, 1);
        }

        #[test]
        fn from_parses_and_resolves_the_shared_source_dsl() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let schemas = temp.path().join(".traces/schemas");
            fs::create_dir_all(&schemas).expect("create Schema registry");
            fs::create_dir_all(temp.path().join("books"))
                .expect("create books");
            fs::write(schemas.join("book.toml"), "")
                .expect("write book Schema");
            fs::write(schemas.join("sci_fi.toml"), "extends = [\"book\"]\n")
                .expect("write sci_fi Schema");
            fs::write(
                temp.path().join("books/dune.md"),
                "---\nclass: sci_fi\n---\n#book\n",
            )
            .expect("write dune");
            fs::write(
                temp.path().join("books/movie.md"),
                "---\nclass: movie\n---\n#movie\n",
            )
            .expect("write movie");
            let list = List {
                from: Some(
                    "class(book).with_children() and \"books/dune.md\""
                        .to_owned(),
                ),
                filter: None,
                sort: None,
                order: SortOrder::Ascending,
            };

            let (rendered, count) =
                list.render(&config(temp.path())).expect("valid query");

            assert_eq!(rendered, "- books/dune.md\n");
            assert_eq!(count, 1);
        }

        #[test]
        fn from_reports_an_unparsable_source_expression() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "#book\n").expect("write a.md");
            let list = List {
                from: Some("#book and".to_owned()),
                filter: None,
                sort: None,
                order: SortOrder::Ascending,
            };

            let error =
                list.render(&config(temp.path())).expect_err("invalid source");

            assert!(matches!(error, CliError::Query {
                source: QueryError::Syntax(_),
                ..
            }));
        }

        #[test]
        fn where_filters_by_frontmatter_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "---\nrating: 9\n---\n")
                .expect("write a.md");
            fs::write(temp.path().join("b.md"), "---\nrating: 3\n---\n")
                .expect("write b.md");
            let list = List {
                from: None,
                filter: Some("rating > 5".to_owned()),
                sort: None,
                order: SortOrder::Ascending,
            };

            let (rendered, count) =
                list.render(&config(temp.path())).expect("valid query");

            assert_eq!(rendered, "- a.md\n");
            assert_eq!(count, 1);
        }

        #[test]
        fn rejects_unparsable_filter_expression() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A\n").expect("write a.md");
            let list = List {
                from: None,
                filter: Some("not a valid expression".to_owned()),
                sort: None,
                order: SortOrder::Ascending,
            };

            let error = list
                .render(&config(temp.path()))
                .expect_err("unparsable filter fails");

            assert!(matches!(error, CliError::Query {
                source: QueryError::Syntax(_),
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
            let list = List {
                from: None,
                filter: None,
                sort: None,
                order: SortOrder::Ascending,
            };

            let error = list
                .render(&config(temp.path()))
                .expect_err("unreadable subdirectory fails");

            assert!(matches!(error, CliError::Index {
                source: FileIndexError::Io { .. },
                ..
            }));
        }
    }

    mod argv {
        use clap::Parser as _;
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::cli::{Cli, Commands};

        /// Extracts the parsed `List` args from `cli`.
        ///
        /// A dedicated `Option`-returning match so the "this is the List
        /// subcommand" invariant is asserted through the same `.expect()`
        /// idiom as the rest of this suite, not a manual `panic!`/
        /// `unreachable!`/`assert!(false, ..)` (all denied by this crate's
        /// clippy lint policy, including inside test code).
        fn list_args(cli: &Cli) -> &List {
            match &cli.command {
                Some(Commands::List(list)) => Some(list),
                _ => None,
            }
            .expect("expected the List subcommand")
        }

        #[test]
        fn parses_from_where_sort_and_order_flags() {
            let cli = Cli::try_parse_from([
                "traces",
                "list",
                "--from",
                "#tag",
                "--where",
                "rating > 5",
                "--sort",
                "rating",
                "--order",
                "desc",
            ])
            .expect("parse list argv");

            let list = list_args(&cli);

            assert_eq!(list.from.as_deref(), Some("#tag"));
            assert_eq!(list.filter.as_deref(), Some("rating > 5"));
            assert_eq!(list.sort.as_deref(), Some("rating"));
            assert_eq!(list.order, SortOrder::Descending);
        }

        #[test]
        fn defaults_from_where_sort_to_none_and_order_to_ascending() {
            let cli = Cli::try_parse_from(["traces", "list"])
                .expect("parse list argv");

            let list = list_args(&cli);

            assert_eq!(list.from, None);
            assert_eq!(list.filter, None);
            assert_eq!(list.sort, None);
            assert_eq!(list.order, SortOrder::Ascending);
        }

        #[test]
        fn order_flag_accepts_the_shortened_asc_and_desc_values() {
            for (value, expected) in
                [("asc", SortOrder::Ascending), ("desc", SortOrder::Descending)]
            {
                let cli =
                    Cli::try_parse_from(["traces", "list", "--order", value])
                        .expect("parse list argv");

                assert_eq!(list_args(&cli).order, expected);
            }
        }

        #[test]
        fn order_flag_rejects_the_unshortened_ascending_spelling() {
            let error =
                Cli::try_parse_from(["traces", "list", "--order", "ascending"])
                    .expect_err(
                        "--order only accepts the shortened asc/desc values",
                    );

            assert_eq!(error.kind(), clap::error::ErrorKind::InvalidValue);
        }
    }

    mod run {
        use std::fs;

        use super::*;
        use crate::cli::CwdGuard;

        #[test]
        fn succeeds_for_a_trusted_project_root() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("project");
            let service = service(temp.path());
            create_trusted_project(&service, &root);
            fs::write(root.join("a.md"), "# A\n").expect("write a.md");
            let _guard = CwdGuard::enter(&root);
            let list = List {
                from: None,
                filter: None,
                sort: None,
                order: SortOrder::Ascending,
            };

            list.run(&service).expect("run list command");
        }
    }
}
