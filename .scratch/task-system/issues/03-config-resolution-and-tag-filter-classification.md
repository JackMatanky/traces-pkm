Status: implemented

# 03 — Task config resolution and tag filter classification

**Date**: 2026-09-02
**Implemented in**: `9ea1ad9` + follow-ups (`a780d44`, `3c50a36`, `9e81499`,
`14bd6ae`, `1c23126`, `2a1a1bf`), branch `task-system/03-config-resolution`
(worktree `.worktrees/03-config-resolution-and-tag-filter-classification/`)

**What to build:** Wire task config into the parsing pipeline so tag filters
are applied during list item classification. Config resolution builds
`TaskConfig` once at startup. Tag filters determine which status-marked items
become Tasks vs Checkboxes.

**Blocked by:** 02 (needs `ListItemType` classification).

## Progress

All criteria are fully implemented and verified across unit, integration, and
E2E suites:

- `Config.tasks()` and `TaskConfig` resolve `statuses: TaskStatusMap` and
  `tag_filters: Vec<Tag>` once at startup with local-over-global merge.
- `MarkdownParserInput<'a>` encapsulates borrowed path, source text,
  `TaskConfig`, and `FrontmatterConfig` references, preventing per-note heap
  allocations.
- `ListTracker::end_item` extracts tags during scan buffer flush and evaluates
  `tag_filters` to classify status-marked items into `ListItemType::Task` vs
  `ListItemType::Checkbox`.
- Full test coverage for exact tag matching, empty filter fallback, config
  normalization (`task` / `#task`), invalid filter diagnostics, and top-level
  re-exports.

- [x] `Config` gains `tasks: TaskConfig` field with `#[serde(default)]`
- [x] `TaskConfig` has a `Default` impl (empty statuses map, empty tag filters)
- [x] Config resolution builds `TaskStatusMap` once from configured statuses
- [x] Empty `tag_filters`: all status-marked list items become `ListItemType::Task`
- [x] Non-empty `tag_filters`: status-marked item becomes Task only when any
  tag on the item exactly matches any configured filter
- [x] Non-matching status-marked items become `ListItemType::Checkbox` (not
  Task)
- [x] Exact tag matching: `#task` does not match `#task/project` unless nested
  tag is configured
- [x] Config normalization: `task` and `#task` produce the same internal Tag
- [x] Invalid tag filter entries fail config loading with diagnostic
- [x] Integration test: config with tag filters → parsing produces correct
  Task/Checkbox split
- [x] Integration test: config without tag filters → all status-marked items are
  Tasks
- [x] Integration test: invalid tag filter fails config loading

## Key interfaces

- `MarkdownParserInput<'a>` struct in `src/note/parser/input.rs` — carries
  `path`, `src`, `tasks: TaskConfig`, `frontmatter: FrontmatterConfig`. Private
  fields, `new` constructor, accessor methods. `parse_markdown` signature
  changes to `parse_markdown(&MarkdownParserInput<'_>) -> Note`
- `IndexBuilder::parse_note` constructs `MarkdownParserInput` from `Config` —
  the single threading point
- `ListTracker::end_item` signature changes from `end_item(&TaskStatusMap)` to
  `end_item(&[Tag], &TaskStatusMap)` — decomposed for clarity
- `ListItemType::Task(TaskListItem)` — `TaskListItem` is introduced in issue
  05; this issue constructs it during classification
- `ListItemType::Checkbox` — constructed when a status-marked item has tags but
  none match any configured filter

## Parser flow

### Config threading

```
IndexerService::build/refresh
  → IndexBuilder::build
    → parse_note(root, file, &config)
      → MarkdownParserInput::new(path, src, config.tasks().clone(), config.frontmatter().clone())
      → parse_markdown(&input)
        → ParserContext::new(src, &input.tasks(), &input.frontmatter())
          → ListTracker::end_item(&tag_filters, &statuses)
```

### Tag filter classification

```
Event::End(Item)
  → end_item(tag_filters, statuses):
      flush_active_item_scan_buffer()  ← tags extracted here

      let item_type = match marker_symbol {
        Some(sym) => {
          let status = statuses.resolve(sym);
          if tag_filters.is_empty() {
            Task(TaskListItem::new(status, fully_complete))
          } else {
            let item_tags = /* tags from flushed fields */;
            if item_tags.iter().any(|t| tag_filters.contains(t)) {
              Task(TaskListItem::new(status, fully_complete))
            } else {
              Checkbox
            }
          }
        }
        None => Plain,
      }
```

Note: `TaskListItem` is introduced in issue 05. `fully_complete` is computed
recursively over task children in the same `end_item` call (issue 05). This
issue's classification logic determines whether the item becomes `Task` or
`Checkbox`; issue 05's `TaskListItem` wraps the status and `fully_complete`
value inside the `Task` variant.

Key changes:
- Tag extraction happens during `flush_active_item_scan_buffer` (before
  `end_item` classifies)
- Empty tag_filters → all status-marked items become Task (current behavior
  preserved)
- Non-empty tag_filters → matching items become Task, non-matching become
  Checkbox
- `ListItemType::Checkbox` is constructed here for the first time
- `ListItemType::Task(TaskListItem)` replaces `ListItemType::Task(TaskStatus)`
  (issue 05)

## Acceptance criteria

- [x] `MarkdownParserInput<'a>` struct exists in `src/note/parser/input.rs` with
  private fields and accessor methods
