Status: ready-for-agent

# Task System Spec

## Problem Statement

Users need Traces to treat markdown tasks as first-class, queryable Note Metadata instead of as simple checked/unchecked list rows. The current task support only exposes task text and a boolean completion field. That is not enough for a personal knowledge workflow that relies on task status, priority, due dates, task tags, parent-child completion, CLI filtering, and template queries.

Users also need task classification to be predictable. A plain checklist item should not always mean the same thing as a task when a task tag filter is configured, and custom task markers such as `[/]`, `[-]`, `[!]`, and unknown single-character markers should not disappear or be downgraded to plain text.

## Solution

Add a richer Task model built from markdown list items during Note parsing. A List Item remains the primary structural type, and each item is classified as a plain bullet, a non-task checkbox, or a Task. Task classification is driven by a custom item-leading marker scanner and optional configured task tag filters.

Tasks become queryable through the FileIndex, CLI query commands, and the template `tasks` namespace. Users can filter and sort by task fields such as status, completion, priority, dates, tags, line, parent, and fully-complete state. The CLI task command gains sorting and table output. The existing `tasks.*` template namespace continues to work, but exposes richer task fields.

## User Stories

1. As a User, I want markdown checklist items to be parsed as tasks, so that I can query work directly from my Notes.
2. As a User, I want custom status markers like `[/]` and `[-]` to be recognized, so that my existing markdown task style works in Traces.
3. As a User, I want `[ ]`, `[x]`, and `[X]` to keep working, so that standard markdown task lists remain supported.
4. As a User, I want unknown single-character markers like `[?]` to be preserved, so that unusual task states are not silently lost.
5. As a User, I want unknown task markers to behave as incomplete todos by default, so that they remain visible in active task queries.
6. As a User, I want task status to be exposed as a named field, so that I can filter and sort tasks by workflow state.
7. As a User, I want task completion to remain available as a derived field, so that simple done/not-done queries stay easy.
8. As a User, I want cancelled tasks excluded from both active and completed views, so that abandoned work does not clutter either side of my workflow.
9. As a User, I want in-progress and on-hold tasks to count as incomplete, so that active work remains visible.
10. As a User, I want task priority emojis to be parsed, so that priority can be queried without rewriting my Notes.
11. As a User, I want missing priority to remain absent, so that Traces does not invent a priority I did not write.
12. As a User, I want task dates from emoji syntax to be parsed, so that due, scheduled, start, created, done, and cancelled dates are queryable.
13. As a User, I want task dates from inline field syntax to keep working, so that Dataview-compatible fields remain useful.
14. As a User, I want parseable task dates to become date values, so that date comparisons work in queries.
15. As a User, I want missing task dates to resolve to null, so that filters can distinguish missing from present values.
16. As a User, I want task tags to be available on task rows, so that I can query by task-level tags.
17. As a User, I want note-level tags to remain available on task rows, so that task queries retain their parent Note context.
18. As a User, I want task tag filters in config, so that only intentionally marked checklist items become Tasks.
19. As a User, I want multiple task tag filters, so that more than one tag can classify an item as a Task.
20. As a User, I want task tag filter config to accept `task` and `#task`, so that config is convenient and markdown-like.
21. As a User, I want invalid task tag filters to fail config loading, so that bad task classification does not happen silently.
22. As a User, I want exact task tag matching, so that `#task` does not unexpectedly match `#task/project`.
23. As a User, I want any tag on a list item to satisfy a task tag filter, so that tag order does not affect task classification.
24. As a User, I want all status-marked items to become Tasks when no tag filter is configured, so that Traces behaves like a normal task-list parser by default.
25. As a User, I want status-marked items that miss the tag filter to remain checkboxes, so that non-task checklists are still represented without polluting task queries.
26. As a User, I want plain bullet items to remain distinct from checkboxes and Tasks, so that list structure stays accurate.
27. As a User, I want task marker prefixes removed from displayed task text, so that query output is clean.
28. As a User, I want raw list text for diagnostics and source-like display, so that I can still see what was written in the Note.
29. As a User, I want clean list text for display and filtering, so that task-only syntax does not clutter output.
30. As a User, I want clean text to strip task markers, configured task tag filters, task date syntax, priority emojis, and inline task fields, so that task names are readable.
31. As a User, I want raw text to exclude only the leading task marker prefix, so that raw text remains source-like without duplicating classification syntax.
32. As a User, I want task line numbers, so that query results can point back to the source Note.
33. As a User, I want task parent line numbers, so that subtasks can be related back to their parent item.
34. As a User, I want task depth, so that nested task structure can be reconstructed.
35. As a User, I want fully-complete status, so that a parent task only counts as fully complete when its task subtree is resolved (all descendant tasks are done or cancelled).
36. As a User, I want non-task child list items ignored for fully-complete calculation, so that supporting bullets and checkboxes do not block task completion.
37. As a User, I want task queries to inherit parent Note metadata, so that task rows can be filtered by Note fields and frontmatter.
38. As a User, I want task item inline fields to override inherited Note metadata, so that local task metadata wins where it is written.
39. As a User, I want `Note.tasks()` to return only filtered Tasks, so that application code does not have to reapply classification rules.
40. As a User, I want `Note.list_items()` to expose all list items, so that advanced code can inspect plain bullets and non-task checkboxes.
41. As a User, I want task rows persisted in the FileIndex, so that repeated queries do not reparse every Note unnecessarily.
42. As a User, I want list rows persisted separately from Note Metadata, so that list/task hierarchy can be queried directly.
43. As a User, I want `traces task` to support `--sort`, so that I can order tasks by due date, priority, status, or file fields.
44. As a User, I want `traces task` to support descending order, so that I can reverse any sort.
45. As a User, I want `traces task --table` to render configurable columns, so that task output can be scanned like other table queries.
46. As a User, I want default task table columns, so that table output is useful without configuration.
47. As a User, I want task table columns to include text, status, due date, and file name by default, so that the most important task context is visible.
48. As a User, I want `--from` to accept tags, folders, File Classes, and specific markdown files across query commands, so that task and Note queries use the same source model.
49. As a User, I want File Class sources to use transitive is-a matching, so that subclassed Notes appear in parent class queries.
50. As a template author, I want `tasks.all()` to return task-level rows, so that templates can render task views.
51. As a template author, I want `tasks.from_tags()` to filter by Note tags, so that task views can start from tagged Notes.
52. As a template author, I want `tasks.from_folder()` to filter by folder, so that task views can be scoped to a project area.
53. As a template author, I want `tasks.from_class()` to filter by File Class, so that task views can follow the Schema model.
54. As a template author, I want task queries to support `where`, `sort`, `limit`, `group_by`, and `flatten`, so that task pipelines behave like existing Pipeline Queries.
55. As a template author, I want terminal renderers like `task_list`, `table`, `list`, and `count` to work for tasks, so that templates can present task results naturally.
56. As a User, I want the old `task.completed` and `task.text` fields to remain available during the pre-release evolution, so that existing local templates are easy to adapt.
57. As a User, I want richer task fields under `task.*`, so that query expressions are readable and predictable.
58. As a maintainer, I want task status configuration built once at startup, so that parsing does not repeatedly rebuild lookup maps.
59. As a maintainer, I want status lookup by symbol, name, and type, so that parsing, display, and query code do not each invent their own mapping.
60. As a maintainer, I want status-name lookup to be normalized, so that config display names are user-friendly without creating case bugs.
61. As a maintainer, I want Tags to be a shared domain type, so that config, Note parsing, and indexing use the same validation and equality rules.
62. As a maintainer, I want task parsing not to depend on pulldown-cmark task-list events, so that standard and custom markers follow one code path.
63. As a maintainer, I want byte offsets converted to source lines through a small tracker, so that line tracking stays local and simple.
64. As a maintainer, I want the parser to classify list item kind during construction, so that indexing and querying consume already-meaningful data.
65. As a maintainer, I want the List Item model to avoid booleans like `is_task` and `is_checked`, so that invalid combinations are unrepresentable.
66. As a maintainer, I want a single enum for plain, checkbox, and task items, so that downstream code can pattern-match safely.
67. As a maintainer, I want a list persistence record with flat accessor methods, so that the LISTS table is easy to query without deserializing nested Note structures at each call site.
68. As a maintainer, I want the Note parser config to compose task and frontmatter config, so that parsing receives all needed config without global state.
69. As a maintainer, I want recurrence, dependencies, and mutation operations deferred, so that the first task-system implementation stays small.

