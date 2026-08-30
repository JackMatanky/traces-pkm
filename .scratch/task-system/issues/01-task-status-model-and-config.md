Status: ready-for-agent

# 01 — Task status model and config

**What to build:** The foundational data model for task statuses and configuration. A `TaskStatusType` enum (todo, in-progress, on-hold, done, cancelled, non-task), a `TaskStatusSymbol` newtype, a `TaskConfig` struct with `statuses: TaskStatusMap` and `tag_filters: Vec<Tag>`, and `TaskStatusMap` with lookup by symbol, name, and type. Tag is moved to a shared domain type used by config, parsing, indexing, and query code. Config parsing validates `tag_filters`, normalizes `task`/`#task` entries, and fails on invalid entries. List items gain `depth`, `line`, and `parent_line` fields populated by the parser.

**Blocked by:** None — can start immediately.

- [ ] `TaskStatusType` enum with variants: todo, in-progress, on-hold, done, cancelled, non-task
- [ ] `TaskStatusSymbol` newtype for single-character marker symbols
- [ ] `TaskConfig` struct with `statuses: TaskStatusMap` and `tag_filters: Vec<Tag>`
- [ ] `TaskStatusMap` built once at config resolution with lookup by symbol, by name, and by type
- [ ] Status-name lookup normalized by case-folding, leading/trailing whitespace trimming, and internal whitespace collapsing to a single space
- [ ] Tag moved to shared domain type used by config, note parsing, indexing, and query code
- [ ] Tag validation and exact-match helper (`is_exact_match`)
- [ ] Config parsing accepts `task` and `#task` (leading `#` optional, normalized before constructing Tag)
- [ ] Empty `tag_filters` means every status-marked list item is a Task
- [ ] Invalid `tag_filters` entries fail config loading with diagnostic identifying offending entry and config location
- [ ] `ListItem` gains `depth: usize`, `line: usize`, and `parent_line: Option<usize>` fields
- [ ] Parser populates depth, line, and parent_line from existing byte offsets during list item construction
- [ ] Completion is a tri-state: `Some(true)` for done, `Some(false)` for incomplete, `None` for cancelled
- [ ] Unit tests for TaskStatusMap lookup by symbol, name, and type
- [ ] Unit tests for tag filter normalization and validation
- [ ] Unit tests for config loading with valid and invalid tag filters
