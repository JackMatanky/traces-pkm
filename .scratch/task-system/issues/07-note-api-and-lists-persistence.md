# 07 — Note API flat storage, memory compaction, and persistence cleanup

**Status:** ready-for-agent

**What to build:** Revert the redb database schema to ADR 0005 (`FILES`,
`NOTES`, `LINKS`), dropping the unread `LISTS` table and removing `ListEntry`
and `ListEntryRef`. Flatten `Note.lists` from a recursive tree into a
contiguous `Box<[ListItem]>` in strict document order. Compact `ListItem`
memory layout by adopting sparse inline fields, lazy clean text, and 4-byte
`DateValue` fields in `TaskDates`. Expose zero-allocation slice iterators on
`Note` (`Note::list_items()` and `Note::tasks()`).

**Blocked by:** 02 (needs `ListItemType`), 05 (needs `TaskListItem` with
`fully_complete`), 06 (needs priority and dates on `TaskListItem`).

## Key Interfaces and Models

- **ADR 0005 Compliance:** `IndexStore` manages only `FILES`, `NOTES`, and
  `LINKS` tables. The unread `LISTS` table is removed, eliminating write
  amplification during index updates.
- **Flat List Storage in `Note`:** `Note` stores `lists: Box<[ListItem]>` in
  document order.
- **Structural Position on `ListItem`:**
  - `depth: u8` - 0-indexed nesting depth level.
  - `parent: Option<SourceLine>` - 1-indexed source line of immediate parent
    item, or None. Niche-optimized to 4 bytes via `NonZeroU32`.
  - `is_ordered: bool` - true for numbered list items (`1. `), false for bullet
    list items (`- `).
- **Slice Traversal:**
  - `Note::list_items(&self) -> impl Iterator<Item = &ListItem>` - yields all
    list items in document order directly from the slice.
  - `Note::tasks(&self) -> impl Iterator<Item = &ListItem>` - filters the slice
    iterator to items where `item.kind().is_task()` is true.
  - Descendant collection - contiguous slice scan using
    `take_while(|child| child.depth() > parent.depth())`.
- **Memory Compaction for `ListItem`:**
  - `fields: Option<Box<IndexMap<FieldKey, Box<[NoteFieldValue]>>>>` - 8-byte
    pointer for empty items (over 95% of list items).
  - `text: ListText { raw: String, clean: Option<String> }` - when raw text
    equals clean text, `clean` is `None` to eliminate duplicate string
    allocations.
  - `TaskDates` uses `Option<crate::DateValue>` (4 bytes each) instead of
    `chrono::NaiveDate` (12 bytes each).

## Acceptance Criteria

- [ ] Remove `LISTS` table definition, key codecs, and associated reader/writer
  methods (`write_lists_for_note`, `remove_lists_for_path`, `read_lists`,
  `read_lists_for_path`, `read_all_lists`) from `src/index/store.rs` and
  `src/index/service.rs`.
- [ ] Remove `ListEntry` and `ListEntryRef` from `src/index/entry.rs` and public
  crate exports.
- [ ] Revert `IndexStore::should_rebuild` probe list to only include `FILES`,
  `NOTES`, and `LINKS`.
- [ ] Flatten `Note.lists` to `Box<[ListItem]>` in document order.
- [ ] Update `ListItem` struct layout with `depth: u8`,
  `parent: Option<SourceLine>`, and `is_ordered: bool`.
- [ ] Compact `ListItem.fields` to
  `Option<Box<IndexMap<FieldKey, Box<[NoteFieldValue]>>>>`.
- [ ] Update `ListText` to store `clean: Option<String>`, populated lazily only
  when clean text differs from raw text.
- [ ] Update `TaskDates` to use `Option<crate::DateValue>` for all six lifecycle
  dates (`created`, `start`, `scheduled`, `due`, `done`, `cancelled`).
- [ ] Implement `Note::list_items()` and `Note::tasks()` as slice-backed
  iterators.
- [ ] Update descendant traversal helper to use non-recursive slice scans
  (`take_while`).
- [ ] Update existing unit tests in `src/note/` and `src/index/` to verify
  flattened list persistence inside `Note` within the `NOTES` table.
- [ ] All checks pass under `mise run verify`.
