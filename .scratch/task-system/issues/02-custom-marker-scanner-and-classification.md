Status: implemented

# 02 — Custom marker scanner and list item classification

**Date**: 2026-09-03
**Implemented in**: `35c8a0d` through `6d47dd8`, branch `task-system/02-custom-marker-scanner` (worktree `.worktrees/02-custom-marker-scanner/`)

**What to build:** Replace the `ENABLE_TASKLISTS` pulldown-cmark option, `set_task_status` method, and old binary `TaskStatus` enum with a custom marker scanner and `ListItemType` enum (Plain, Checkbox, Task). The scanner is the only source of truth for task marker identity. `ListItem` stores `ListItemType` replacing `task_status: Option<TaskStatus>`. Completion checks use `TaskStatusType::completed()` from issue 01. Free functions `extract_inline_fields` and `extract_task_inline_fields` are replaced by `InlineTokenLexer` struct with `has_marker` flag — no branching at call site, lexer returns flat token lists. The scanner recognizes `[x]`, `[X]`, `[/]`, `[-]`, `[!]`, `[?]`, and unknown single-character markers. `[x]` and `[X]` are equivalent and both map to Done.

**Blocked by:** 01 (needs `TaskStatusMap` for symbol→status resolution).

## Current behavior

The parser enables `ENABLE_TASKLISTS` on pulldown-cmark, which emits `Event::TaskListMarker(bool)`. The `set_task_status` method converts this to a binary `TaskStatus::Complete` / `TaskStatus::Incomplete` (the existing enum in `src/note/lists.rs:58-64`) stored as `task_status: Option<TaskStatus>` on `ListItem`. This enum is a pulldown-cmark DTO — it only represents the boolean checked/unchecked state. All task classification flows through this single path. There is no support for custom markers (`[/]`, `[-]`, `[!]`, unknown), and `ListItem` has no way to distinguish a plain bullet from a checkbox from a task. The old `TaskStatus` enum is publicly re-exported from `src/note/mod.rs:40` and used in `query/record.rs` for `TaskRow.status` and completion comparison.

## Desired behavior

A custom marker scanner is the only source of truth for task marker identity. It recognizes `[x]`, `[X]`, `[/]`, `[-]`, `[!]`, `[?]`, and unknown single-character markers at item-leading position. `ListItem` stores a `ListItemType` enum (`Plain`, `Checkbox`, `Task`) replacing the old `task_status: Option<TaskStatus>` field. The scanner trims the leading marker prefix exactly once. Unknown markers are never downgraded to plain bullets. `[x]` and `[X]` are equivalent and both map to Done via `TaskStatusMap` (from issue 01). Free functions `extract_inline_fields` and `extract_task_inline_fields` are replaced by `InlineTokenLexer` struct with `has_marker: bool` flag — no branching at call site, lexer returns flat token lists (`Vec<(FieldKey, NoteFieldValue)>` and `Vec<Tag>`), caller aggregates into `IndexMap`.

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

