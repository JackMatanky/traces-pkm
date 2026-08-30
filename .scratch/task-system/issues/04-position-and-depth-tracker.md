Status: ready-for-agent

# 04 — Position and depth tracker

**What to build:** A byte-to-line tracker utility that precomputes line starts from source text and converts byte offsets to source line numbers. Pure utility, no dependency on task classification or list item types.

**Blocked by:** None — can start immediately (parallel with 01).

**Note:** This issue builds the `ByteTracker` utility only. Issue 01 owns the `ListItem` position fields (`depth`, `line`, `parent_line`) and parser integration. The parser in 01 should use this tracker for byte-to-line conversion.

- [ ] `ByteTracker` struct with `line_starts: Vec<usize>` field
- [ ] `ByteTracker::new(source: &str) -> Self` — precomputes line start byte offsets from source text
- [ ] `ByteTracker::byte_to_line(&self, offset: usize) -> u32` — converts byte offset to 1-indexed line number via `partition_point` (binary search)
- [ ] Handles edge cases: offset 0 → line 1, offset at line boundary → correct line, empty source → line 1
- [ ] Unit test: single-line source, any offset returns line 1
- [ ] Unit test: multi-line source, offsets at line starts return correct lines
- [ ] Unit test: empty lines counted as separate lines
- [ ] Unit test: offset at exact line boundary returns that line (not next)
- [ ] Unit test: offset beyond source length returns last line
- [ ] Unit test: empty source returns line 1
