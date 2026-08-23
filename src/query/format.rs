//! Display formatting for query results.

use super::{
    QueryError, QueryRecordSet, grammar::FieldPath, value::escape_table_text,
};

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
    /// Markdown task list rendered from task rows.
    TaskList,
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
    pub(super) const fn task_list() -> Self {
        Self::TaskList
    }
}

impl QueryRecordSet {
    /// Formats this record set for display.
    ///
    /// # Errors
    ///
    /// Returns existing query errors for malformed field paths, table column
    /// mismatches, or task-list rendering on page rows.
    pub(super) fn format(
        &self,
        format: &QueryDisplayFormat,
    ) -> Result<String, QueryError> {
        match format {
            QueryDisplayFormat::Table {
                headers,
                columns,
            } => self.format_table(headers, columns),
            QueryDisplayFormat::List {
                field,
            } => self.format_list(field),
            QueryDisplayFormat::TaskList => self.format_task_list(),
        }
    }

    fn format_table(
        &self,
        headers: &[String],
        columns: &[String],
    ) -> Result<String, QueryError> {
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
        for record in self {
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

    fn format_list(&self, field: &str) -> Result<String, QueryError> {
        let field_path = FieldPath::parse(field)?;
        let mut out = String::new();
        for record in self {
            out.push_str("- ");
            record.resolve_ref(&field_path).append_text(&mut out);
            out.push('\n');
        }
        Ok(out)
    }

    fn format_task_list(&self) -> Result<String, QueryError> {
        let mut out = String::new();
        for record in self {
            let Some(completed) = record.task_completed() else {
                return Err(QueryError::TaskListRequiresTaskRows);
            };
            out.push_str(if completed {
                "- [x] "
            } else {
                "- [ ] "
            });
            out.push_str(record.task_text().unwrap_or_default());
            out.push('\n');
        }
        Ok(out)
    }
}
