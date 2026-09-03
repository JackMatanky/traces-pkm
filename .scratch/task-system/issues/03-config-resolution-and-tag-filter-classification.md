Status: implemented

# 03 — Task config resolution and tag filter classification

**What to build:** Wire task config into the parsing pipeline so tag filters are applied during list item classification. Config resolution builds `TaskConfig` once at startup. Tag filters determine which status-marked items become Tasks vs Checkboxes.

**Blocked by:** 02 (needs `ListItemType` classification).

## Progress

Config plumbing is complete: `Config.tasks`, `TaskConfig` default, `TaskStatusMap`
resolution, `task`/`#task` normalization, invalid filter diagnostics, and
local-over-global merge all work. The parser does not use any of this yet —
`parse_markdown` accepts `(path, src)` with no config, and `end_item` classifies
every marker hit as `Task` without consulting tag filters.

- [x] `Config` gains `tasks: TaskConfig` field with `#[serde(default)]`
- [x] `TaskConfig` has a `Default` impl (empty statuses map, empty tag filters)
- [x] Config resolution builds `TaskStatusMap` once from configured statuses
- [x] Empty `tag_filters`: all status-marked list items become `ListItemType::Task`
- [x] Non-empty `tag_filters`: status-marked item becomes Task only when any tag on the item exactly matches any configured filter
- [x] Non-matching status-marked items become `ListItemType::Checkbox` (not Task)
- [x] Exact tag matching: `#task` does not match `#task/project` unless nested tag is configured
- [x] Config normalization: `task` and `#task` produce the same internal Tag
- [x] Invalid tag filter entries fail config loading with diagnostic
- [x] Integration test: config with tag filters → parsing produces correct Task/Checkbox split
- [x] Integration test: config without tag filters → all status-marked items are Tasks
- [x] Integration test: invalid tag filter fails config loading

## Key interfaces

- `MarkdownParserInput<'a>` struct in `src/note/parser/input.rs` — carries `path`, `src`, `tasks: TaskConfig`, `frontmatter: FrontmatterConfig`. Private fields, `new` constructor, accessor methods. `parse_markdown` signature changes to `parse_markdown(&MarkdownParserInput<'_>) -> Note`
- `IndexBuilder::parse_note` constructs `MarkdownParserInput` from `Config` — the single threading point
- `ListTracker::end_item` signature changes from `end_item(&TaskStatusMap)` to `end_item(&[Tag], &TaskStatusMap)` — decomposed for clarity
- `ListItemType::Task(TaskListItem)` — `TaskListItem` is introduced in issue 05; this issue constructs it during classification
- `ListItemType::Checkbox` — constructed when a status-marked item has tags but none match any configured filter

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
- Tag extraction happens during `flush_active_item_scan_buffer` (before `end_item` classifies)
- Empty tag_filters → all status-marked items become Task (current behavior preserved)
- Non-empty tag_filters → matching items become Task, non-matching become Checkbox
- `ListItemType::Checkbox` is constructed here for the first time
- `ListItemType::Task(TaskListItem)` replaces `ListItemType::Task(TaskStatus)` (issue 05)

## Acceptance criteria

- [x] `MarkdownParserInput<'a>` struct exists in `src/note/parser/input.rs` with private fields and accessor methods
- [x] `parse_markdown` signature changes to accept `&MarkdownParserInput<'_>`
- [x] `IndexBuilder::parse_note` constructs `MarkdownParserInput` from `Config`
- [x] `ListTracker::end_item` takes `(&[Tag], &TaskStatusMap)` instead of `&TaskStatusMap`
- [x] `ListItemType::Task` constructs `TaskListItem` (deferred to issue 05; currently `TaskStatus`)
- [x] `ListItemType::Checkbox` is constructed for non-matching status-marked items when tag_filters is non-empty
- [x] Empty tag_filters preserves current behavior: all status-marked items become Task
- [x] Tag extraction (during scan buffer flush) happens before classification (in end_item)
- [x] Unit test: tag filter matching — item with matching tag becomes Task
- [x] Unit test: tag filter matching — item with non-matching tag becomes Checkbox
- [x] Unit test: empty tag_filters — all status-marked items are Tasks
- [x] Unit test: exact tag matching — `#task` does not match `#task/project`
- [x] `mise run verify` passes

## Out of scope

- `MarkdownParserInput` frontmatter threading — issue 06 uses `frontmatter` accessor for config-aware field names
- `ListText` raw/clean normalization — issue 06
- `fully_complete` computation — issue 05
- LISTS persistence table — issue 07
- Query record enrichment — issue 08
