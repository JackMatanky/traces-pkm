Status: ready-for-agent

# 02 — Custom marker scanner and list item classification

**What to build:** Replace the `ENABLE_TASKLISTS` pulldown-cmark extension, `set_task_status` method, and old binary `TaskStatus` enum with a custom marker scanner and `ListItemType` enum (Plain, Checkbox, Task). The scanner is the only source of truth for task marker identity. `ListItem` stores `ListItemType` replacing `task_status: Option<TaskStatus>`. Completion checks use `TaskStatusType::completed()` from issue 01. The scanner recognizes `[x]`, `[X]`, `[/]`, `[-]`, `[!]`, `[?]`, configured custom markers, and unknown single-character markers. `[x]` and `[X]` are equivalent and both map to Done. Unknown markers behave as incomplete todos and are never downgraded to plain bullets. All existing task classification tests are rewritten against the scanner.

**Blocked by:** 01 (needs `TaskStatusMap` for symbol→status resolution).

## Current behavior

The parser enables `ENABLE_TASKLISTS` on pulldown-cmark, which emits `Event::TaskListMarker(bool)`. The `set_task_status` method converts this to a binary `TaskStatus::Complete` / `TaskStatus::Incomplete` (the existing enum in `src/note/lists.rs:58-64`) stored as `task_status: Option<TaskStatus>` on `ListItem`. This enum is a pulldown-cmark DTO — it only represents the boolean checked/unchecked state. All task classification flows through this single path. There is no support for custom markers (`[/]`, `[-]`, `[!]`, unknown), and `ListItem` has no way to distinguish a plain bullet from a checkbox from a task. The old `TaskStatus` enum is publicly re-exported from `src/note/mod.rs:40` and used in `query/record.rs` for `TaskRow.status` and completion comparisons.

## Desired behavior

A custom marker scanner is the only source of truth for task marker identity. It recognizes `[x]`, `[X]`, `[/]`, `[-]`, `[!]`, `[?]`, configured custom markers, and unknown single-character markers at item-leading position. `ListItem` stores a `ListItemType` enum (`Plain`, `Checkbox`, `Task`) replacing the old `task_status: Option<TaskStatus>` field. The scanner trims the leading marker prefix exactly once. Unknown markers are never downgraded to plain bullets. `[x]` and `[X]` are equivalent and both map to Done via `TaskStatusMap` (from issue 01).

When no tag filters are configured, all status-marked list items become `ListItemType::Task`. Tag-based reclassification (matching items → Task, non-matching → Checkbox) is issue 03's scope. This issue's scanner should not check tags.

## Key interfaces

- `ListItemType` enum — new type with `Plain`, `Checkbox`, `Task` variants. `Task` variant carries issue 01's `TaskStatus` struct (symbol + name + kind), not the old `Incomplete`/`Complete` enum
- `ListItem` struct — replace `task_status: Option<TaskStatus>` (old enum) with `item_type: ListItemType`
- Scanner function — pure function: `fn scan_marker(text: &str) -> Option<MarkerScan>` where `MarkerScan` captures the recognized symbol and trimmed remainder. Called during list item construction, not during event handling
- `Note.tasks()` — filter logic changes from `is_task()` / `task_status()` to `matches!(item.item_type, ListItemType::Task(_))`. Return type changes (iterator) are issue 07
- `Note.list_items()` — no filtering change needed at this step; full API shaping is issue 07
- `ItemFrame.task_status` — removed; classification happens in the scanner, not in the event handler
- `ListItem.is_task()` and `ListItem.is_completed()` — removed; replaced by `ListItemType` pattern matching
- Completion checks — use `TaskStatusType::completed()` from issue 01 (returns tri-state: `Some(true)` for done, `Some(false)` for incomplete, `None` for cancelled) instead of comparing against the old binary `TaskStatus::Complete`
- `query/record.rs` — `TaskRow.status` changes from old `TaskStatus` enum to issue 01's `TaskStatus` struct; `with_task_item` reads from `ListItemType::Task(task)`; `task_completed()` uses `TaskStatusType::completed()` instead of `task.status == TaskStatus::Complete`

