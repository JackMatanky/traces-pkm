# 08 — Zero-allocation QueryRow and canonical list.<field> resolution

**Status:** ready-for-agent

**What to build:** Implement zero-allocation positional list rows on
`QueryRow` via `RowKind::List { item_idx: u32 }`, replacing `TaskRow` and
eliminating string clones during query expansion. Replace `FieldPath::Task`
with `FieldPath::List(ListField)`, embedding `TaskField` within `ListField` to
represent task-specific fields while presenting a unified, canonical
`list.<field>` query interface with no `task.*` aliasing. Resolve list field
values as borrowed references directly from the in-memory `Note` in
`FileIndex`. Implement inline metadata precedence and add `file.tags`.

**Blocked by:** 07 (needs flat `Note.lists` storage and compacted `ListItem`).

## Key Interfaces and Models

- **Positional Row Representation:**

  ```rust
  pub(crate) enum RowKind {
      Page,
      List { item_idx: u32 },
  }
  ```

  `QueryRow` accesses its list item in O(1) via
  `self.entry().note().unwrap().list_items()[item_idx as usize]`.

- **Field Path Typestate & Embedded TaskField:**

  ```rust
  pub(crate) enum TaskField {
      Status,
      StatusType,
      StatusSymbol,
      Completed,
      Priority,
      Due,
      Done,
      Created,
      Start,
      Scheduled,
      Cancelled,
      FullyComplete,
  }

  pub(crate) enum ListField {
      Text,
      RawText,
      Line,
      Parent,
      Depth,
      Tags,
      IsTask,
      Kind,
      IsOrdered,
      Task(TaskField),
  }
  ```

- **Query Syntax & Resolution:**
  - Canonical namespace is strictly `list.<field>`. Users write `list.text`,
    `list.due`, `list.status`.
  - `FieldPath::parse` routes all `list.<field>` paths through
    `ListField::parse`, which checks universal fields first, then task fields.
  - Queries using obsolete `task.<field>` syntax are rejected with a helpful
    diagnostic error and typo suggestion pointing to `list.<field>`.

- **Null Handling on Non-Task Items:**
  - Universal list fields evaluate on all items (plain bullets, checkboxes,
    tasks).
  - Task fields evaluate on task items and return `NoteFieldValueRef::Null` on
    plain bullets and checkboxes via a single pattern match arm on
    `ListField::Task(_)`.

- **Metadata Precedence:**
  - On a `List` row, bare metadata keys (`<key>`) check the item's own inline
    fields (`item.fields`) first. If absent, fallback to note-level metadata.
  - `list.tags` resolves item-level tags.
  - `file.tags` resolves note-level tags.
  - Bare `tags` resolves to `list.tags` on list rows, and `file.tags` on page
    rows.

## Acceptance Criteria

- [ ] Replace `RowKind::Task(TaskRow)` with `RowKind::List { item_idx: u32 }` in
  `src/query/results.rs`.
- [ ] Update `QueryService::task_rows` (or new `list_rows`) to construct
  `QueryRow` instances holding only `item_idx`, performing zero string clones
  and zero status allocations.
- [ ] Define `TaskField` enum with all 12 task-specific field variants in
  `src/query/grammar/field.rs`.
- [ ] Define `ListField` enum in `src/query/grammar/field.rs` containing 9
  universal variants and `ListField::Task(TaskField)`.
- [ ] Update `FieldPath` to replace `Task(TaskField)` with `List(ListField)`.
- [ ] Implement `ListField::parse` to accept canonical `list.<name>` paths for
  all universal and task fields.
- [ ] Reject `task.<field>` in `FieldPath::parse` with an error suggesting the
  equivalent `list.<field>`.
- [ ] Implement `QueryRow::resolve_ref` for `ListField`, borrowing text slices
  (`list.text`, `list.raw_text`, `list.status`, `list.priority`) directly from
  `FileIndex`.
- [ ] Implement tri-state completion on `list.completed`: `Some(true)` (Done),
  `Some(false)` (Incomplete: Todo, In Progress, On Hold), `None` (Cancelled and
  non-task items).
- [ ] Add `FileField::Tags` (`file.tags`) to access note-level tags on list rows.
- [ ] Implement metadata precedence in `QueryRow::resolve_ref`: check item
  inline fields before note frontmatter.
- [ ] Unit tests for all universal and task field paths on list rows.
- [ ] Unit tests verifying task fields return `Null` on plain bullets and
  non-task checkboxes.
- [ ] Unit tests verifying item inline fields override note-level frontmatter on
  list rows.
- [ ] All checks pass under `mise run verify`.
