Status: implemented

**Date**: 2026-09-04
**Implemented in**: branch `task-system/06-text-normalization-priority-and-dates`
(worktree `.worktrees/task-system-06/`)

# 06 — Text normalization, priority, and task dates

**What to build:** List text normalization with two variants, task priority parsing, and task date extraction. `ListText.raw` is source text minus the leading marker prefix only — all inline syntax preserved. `ListText.clean` strips the task marker, configured task tag filters, date syntax, priority emojis, and inline task fields — config-aware normalization. Task priority is a fixed enum stored as optional on `TaskListItem`. Task dates support both emoji syntax and existing inline field syntax.

**Blocked by:** 02 (needs classification to know what to strip), 03 (needs config for tag filter stripping), 05 (needs `TaskListItem` to carry priority and dates).

## Key interfaces

- `TaskListItem` (from issue 05) gains `priority: Option<TaskPriority>` and `dates: TaskDates` fields
- `ListText` struct with `raw: String` and `clean: String` — lives on `ListItem`, not `TaskListItem`
- `TaskPriority` enum: lowest, low, normal, medium, high, highest
- `TaskDates` struct with optional date fields: created, scheduled, start, due, done, cancelled
- Date type: `Option<NaiveDate>` (chrono) for each field — missing dates are `None`/null in queries

## Emoji-to-field mapping

| Emoji | Field     | Source                        |
| ----- | --------- | ----------------------------- |
| ➕    | created   | Tasks plugin convention       |
| 🛫    | start     | Tasks plugin convention       |
| ⏳    | scheduled | Tasks plugin convention       |
| 📅    | due       | Tasks plugin convention       |
| ✅    | done      | Tasks plugin convention       |
| ❌    | cancelled | Chosen for Traces (no Tasks plugin equivalent) |

## Priority emoji mapping

| Emoji | Priority |
| ----- | -------- |
| 🔺    | highest  |
| ⏫    | high     |
| 🔼    | medium   |
| 🔽    | low      |
| ⏬    | lowest   |

No emoji → `None` (missing priority, does not default to normal).

## Inline field syntax

Existing inline fields (`[field:: value]`) continue to work for task dates. Field names: `created`, `start`, `scheduled`, `due`, `done`, `cancelled`.

## Precedence

When both emoji and inline field syntax are present for the same date, **emoji wins**. This matches the Tasks plugin convention where emoji is the primary syntax.

## Worked examples

Input: `- [ ] Buy milk 📅 2025-01-15 #task`

- `raw`: `Buy milk 📅 2025-01-15 #task` (marker prefix stripped)
- `clean` (with `tag_filters: ["#task"]`): `Buy milk` (marker, tag, date stripped)
- `clean` (with `tag_filters: []`): `Buy milk #task` (marker stripped, no tag filter configured so tag not stripped, date stripped)

Input: `- [x] Pay rent 🔼 [due:: 2025-02-01]`

- `raw`: `Pay rent 🔼 [due:: 2025-02-01]`
- `clean`: `Pay rent` (marker, priority, inline field stripped)

## Checklist

### TaskListItem extension

- [x] `TaskListItem` gains `priority: Option<TaskPriority>` field
- [x] `TaskListItem` gains `dates: TaskDates` field
- [x] `TaskListItem::priority(&self) -> Option<TaskPriority>`
- [x] `TaskListItem::dates(&self) -> TaskDates`
- [x] `TaskListItem::new` updated to accept priority and dates
- [x] Parser constructs `TaskListItem` with parsed priority and dates

### ListText

- [x] `ListText` struct with `raw: String` and `clean: String` fields
- [x] `raw` is source text minus leading `[<char>]` marker prefix only
- [x] `clean` strips in order: marker prefix → configured tag filters → date syntax → priority emojis → inline task fields
- [x] `clean` is config-aware: only strips tags that match configured `tag_filters`
- [x] When `tag_filters` is empty, `clean` strips no tags (tag stripping is config-aware, not classification-aware)

### Priority

- [x] Task priority enum: lowest, low, normal, medium, high, highest
- [x] Priority stored as `Option<TaskPriority>` — missing priority remains absent, does not default to normal
- [x] Priority emojis parsed into priority enum (do not store raw emoji as model data)
- [x] Emoji-to-priority mapping: 🔺→highest, ⏫→high, 🔼→medium, 🔽→low, ⏬→lowest

### Dates

- [x] Task dates: created, scheduled, start, due, done, cancelled
- [x] Emoji date syntax parsed (➕, 🛫, ⏳, 📅, ✅, ❌)
- [x] Emoji-to-date mapping: ➕→created, 🛫→start, ⏳→scheduled, 📅→due, ✅→done, ❌→cancelled
- [x] Existing inline field syntax for task dates continues to work (`[created::]`, `[start::]`, `[scheduled::]`, `[due::]`, `[done::]`, `[cancelled::]`)
- [x] When both emoji and inline field present for same date, emoji wins
- [x] Valid `YYYY-MM-DD` dates parsed as `NaiveDate` values
- [x] Missing dates resolve to null in query results

### Tests