- [x] `ListItemType` enum exists with `Plain`, `Checkbox`, `Task` variants
- [x] `ListItem` stores `ListItemType` instead of `task_status: Option<TaskStatus>` (old enum)
- [x] `ListItemType::Task` carries issue 01's `TaskStatus` struct
- [x] Old `TaskStatus` enum (`Incomplete`/`Complete`) deleted from `src/note/lists.rs`
- [x] Old `TaskStatus` removed from `pub use` in `src/note/mod.rs`
- [x] `ListItem.is_task()`, `is_completed()`, `task_status()` accessors removed
- [x] `ItemFrame` stores `marker_symbol: Option<char>` instead of `task_status: Option<TaskStatus>`
- [x] Free functions `extract_inline_fields` and `extract_task_inline_fields` replaced by `InlineTokenLexer` struct
- [x] `InlineTokenLexer` accepts `has_marker: bool` flag; `true` enables task emoji shorthand recognition
- [x] `InlineTokenLexer::extract_fields` returns `Vec<(FieldKey, NoteFieldValue)>` — flat token list, not IndexMap
- [x] `InlineTokenLexer::extract_tags` returns `Vec<Tag>` — flat token list
- [x] Lexer is unconditional on all list item types — no branch at call site, `has_marker` controls behavior internally
- [x] `flush_active_item_scan_buffer` creates lexer with `marker_symbol.is_some()` — single line, no conditional logic
- [x] Scanner recognizes `[x]`, `[X]`, `[/]`, `[-]`, `[!]`, `[?]`, and unknown single-character markers
- [x] Scanner only accepts markers at item-leading position followed by whitespace
- [x] Later bracket text in item body is not trimmed as a marker
- [x] `[x]` and `[X]` both resolve to Done status via `TaskStatusMap`
- [x] `[/]`, `[-]`, `[!]` resolve to configured default statuses (in-progress, on-hold, non-task)
- [x] Unknown markers (e.g. `[?]`) are preserved and resolved as incomplete todo by default
- [x] Unknown markers are never downgraded to plain bullets
- [x] When no tag filters are configured, all status-marked items become `ListItemType::Task`
- [x] `ENABLE_TASKLISTS` is removed from pulldown-cmark options
- [x] `set_task_status` method removed
- [x] `Event::TaskListMarker` arm removed from event handler
- [x] `end_item` classifies marker items as `Task` — all status-marked items become `Task` when no tag filters are configured
- [x] `Note.tasks()` classification filter uses `ListItemType::Task` (return type changes deferred to issue 07)
- [x] `query/record.rs` `TaskRow.status` uses issue 01's `TaskStatus` struct, not old enum
- [x] `query/record.rs` completion checks use `TaskStatusType::completed()`, not `== TaskStatus::Complete`
- [x] Unit tests cover scanner recognizing all marker types: `[x]`, `[X]`, `[/]`, `[-]`, `[!]`, `[?]`, unknown
- [x] Unit tests cover scanner only accepts markers at item-leading position followed by whitespace
- [x] Unit tests cover later bracket text in item body is not trimmed as marker
- [x] Unit tests cover unknown markers preserved and classified as incomplete todo
- [x] Unit tests cover `[x]`/`[X]` equivalence — both resolve to Done
- [x] Unit tests cover `[/]`, `[-]`, `[!]` resolution to default statuses
- [x] Unit tests cover `InlineTokenLexer` with `has_marker: true` and `has_marker: false`
- [x] All existing task classification tests rewritten against the scanner
- [x] `cargo test` passes, `cargo clippy` clean

## Implementation notes

### Where it landed

| File | Lines | Purpose |
|---|---|---|
| `src/note/parser/marker.rs` | 276 (new) | Custom item-leading marker scanner (`scan_marker_prefix`, `scan_marker_at_line_end`), `MarkerScan`, `MarkerPrefix` |
| `src/note/parser/list.rs` | 733 (new) | `ListTracker`, `ItemFrame`, `ItemClassificationState` incremental state machine, 18 tests |
| `src/note/parser/lexer.rs` | 708 (new) | `InlineTokenLexer` extracting inline fields and tags via logos with `has_marker` gating, 32 tests |
| `src/note/parser/inline.rs` | 511 (new) | Recursive-descent inline field value parser (`parse_inline_value`), zero-allocation duration parser, 24 tests |
| `src/note/parser/line.rs` | 136 (new) | `ByteTracker` for binary-searched byte offset to `SourceLine` translation |
| `src/note/parser.rs` | 1,307 | Facade coordinate submodule pipeline, Markdown event loop with `ENABLE_TASKLISTS` removed |
| `src/delimiter.rs` | 444 (new) | Zero-allocation delimiter tracking (`DelimiterStack`, `DelimiterType::find_closing`, `QuoteType`), 14 tests |
| `src/lexer.rs` | 733 | Shared lexer abstractions (`LexTokenStream`, `TokenSpec`, `LexedToken`, `LexError`, string unquoting) |
| `src/note/lists.rs` | 400 | `ListItemType` enum (`Plain`, `Checkbox`, `Task`), `ListItem::item_type`, old `TaskStatus` removed |
| `src/note/model.rs` | 425 | `Note::tasks()` filtering over `ListItemType::Task` |
| `src/query/record.rs` | 520 | `TaskRow.status` uses `TaskStatus`, completion checks use `TaskStatusType::completed()` |
| `src/task.rs` | 555 | `DEFAULT_TASK_STATUSES` `LazyLock<TaskStatusMap>` default status map bridge |
| `benches/note_parsing.rs` | 552 | `marker_variety` and `task_metadata` Criterion benchmark suites |

