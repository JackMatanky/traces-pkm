Status: ready-for-agent

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

- [ ] `SourceLine(u32)` newtype with `From`, `Into`, `Display`, `PartialOrd`, `Ord`, `Eq`, `Hash`, `Copy`, `Clone`
- [ ] `ByteOffset(usize)` newtype with `From<usize>`, `From<ByteOffset> for usize`, `Copy`, `Clone`
- [ ] `ByteTracker` struct with `line_starts: Vec<usize>` field
- [ ] `ByteTracker::new(source: &str) -> Self` precomputes line starts
- [ ] `ByteTracker::byte_to_line(offset: ByteOffset) -> SourceLine` uses `partition_point`
- [ ] Edge case: `ByteOffset(0)` → `SourceLine(1)`
- [ ] Edge case: offset at line boundary returns that line (not next)
- [ ] Edge case: empty source → `SourceLine(1)`
- [ ] Edge case: offset beyond source length → last line
- [ ] Unit test: single-line source, any offset returns `SourceLine(1)`
- [ ] Unit test: multi-line source, offsets at line starts return correct `SourceLine` values
- [ ] Unit test: empty lines counted as separate lines
- [ ] Unit test: offset at exact line boundary returns that line
- [ ] Unit test: offset beyond source length returns last line
- [ ] Unit test: empty source returns `SourceLine(1)`
- [ ] Parser updated to use `ByteOffset` and `SourceLine` instead of raw `usize`
- [ ] `ListItem.line` and `ListItem.parent_line` use `SourceLine` instead of `usize`
- [ ] `cargo test` passes, `cargo clippy` clean

## Out of scope

- Task classification, `ListItemType`, custom marker scanner — issue 02
- Tag filter classification — issue 03
- `fully_complete` computation — issue 05
- Text normalization, priority, dates — issue 06
- LISTS persistence, `ListRecord` — issue 07
