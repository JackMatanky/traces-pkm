# 07 — Note API flat storage, memory compaction, and persistence cleanup

**Category:** enhancement
**Status:** done

**What to build:** Amend ADR 0005 by dropping the unread `LISTS` table from redb
and removing `ListEntry` and `ListEntryRef`, leaving `NOTES` as the single source
of truth for persisted list items. Flatten `Note.lists` from a recursive tree
into a contiguous `Box<[ListItem]>` in strict document order. Compact `ListItem`
memory layout by adopting sparse inline fields, lazy clean text, and 4-byte
`DateValue` fields in `TaskDates`. Expose zero-allocation slice iterators on
`Note` (`Note::list_items()` and `Note::tasks()`).

**Blocked by:** 02 (needs `ListItemType`), 05 (needs `TaskListItem` with
`fully_complete`), 06 (needs priority and dates on `TaskListItem`).

## Acceptance Criteria

- [x] Remove `LISTS` table definition, key codecs, and associated reader/writer
  methods (`write_lists_for_note`, `remove_lists_for_path`, `read_lists`,
  `read_lists_for_path`, `read_all_lists`) from `src/index/store.rs` and
  `src/index/service.rs`.
- [x] Remove `ListEntry` and `ListEntryRef` from `src/index/entry.rs` and public
  crate exports (`src/index/mod.rs` and `src/lib.rs`).
- [x] Revert `IndexStore::check_rebuild_needed` probe list to only include
  `FILES`, `NOTES`, and `LINKS`, and remove `WriteTarget::Lists` from
  `WriteTarget::ALL`.
- [x] Flatten `Note.lists` to `Box<[ListItem]>` in document order, and update
  `Note::new` and `Note::lists(&self) -> &[ListItem]`.
- [x] Dissolve `ListItemPosition` and update `ListItem` struct layout with
  `text: ListText`, `kind: ListItemType`, `depth: u8`,
  `line: SourceLine`, `parent: Option<SourceLine>`, `is_ordered: bool`,
  `fields: Option<Box<IndexMap<FieldKey, Box<[NoteFieldValue]>>>>`, and
  `tags: Box<[Tag]>`, removing `children: Box<[List]>`.
- [x] Compact `ListItem.fields` to
  `Option<Box<IndexMap<FieldKey, Box<[NoteFieldValue]>>>>`, returning
  `Option<&IndexMap<FieldKey, Box<[NoteFieldValue]>>>` from `ListItem::fields`.
- [x] Update `ListText` to store `clean: Option<String>`, setting `clean` to
  `None` when clean text equals raw text, and returning
  `self.clean.as_deref().unwrap_or(&self.raw)` from `ListText::clean()`.
- [x] Update `TaskDates` to use `Option<crate::DateValue>` for all six lifecycle
  dates (`created`, `start`, `scheduled`, `due`, `done`, `cancelled`), and
  implement `From<chrono::NaiveDate> for crate::DateValue`.
- [x] Implement `Note::list_items()` and `Note::tasks()` as slice-backed
  iterators directly over `self.lists`, eliminating the stack-allocated
  `ListItemIter`.
- [x] Ensure markdown list parser outputs `ListItem`s in strict pre-order
  document order (`[Parent, Child, Sibling]`), accounting for pulldown-cmark's
  child-before-parent `End(Item)` event sequence.
- [x] Update descendant traversal helper to use non-recursive slice scans
  (`take_while(|child| child.depth() > parent_depth)`).
- [x] Update existing unit tests in `src/note/` and `src/index/`, and
  integration tests in `tests/integration/index_persistence_roundtrip.rs`, to
  verify flattened list persistence inside `Note` within the `NOTES` table.
- [x] All checks pass under `mise run verify`.

## Key Interfaces and Models

- **ADR 0005 Amendment:** `IndexStore` drops the unread `LISTS` table, reducing
  redb tables from 8 to 7. All list and task items persist directly inside `Note`
  within `NOTES`, eliminating write amplification during index updates.
- **Flat List Storage in `Note`:** `Note` stores `lists: Box<[ListItem]>` in
  strict document order. `Note::lists(&self) -> &[ListItem]`.