### Key design decisions

1. **Parser Submodule Decomposition**: `src/note/parser.rs` was split into five focused submodules under `src/note/parser/` (`inline`, `lexer`, `line`, `list`, `marker`), isolating recursive-descent parsing, Logos token scanning, byte-line translation, list stack management, and task marker identity.
2. **Incremental Stream Re-Classification**: Pulldown-cmark emits leading marker characters across multiple `Event::Text` chunks (e.g. `"["`, `"x"`, `"]"`). `ItemFrame` maintains `ItemClassificationState::Pending` until the buffer reaches `Complete` or `Rejected` before any inline content or structural block event.
3. **End-of-Line Marker Semantics**: When a list item has no trailing text before a newline or nested list (e.g. `- [x]\n  - child`), `scan_marker_at_line_end` treats line termination as whitespace, mirroring pulldown-cmark's first-pass behavior.
4. **Shared Delimiter Infrastructure**: Extracted `src/delimiter.rs` providing `DelimiterType` and stack-allocated `DelimiterStack` (`MAX_DELIMITER_DEPTH = 16`), shared across wikilinks, inline field lexing, and grammar parsers with zero heap allocations.
5. **Flat Lexer Returns**: `InlineTokenLexer::extract_fields` and `extract_tags` return flat `Vec` token streams, deferring `IndexMap` aggregation to the caller while `has_marker` internally gates task emoji shorthands without call-site branches.
6. **Principle of Least Privilege**: `DelimiterStack`, `QuoteType`, `lexical_backslash_unescape`, and `ItemClassificationState` are strictly private to their respective modules, and `InlineTokenLexer` is restricted to `pub(super)`.
7. **Zero-Allocation Inline Helpers**: `is_duration_unit` uses a stack-allocated buffer to perform case-insensitive phf set matching without heap allocations.
8. **Lazy Status Resolution Bridge**: `DEFAULT_TASK_STATUSES` `std::sync::LazyLock<TaskStatusMap>` provides instant status resolution for standalone parser runs until Issue 03 injects resolved configuration.

### Test inventory

- `note/parser/marker.rs` (10 tests): scanner recognizes `[x]`, `[X]`, `[/]`, `[-]`, `[!]`, `[?]`, unknown markers, whitespace requirements, and rejection on non-leading/inline text.
- `note/parser/list.rs` (18 tests): classification across nested lists, line end decisions, code exclusions, and `ItemClassificationState` accessors.
- `note/parser/lexer.rs` (32 tests): `InlineTokenLexer` with `has_marker: true`/`false`, body fields, wrapped fields, task emoji shorthands, and tags.
- `note/parser/inline.rs` (24 tests): comma-separated lists, quoted strings, wikilinks, durations, booleans, nulls, dates, numbers, and tags.
- `delimiter.rs` (14 tests): matching parenthesis, brackets, braces, double brackets, nested quotes, and active quote state tracking.
- `benches/note_parsing.rs`: benchmark coverage for small, medium, large, pure prose, code blocks, dense frontmatter, dense wikilinks, dense tasks, marker variety, and task metadata.

### Verification

```sh
cargo test --all-features # 2,078 passed (0 failed)
cargo clippy --workspace --all-targets --all-features # clean (0 warnings)
cargo fmt -- --check # clean
cargo doc --no-deps --all-features # clean with RUSTDOCFLAGS="-D warnings"
```

### Unblocked

- **Issue 03** (config resolution + tag filter classification) can now inject `TaskConfig::statuses()` into the parser and implement tag-based `ListItemType::Checkbox` vs `ListItemType::Task` reclassification.
- **Issue 05** (`fully_complete` computation) and **Issue 07** (`Note.tasks()` iterator / LISTS persistence) can consume `ListItemType` and `ListItem::item_type`.
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
