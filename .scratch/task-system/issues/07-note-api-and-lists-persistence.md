Status: implemented

**Date**: 2026-09-04
**Implemented in**: branch `task-system/07-note-api-and-lists-persistence`
(worktree `.worktrees/task-system-07/`)

# 07 — Note API and LISTS persistence

**What to build:** Expose parsed list items through the Note API and persist them in the FileIndex. `Note.list_items()` returns a lazy iterator over the nested list hierarchy. `Note.tasks()` composes from `ListItemIter` with a filter, replacing the redundant `TaskIter`. `ListRecord` wraps a project-relative path and the parsed `ListItem` with accessor methods for query-relevant fields. LISTS persistence table in redb stores postcard-encoded records keyed by `(path, line)`.

**Blocked by:** 02 (needs `ListItemType`), 05 (needs `TaskListItem` with `fully_complete`), 06 (needs priority and dates on `TaskListItem`).

## Key interfaces

- `ListItemIter<'_>` — lazy depth-first iterator over `&ListItem`, walking nested lists in document order. The single traversal type for all list iteration.
- `Note.list_items()` returns `ListItemIter<'_>` — **public** API, yields all item kinds (Plain, Checkbox, Task)
- `Note.tasks()` returns `ListItemIter<'_>` filtered to `ListItemType::Task` items — replaces `TaskIter`, which is removed (both traversed identically; `TaskIter` filtered at yield time, not traversal time)
- `ListEntry` (relocated to `index/entry.rs` in round 4, see review history) struct wrapping `path: String` and `ListItem`; derives `Serialize`/`Deserialize` for postcard encoding
- `ListEntry` accessor methods delegate through `ListItemType` discriminant:
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

- [x] `Note.list_items()` returns `ListItemIter<'_>` iterator over nested list hierarchy in document order (depth-first, matching parser construction order)
- [x] `Note.tasks()` returns `ListItemIter<'_>` filtered to Task items — replaces `TaskIter` return type
- [x] Remove `TaskIter` struct and its `Iterator` impl from `src/note/lists.rs`
- [x] Remove `TaskIter` references from `src/note/model.rs` (`Note::tasks()` return type and doc)

### ListEntry (relocated to `index/entry.rs` in round 4 — see review history)

- [x] `ListEntry` struct wrapping `path: String` and `ListItem`
- [x] `ListEntry` derives `Serialize`/`Deserialize`
- [x] `ListEntry::status_type(&self)` reads from `ListItemType::Task(task)` → `task.status().kind()`
- [x] `ListEntry::priority(&self)` reads from `ListItemType::Task(task)` → `task.priority()`
- [x] `ListEntry::due_date(&self)` reads from `ListItemType::Task(task)` → `task.dates().due`
- [x] `ListEntry::is_fully_complete(&self)` reads from `ListItemType::Task(task)` → `task.is_fully_complete()`
- [x] `ListEntry::text(&self)` delegates to `ListItem::text()`
- [x] `ListEntry::line(&self)` delegates to `ListItem::line()`
- [x] `ListEntry::depth(&self)` delegates to `ListItem::depth()`
- [x] `ListEntry::parent_line(&self)` delegates to `ListItem::parent()`
- [x] Accessor methods return `None` for non-Task items

### LISTS persistence

- [x] LISTS table defined in redb as `TableDefinition<&[u8], &[u8]>` keyed by `(path, line)` bytes — path as UTF-8 bytes, line as 4-byte big-endian `u32`, concatenated
- [x] `ListEntry` serializes via postcard as `path` + `ListItem`
- [x] Index rebuild writes LISTS table alongside FILES, NOTES, LINKS
- [x] Index `should_rebuild` includes LISTS in probe list
- [x] Incremental persistence supports LISTS table
- [x] Deleted notes remove their LISTS entries during incremental persistence

### Tests

