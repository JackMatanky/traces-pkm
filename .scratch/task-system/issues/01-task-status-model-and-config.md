Status: ready-for-agent

# 01 — Task status model and config

**What to build:** The foundational data model for task statuses and configuration. New types live in `src/task.rs`. A `TaskStatusType` enum (todo, in-progress, on-hold, done, cancelled, non-task), a `TaskStatusSymbol` newtype (char), a `TaskStatus` struct (symbol + name + kind), and a `TaskStatusMap` with lookup by symbol, name, and type. Config gains a `[tasks]` section with `tag_filters: Vec<Tag>` and `statuses: TaskStatusMap`. `Tag::is_exact_match` added to `src/tag.rs`. List items gain `depth`, `line`, and `parent_line` position fields populated by the parser from byte offsets.

**Blocked by:** None — can start immediately.

## Clarifications

- **No `ListItemType` restructuring.** The existing `ListItem` keeps its current `task_status: Option<TaskStatus>` field. The `ListItemType` enum (Plain/Checkbox/Task) is issue 02.
- **No rename of existing `TaskStatus` enum.** The current `Incomplete`/`Complete` enum stays as-is. It is a DTO for pulldown-cmark task status markers and will be replaced by `ListItemType` in issue 02.
- **Tag already exists at `src/tag.rs`.** No module move needed. Just add `is_exact_match`.
- **New types in `src/task.rs`.** Not in `lists.rs` — keeps task status domain separate from list structure.

## Checklist

- [ ] `TaskStatusType` enum with variants: todo, in-progress, on-hold, done, cancelled, non-task
- [ ] `TaskStatusType::completed()` returns tri-state: `Some(true)` for done, `Some(false)` for incomplete, `None` for cancelled
- [ ] `TaskStatusSymbol` newtype wrapping `char` — the marker character inside `[<char>]`
- [ ] `TaskStatus` struct with fields: `symbol: TaskStatusSymbol`, `name: String`, `kind: TaskStatusType`
- [ ] `TaskStatusMap` built once at config resolution with lookup by symbol, by name, and by type
- [ ] Status-name lookup normalized by case-folding, leading/trailing whitespace trimming, and internal whitespace collapsing to a single space
- [ ] Default statuses always available; user config may add or override
- [ ] `Tag::is_exact_match(&Tag)` helper on `src/tag.rs` — exact equality on normalized Tag values
- [ ] Config gains `[tasks]` section: `RawTaskConfig` with `tag_filters: Vec<String>`, resolved to `TaskConfig { statuses: TaskStatusMap, tag_filters: Vec<Tag> }`
- [ ] Config parsing accepts `task` and `#task` (leading `#` optional, normalized before constructing Tag)
- [ ] Empty `tag_filters` is valid — means no filter configured, all status-marked items become tasks
- [ ] Invalid `tag_filters` entries fail config loading with diagnostic identifying offending entry and config location
- [ ] `ListItem` gains `depth: usize`, `line: usize`, and `parent_line: Option<usize>` fields
- [ ] Parser populates depth, line, and parent_line from existing byte offsets during list item construction
- [ ] Unit tests for `TaskStatusMap` lookup by symbol, name, and type
- [ ] Unit tests for tag filter normalization and validation
- [ ] Unit tests for config loading with valid and invalid tag filters
- [ ] `cargo test` passes, `cargo clippy` clean
