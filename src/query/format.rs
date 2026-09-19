//! Markdown formatting and rendering for query result collections.
//!
//! This module converts transformed [`QueryRow`] items into user-facing
//! Markdown formats, including GitHub-flavored Markdown tables, bulleted lists,
//! and task checkbox lists. It supports optional file-path parenthetical
//! suffixes via [`TaskPathStyle`] to disambiguate task origin in CLI task
//! aggregation.
use super::{QueryError, QueryResult, grammar::FieldPath, results::QueryRow};

/// Controls file-path rendering in task list output.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum TaskPathStyle {
    /// Omit paths for template rendering.
    #[default]
    None,
    /// Append paths in parentheses for `traces task`.
    Suffix,
    /// Append file path and 1-indexed source line coordinates for
    /// `traces task -l`.
    Coordinates,
}

/// Markdown display formats supported by query results.
pub(super) enum QueryDisplayFormat {
    /// Markdown table with display headers and field-path columns.
    Table {
        headers: Vec<String>,
        columns: Vec<String>,
    },
    /// Markdown bullet list rendered from one field path.
    List {
        field: String,
    },
    /// Markdown task list rendered from task rows, optionally suffixed with
    /// each row's file path.
    TaskList {
        path_style: TaskPathStyle,
    },
}

impl QueryDisplayFormat {
    #[must_use]
    pub(super) fn table(headers: &[&str], columns: &[&str]) -> Self {
        Self::Table {
            headers: headers
                .iter()
                .map(|header| (*header).to_owned())
                .collect(),
            columns: columns
                .iter()
                .map(|column| (*column).to_owned())
                .collect(),
        }
    }

    #[must_use]
    pub(super) fn list(field: &str) -> Self {
        Self::List {
            field: field.to_owned(),
        }
    }

    #[must_use]
    pub(super) const fn task_list(path_style: TaskPathStyle) -> Self {
        Self::TaskList {
            path_style,
        }
    }

    /// Renders `rows` according to this display format.
    ///
    /// # Errors
    ///
    /// - [`FieldPath`]: a column or list field path is malformed.
    /// - [`TableColumnCountMismatch`]: `Self::Table` headers and columns differ
    ///   in length.
    /// - [`TaskListRequiresTaskRows`]: `Self::TaskList` renders page-level
    ///   rows.
    ///
    /// [`FieldPath`]: QueryError::FieldPath
    /// [`TableColumnCountMismatch`]: QueryError::TableColumnCountMismatch
    /// [`TaskListRequiresTaskRows`]: QueryError::TaskListRequiresTaskRows
    pub(super) fn render(&self, rows: &[QueryRow]) -> QueryResult<String> {
        match self {
            Self::Table {
                headers,
                columns,
            } => Self::render_table(headers, columns, rows),
            Self::List {
                field,
            } => Self::render_list(field, rows),
            Self::TaskList {
                path_style,
            } => Self::render_task_list(rows, *path_style),
        }
    }

    /// Renders a Markdown table with resolved, escaped cells.
    fn render_table(
        headers: &[String],
        columns: &[String],
        rows: &[QueryRow],
    ) -> QueryResult<String> {
        if headers.len() != columns.len() {
            return Err(QueryError::TableColumnCountMismatch {
                headers: headers.len(),
                columns: columns.len(),
            });
        }
        let paths = columns
            .iter()
            .map(|column| FieldPath::parse(column))
            .collect::<Result<Vec<_>, _>>()?;
        let mut table = comfy_table::Table::new();
        table.load_preset(comfy_table::presets::ASCII_MARKDOWN);
        table.set_header(
            headers.iter().map(|header| Self::escape_table_cell(header)),
        );
        for row in rows {
            table.add_row(paths.iter().map(|path| {
                Self::escape_table_cell(&row.resolve_ref(path).text())
            }));
        }
        let mut out = table.to_string();
        out.push('\n');
        Ok(out)
    }

    /// Renders resolved `field` values as Markdown bullets.
    fn render_list(field: &str, rows: &[QueryRow]) -> QueryResult<String> {
        let field_path = FieldPath::parse(field)?;
        let mut out = String::new();
        for row in rows {
            out.push_str("- ");
            row.resolve_ref(&field_path).append_text(&mut out);
            out.push('\n');
        }
        Ok(out)
    }

    /// Renders task rows as Markdown checkbox lines.
    ///
    /// # Errors
    ///
    /// - [`QueryError::TaskListRequiresTaskRows`] if any row in `rows` is not a
    ///   task row.
    fn render_task_list(
        rows: &[QueryRow],
        path_style: TaskPathStyle,
    ) -> QueryResult<String> {
        use std::fmt::Write as _;

        let mut out = String::with_capacity(rows.len().saturating_mul(48));
        for row in rows {
            let Some(text) = row.task_text() else {
                return Err(QueryError::TaskListRequiresTaskRows);
            };
            for _ in 0..row.depth() {
                out.push_str("  ");
            }
            let symbol = row.status_symbol().map_or_else(
                || match row.task_completed() {
                    Some(true) => 'x',
                    Some(false) => ' ',
                    None => '-',
                },
                |symbol| symbol.as_char(),
            );
            let _ = write!(out, "- [{symbol}] {text}");
            match path_style {
                TaskPathStyle::Suffix => {
                    let _ = write!(out, " ({})", row.file().path().display());
                }
                TaskPathStyle::Coordinates => {
                    if let Some(line) = row.line() {
                        let _ = write!(
                            out,
                            " ({}:{})",
                            row.file().path().display(),
                            line
                        );
                    } else {
                        let _ =
                            write!(out, " ({})", row.file().path().display());
                    }
                }
                TaskPathStyle::None => {}
            }
            out.push('\n');
        }
        Ok(out)
    }