- [x] Integration test: note with tasks → LISTS table contains correct records
- [x] Integration test: `Note.list_items()` returns all item kinds
- [x] Integration test: `Note.tasks()` returns only Task items
- [x] Integration test: index persistence roundtrip includes LISTS-derived fields
- [x] `mise run verify` passes

## Out of scope

- Query record enrichment — issue 08
- Template `tasks.*` namespace changes — issue 09

## Implementation notes

### Where it landed

| File | Purpose |
|---|---|
| `src/note/lists.rs` | `ListItemIter<'a>` (replaces `TaskIter`; unified depth-first walker with a private `tasks_only: bool` flag, filtered at yield time via two constructors `new`/`tasks`); `ListItem::without_children()` (round 4: returns a clone with descendant lists cleared, for persisting one flat row per item); `ListItem::tags()`, `ListItemType::as_task()` (round 4, independent additions). |
| `src/note/model.rs` | `Note::list_items() -> ListItemIter<'_>` (new, public); `Note::tasks()` changed to return `ListItemIter<'_>` (was `TaskIter<'_>`). |
| `src/note/mod.rs`, `src/lib.rs` | Public re-exports: `ListItemIter` unconditional; `TaskIter` removed. `SourceLine` promoted from `pub(crate)` to `pub` (required for `ListEntry::line()`/`parent_line()` return values to be usable outside the crate). |
| `src/position.rs` | `SourceLine` and `SourceLine::new` promoted to `pub` alongside the re-export change. |
| `src/index/entry.rs` (round 4: relocated here from `note/lists.rs`, see review history) | `ListEntry` struct wrapping `path: String` and `ListItem` — mirrors how `NoteEntry` wraps `Note` — with accessors `path()`, `status_type()`, `priority()`, `due_date()`, `is_fully_complete()`, `text()`, `raw_text()`, `clean_text()`, `tags()`, `line()`, `depth()`, `parent_line()` delegating through the `ListItemType` discriminant; `pub` only under `test`/`test-utils` (matching `FileEntry`'s visibility), `pub(crate)` otherwise. `ListEntryRef<'a>` borrowed mirror (relocated from `index/store.rs`, formerly `ListRecordRef`) for zero-clone writes, borrowing a local `without_children()`-derived item. |
| `src/index/store.rs` | `LISTS: TableDefinition<&[u8], &[u8]>` table constant (private); `IndexStore::list_key`/`list_key_bounds`/`list_key_matches_path` private associated functions for the `(path, line)` key scheme; `read_lists`, `read_lists_for_path`, `read_all_lists` (`pub(super)`, return `Vec<ListEntry>`); `write_lists_for_note`, `remove_lists_for_path` (private); `should_rebuild` probes LISTS; `write_all` and `upsert_files_and_notes` write/delete LISTS rows alongside FILES/NOTES/LINKS. |
| `src/index/service.rs` | `IndexerService::read_lists() -> IndexResult<Vec<ListEntry>>` (public, thin wrapper over `IndexStore::read_all_lists`). |
| `src/index/codec.rs` | `path_from_bytes` doc updated to list the LISTS table readers as consumers and note the lossy-decode fallback never triggers there (LISTS keys are UTF-8 by construction via `write_lists_for_note`'s `to_string_lossy()`). |
| `tests/integration/index_persistence_roundtrip.rs` | Three integration tests: item-kind/task-filter coverage (tag-filtered fixture so `Checkbox` is actually observed, not just `Task`), LISTS-table record correctness (table-driven), and full build→persist→fresh-service LISTS round trip. |

### Key design and algorithmic decisions

1. **`ListItemIter` unifies `TaskIter` and the new unfiltered walker behind one seam.**
   Both `Note::list_items()` and `Note::tasks()` return the identical `ListItemIter<'_>` type per the ticket's explicit requirement ("The single traversal type for all list iteration"). Internally, a private `tasks_only: bool` field — set via two `pub(crate)` constructors, `new` (unfiltered) and `tasks` (filtered) — selects filter-at-yield-time behavior in one shared `next()` implementation, so descending into a non-task item's children is unaffected and nested tasks under a plain bullet are still reached. Neither constructor is reachable outside the crate; external callers only ever obtain an iterator through `Note`.

2. **`ListRecordRef` eliminates a clone-per-row on every persist.**
   `write_lists_for_note` serializes through a borrowed `ListRecordRef<'a> { path: &'a str, item: &'a ListItem }` rather than constructing an owned `ListRecord` (which would require cloning the note's path string and deep-cloning every `ListItem`, including its nested `IndexMap` fields and child lists). Field order and types are identical between `ListRecordRef` and `ListRecord`, so postcard's positional encoding is byte-for-byte compatible; `decode_row` on read transparently reconstructs an owned `ListRecord` from the same bytes. Profiled via a temporary `mise bench` fixture patch (not committed): with a 5-task-per-note fixture, `FileIndex::persist/1000` scaled proportionally to row count once this clone was removed, with no separate clone-driven overhead on top of the expected per-row serialization cost.

3. **`ListRecord`'s interface deliberately hides `ListItemType` entirely.**
   Every accessor (`status_type`, `priority`, `due_date`, `is_fully_complete`) pattern-matches `ListItemType` internally and returns `None` for `Plain`/`Checkbox` items or task-less fields — callers never see the enum. Two speculative additions from an earlier pass were removed after review: `ListRecord::item(&self) -> &ListItem` (a raw escape hatch that let callers bypass every accessor and pattern-match `ListItemType` directly, undermining the ticket's explicit composability goal — "adding fields to `TaskListItem` does not require updating `ListRecord`'s struct layout") and `ListRecord::fully_complete()` (a verbatim duplicate alias of `is_fully_complete()` with no caller outside its own test). Neither had a real caller; both failed the deletion test.

4. **`(path, line)` key encoding is a private `IndexStore` concern.**
   `list_key`, `list_key_bounds`, and `list_key_matches_path` are private associated functions on `IndexStore`, shared by `read_lists_for_path`, `remove_lists_for_path`, and `write_lists_for_note` — the byte-concatenation scheme (UTF-8 path bytes + 4-byte big-endian `SourceLine`) and the range-scan/prefix-match logic exist in exactly one place. `ListItem::depth()`/`line()`/`parent()` are `pub(crate)` (not `pub`): their only callers are `ListRecord`'s same-module accessors and `IndexStore::write_lists_for_note`, both crate-internal, so no caller needs — or gets — the wider visibility.

5. **LISTS keys are UTF-8-only by construction, unlike FILES/NOTES/LINKS.**
   FILES/NOTES/LINKS key on `path.as_os_str().as_encoded_bytes()` (raw OS-native bytes, losslessly recoverable even for non-Unicode paths). LISTS keys instead use `note.path().to_string_lossy()`, matching the ticket's explicit "path as UTF-8 bytes" schema. This is a deliberate, ticket-mandated simplification versus the other tables' lossless handling: a project with non-UTF-8 file paths would have those paths' `ListRecord.path` field lossy-mangled (replacement characters), and two such paths differing only in their non-UTF-8 segments could theoretically collide after lossy conversion. Flagged here as a known, scoped edge case rather than silently fixed, since correcting it would mean changing the ticket's specified key encoding.

### Verification command output

```text
$ cargo check --manifest-path Cargo.toml --lib --tests --all-features
status: ok (0 errors)

$ mise run --force lint
status: ok (0 errors, 0 warnings on modified files; pre-existing stack-size-threshold notes on unrelated Config/WriteTransaction types untouched)

$ cargo test --manifest-path Cargo.toml --lib --tests --all-features
test result: ok. 2201 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.86s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.24s
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.16s

$ cargo test --manifest-path Cargo.toml --doc --all-features
test result: ok. 71 passed; 0 failed; 10 ignored; 0 measured; 0 filtered out; finished in 0.03s

$ mise run --force verify
Full gate passed in 22.90s (check, hk check, fmt, clippy -D warnings, nextest, doctests).
```

### Review history

Three ruthless review passes applied on top of the initial implementation (`590064f`), each verified against `mise run verify` and the full test suite before commit:

1. Silent error suppression in `remove_lists_for_path`'s original range scan (`.filter_map(Result::ok)` swallowed `redb::StorageError`), least-privilege reduction on `LISTS`/`list_key`/`read_lists`/`read_all_lists`, allocation elimination in `write_lists_for_note`, range-bound helper extraction, `FusedIterator` impl for `ListItemIter`, `into_item`/`into_parts` → `Into<String>`/`ListItem` accessors.
2. `Note::tasks()` corrected to literally return `ListItemIter<'_>` (was an opaque `impl Iterator` via `.filter()`, violating the ticket's single-traversal-type requirement); integration test rewritten with a tag-filtered fixture after discovering the original fixture could never produce a `ListItemType::Checkbox` item despite the test's docstring claiming full item-kind coverage; `ListRecordRef` borrowed serialization view; `list_key`/`list_key_bounds`/`list_key_matches_path` consolidated as private `IndexStore` associated functions; `ListRecord::into_item`/`into_parts` removed (speculative, zero callers); redundant `SourceLine::get` removed in favor of the existing `From<SourceLine> for u32` impl; `impl_trait_in_params` clippy fix on `ListRecord::new`; cognitive-complexity fix via table-driven test refactor; stale LISTS-omitting doc comments fixed on four `IndexStore` items.
3. `ListItem::depth()`/`line()`/`parent()` reverted from `pub` back to `pub(crate)` (widened without any actual external caller — regression from round 2's cleanup); `ListRecord::item()` and `ListRecord::fully_complete()` removed (zero real callers, both failed the deletion test, `item()` specifically undermined the ticket's composability goal by exposing `ListItemType` as a bypass around the accessor methods); `path_from_bytes` doc comment updated to list the LISTS table readers as consumers. Also rebased onto `main`'s redb 4.1 → 4.2 dependency bump (additive-only API change, no source changes required).
4. Architecture review (`.scratch/task-system/architecture-review-lists-and-query-rows.md`) found `ListRecord` in the wrong module — `note/lists.rs` (parsing) instead of `index/entry.rs`, the established seam for persisted row types alongside `FileEntry`/`NoteEntry` — and a confirmed quadratic-blowup bug: `ListRecord` wrapped the whole `ListItem`, including its `children: Vec<List>`, so a `LISTS` row for an ancestor recursively re-embedded its entire descendant subtree (measured: root row 348 bytes shrinking to leaf row 64 bytes on a 6-level chain). Relocated as `ListEntry` in `index/entry.rs`, wrapping `path` + `ListItem` directly — mirroring how `NoteEntry` wraps `Note` — with a new `ListItem::without_children()` derivation clearing descendant lists before wrapping (the bug wasn't "a wrapped domain type can't have children," since `NoteEntry` wraps `Note`, which also nests arbitrarily deep lists, with no bug — `Note` is persisted once regardless of depth; the bug was specific to `LISTS` being one row per list item, so each row must exclude the subtree already covered by its own descendants' independent rows). `ListEntryRef` (renamed from `ListRecordRef`) relocated alongside it, now borrowing a local children-cleared item for zero-clone writes. Re-measured after the fix: every row a flat ~64–65 bytes regardless of depth. `ListItemType::as_task()` and `ListItem::tags()` also landed this round (see commit history) — small, independently-motivated additions the same review surfaced, unblocking `task.tags` for issue 08 regardless of the `RowKind` question below.

### Unblocked

- Issue 08: Query record enrichment with task fields (`IndexerService::read_lists()`, `IndexStore::read_lists_for_path` are available for query-time enrichment)
- Issue 09: Template `tasks.*` namespace field paths