- [x] Unit tests for raw vs clean text with various inline syntax
- [x] Unit tests for priority emoji parsing and missing priority
- [x] Unit tests for date extraction from emoji and inline field syntax
- [x] Unit tests for clean text stripping with and without tag filters
- [x] Unit test for emoji-over-inline-field precedence
- [x] `mise run verify` passes

## Out of scope

- LISTS persistence and `ListRecord` — issue 07
- Query record enrichment — issue 08
- Template `tasks.*` namespace changes — issue 09

## Implementation notes

### Where it landed

| File | Purpose |
|---|---|
| `src/note/lists.rs` | `TaskPriority` enum (`Lowest`, `Low`, `Normal`, `Medium`, `High`, `Highest`), `TaskDates` struct (`Option<NaiveDate>` for 6 dates), `ListText` struct (`raw: String`, `clean: String`), and extended `TaskListItem` (`dates: TaskDates`, `priority: Option<TaskPriority>`, `status: TaskStatus`, `fully_complete: bool`). Accessors: `priority(&self)`, `dates(&self)`, `raw_text(&self)`, `clean_text(&self)`. |
| `src/note/parser/list.rs` | Date extraction (`extract_task_dates`), priority extraction (`extract_task_priority`), and clean text normalization (`compute_clean_text`). Construction of `TaskListItem` with parsed dates and priority in `ListTracker::end_item`. Unit tests for raw vs clean, priority, dates, tag filter awareness, and stripping order. |
| `src/note/parser/lexer.rs` | Updated `FieldToken` lexer to recognize both `📅` (`\u{1F4C5}`) and `🗓️` (`\u{1F5D3}`), `✅` mapped to `"done"`, `❌` mapped to `"cancelled"`, and support for variation selector 16 (`\u{FE0F}`) across all date emojis. |
| `src/note/mod.rs` | Public re-exports of `ListText`, `TaskDates`, `TaskPriority`. |
| `src/lib.rs` | Test-utils re-exports of `ListText`, `TaskDates`, `TaskPriority`. |
| `src/query/results.rs` | `with_task_item` updated to populate `TaskRow.text` using `item.clean_text().to_owned()`. |
| `tests/integration/task_tag_filters.rs` | Updated query text assertion to verify that configured tag filters are stripped from query row text while raw text is preserved on the list item. |

### Key design and algorithmic decisions

1. **Struct Layout and Memory Efficiency**:
   `TaskListItem` fields ordered as `dates: TaskDates`, `priority: Option<TaskPriority>`, `status: TaskStatus`, `fully_complete: bool`.
   Under `repr(Rust)` on 64-bit platforms, rustc orders fields by alignment (8 $\to$ 4 $\to$ 1):
   - `status`: 32 bytes (align 8)
   - `dates`: 24 bytes (align 4)
   - `priority`: 1 byte (align 1, niche optimized)
   - `fully_complete`: 1 byte (align 1)
   - Tail padding: 6 bytes
   Total size is 64 bytes (8-byte aligned), with zero wasted interior padding.

2. **Zero-Allocation Tag Matching**:
   `find_tag_filter_spans` scans candidate tag text without invoking `Tag::parse` (which heap-allocates `full` and `segments` strings). Candidate slices `&'a str` are compared directly against pre-parsed filters via `filter.as_str() == candidate`, completely eliminating heap allocations in the item scanning hot path.

3. **Fast-Path Clean Normalization**:
   `compute_clean_text` checks `if remove_spans.is_empty() { return normalize_whitespace(raw_text); }`, avoiding vector sorting, interval merging, and substring reconstruction on plain bullets and simple tasks.

4. **Unicode Variation Selector 16 (`\u{FE0F}`) Resilience**:
   Date emoji parsing in `parse_emoji_date`, `find_emoji_date_spans`, and `lexer.rs` explicitly inspects and skips optional variation selector 16 (`\u{FE0F}`) immediately following the base emoji before trimming whitespace, ensuring mobile and desktop keyboard emojis are parsed identically.

5. **Dual Calendar Emojis**:
   Supported both `📅` (`U+1F4C5` Tear-off Calendar, Tasks plugin convention) and `🗓️` / `🗓` (`U+1F5D3` Spiral Calendar).

6. **Precedence**:
   Emoji date syntax is scanned and extracted before inline field lookups. When both are present for the same date field, emoji date takes precedence.

### Verification command output

```text
$ cargo check --workspace --all-targets --all-features
status: ok (0 errors, pre-existing stack size warnings in query/cli/index untouched)

$ cargo clippy --workspace --all-targets --all-features
status: ok (0 errors, 0 warnings on modified files)

$ cargo test --workspace --all-targets --all-features
test result: ok. 2,230 passed; 0 failed; 0 ignored; 0 measured; finished in 0.10s

$ cargo test --doc --features test-utils
test result: ok. 23 passed; 0 failed; 10 ignored; 0 measured; finished in 0.03s

$ mise run verify
Full gate passed in 29.64s (check, hk check, fmt, clippy, nextest, doctests).
```

### Unblocked

- Issue 07: Note API and LISTS persistence (`Note.list_items()`, `Note.tasks()`, `ListRecord` delegating into `TaskListItem.dates().due`, `TaskListItem.priority()`, `ListItem.text()`)
- Issue 08: Query record enrichment with task fields
