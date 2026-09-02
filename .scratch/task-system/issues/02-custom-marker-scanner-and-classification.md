Status: ready-for-agent

# 02 — Custom marker scanner and list item classification

**What to build:** Replace the `ENABLE_TASKLISTS` pulldown-cmark option, `set_task_status` method, and old binary `TaskStatus` enum with a custom marker scanner and `ListItemType` enum (Plain, Checkbox, Task). The scanner is the only source of truth for task marker identity. `ListItem` stores `ListItemType` replacing `task_status: Option<TaskStatus>`. Completion checks use `TaskStatusType::completed()` from issue 01. Free functions `extract_inline_fields` and `extract_task_inline_fields` are replaced by `InlineTokenLexer` struct with `has_marker` flag — no branching at call site, lexer returns flat token lists. The scanner recognizes `[x]`, `[X]`, `[/]`, `[-]`, `[!]`, `[?]`, and unknown single-character markers. `[x]` and `[X]` are equivalent and both map to Done. Unknown markers behave as incomplete todos and are never downgraded to plain bullets. All existing task classification tests are rewritten against the scanner.

**Blocked by:** 01 (needs `TaskStatusMap` for symbol→status resolution).

## Current behavior

The parser enables `ENABLE_TASKLISTS` on pulldown-cmark, which emits `Event::TaskListMarker(bool)`. The `set_task_status` method converts this to a binary `TaskStatus::Complete` / `TaskStatus::Incomplete` (the existing enum in `src/note/lists.rs:58-64`) stored as `task_status: Option<TaskStatus>` on `ListItem`. This enum is a pulldown-cmark DTO — it only represents the boolean checked/unchecked state. All task classification flows through this single path. There is no support for custom markers (`[/]`, `[-]`, `[!]`, unknown), and `ListItem` has no way to distinguish a plain bullet from a checkbox from a task. The old `TaskStatus` enum is publicly re-exported from `src/note/mod.rs:40` and used in `query/record.rs` for `TaskRow.status` and completion comparisons. Free functions `extract_inline_fields` and `extract_task_inline_fields` in `src/note/lexer.rs` are called conditionally based on `task_status.is_some()` — branching happens at the call site, not inside the functions.

## Desired behavior

A custom marker scanner is the only source of truth for task marker identity. It recognizes `[x]`, `[X]`, `[/]`, `[-]`, `[!]`, `[?]`, and unknown single-character markers at item-leading position. `ListItem` stores a `ListItemType` enum (`Plain`, `Checkbox`, `Task`) replacing the old `task_status: Option<TaskStatus>` field. The scanner trims the leading marker prefix exactly once. Unknown markers are never downgraded to plain bullets. `[x]` and `[X]` are equivalent and both map to Done via `TaskStatusMap` (from issue 01). Free functions `extract_inline_fields` and `extract_task_inline_fields` are replaced by `InlineTokenLexer` struct with `has_marker: bool` flag — no branching at call site, lexer returns flat token lists (`Vec<(FieldKey, NoteFieldValue)>` and `Vec<Tag>`).

When no tag filters are configured, all status-marked list items become `ListItemType::Task`. Tag-based reclassification (matching items → Task, non-matching → Checkbox) is issue 03's scope. This issue's scanner should not check tags.

## Key interfaces

