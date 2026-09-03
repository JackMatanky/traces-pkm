Status: ready-for-agent

# 07 — Note API and LISTS persistence

**What to build:** Expose parsed list items through the Note API and persist them in the FileIndex. `Note.list_items()` returns a lazy iterator over the nested list hierarchy. `Note.tasks()` continues to return only Task items (already implemented). `ListRecord` wraps a project-relative path and the parsed `ListItem` with accessor methods for query-relevant fields. LISTS persistence table in redb stores postcard-encoded records keyed by `(path, line)`.

**Blocked by:** 02 (needs `ListItemType`), 05 (needs `TaskListItem` with `fully_complete`), 06 (needs priority and dates on `TaskListItem`).

## Key interfaces

- `ListItemIter<'_>` — lazy depth-first iterator over `&ListItem`, walking nested lists in document order
- `Note.list_items()` returns `ListItemIter<'_>` — **public** API, yields all item kinds (Plain, Checkbox, Task)
- `Note.tasks()` returns `TaskIter<'_>` — already implemented, no change
- `ListRecord` struct wrapping `path: String` and `ListItem`; derives `Serialize`/`Deserialize` for postcard encoding
- `ListRecord` accessor methods delegate through `ListItemType` discriminant:
  - `status_type()` → `ListItemType::Task(task)` → `task.status().kind()`
  - `priority()` → `ListItemType::Task(task)` → `task.priority()`
  - `due_date()` → `ListItemType::Task(task)` → `task.dates().due`
  - `is_fully_complete()` → `ListItemType::Task(task)` → `task.is_fully_complete()`
  - `text()` → `ListItem::text()`
  - `line()` → `ListItem::line()`
  - `depth()` → `ListItem::depth()`
  - `parent_line()` → `ListItem::parent()`
- Accessor methods return `None` for page-level records or non-Task items
- Accessor methods are composable — adding fields to `TaskListItem` does not require updating `ListRecord`'s struct layout

## LISTS persistence

- LISTS table defined in redb as `TableDefinition<&[u8], &[u8]>` keyed by `(path, line)` bytes — path as UTF-8 bytes, line as 4-byte big-endian `u32`, concatenated
- `ListRecord` serializes via postcard as `path` + `ListItem`
- `ListItem` derives `Serialize`/`Deserialize` and postcard handles `IndexMap` fields — no custom serialization needed
- Index rebuild writes LISTS table alongside FILES, NOTES, LINKS
- Index `should_rebuild` includes LISTS in the probe list — detects schema drift alongside existing tables
- Incremental persistence supports LISTS table — deleted notes must remove their LISTS entries

## Checklist

### Note API

- [ ] `Note.list_items()` returns `ListItemIter<'_>` iterator over nested list hierarchy in document order (depth-first, matching parser construction order)
- [ ] `Note.tasks()` returns filtered iterator yielding only `ListItemType::Task` items (already implemented, verify no regression)

### ListRecord

- [ ] `ListRecord` struct wrapping `path: String` and `ListItem`
- [ ] `ListRecord` derives `Serialize`/`Deserialize`
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
- [ ] Index rebuild writes LISTS table alongside FILES, NOTES, LINKS
- [ ] Index `should_rebuild` includes LISTS in probe list
- [ ] Incremental persistence supports LISTS table
- [ ] Deleted notes remove their LISTS entries during incremental persistence

### Tests

- [ ] Integration test: note with tasks → LISTS table contains correct records
- [ ] Integration test: `Note.list_items()` returns all item kinds
- [ ] Integration test: `Note.tasks()` returns only Task items
- [ ] Integration test: index persistence roundtrip includes LISTS-derived fields
- [ ] `mise run verify` passes

## Out of scope

- Query record enrichment — issue 08
- Template `tasks.*` namespace changes — issue 09
