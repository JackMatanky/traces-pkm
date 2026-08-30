Status: ready-for-agent

# 07 — Note API and LISTS persistence

**What to build:** Expose parsed list items through the Note API and persist them in the FileIndex. `Note.list_items()` returns an iterator over the nested list hierarchy. `Note.tasks()` returns a filtered iterator yielding only Task items. `ListRecord` wraps a project-relative path and the parsed `ListItem` with accessor methods for query-relevant fields. LISTS persistence table in redb stores postcard-encoded records keyed by `(path, line)`.

**Blocked by:** 02 (needs `ListItemType`), 05 (needs `fully_complete` on `ListItem`), 06 (needs text and priority/dates on `ListItem`).

- [ ] `Note.list_items()` returns `ListItemIter<'_>` iterator over nested list hierarchy
- [ ] `Note.tasks()` returns filtered iterator yielding only `ListItemType::Task` items
- [ ] `ListRecord` struct wrapping `path: String` and `ListItem`
- [ ] `ListRecord` accessor methods: `status_type()`, `priority()`, `due_date()`, `is_fully_complete()`, `text()`, `line()`, `depth()`, `parent_line()`
- [ ] Accessor methods delegate into `ListItemType` discriminant (composable — adding fields to `ListItem` does not require updating `ListRecord`)
- [ ] LISTS table defined in redb as `TableDefinition<&[u8], &[u8]>` keyed by `(path, line)` bytes
- [ ] `ListRecord` serializes via postcard as `path` + `ListItem`
- [ ] Index rebuild writes LISTS table alongside FILES, NOTES, LINKS
- [ ] Index `should_rebuild` includes LISTS in probe list
- [ ] Incremental persistence supports LISTS table
- [ ] Integration test: note with tasks → LISTS table contains correct records
- [ ] Integration test: `Note.list_items()` returns all item kinds
- [ ] Integration test: `Note.tasks()` returns only Task items
- [ ] Integration test: index persistence roundtrip includes LISTS-derived fields
