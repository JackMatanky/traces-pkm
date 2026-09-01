Status: implemented

**Date**: 2026-09-01
**Implemented in**: `251a215`..`f789fdf`, branch `task-system/04-position-and-depth-tracker`
(worktree `.worktrees/04-position-and-depth-tracker/`, not yet merged to `main`)

# 04 — ByteTracker

**What to build:** A byte-to-line tracker utility with `SourceLine` and `ByteOffset` newtypes. The tracker precomputes line starts from source text and converts byte offsets to source line numbers. The newtypes prevent byte/line confusion at compile time.

**Blocked by:** None — can start immediately (parallel with 01).

**Note:** Issue 01 integrated a `LineTracker` struct into the parser. This issue extracts that into a public `ByteTracker` module, adds `SourceLine(u32)` and `ByteOffset(usize)` newtypes per grilling session Q24/Q59, and updates the parser to use them. The `LineTracker` implementation moves here; the parser imports `ByteTracker`.

## Current behavior

`LineTracker` is a private struct in `src/note/parser.rs:362-394` with `line_starts: Vec<usize>`, `new(source)`, and `line_for(offset) -> usize`. `ListItem` stores `line: usize` and `parent_line: Option<usize>`. There is no type distinction between byte offsets and line numbers — both are raw `usize`.

## Desired behavior

`ByteTracker` is a public utility module with:
- `SourceLine(u32)` newtype for 1-indexed line numbers (grilling Q24, Q59)
- `ByteOffset(usize)` newtype for byte offsets
- `ByteTracker` struct with `line_starts: Vec<usize>`
- Conversion methods returning `SourceLine` instead of raw `usize`

The parser uses `ByteOffset` when reading byte offsets from pulldown-cmark and `SourceLine` when storing line numbers on `ListItem`. The type system prevents passing a byte offset where a line number is expected.

## Key interfaces

- `SourceLine(u32)` — newtype, 1-indexed line number. Implements `From<SourceLine> for u32`, `Display`, `PartialOrd`, `Ord`, `Eq`, `Hash`, `Copy`, `Clone`
- `ByteOffset(usize)` — newtype for byte offsets. Implements `From<usize> for ByteOffset`, `From<ByteOffset> for usize`, `Copy`, `Clone`
- `ByteTracker` struct — `line_starts: Vec<usize>` field
- `ByteTracker::new(source: &str) -> Self` — precomputes line start byte offsets
- `ByteTracker::byte_to_line(&self, offset: ByteOffset) -> SourceLine` — converts byte offset to 1-indexed line via `partition_point`
- Edge cases: `ByteOffset(0)` → `SourceLine(1)`, offset at line boundary → correct line, empty source → `SourceLine(1)`, offset beyond source length → last line

## Acceptance criteria

- [x] `SourceLine(u32)` newtype with `From`, `Into`, `Display`, `PartialOrd`, `Ord`, `Eq`, `Hash`, `Copy`, `Clone`
- [x] `ByteOffset(usize)` newtype with `From<usize>`, `From<ByteOffset> for usize`, `Copy`, `Clone`
- [x] `ByteTracker` struct with `line_starts: Vec<usize>` field
- [x] `ByteTracker::new(source: &str) -> Self` precomputes line starts
- [x] `ByteTracker::byte_to_line(offset: ByteOffset) -> SourceLine` uses `partition_point`
- [x] Edge case: `ByteOffset(0)` → `SourceLine(1)`
- [x] Edge case: offset at line boundary returns that line (not next)
- [x] Edge case: empty source → `SourceLine(1)`
- [x] Edge case: offset beyond source length → last line
- [x] Unit test: single-line source, any offset returns `SourceLine(1)`
- [x] Unit test: multi-line source, offsets at line starts return correct `SourceLine` values
- [x] Unit test: empty lines counted as separate lines
- [x] Unit test: offset at exact line boundary returns that line
- [x] Unit test: offset beyond source length returns last line
- [x] Unit test: empty source returns `SourceLine(1)`
- [x] Parser updated to use `ByteOffset` and `SourceLine` instead of raw `usize`
- [x] `ListItem.line` and `ListItem.parent_line` use `SourceLine` instead of `usize`
- [x] `cargo test` passes, `cargo clippy` clean

## Implementation notes

### Where it landed