## Removed types

- **Delete** `TaskStatus` enum (`Incomplete`/`Complete`) from `src/note/lists.rs:58-64`. It is a pulldown-cmark DTO that only represents `Event::TaskListMarker(bool)`. With `ENABLE_TASKLISTS` removed, this enum has no source.
- **Remove** `pub use lists::{..., TaskStatus}` from `src/note/mod.rs:40`
- **Remove** `ListItem.is_task()`, `ListItem.is_completed()`, `ListItem.task_status()` accessors — all replaced by `ListItemType` pattern matching
- Any code comparing against `TaskStatus::Complete` (e.g. `query/record.rs:110,265`) must switch to `TaskStatusType::completed()` from issue 01

## Acceptance criteria

- [ ] `ListItemType` enum exists with `Plain`, `Checkbox`, `Task` variants
- [ ] `ListItem` stores `ListItemType` instead of `task_status: Option<TaskStatus>` (old enum)
- [ ] `ListItemType::Task` carries issue 01's `TaskStatus` struct
- [ ] Old `TaskStatus` enum (`Incomplete`/`Complete`) deleted from `src/note/lists.rs`
- [ ] Old `TaskStatus` removed from `pub use` in `src/note/mod.rs`
- [ ] `ListItem.is_task()`, `is_completed()`, `task_status()` accessors removed
- [ ] Scanner recognizes `[x]`, `[X]`, `[/]`, `[-]`, `[!]`, `[?]`, and unknown single-character markers
- [ ] Scanner only accepts markers at item-leading position followed by whitespace
- [ ] Later bracket text in item body is not trimmed as a marker
- [ ] `[x]` and `[X]` both resolve to Done status via `TaskStatusMap`
- [ ] `[/]`, `[-]`, `[!]` resolve to configured default statuses (in-progress, on-hold, non-task)
- [ ] Unknown markers (e.g. `[?]`) are preserved and resolved as incomplete todo by default
- [ ] Unknown markers are never downgraded to plain bullets
- [ ] When no tag filters are configured, all status-marked items become `ListItemType::Task`
- [ ] `ENABLE_TASKLISTS` is removed from pulldown-cmark options
- [ ] `set_task_status` method and `ItemFrame.task_status` derivation path are removed
- [ ] `flush_active_item_scan_buffer` branches on `ListItemType::Task(_)` instead of old `task_status.is_some()`
- [ ] `Note.tasks()` classification filter uses `ListItemType::Task` (return type changes deferred to issue 07)
- [ ] `query/record.rs` `TaskRow.status` uses issue 01's `TaskStatus` struct, not old enum
- [ ] `query/record.rs` completion checks use `TaskStatusType::completed()`, not `== TaskStatus::Complete`
- [ ] Unit tests cover scanner recognizing all marker types
- [ ] Unit tests cover marker scanning at item-leading position only
- [ ] Unit tests cover unknown markers preserved and classified as incomplete todo
- [ ] Unit tests cover `[x]`/`[X]` equivalence
- [ ] All existing task classification tests rewritten against the scanner
- [ ] `cargo test` passes, `cargo clippy` clean

## Out of scope

- Tag filter classification — issue 03 wires tag filters into `ListItemType` determination (Task vs Checkbox for matching vs non-matching items)
- Task priority parsing — issue 06
- Task date parsing — issue 06
- `ListText` raw/clean normalization — issue 06
- `fully_complete` computation — issue 05
- `Note.list_items()` / `Note.tasks()` iterator return types and `ListRecord` — issue 07
- LISTS persistence table — issue 07
- Query record enrichment — issue 08
- Template `tasks.*` namespace changes — issue 09
- CLI task command changes (`--sort`, `--table`, `--from`) — issue 10
- `ByteTracker` utility — issue 04 (parallel with issue 01, not this issue)
