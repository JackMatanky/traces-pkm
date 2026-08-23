//! Display formatting for query results.

use std::fmt::Write as _;

use super::{
    QueryError, QueryRecordSet,
    grammar::FieldPath,
    record::{QueryFieldValueRef, QueryListValueRef},
};
use crate::note::NoteFieldValue;

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

impl QueryFieldValueRef<'_> {
    fn append_text(&self, out: &mut String) {
        match self {
            Self::Null => {}
            Self::Bool(value) => out.push_str(if *value {
                "true"
            } else {
                "false"
            }),
            Self::Number(value) => {
                let _ = write!(out, "{value}");
            }
            Self::Text(value) | Self::Date(value) | Self::Duration(value) => {
                out.push_str(value);
            }
            Self::Link(link) => out.push_str(link.target()),
            Self::Object(fields) => {
                for (idx, (key, field)) in fields.iter().enumerate() {
                    if idx > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(key);
                    out.push_str(": ");
                    Self::from(field).append_text(out);
                }
            }
            Self::List(list) => list.append_text(out),
            Self::Owned(value) => append_owned_field_text(out, value),
        }
    }

    fn text(&self) -> String {
        let mut out = String::new();
        self.append_text(&mut out);
        out
    }

    fn table_cell_text(&self) -> String {
        escape_table_text(&self.text())
    }
}

impl QueryListValueRef<'_> {
    fn append_text(&self, out: &mut String) {
        match self {
            Self::Values(values) => {
                append_joined(out, values, append_owned_field_text);
            }
            Self::Tags(tags) => {
                append_joined(out, tags, |out, tag| out.push_str(tag.as_str()));
            }
            Self::Inlinks(inlinks) => {
                append_joined(out, inlinks, |out, path| {
                    out.push_str(&path.to_string_lossy());
                });
            }
        }
    }
}

fn append_joined<T>(
    out: &mut String,
    values: &[T],
    mut append: impl FnMut(&mut String, &T),
) {
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        append(out, value);
    }
}

fn append_owned_field_text(out: &mut String, value: &NoteFieldValue) {
    match value {
        NoteFieldValue::Null => {}
        NoteFieldValue::Bool(value) => out.push_str(&value.to_string()),
        NoteFieldValue::Number(value) => out.push_str(&value.to_string()),
        NoteFieldValue::String(value)
        | NoteFieldValue::Date(value)
        | NoteFieldValue::Duration(value) => out.push_str(value),
        NoteFieldValue::Link(link) => out.push_str(link.target()),
        NoteFieldValue::List(items) => {
            append_joined(out, items, append_owned_field_text);
        }
        NoteFieldValue::Object(fields) => {
            for (idx, (key, field)) in fields.iter().enumerate() {
                if idx > 0 {
                    out.push_str(", ");
                }
                out.push_str(key);
                out.push_str(": ");
                append_owned_field_text(out, field);
            }
        }
    }
}

fn escape_table_text(text: &str) -> String {
    text.replace('\n', " ").replace('|', "\\|")
}
