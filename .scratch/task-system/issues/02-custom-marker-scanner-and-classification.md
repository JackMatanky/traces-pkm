Status: implemented

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

## Implementation notes

**Date**: 2026-09-02. **Implemented in**: `35c8a0d`, branch
`task-system/02-custom-marker-scanner` (worktree
`.worktrees/02-custom-marker-scanner/`, not merged to `main`).

### Where it landed

| File | Purpose |
|------|---------|
| `src/lexer.rs` | `DelimiterType`, `QuoteType`, `DelimiterStack`, `find_closing_delimiter`, `lexical_unquote`, `LexTokenStream::delimited` — deep shared primitives for balanced delimiter tracking and quote unescaping; 18 tests |
| `src/note/parser/marker.rs` | `MarkerScan` / `MarkerPrefix` / `scan_marker_prefix` / `scan_marker_at_line_end` — the sole source of truth for marker identity; 17 tests |
| `src/note/parser/inline.rs` | `parse_inline_value` / `InlineValueParser` — recursive-descent value parser for Dataview inline field values (lists, quotes, durations, wikilinks, dates, numbers, tags); 6 test modules |
| `src/note/parser/lexer.rs` | `InlineTokenLexer { has_marker }` with `extract_fields`/`extract_tags` — Logos token scanner using shared `find_closing_delimiter` for wrapped fields; tests rewritten onto the struct |
| `src/note/parser/line.rs` | `ByteTracker` — precomputed newline byte offsets for $O(\log n)$ line lookup; 5 tests |
| `src/note/parser/list.rs` | `ListTracker`/`ItemFrame.leading` (`LeadingMarker` Pending/Decided) incremental scanner; `end_item` classifies via `TaskStatusMap::resolve`; 20 tests |
| `src/note/parser.rs` | `parse_markdown` orchestrator + `ParserContext` event loop |
| `src/note/lists.rs` | `ListItemType` (Plain/Checkbox/Task) replaces old `TaskStatus` DTO; `ListItem.item_type()` |
| `src/note/links.rs` | `find_wikilink_close` delegates to shared `find_closing_delimiter(..., DelimiterType::DoubleBracket)` |
| `src/note/model.rs` | `TaskIter` filters on `matches!(item_type, Task(_))` |
| `src/task.rs` | serde derives; `TaskStatusMap::resolve(symbol)` |
| `src/query/record.rs` | `TaskRow.status` is issue 01's `TaskStatus`; `task_completed()` routes through `TaskStatusType::completed()` |
| `src/query/format.rs` | `render_task_list` distinguishes page rows from cancelled tasks |
| `src/query/grammar/{filter,source}.rs` | Uses shared `lexical_unquote` from `src/lexer.rs` |

`src/note/parser.rs` is organized into five specialized submodules:
`src/note/parser/{inline,lexer,line,list,marker}.rs`, matching the codebase's
sibling-file-plus-directory convention (`schema/fields.rs` +
`schema/fields/`, `template/engine.rs` + `template/engine/`).
2081 lib tests across the workspace; clippy, fmt, and doc clean.
### Key design decisions