## Implementation Decisions

- Add an explicit Task term to the domain model. A Task is a status-marked markdown List Item that either matches configured task tag filters or, when no filters are configured, any status-marked list item.
- Keep List Item as the primary structural type. A Task is not a separate tree; task-specific data is composed into a List Item.
- Replace the current checked/unchecked-only task model with a List Item kind enum: plain bullet, checkbox, or Task.
- Plain list items carry no task data.
- Checkbox list items carry only derived completion state and do not appear in `Note.tasks()`.
- This is a deliberate simplification. Non-task checkboxes lose their status symbol identity (`[/]`, `[-]`, `[!]`) and retain only completion state. Non-task checkboxes are display-only; their status symbol is not needed for query or classification. If future features need non-task status identity, the `Checkbox` variant can be extended to carry `TaskStatusSymbol` without breaking the enum.
- Task list items carry status, optional priority, and task dates.
- `Note.tasks()` is filtered by construction and returns only List Items whose kind is Task.
- `Note.list_items()` is the unfiltered escape hatch and should expose all parsed List Items.
- `Note.list_items()` returns an iterator over the nested list hierarchy, not a slice. Building a flattened `Vec<ListItem>` on the Note would duplicate data that the LISTS table already persists. An iterator avoids this duplication while providing the same access. The return type is `ListItemIter<'_>` (or equivalent), not `&[ListItem]`.
- Drop pulldown-cmark task-list parsing for task classification. Do not enable the task-list extension for this feature. This is a refactoring step: the existing `set_task_status` method (which derives task status from `Event::TaskListMarker(bool)`) and the `ItemFrame.task_status` derivation path are both replaced by the custom marker scanner. All existing task classification tests must be rewritten against the scanner.
- Implement one custom marker scanner over item-leading text. It recognizes `[` followed by any single non-`]` character, followed by `]`, followed by whitespace.
- The custom scanner is the only source of truth for task marker identity. It handles `[ ]`, `[x]`, `[X]`, configured custom markers, and unknown single-character markers.
- `[x]` and `[X]` are equivalent; both map to the Done status. The custom scanner accepts any single non-`]` character; `TaskStatusMap` maps both `'x'` and `'X'` to Done.
- The custom scanner trims the leading marker prefix exactly once and only when it appears at the item-leading marker position.
- Preserve unknown marker symbols for diagnostics and fallback behavior.
- Unknown markers are never downgraded to plain bullets.
- Unknown markers resolve as incomplete todo statuses unless a configured status overrides them.
- Introduce a task status model with symbol, display name, and status type.
- Default statuses are always available and user configuration may add or override statuses.
- Default status types are todo, in-progress, on-hold, done, cancelled, and non-task.
- Completion is a tri-state derived from status type. Cancelled is a terminal state outside the complete/incomplete binary.