    /// Escapes Markdown table cell text by replacing newlines with spaces and
    /// escaping pipes. Short-circuits to a plain copy when neither character is
    /// present, avoiding the two intermediate allocations a chained
    /// `.replace().replace()` would otherwise cost every cell.
    fn escape_table_cell(text: &str) -> String {
        if !text.contains(['\n', '|']) {
            return text.to_owned();
        }
        let mut out = String::with_capacity(text.len());
        for ch in text.chars() {
            match ch {
                '\n' => out.push(' '),
                '|' => out.push_str("\\|"),
                other => out.push(other),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod escape_table_cell {
        use pretty_assertions::assert_eq;

        use super::QueryDisplayFormat;

        #[test]
        fn escapes_pipe_characters_in_table_cells() {
            assert_eq!(
                QueryDisplayFormat::escape_table_cell("A | B"),
                "A \\| B"
            );
        }

        #[test]
        fn replaces_newlines_with_spaces_in_table_cells() {
            assert_eq!(
                QueryDisplayFormat::escape_table_cell("line1\nline2"),
                "line1 line2"
            );
        }

        #[test]
        fn passes_plain_text_unmodified() {
            assert_eq!(
                QueryDisplayFormat::escape_table_cell("hello world"),
                "hello world"
            );
        }

        #[test]
        fn escapes_pipes_and_newlines_together() {
            assert_eq!(
                QueryDisplayFormat::escape_table_cell("A\n| B\n"),
                "A \\| B "
            );
        }

        #[test]
        fn escapes_consecutive_pipes() {
            assert_eq!(QueryDisplayFormat::escape_table_cell("||"), "\\|\\|");
        }

        #[test]
        fn passes_empty_string_unmodified() {
            assert_eq!(QueryDisplayFormat::escape_table_cell(""), "");
        }
    }

    mod render_task_list {
        use std::{fs, sync::Arc};

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::{
            index::IndexerService,
            query::{QueryBuilder, QueryService, SourceSelector},
        };

        fn outcome_for_tasks(
            temp: &std::path::Path,
            source: &str,
        ) -> crate::query::QuerySet {
            fs::write(temp.join("todo.md"), source).expect("write todo.md");
            let index = Arc::new(
                IndexerService::new(temp).build().expect("build index"),
            );
            QueryService::new("class")
                .run(&index, QueryBuilder::tasks(SourceSelector::All))
        }

        #[test]
        fn preserves_custom_status_markers() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let source = "- [ ] buy milk\n- [x] pay rent\n- [/] in \
                          progress\n- [-] cancelled\n- [!] urgent\n- [?] \
                          question\n";
            let outcome = outcome_for_tasks(temp.path(), source);
            let rendered = outcome
                .task_list(TaskPathStyle::None)
                .expect("render task list");
            assert_eq!(
                rendered,
                "- [ ] buy milk\n- [x] pay rent\n- [/] in progress\n- [-] \
                 cancelled\n- [!] urgent\n- [?] question\n"
            );
        }

        #[test]
        fn indents_nested_tasks_by_two_spaces_per_depth() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let source = "- [ ] parent\n  - [/] child\n    - [x] grandchild\n";
            let outcome = outcome_for_tasks(temp.path(), source);
            let rendered = outcome
                .task_list(TaskPathStyle::None)
                .expect("render task list");
            assert_eq!(
                rendered,
                "- [ ] parent\n  - [/] child\n    - [x] grandchild\n"
            );
        }

        #[test]
        fn formats_suffix_path_style() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let source = "- [ ] buy milk\n";
            let outcome = outcome_for_tasks(temp.path(), source);
            let rendered = outcome
                .task_list(TaskPathStyle::Suffix)
                .expect("render task list");
            assert_eq!(rendered, "- [ ] buy milk (todo.md)\n");
        }

        #[test]
        fn formats_coordinate_path_style_with_line_numbers() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let source = "# Header\n\n- [ ] first\n  - [x] second\n";
            let outcome = outcome_for_tasks(temp.path(), source);
            let rendered = outcome
                .task_list(TaskPathStyle::Coordinates)
                .expect("render task list");
            assert_eq!(
                rendered,
                "- [ ] first (todo.md:3)\n  - [x] second (todo.md:4)\n"
            );
        }

        #[test]
        fn rejects_non_task_rows_with_error() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("page.md"), "# Heading\n")
                .expect("write page.md");
            let index = Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            let outcome = QueryService::new("class")
                .run(&index, QueryBuilder::pages(SourceSelector::All));
            let error = outcome
                .task_list(TaskPathStyle::None)
                .expect_err("non-task rows must fail");
            assert!(matches!(error, QueryError::TaskListRequiresTaskRows));
        }
    }
}