1. **Incremental leading-marker state machine, gated like pulldown-cmark's
   first pass.** Studying `pulldown-cmark`'s `scan_task_list_marker`
   (`scanners.rs`) and its `firstpass.rs` call site shows the marker is
   scanned immediately after the list bullet, before any inline content —
   and without `ENABLE_TASKLISTS`, pulldown splits a leading `[x] Task`
   into four `Event::Text` chunks (`"["`, `"x"`, `"]"`, `" Task"`).
   Consequences the parser must reproduce:
   - each leading chunk extends a candidate that is re-classified until
     the scanner decides (`ItemFrame::LeadingMarker`: Pending → Decided);
   - inline content events (emphasis/code/links/images/inline HTML)
     occupying the leading slot reject the marker (`- **[x] Task**` and
     `` - `[x]` Task `` stay plain, matching old behavior);
   - the trailing whitespace is ASCII-only (NBSP is ordinary text);
   - a line terminator (soft break, nested-list start, item end) supplies
     the trailing whitespace, so `- [x]` alone is still a task —
     `scan_marker_at_line_end` implements this.
   Scanning only the first chunk (the issue's `push_text` sketch) or only
   once at item end both miss markers or misclassify; the state machine
   keeps the issue's `push_text` placement while handling chunking.
2. **`TaskStatusMap::resolve` centralizes the unknown-marker fallback**
   (by-symbol hit, else incomplete Todo preserving the symbol). Issue 03
   can reuse it unchanged when the configured map replaces the parser's
   default map.
3. **Parser resolves `TaskStatusMap::default()` via a `LazyLock` static
   (`task::DEFAULT_TASK_STATUSES`), built once per process instead of once
   per `parse_markdown` call.** Real config threading into `parse_markdown`
   is issue 03's "config resolution" scope; this issue's ~570
   `parse_markdown` call sites keep their signature. Benchmarking (below)
   caught the per-call rebuild costing 6 `String` allocations + 3 `HashMap`s
   on every parse before the static existed.
4. **`Checkbox` ships as a bare unit variant.** No issue-02 code path
   constructs it (no tag filters yet); the spec reserves extending the
   variant without breaking the enum.
5. **The tri-state surfaced one real bug beyond the brief**:
   `render_task_list` treated "cancelled task" (`None`) and "not a task
   row" (`None`) identically, so any `- [-]` note made `task_list`
   rendering error out. Fixed by keying the row check on `task_text()`
   presence; cancelled tasks render as `- [-]` (regression test added).

### Deviations from the ticket

| Ticket said | What happened | Why |
|------------|---------------|-----|
| `[/]`, `[-]`, `[!]` resolve to "in-progress, on-hold, non-task" | They resolve to the issue 01 default table: `/`→In Progress, `-`→Cancelled, `!`→On Hold | The issue-01 status table is the source of truth and already shipped; redefining defaults here would contradict the shipped model |
| Scanner recognizes only "trailing whitespace" (unqualified) | ASCII whitespace only (space, tab, CR, LF, VT, FF) | Mirrors pulldown-cmark's `is_ascii_whitespace`; Unicode spaces such as NBSP are ordinary Markdown text |

### Verification

```sh
cargo test --all-features   # 2063 lib + 4 unit-bin + 20 e2e
                            # + 12 integration + 14 doc-tests, all pass
cargo clippy --workspace --all-targets --all-features  # clean
cargo fmt -- --check        # clean
cargo doc --no-deps --all-features  # clean with RUSTDOCFLAGS=-D warnings
```

### Performance

Added two `benches/note_parsing.rs` groups (`task_marker_variants`,
`task_marker_scaling`) and re-ran the full suite on this branch against
`main`'s pre-issue-02 implementation (same fixtures, same toolchain,
`cargo bench --features test-utils`; medians from `target/criterion`
`estimates.json`).

Three rounds of measurement, not one:

1. First pass showed small/medium notes regressing +142%/+23% alongside
   the expected task-item cost. Root cause: `TaskStatusMap::default()` was
   rebuilt (6 `String` allocs + 3 `HashMap`s) on every `parse_markdown`
   call. Fixed by hoisting it to `task::DEFAULT_TASK_STATUSES`, a
   `LazyLock<TaskStatusMap>` built once. Re-measured: small/medium/prose/
   frontmatter/wikilink/line-density benches all landed within ±2% of
   `main` (noise).
2. A second experiment (temporarily stripping the `TaskStatus.name`
   `String` clone out of `resolve()`) showed only ~2% recovery, ruling out
   per-item allocation as the driver of the remaining cost.
3. A third experiment tested whether pulldown fragmenting a task item's
   leading `[ ]` into 4 separate `Text` events (`"["`, `" "`, `"]"`,
   `" remainder"`, once `ENABLE_TASKLISTS` is off) was the driver, on the
   theory that 4x-ing `handle_event` dispatch explained the cost. Wiring
   in `pulldown_cmark::TextMergeWithOffset` (public, offset-iterator
   compatible, confirmed by a standalone probe to collapse those 4 events
   into 1 `Text("[ ] remainder")` per item) produced **no measurable
   change** on `task_marker_scaling`/`list_item_scaling` and a small
   regression elsewhere (`plain_bullets` +2%→+10%, from the wrapper's
   extra per-event indirection with nothing to merge). Reverted — the
   dispatch-count theory was wrong.

What's left is task-item-count-scaled: `list_item_scaling` and
`task_marker_scaling` run +27–60% slower than `main` per task item. The
merge experiment isolates the cause to the classification work itself,
not event count: with `ENABLE_TASKLISTS`, main gets a free
`TaskListMarker(bool)` event and sets `task_status` from the bool
directly - no lookup, no allocation, no state machine. This branch runs
the full `LeadingMarker` char-by-char scan plus a `TaskStatusMap::resolve`
`HashMap` lookup (config-driven, so it can't be a compile-time match) and
builds a wider `ListItemType::Task(TaskStatus)` variant, once per task
item - real work in exchange for arbitrary configurable single-char
markers instead of a hardcoded `x`/`X` boolean. That trade is this
issue's entire purpose; recovering the difference would mean caching or
skipping the classification, not touching event plumbing. In absolute
terms it is small: a realistic note with a few dozen task items costs
low-single-digit microseconds more to parse.

Environment note: `hk check`/`mise run check` fail in this sandbox from
a corrupted mbx build cache (cached build-script binaries are literally
the `mbx` binary — reproduced on pristine `main`); verification ran with
the same cargo toolchain invoked directly.