| StatusType       | `completed`     |
| ---------------- | --------------- |
| Done             | `Some(true)`      |
| Cancelled        | `None`            |
| Todo             | `Some(false)`     |
| InProgress       | `Some(false)`     |
| OnHold           | `Some(false)`     |
| NonTask          | `Some(false)`     |
| Unknown fallback | `Some(false)`     |
- Keep `task.completed` as a derived `Option<bool>` field. `Some(true)` for done, `Some(false)` for incomplete, `None` for cancelled. Query filters on `completed == false` exclude cancelled tasks; filters on `completed == true` also exclude them.
- Add `task.status` as the status name/canonical status field used for filtering, sorting, and display.
- Build status lookup maps once when config is resolved. Lookup maps include by-symbol, by-name, and by-type.
- Normalize status-name lookup by case-folding, leading/trailing whitespace trimming, and internal whitespace collapsing to a single space. Display names remain exactly as configured.
- Add task priority as a fixed enum: lowest, low, normal, medium, high, highest.
- Store priority as optional. Missing priority remains absent and does not default to normal.
- Parse priority emojis into the priority enum. Do not store the raw emoji as model data.
- Add task dates for created, scheduled, start, due, done, and cancelled.
- Support both emoji date syntax and existing inline field syntax for task dates.
- Parse valid `YYYY-MM-DD` task dates as date field values. Missing dates are null in query results.
- Do not implement recurrence, dependency IDs, on-completion behavior, or urgency scoring in this spec.
- Add a task config section with `tag_filters` and `statuses`. The resolved config shape:

  ```rust
  pub struct TaskConfig {
      pub statuses: TaskStatusMap,
      pub tag_filters: Vec<Tag>,
  }
  ```

  `TaskStatusMap` is built once at config resolution and provides lookup by symbol, by name, and by type. `tag_filters` is empty when no filter is configured.
