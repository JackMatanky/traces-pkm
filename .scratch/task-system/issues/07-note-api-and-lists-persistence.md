Status: ready-for-agent

# 07 — Note API and LISTS persistence

**What to build:** Expose parsed list items through the Note API and persist them in the FileIndex. `Note.list_items()` returns an iterator over the nested list hierarchy. `Note.tasks()` returns a filtered iterator yielding only Task items. `ListRecord` wraps a project-relative path and the parsed `ListItem` with accessor methods for query-relevant fields. LISTS persistence table in redb stores postcard-encoded records keyed by `(path, line)`.

**Blocked by:** 02 (needs `ListItemType`), 05 (needs `TaskListItem` with `fully_complete`), 06 (needs priority and dates on `TaskListItem`).

## Key interfaces

- `ListRecord` accessor methods delegate through `ListItemType` discriminant into `TaskListItem`:
  - `status_type()` → `task.status().kind()`
  - `priority()` → `task.priority()`
  - `due_date()` → `task.dates().due`
  - `is_fully_complete()` → `task.fully_complete()`
  - `text()` → `list_item.text()`
  - `line()` → `list_item.line()`
  - `depth()` → `list_item.depth()`
  - `parent_line()` → `list_item.parent()`
- Accessor methods are composable — adding fields to `TaskListItem` does not require updating `ListRecord`'s struct layout

## Checklist

### Note API

- [ ] `Note.list_items()` returns `ListItemIter<'_>` iterator over nested list hierarchy in document order (depth-first, matching parser construction order)
- [ ] `Note.tasks()` returns filtered iterator yielding only `ListItemType::Task` items

### ListRecord

- [ ] `ListRecord` struct wrapping `path: String` and `ListItem`
- [ ] `ListRecord::status_type(&self)` reads from `ListItemType::Task(task)` → `task.status().kind()`
- [ ] `ListRecord::priority(&self)` reads from `ListItemType::Task(task)` → `task.priority()`
- [ ] `ListRecord::due_date(&self)` reads from `ListItemType::Task(task)` → `task.dates().due`
- [ ] `ListRecord::is_fully_complete(&self)` reads from `ListItemType::Task(task)` → `task.fully_complete()`
- [ ] `ListRecord::text(&self)` delegates to `ListItem::text()`
- [ ] `ListRecord::line(&self)` delegates to `ListItem::line()`
- [ ] `ListRecord::depth(&self)` delegates to `ListItem::depth()`
- [ ] `ListRecord::parent_line(&self)` delegates to `ListItem::parent()`
- [ ] Accessor methods return `None` for page-level records or non-Task items

### LISTS persistence

- [ ] LISTS table defined in redb as `TableDefinition<&[u8], &[u8]>` keyed by `(path, line)` bytes — path as UTF-8 bytes, line as 4-byte big-endian `u32`, concatenated
- [ ] `ListRecord` serializes via postcard as `path` + `ListItem`
- [ ] `ListItem` derives `Serialize`/`Deserialize` and postcard handles `IndexMap` fields — no custom serialization needed
- [ ] Index rebuild writes LISTS table alongside FILES, NOTES, LINKS
- [ ] Index `should_rebuild` includes LISTS in probe list
- [ ] Incremental persistence supports LISTS table

### Tests

- [ ] Integration test: note with tasks → LISTS table contains correct records
- [ ] Integration test: `Note.list_items()` returns all item kinds
- [ ] Integration test: `Note.tasks()` returns only Task items
- [ ] Integration test: index persistence roundtrip includes LISTS-derived fields
- [ ] `mise run verify` passes

## Out of scope

- Query record enrichment — issue 08
- Template `tasks.*` namespace changes — issue 09