- **Structural Position on `ListItem`:**
  - `depth: u8`: 0-indexed nesting depth level.
  - `line: Option<SourceLine>`: 1-indexed source line of the item itself.
  - `parent: Option<SourceLine>`: 1-indexed source line of immediate parent
    item, or None. Niche-optimized to 4 bytes via `NonZeroU32`.
  - `is_ordered: bool`: true for numbered list items (`1. `), false for bullet
    list items (`- `).
  - `children: Box<[List]>` is eliminated; hierarchy is reconstructed via
    `depth` and `parent`.
- **Slice Traversal:**
  - `Note::list_items(&self) -> impl Iterator<Item = &ListItem>`: yields all
    list items in document order directly from the slice (e.g. `self.lists.iter()`).
  - `Note::tasks(&self) -> impl Iterator<Item = &ListItem>`: filters the slice
    iterator to items where `item.kind().is_task()` is true.
  - Descendant collection: contiguous slice scan using
    `take_while(|child| child.depth() > parent.depth())`.
- **Memory Compaction for `ListItem`:**
  - `fields: Option<Box<IndexMap<FieldKey, Box<[NoteFieldValue]>>>>`: 8-byte
    pointer for empty items (over 95% of list items).
  - `text: ListText { raw: String, clean: Option<String> }`: when raw text
    equals clean text, `clean` is `None` to eliminate duplicate string
    allocations.
  - `TaskDates` uses `Option<crate::DateValue>` (4 bytes each) instead of
    `chrono::NaiveDate` (12 bytes each).
## Comments

> *This was generated by AI during triage.*

## Agent Brief

**Category:** enhancement
**Summary:** Drop the dead LISTS table from redb, flatten Note.lists into a
contiguous slice, and compact ListItem memory.

**Current behavior:**
`Note` represents list structures as a mutually recursive tree
(`Box<[List]> -> ListItem -> Box<[List]>`), requiring heap-allocated stack
iterators to traverse. `src/index/store.rs` writes every list item to a `LISTS`
redb table that has zero production query readers and produces quadratic write
amplification on nested outlines. `ListItem` holds an unconditionally allocated
`IndexMap` for inline fields, duplicated clean text strings, and 72 bytes of
`chrono::NaiveDate` options.

**Desired behavior:**
Amend ADR 0005 by dropping the unread `LISTS` table from redb and removing
`ListEntry` and `ListEntryRef`. Flatten `Note.lists` into a single contiguous
`Box<[ListItem]>` stored in document order, encoding nesting via `depth: u8`,
`line: Option<SourceLine>`, and `parent: Option<SourceLine>`. Compact `ListItem`
so empty inline fields use an 8-byte `None` pointer, identical clean text is
not duplicated, and `TaskDates` uses compact 4-byte `DateValue` primitives.
Provide zero-allocation slice iterators `Note::list_items()` and `Note::tasks()`.

**Key interfaces:**

- `Note.lists`: Change type from `Box<[List]>` to `Box<[ListItem]>`.
- `Note::lists(&self) -> &[ListItem]`: Slice accessor in document order.
- `Note::list_items(&self) -> impl Iterator<Item = &ListItem>`: Slice-backed
  borrowing iterator.
- `Note::tasks(&self) -> impl Iterator<Item = &ListItem>`: Slice-backed filter
  iterator.
- `ListItem`: Store `depth: u8`, `line: Option<SourceLine>`,
  `parent: Option<SourceLine>`, `is_ordered: bool`, dissolving `ListItemPosition`
  and removing `children: Box<[List]>`.
- `ListItem.fields`: Change to
  `Option<Box<IndexMap<FieldKey, Box<[NoteFieldValue]>>>>`.
- `ListText`: Store `clean: Option<String>`, returning `&self.raw` when `None`.
- `TaskDates`: Change date fields from `Option<chrono::NaiveDate>` to
  `Option<crate::DateValue>`.
- `IndexStore`: Remove `LISTS` table definition and all associated methods; update
  `check_rebuild_needed` probe list to `[FILES, NOTES, LINKS]`.
**Acceptance criteria:**
Refer to the single Acceptance Criteria checklist at the top of this ticket.

**Out of scope:**

- Query row resolution and `list.<field>` expressions (addressed in Issue 08).
- Template pipeline globals (addressed in Issue 09).
- CLI task formatting and argument parsing (addressed in Issue 10).