- `tag_filters` is an array only. Do not add single-filter sugar.
- Empty `tag_filters` means every status-marked list item is a Task.
- Non-empty `tag_filters` means a status-marked item becomes a Task only when any tag on the item exactly matches any configured filter.
- Exact tag matching is required for task classification. Nested tags do not match unless explicitly configured.
- Config entries may include or omit the leading `#`. Config parsing normalizes entries before constructing Tags.
- Invalid tag filter entries fail config loading with a diagnostic that identifies the offending entry and config location.
- Move Tag into a shared domain type used by config, Note parsing, indexing, and query code.
- Add Tag validation and a small exact-match helper. Do not add a matching policy system.
- Keep task tags and Note tags available on task query rows.
- Task item fields inherit from Note frontmatter and Note inline fields. Item fields win over inherited Note fields.
- Add line, parent line, and depth to list items.
- Use byte offsets from markdown parsing only for source position tracking, not task marker identity.
- Add a small byte-to-line tracker that precomputes line starts and converts byte offsets to source lines.
- Compute `fully_complete` on Task List Items by checking the task itself and recursively checking task children only. A parent task is `fully_complete` when it and every descendant task (at any depth) has a complete or cancelled status — the subtree is resolved.
- Plain bullet children and non-task checkbox children are ignored for `fully_complete` calculation. Only `ListItemType::Task` descendants participate.
- Store `fully_complete` as a flattened indexed field for O(1) query access.
- Add a separate LISTS persistence table for list records.
- `ListRecord` wraps a project-relative `path` and the parsed `ListItem`. It exposes accessor methods for query-relevant fields (`status_type`, `priority`, `due_date`, `is_fully_complete`, etc.) that delegate into the `ListItemType` discriminant. This keeps the persistence shape composable — adding a field to `ListItem` or `TaskListItem` does not require updating `ListRecord`'s struct layout — while providing flat query access through methods.
- `ListRecord` serializes via postcard as `path` + `ListItem` (which contains `ListItemType`). The LISTS table stores the postcard-encoded `ListRecord` keyed by `(path, line)`. Query code deserializes the record once, then calls accessor methods for flat field access.
- `ListText.raw` is source text minus the leading `[<char>] ` marker prefix only. All other inline syntax (task tags, date syntax, priority emojis, inline fields) is preserved in `raw`. `raw` is not byte-exact source text — it is source-like text after parser-level marker handling.
- `ListText.clean` is normalized display/query text. It strips the task marker, configured task tag filters, task date syntax, priority emojis, and inline task fields.
- Query records for tasks retain parent Note metadata plus task fields.
- Existing task terminal rendering remains markdown task-list oriented by default.
- Add table output to the task CLI using a `--table` flag and a column array.
- Default task table columns are task text, task status, task due date, and file name.
- Add sorting and descending order to the task CLI, consistent with existing list and table query commands.
- Expand CLI `--from` source parsing across list, table, and task commands to accept tag, folder, File Class, and specific markdown file sources.
- File Class sources use transitive is-a matching consistent with Schema Extends.
- The existing template pipeline (`where`/`filter`, `sort`, `limit`, `group_by`, `flatten`, `table`, `list`, `task_list`, `count`) already works for task rows. This spec adds richer field paths (`task.status`, `task.priority`, `task.due`, `task.tags`, `task.parent`, `task.fully_complete`, `task.line`, `task.depth`) to the task row shape, not new pipeline mechanics.
- Do not add a ListView abstraction in the first implementation. Add it only if the implementation proves real duplication.
- Do not split the FileIndex as part of this work.