- `ListItemType` enum — new type with `Plain`, `Checkbox`, `Task` variants. `Task` variant carries issue 01's `TaskStatus` struct (symbol + name + kind), not the old `Incomplete`/`Complete` enum
- `ListItem` struct — replace `task_status: Option<TaskStatus>` (old enum) with `item_type: ListItemType`
- Scanner function — pure function: `fn scan_marker(text: &str) -> Option<MarkerScan>` where `MarkerScan` captures the recognized symbol and trimmed remainder. Called during list item construction, not during event handling
- `InlineTokenLexer` struct — replaces free functions `extract_inline_fields` and `extract_task_inline_fields`. Owns a `has_marker: bool` flag that controls whether task emoji shorthands (dates, priority) are recognized. Returns flat `Vec<(FieldKey, NoteFieldValue)>` for fields and `Vec<Tag>` for tags — the lexer extracts tokens, the consumer aggregates into `IndexMap`
- `Note.tasks()` — filter logic changes from `is_task()` / `task_status()` to `matches!(item.item_type, ListItemType::Task(_))`. Return type changes (iterator) are issue 07
- `Note.list_items()` — no filtering change needed at this step; full API shaping is issue 07
- `ItemFrame` — replace `task_status: Option<TaskStatus>` with `marker_symbol: Option<char>`. Scanner sets this during item-leading text; classification happens in `end_item`
- `ListItem.is_task()` and `ListItem.is_completed()` — removed; replaced by `ListItemType` pattern matching
- Completion checks — use `TaskStatusType::completed()` from issue 01 (returns tri-state: `Some(true)` for done, `Some(false)` for incomplete, `None` for cancelled) instead of comparing against the old binary `TaskStatus::Complete`
- `query/record.rs` — `TaskRow.status` changes from old `TaskStatus` enum to issue 01's `TaskStatus` struct; `with_task_item` reads from `ListItemType::Task(task)`; `task_completed()` uses `TaskStatusType::completed()` instead of `task.status == TaskStatus::Complete`

## Parser flow

### Current (with `ENABLE_TASKLISTS`)

```
Event::TaskListMarker(checked)
  → set_task_status(checked): ItemFrame.task_status = Some(Complete/Incomplete)

Event::Text(text)
  → push_text(): append to text_buffer

Event::End(Item)
  → end_item(): ListItem::with_children(text, task_status, children)
  → flush_active_item_scan_buffer():
      if task_status.is_some() → extract_task_inline_fields()
      else                     → extract_inline_fields()
```

### After issue 02

```
Event::Text(text)
  → push_text():
      if text_buffer.is_empty()            ← item-leading text
        → scan_marker(text)
            Some(MarkerScan { symbol, remainder }) →
              item_frame.marker_symbol = Some(symbol)
              text = remainder
            None → text unchanged
      append text to buffers

Event::End(Item)
  → end_item():
      let item_type = match marker_symbol {
        Some(sym) => Task(TaskListItem::from_symbol(sym)),
        None      => Plain,
      }
      ListItem::with_children(text, item_type, children)

  → flush_active_item_scan_buffer():
      let lexer = InlineTokenLexer::new(marker_symbol.is_some());
      let fields = lexer.extract_fields(&scan_buffer);   ← no branch
      let tags = lexer.extract_tags(&scan_buffer);
```

Key changes:
- `Event::TaskListMarker` arm removed — event never fires without `ENABLE_TASKLISTS`
- Scanner runs in `push_text` on item-leading text (detected by `text_buffer.is_empty()`)
- Classification moves to `end_item` using stored marker symbol — all markers become `Task` when no tag filters are configured (tag-based reclassification is issue 03)
- `InlineTokenLexer` created with `marker_symbol.is_some()` — no branch at call site, `has_marker` flag controls internal behavior
- `extract_tags` is unconditional — tags exist on all list item types

## Removed types

- **Delete** `TaskStatus` enum (`Incomplete`/`Complete`) from `src/note/lists.rs:58-64`. It is a pulldown-cmark DTO that only represents `Event::TaskListMarker(bool)`. With `ENABLE_TASKLISTS` removed, this enum has no source.
- **Remove** `pub use lists::{..., TaskStatus}` from `src/note/mod.rs:40`
- **Remove** `ListItem.is_task()`, `ListItem.is_completed()`, `ListItem.task_status()` accessors — all replaced by `ListItemType` pattern matching
- **Remove** `ItemFrame.task_status: Option<TaskStatus>` field — replaced by `marker_symbol: Option<char>`
- **Replace** free functions `extract_inline_fields` and `extract_task_inline_fields` in `src/note/lexer.rs` with `InlineTokenLexer` struct. The two functions become methods on the new struct; `has_marker` flag replaces the caller's branch.
- Any code comparing against `TaskStatus::Complete` (e.g. `query/record.rs:110,265`) must switch to `TaskStatusType::completed()` from issue 01