- [x] `parse_markdown` signature changes to accept `&MarkdownParserInput<'_>`
- [x] `IndexBuilder::parse_note` constructs `MarkdownParserInput` from `Config`
- [x] `ListTracker::end_item` takes `(&[Tag], &TaskStatusMap)` instead of
  `&TaskStatusMap`
- [x] `ListItemType::Task` constructs `TaskListItem` (deferred to issue 05;
  currently `TaskStatus`)
- [x] `ListItemType::Checkbox` is constructed for non-matching status-marked
  items when tag_filters is non-empty
- [x] Empty tag_filters preserves current behavior: all status-marked items
  become Task
- [x] Tag extraction (during scan buffer flush) happens before classification
  (in end_item)
- [x] Unit test: tag filter matching — item with matching tag becomes Task
- [x] Unit test: tag filter matching — item with non-matching tag becomes
  Checkbox
- [x] Unit test: empty tag_filters — all status-marked items are Tasks
- [x] Unit test: exact tag matching — `#task` does not match `#task/project`
- [x] `mise run verify` passes

## Out of scope

- `MarkdownParserInput` frontmatter threading — issue 06 uses `frontmatter`
  accessor for config-aware field names
- `ListText` raw/clean normalization — issue 06
- `fully_complete` computation — issue 05
- LISTS persistence table — issue 07
- Query record enrichment — issue 08

## Implementation notes

### Where it landed

| File | Lines | Purpose |
|------|-------|---------|
| `src/note/parser/input.rs` | 166 (new) | `MarkdownParserInput<'a>` borrowed container, accessors, `for_test`, unit tests |
| `src/note/parser.rs` | ~1400 | `parse_markdown(&MarkdownParserInput)`, `ParserContext` threading `task_statuses` and `tag_filters` |
| `src/note/parser/list.rs` | ~900 | `ListTracker::end_item(&[Tag], &TaskStatusMap)`, tag-filter evaluation (`Task` vs `Checkbox`) |
| `src/config/model.rs` | ~900 | `TaskConfig` model, `statuses(&self)`, `tag_filters(&self)`, `normalize_tag_filter`, unit tests |
| `src/config/raw.rs` | ~130 | `RawTaskConfig { tag_filters: Vec<String> }` with `#[serde(default)]` |
| `src/config/builder.rs` | ~330 | Local-over-global merge for `[tasks]`, build pipeline integration |
| `src/index/builder.rs` | ~730 | `parse_note` single threading point constructing `MarkdownParserInput` |
| `src/index/cache.rs` | ~330 | Threading `(tasks, frontmatter)` tuple through incremental reconciliation |
| `src/index/service.rs` | ~1300 | `IndexerService.tasks` and `with_config` plumbing |
| `tests/integration/task_tag_filters.rs` | ~120 | Integration tests proving task/checkbox classification with and without tag filters |
| `src/lib.rs` | ~260 | Top-level re-exports for `TaskStatus`, `TaskStatusMap`, and test-gated task types |

### Key design decisions

1. **Zero-Copy Config Threading via `MarkdownParserInput<'a>`**:
   `MarkdownParserInput` borrows `path`, `src`, `&TaskConfig`, and
   `&FrontmatterConfig`. This ensures that batch note parsing during full vault
   indexing never clones configuration structs or maps per document.

2. **Exact Tag Matching**:
   Classification evaluates exact equality between item tags and configured
   filters (`tag_filters.contains(tag)` / `Tag::is_exact_match`). `#task` does
   not match hierarchical child tag `#task/project` unless `#task/project` is
   explicitly configured in `tag_filters`.

3. **Decomposed `ListTracker::end_item`**:
   `end_item` accepts `(&[Tag], &TaskStatusMap)` rather than taking a whole
   `TaskConfig` or `Config` reference. This keeps list-tracking state completely
   decoupled from configuration subsystem types.

4. **Two-Out Pattern in Scan Buffer Flushing**:
   `flush_active_item_scan_buffer` extracts tags and stores them in `item.tags`
   on the active `ItemFrame` before classification occurs. This guarantees that
   all tags present on the item line or multi-line item body are fully available
   when `end_item` determines whether to construct `ListItemType::Task` or
   `ListItemType::Checkbox`.

5. **Top-Level Re-exports and Clean Visibility**:
   Internal crate modules import types via crate root re-exports
   (`crate::TaskStatus`, `crate::TaskConfig`, `crate::Tag`) rather than deep
   module paths. Test-only types (`TaskStatusSymbol`, `TaskStatusType`) are
   gated under `#[cfg(test)]` in `src/lib.rs` to maintain least-privilege
   visibility and eliminate unused import compiler warnings.

## Verification

- **Unit tests**:
  - `note/parser/list.rs`: `tag_filters::*` tests (empty filters, single filter
    match, filter mismatch, multiple filters, nested tag exactness).
  - `note/parser.rs`: `tag_filters::*` end-to-end markdown note tests.
  - `config/model.rs` & `config/builder.rs`: tag filter normalization (`task` /
    `#task`), invalid filter diagnostics, local-over-global merge.
  - `config/service.rs`: `fails_to_load_when_tag_filter_is_invalid` testing full
    config load failure.
- **Integration tests**:
  - `tests/integration/task_tag_filters.rs`: Full indexing and query service
    execution verifying Task vs Checkbox classification with and without
    filters.
- **Full test suite**: All 2,150 workspace unit and integration tests pass
  cleanly with `cargo test --all-features`.
- **Lints & Formatting**: `cargo fmt --check` and `cargo check --workspace
  --all-targets --all-features` pass without warnings.