## Testing Decisions

- Prefer one high-level integration seam over many small tests: build a temporary project with Notes and config, index it, then assert CLI and template task query behavior.
- Good tests should verify external behavior: parsed task rows, query fields, CLI output, template output, and persisted index behavior. They should not assert parser frame internals or private lookup maps.
- Reuse existing integration and e2e test patterns for FileIndex query behavior, config lifecycle, template rendering, and CLI command behavior.
- Add a focused unit seam for the custom marker scanner because it is a pure boundary with many edge cases.
- Add a focused unit seam for task tag filter normalization because invalid config must fail early and clearly.
- Test that `[ ]`, `[x]`, `[X]`, `[/]`, `[-]`, `[!]`, and `[?]` are classified through the same marker path.
- Test that marker scanning only accepts an item-leading marker followed by whitespace.
- Test that later bracket text is not trimmed as a task marker.
- Test that unknown markers are preserved and behave as incomplete todos.
- Test that no configured tag filters makes all status-marked list items Tasks.
- Test that configured tag filters make matching status-marked items Tasks and non-matching status-marked items checkboxes.
- Test exact tag matching, including `#task` not matching `#task/project` unless that nested tag is configured.
- Test config normalization for `task` and `#task` producing the same internal Tag.
- Test invalid `tag_filters` entries fail config loading.
- Test task date and priority extraction from source markdown into query fields.
- Test missing priority remains null/absent.
- Test completion mapping for done (`Some(true)`), cancelled (`None`), and incomplete statuses (`Some(false)`).
- Test `fully_complete` with nested task children (all done or cancelled → true, any incomplete → false) and verify that plain bullet children and non-task checkbox children are excluded from the calculation.
- Test `Note.tasks()` returns an iterator yielding only `ListItemType::Task` items, excluding plain bullets and non-task checkboxes.
- Test `Note.list_items()` exposes all list item kinds.
- Test task query rows inherit parent Note metadata and allow item fields to override inherited fields.
- Test `traces task --sort` and descending order with at least due date or priority.
- Test `traces task --table` default columns and custom column arrays.
- Test `--from` accepts specific markdown file and File Class sources for task queries. The `--from` flag routes through `SourceSelector::parse`, which already handles tags (`#tag`), folder globs, File Classes (`@Class*`), and file paths (detected by extension). No new source parsing is needed; the CLI task command uses the same `parse_source` path as `list` and `table`.
- Test template `tasks.from_class()` uses transitive File Class matching.
- Test index persistence roundtrip includes LISTS-derived task fields without requiring a reparse.

## Out of Scope

- Mutating task status from the CLI.
- Status cycling and next-status transitions.
- Editing Notes to complete, cancel, or reschedule tasks.
- Recurrence rules.
- Task dependencies and dependency IDs.
- On-completion behaviors.
- Urgency scoring.
- Nested tag matching for task classification.
- Arbitrary string filters for task classification.
- A ListView abstraction.
- Splitting or redesigning the FileIndex beyond the LISTS table needed for this feature.
- Backward compatibility with shipped external consumers, because the project has not shipped yet.

## Further Notes

- This spec follows the accepted redb/FileIndex and QueryOps architecture from the existing ADRs.
- The design deliberately keeps one parser classification path instead of combining pulldown-cmark task events with custom parsing.
- The task system should update the project domain glossary with explicit Task, List Item, Checkbox, Task Status, Task Priority, Task Dates, and Fully Complete terms.
- `Note.list_items()` returns an iterator, not a slice. Building a flattened `Vec<ListItem>` on the Note would duplicate data already persisted in the LISTS table. The iterator walks the nested list hierarchy on demand.