## Acceptance criteria

- [ ] `ListItemType` enum exists with `Plain`, `Checkbox`, `Task` variants
- [ ] `ListItem` stores `ListItemType` instead of `task_status: Option<TaskStatus>` (old enum)
- [ ] `ListItemType::Task` carries issue 01's `TaskStatus` struct
- [ ] Old `TaskStatus` enum (`Incomplete`/`Complete`) deleted from `src/note/lists.rs`
- [ ] Old `TaskStatus` removed from `pub use` in `src/note/mod.rs`
- [ ] `ListItem.is_task()`, `is_completed()`, `task_status()` accessors removed
- [ ] `ItemFrame` stores `marker_symbol: Option<char>` instead of `task_status: Option<TaskStatus>`
- [ ] Free functions `extract_inline_fields` and `extract_task_inline_fields` replaced by `InlineTokenLexer` struct
- [ ] `InlineTokenLexer` accepts `has_marker: bool` flag; `true` enables task emoji shorthand recognition
- [ ] `InlineTokenLexer::extract_fields` returns `Vec<(FieldKey, NoteFieldValue)>` — flat token list, not IndexMap
- [ ] `InlineTokenLexer::extract_tags` returns `Vec<Tag>` — flat token list
- [ ] Lexer is unconditional on all list item types — no branch at call site, `has_marker` controls behavior internally
- [ ] `flush_active_item_scan_buffer` creates lexer with `marker_symbol.is_some()` — single line, no conditional logic
- [ ] Scanner recognizes `[x]`, `[X]`, `[/]`, `[-]`, `[!]`, `[?]`, and unknown single-character markers
- [ ] Scanner only accepts markers at item-leading position followed by whitespace
- [ ] Later bracket text in item body is not trimmed as a marker
- [ ] `[x]` and `[X]` both resolve to Done status via `TaskStatusMap`
- [ ] `[/]`, `[-]`, `[!]` resolve to configured default statuses (in-progress, on-hold, non-task)
- [ ] Unknown markers (e.g. `[?]`) are preserved and resolved as incomplete todo by default
- [ ] Unknown markers are never downgraded to plain bullets
- [ ] When no tag filters are configured, all status-marked items become `ListItemType::Task`
- [ ] `ENABLE_TASKLISTS` is removed from pulldown-cmark options
- [ ] `set_task_status` method removed
- [ ] `Event::TaskListMarker` arm removed from event handler
- [ ] `end_item` classifies marker items as `Task` — all status-marked items become `Task` when no tag filters are configured
- [ ] `Note.tasks()` classification filter uses `ListItemType::Task` (return type changes deferred to issue 07)
- [ ] `query/record.rs` `TaskRow.status` uses issue 01's `TaskStatus` struct, not old enum
- [ ] `query/record.rs` completion checks use `TaskStatusType::completed()`, not `== TaskStatus::Complete`
- [ ] Unit tests cover scanner recognizing all marker types: `[x]`, `[X]`, `[/]`, `[-]`, `[!]`, `[?]`, unknown
- [ ] Unit tests cover scanner only accepts markers at item-leading position followed by whitespace
- [ ] Unit tests cover later bracket text in item body is not trimmed as marker
- [ ] Unit tests cover unknown markers preserved and classified as incomplete todo
- [ ] Unit tests cover `[x]`/`[X]` equivalence — both resolve to Done
- [ ] Unit tests cover `[/]`, `[-]`, `[!]` resolution to default statuses
- [ ] Unit tests cover `InlineTokenLexer` with `has_marker: true` and `has_marker: false`
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