| File | Purpose |
|------|---------|
| `src/position.rs` | New. `SourceLine`, `ByteOffset` (`pub(crate)`), 1 test |
| `src/note/parser.rs` | `LineTracker` renamed `ByteTracker` (stays private, same file); 5 tests moved verbatim into `parser::tests::byte_tracker`; `ParserContext.line_tracker` is `ByteTracker`; `handle_event`/`start_item` take `ByteOffset` end to end from `range.start` |
| `src/note/lists.rs` | `ListItem.line`/`parent_line` retyped to `SourceLine`/`Option<SourceLine>`; `with_position`/`line()`/`parent_line()` signatures updated; imports `SourceLine` from `crate::position` |
| `src/lib.rs` | `mod position;` declaration |

### Where it did *not* land

`ByteTracker` was drafted as its own `src/note/byte_tracker.rs` module first
(matching the issue text's "extracts that into a public `ByteTracker`
module" literally), then moved back into `note::parser` as a private struct
after review: `ByteTracker` is a precomputing index tuned for the parser's
repeated-lookup pattern (one `byte_to_line` call per list item, amortized via
a `Box<[usize]>` built once), which is a different shape of problem from
`cli/error.rs`'s independent one-shot `line_column` (byte offset → column,
for a single minijinja render error). No second caller needs `ByteTracker`
itself, so it stays local per `spec.md` line 63 ("line tracking stays local
and simple"). `SourceLine`/`ByteOffset` moved to `src/position.rs` instead —
they are general position vocabulary (not a note-parsing concept), so they
live where any future caller (e.g. a `cli/error.rs` rewrite) can reach them
without depending on `note::`.

### Key design decisions

1. **`ByteOffset` flows end to end from pulldown-cmark.** `parse_markdown` wraps
   `range.start` as `ByteOffset::from(range.start)` immediately, and
   `ParserContext::handle_event`/`start_item` carry `ByteOffset` (not raw
   `usize`) through to `ByteTracker::byte_to_line`, matching the issue's "the
   parser uses `ByteOffset` when reading byte offsets from pulldown-cmark".
2. **`SourceLine` default sentinel stays `SourceLine::new(0)`.** `ListItem::new`/
   `with_children` default to `depth: 0`, `line: SourceLine::new(0)`,
   `parent_line: None` until `with_position` is called — unchanged sentinel
   semantics from the prior `line: usize = 0`, just typed. `0` is never
   produced by `byte_to_line` itself (`partition_point` over a non-empty
   `line_starts` always yields `>= 1`); it is only the pre-parse "unset" marker.
3. **`u32::try_from(line).unwrap_or(u32::MAX)`** narrows the `partition_point`
   `usize` result to `SourceLine`'s `u32`, matching the existing
   `DenseIndex::saturating_u32` idiom in `src/schema/graph/adjacency.rs`
   instead of `as` (denied by `clippy::as_conversions` intent, though only a
   warn) or `unwrap()` (denied by `clippy::unwrap_used`).
4. **`ByteOffset` derives only `Copy, Clone`** per the issue's key-interfaces
   list; `SourceLine` additionally derives `Debug, Eq, Hash, Ord, PartialEq,
   PartialOrd, Deserialize, Serialize` — `Debug`/`PartialEq` because
   `assert_eq!` needs them in tests, `Deserialize`/`Serialize` because
   `ListItem` (which now holds a `SourceLine` field) derives both for postcard
   persistence.
5. **Visibility, narrowest first.** `ByteTracker` (struct + `new`/`byte_to_line`)
   has no `pub` modifier at all — module-private to `note::parser`, same as the
   original `LineTracker`. `SourceLine`/`ByteOffset` in `src/position.rs` are
   `pub(crate)`: the minimum that lets `note::lists`/`note::parser` (a sibling
   subtree) reach them, and the correct scope given they're deliberately
   general-purpose, not note-only.

### Verification

```sh
cargo test --lib --all-features note::      # 219 passed
cargo test --lib --all-features parser::tests::byte_tracker  # 5 passed
cargo test --lib --all-features position::  # 1 passed
cargo clippy --workspace --all-targets --all-features  # clean (pre-existing
  large_stack_frames warnings in config/builder.rs, index/store.rs,
  cli/template.rs are untouched by this change)
cargo fmt --all -- --check  # clean
cargo test --workspace --all-features  # 2010 + 4 + 20 + 12 passed, 14 doctests
```

## Out of scope

- Task classification, `ListItemType`, custom marker scanner — issue 02
- Tag filter classification — issue 03
- `fully_complete` computation — issue 05
- Text normalization, priority, dates — issue 06
- LISTS persistence, `ListRecord` — issue 07
