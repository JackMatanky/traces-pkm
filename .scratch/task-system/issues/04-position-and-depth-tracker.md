Status: ready-for-agent

# 04 — Position and depth tracker

**What to build:** A byte-to-line tracker utility that precomputes line starts from source text and converts byte offsets to source line numbers. This is a pure utility with no dependency on task classification. Populate `depth`, `line`, and `parent_line` on list items during parsing using the tracker.

**Blocked by:** None — can start immediately (parallel with 01).

- [ ] Byte-to-line tracker struct that precomputes line start byte offsets
- [ ] Tracker converts byte offset to 1-indexed line number via binary search
- [ ] Tracker is constructed once per note parse from source text
- [ ] Parser populates `line` on each list item from its opening byte offset
- [ ] Parser populates `depth` from list nesting level
- [ ] Parser populates `parent_line` from the nearest ancestor list item's line
- [ ] Top-level items have `parent_line: None`
- [ ] Unit tests for byte-to-line conversion (single line, multi-line, empty lines)
- [ ] Unit tests for depth and parent_line population in nested lists
- [ ] Unit tests for top-level items having `parent_line: None`
