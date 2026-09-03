//! Markdown display formats for query result rows.

use super::{QueryError, QueryResult, grammar::FieldPath, results::QueryRow};

/// Whether [`QueryDisplayFormat::TaskList`] appends each row's file path.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) enum TaskPathStyle {
    /// `- [x] text` — used by the template `tasks` namespace.
    #[default]
    None,
    /// `- [x] text (path)` — used by `traces task`.
    Suffix,
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
    /// Builds a table display format.
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

    /// Builds a bullet-list display format.
    #[must_use]
    pub(super) fn list(field: &str) -> Self {
        Self::List {
            field: field.to_owned(),
        }
    }

    /// Builds a task-list display format.
    #[must_use]
    pub(super) const fn task_list(path_style: TaskPathStyle) -> Self {
        Self::TaskList {
            path_style,
        }
    }

    /// Renders `records` according to this display format.
    ///
    /// # Errors
    ///
    /// Returns query errors for malformed field paths, table column
    /// mismatches, or task-list rendering on page rows.
    pub(super) fn render(&self, records: &[QueryRow]) -> QueryResult<String> {
        match self {
            Self::Table {
                headers,
                columns,
            } => Self::render_table(headers, columns, records),
            Self::List {
                field,
            } => Self::render_list(field, records),
            Self::TaskList {
                path_style,
            } => Self::render_task_list(records, *path_style),
        }
    }

    fn render_table(
        headers: &[String],
        columns: &[String],
        records: &[QueryRow],
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
        table
            .set_header(headers.iter().map(|header| escape_table_text(header)));
        for record in records {
            table.add_row(
                paths
                    .iter()
                    .map(|path| record.resolve_ref(path).table_cell_text()),
            );
        }
        let mut out = table.to_string();
        out.push('\n');
        Ok(out)
    }

    fn render_list(field: &str, records: &[QueryRow]) -> QueryResult<String> {
        let field_path = FieldPath::parse(field)?;
        let mut out = String::new();
        for record in records {
            out.push_str("- ");
            record.resolve_ref(&field_path).append_text(&mut out);
            out.push('\n');
        }
        Ok(out)
    }

    fn render_task_list(
        records: &[QueryRow],
        path_style: TaskPathStyle,
    ) -> QueryResult<String> {
        use std::fmt::Write as _;

        let mut out = String::new();
        for record in records {
            let Some(text) = record.task_text() else {
                return Err(QueryError::TaskListRequiresTaskRows);
            };
            out.push_str(match record.task_completed() {
                Some(true) => "- [x] ",
                Some(false) => "- [ ] ",
                None => "- [-] ",
            });
            out.push_str(text);
            match path_style {
                TaskPathStyle::Suffix => {
                    let _ =
                        write!(out, " ({})", record.file().path().display());
                }
                TaskPathStyle::None => {}
            }
            out.push('\n');
        }
        Ok(out)
    }
}

pub(super) fn escape_table_text(text: &str) -> String {
    text.replace('\n', " ").replace('|', "\\|")
}
