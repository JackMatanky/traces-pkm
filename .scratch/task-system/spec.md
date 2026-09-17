# Task System Spec

**Status:** ready-for-agent

## Problem Statement

Users need Traces to treat markdown tasks and list items as first-class,
queryable Note Metadata instead of simple checked/unchecked list rows. The
current task support only exposes task text and a boolean completion field. That
is insufficient for a personal knowledge workflow that relies on custom status
markers (`[/]`, `[-]`, `[!]`), priorities, lifecycle dates, task tags,
parent-child completion, CLI filtering, and template queries.

Users also need task classification to be predictable. A plain checklist item
should not always mean the same thing as a task when a task tag filter is
configured, and custom task markers should never disappear, be flattened to
`"- [ ] "`, or be downgraded to plain text. Furthermore, queries and outline
views must support generic list items (outlines, checklists) alongside tasks
under a unified, high-performance querying and sorting model.

## Solution

Add a deep, high-performance Task and List Item model built from markdown list
items during Note parsing. List Item remains the primary structural type, and
each item is classified as a plain bullet, a non-task checkbox, or a Task. Task
classification is driven by a custom item-leading marker scanner and optional
configured task tag filters.

In accordance with ADR 0005, list items persist directly inside `Note` records
within the `NOTES` redb table as a contiguous `Box<[ListItem]>`, eliminating
write amplification and dropping the dead `LISTS` redb table. List items are
compacted in memory via sparse inline fields, lazy clean text, and compact
4-byte `DateValue` primitives.

List items and tasks become queryable through zero-allocation positional
`QueryRow` references (`RowKind::List { item_idx: u32 }`). The query grammar
exposes a canonical, unified `list.<field>` namespace with embedded `TaskField`
variants, rejecting obsolete `task.<field>` syntax with helpful diagnostic
suggestions. Templates gain dual `lists.from(...)` and `tasks.from(...)`
globals. The CLI `traces task` command gains status marker preservation, depth
indentation, clickable `({path}:{line})` coordinates, convenience filter
shortcuts (`--todo`, `--done`, `--status <char>`), sorting, and table output.

## User Stories

1. As a User, I want markdown checklist items to be parsed as tasks, so that I
   can query work directly from my Notes.
2. As a User, I want custom status markers like `[/]` and `[-]` to be
   recognized, so that my existing markdown task style works in Traces.
3. As a User, I want `[ ]`, `[x]`, and `[X]` to keep working, so that standard
   markdown task lists remain supported.
4. As a User, I want unknown single-character markers like `[?]` to be
   preserved, so that unusual task states are not silently lost.
5. As a User, I want unknown task markers to behave as incomplete todos by
   default, so that they remain visible in active task queries.
6. As a User, I want task status to be exposed as `list.status`, so that I can
   filter and sort tasks by workflow state.
7. As a User, I want task completion to remain available as `list.completed`, so
   that simple done/not-done queries stay easy.
8. As a User, I want cancelled tasks excluded from both active and completed
   views, so that abandoned work does not clutter either side of my workflow.
9. As a User, I want in-progress and on-hold tasks to count as incomplete, so
   that active work remains visible.
10. As a User, I want task priority emojis to be parsed into `list.priority`, so
    that priority can be queried without rewriting my Notes.
11. As a User, I want missing priority to remain absent, so that Traces does not
    invent a priority I did not write.
12. As a User, I want task dates from emoji syntax to be parsed, so that due,
    scheduled, start, created, done, and cancelled dates are queryable.
13. As a User, I want task dates from inline field syntax to keep working, so
    that Dataview-compatible fields remain useful.
14. As a User, I want parseable task dates to become date values, so that date
    comparisons work in queries.
15. As a User, I want missing task dates to resolve to null, so that filters can
    distinguish missing from present values.
16. As a User, I want task tags to be available via `list.tags`, so that I can
    query by task-level tags.
17. As a User, I want note-level tags to remain available on list rows via
    `file.tags`, so that queries retain their parent Note context.
18. As a User, I want task tag filters in config, so that only intentionally
    marked checklist items become Tasks.
19. As a User, I want multiple task tag filters, so that more than one tag can
    classify an item as a Task.
20. As a User, I want task tag filter config to accept `task` and `#task`, so
    that config is convenient and markdown-like.
21. As a User, I want invalid task tag filters to fail config loading, so that
    bad task classification does not happen silently.
22. As a User, I want exact task tag matching, so that `#task` does not
    unexpectedly match `#task/project`.
23. As a User, I want any tag on a list item to satisfy a task tag filter, so
    that tag order does not affect task classification.
24. As a User, I want all status-marked items to become Tasks when no tag filter
    is configured, so that Traces behaves like a normal task-list parser by
    default.
25. As a User, I want status-marked items that miss the tag filter to remain
    checkboxes, so that non-task checklists are still represented without
    polluting task queries.
26. As a User, I want plain bullet items to remain distinct from checkboxes and
    Tasks, so that list structure stays accurate.
27. As a User, I want task marker prefixes removed from displayed task text, so
    that query output is clean.
28. As a User, I want raw list text available via `list.raw_text` for
    diagnostics and source-like display.
29. As a User, I want clean list text available via `list.text` for display and
    filtering.
30. As a User, I want clean text to strip task markers, configured task tag
    filters, task date syntax, priority emojis, and inline task fields.
31. As a User, I want raw text to exclude only the leading task marker prefix,
    preserving all other inline syntax.
32. As a User, I want task line numbers via `list.line`, so that query results
    can point back to the source Note.
33. As a User, I want task parent line numbers via `list.parent`, so that
    subtasks can be related back to their parent item.
34. As a User, I want task depth via `list.depth`, so that nested task structure
    can be reconstructed.
35. As a User, I want fully-complete status via `list.fully_complete`, so that a
    parent task only counts as fully complete when its entire task subtree is
    resolved (all descendant tasks are done or cancelled).
36. As a User, I want non-task child list items ignored for fully-complete
    calculation, so that supporting bullets and checkboxes do not block task
    completion.
37. As a User, I want list queries to inherit parent Note metadata, so that list
    rows can be filtered by Note fields and frontmatter.
38. As a User, I want item-level inline fields to override inherited Note
    metadata, so that local task metadata wins where it is written.
39. As a User, I want `Note.tasks()` to return only filtered Tasks as a
    zero-allocation slice iterator.
40. As a User, I want `Note.list_items()` to expose all list items as a
    zero-allocation slice iterator.
41. As a User, I want list items persisted inside `Note` in the `NOTES` redb
    table per ADR 0005, so that index writes are fast and no separate `LISTS`
    table is required.
42. As a User, I want `traces task` to preserve custom status markers (`[/]`,
    `[-]`, `[!]`), so that terminal output matches source note status.
43. As a User, I want `traces task` to indent nested tasks by `list.depth * 2`
    spaces, so that outline hierarchy is visually preserved.
44. As a User, I want `traces task` to support `--line-numbers` (`-l`), so that
    output renders clickable `({path}:{line})` editor coordinates.
45. As a User, I want convenience filter shortcuts `--todo`, `--done`, and
    `--status <char>`, so that routine queries do not require typing verbose
    `--where` expressions.
46. As a User, I want `traces task` to support `--sort`, `--asc`, and `--desc`,
    so that I can order tasks by any list or file field.
47. As a User, I want `traces task --table` to render configurable columns, so
    that task output can be viewed in tabular form.
48. As a User, I want default task table columns (`Task`, `Status`, `Due`,
    `Priority`, `File`) when `--table` is passed without explicit columns.
49. As a User, I want `traces task --count` to print only the matching count,
    so that scripts and shell prompts can display active task totals easily.
50. As a User, I want `--from` to accept tags, folders, File Classes, and
    specific markdown files across query commands.
51. As a User, I want File Class sources to use transitive is-a matching, so
    that subclassed Notes appear in parent class queries.
52. As a template author, I want `tasks.from(...)` to return task-level rows, so
    that templates can render task views.
53. As a template author, I want `lists.from(...)` to return all list items, so
    that templates can render generic outlines and checklists.
54. As a template author, I want template queries to support `where`, `sort`,
    `limit`, `group_by`, and `flatten` over `list.<field>` paths.
55. As a template author, I want terminal renderers (`task_list`, `table`,
    `list`, `count`) to operate cleanly on list and task query sets.
56. As a User, I want a single canonical `list.<field>` query namespace without
    confusing `task.*` aliasing.
57. As a User, I want queries using obsolete `task.<field>` syntax to fail with
    a helpful error pointing to `list.<field>`.
58. As a maintainer, I want task status configuration built once at startup, so
    that parsing does not repeatedly rebuild lookup maps.
59. As a maintainer, I want status lookup by symbol, name, and type, so that
    parsing, display, and query code share a single mapping.
60. As a maintainer, I want status-name lookup to be normalized, so that
    configured display names are user-friendly without creating case bugs.
61. As a maintainer, I want Tags to be a shared domain type across config, Note
    parsing, and indexing.
62. As a maintainer, I want task parsing not to depend on pulldown-cmark
    task-list events, so that standard and custom markers follow one code path.
63. As a maintainer, I want byte offsets converted to source lines through a
    small tracker, so that line tracking stays local and simple.
64. As a maintainer, I want the parser to classify list item kind during
    construction.
65. As a maintainer, I want the List Item model to avoid booleans like `is_task`
    and `is_checked`, so that invalid combinations are unrepresentable.
66. As a maintainer, I want a single enum for plain, checkbox, and task items,
    so that downstream code can pattern-match safely.
67. As a maintainer, I want list persistence to follow ADR 0005 strictly,
    avoiding extra redb tables and $O(\text{depth}^2)$ write amplification.
68. As a maintainer, I want `QueryRow` list evaluation to be zero-allocation,
    borrowing strings and statuses on demand from `FileIndex`.
69. As a maintainer, I want recurrence, dependencies, and mutation operations
    deferred, so that the task-system implementation stays focused.

## Implementation Decisions

- Add an explicit Task term to the domain model. A Task is a status-marked
  markdown List Item that either matches configured task tag filters or, when no
  filters are configured, any status-marked list item.
- Keep List Item as the primary structural type. A Task is an overlay on a List
  Item, not a separate tree or independent storage model.
- Replace the checked/unchecked-only model with a List Item kind enum: plain
  bullet, checkbox, or Task.
- Plain list items carry no task data.
- Checkbox list items carry only derived completion state and do not appear in
  `Note.tasks()`.
- Non-task checkboxes retain completion state for display and outline querying,
  while Task list items carry status, optional priority, and task dates.
- `Note.tasks()` returns a zero-allocation iterator over List Items whose kind is
  Task.
- `Note.list_items()` returns a zero-allocation iterator over all List Items.
- In accordance with ADR 0005, `Note` stores list items in a flat, contiguous
  `Box<[ListItem]>` in strict document order. Each item stores `depth: u8` and
  `parent: Option<SourceLine>`. Descendant collection uses non-recursive slice
  scans (`take_while`).
- Drop the unread `LISTS` redb table, removing `ListEntry` and `ListEntryRef`.
  All list and task data persists directly inside `Note` records within the
  `NOTES` table.
- Compact `ListItem` memory layout:
  - `fields: Option<Box<IndexMap<FieldKey, Box<[NoteFieldValue]>>>>`: 8-byte
    pointer for empty items (over 95% of items).
  - `text: ListText { raw: String, clean: Option<String> }`: `clean` is `None`
    when raw text equals clean text, saving an extra heap allocation.
  - `TaskDates` uses `Option<crate::DateValue>` (4 bytes each) instead of
    `chrono::NaiveDate` (12 bytes each).
- Drop pulldown-cmark task-list parsing. Implement one custom marker scanner
  over item-leading text recognizing `[` followed by any single non-`]`
  character, followed by `]`, followed by whitespace.
- The custom scanner handles `[ ]`, `[x]`, `[X]`, configured custom markers,
  and unknown single-character markers (`[?]`, `[!]`).
- `[x]` and `[X]` are equivalent and map to Done.
- Preserve unknown marker symbols. Unknown markers resolve as incomplete todo
  statuses unless a configured status overrides them.
- Introduce a task status model with symbol, display name, and status type.
  Default status types are todo, in-progress, on-hold, done, cancelled, and
  non-task.
- Completion is a tri-state derived from status type:

  | StatusType       | `completed`     |
  | ---------------- | --------------- |
  | Done             | `Some(true)`    |
  | Cancelled        | `None`          |
  | Todo             | `Some(false)`   |
  | InProgress       | `Some(false)`   |
  | OnHold           | `Some(false)`   |
  | NonTask          | `Some(false)`   |
  | Unknown fallback | `Some(false)`   |

- Canonical query namespace is strictly `list.<field>`. Obsolete `task.<field>`
  syntax is rejected with an actionable diagnostic hint.
- `TaskField` is embedded within `ListField::Task(TaskField)`. Non-task items
  return `Null` for task fields via a single pattern match arm.
- Represent list query rows via zero-allocation positional indexing on
  `QueryRow`: `RowKind::List { item_idx: u32 }`. Row evaluation borrows strings
  and statuses directly from the in-memory `Note` in `FileIndex`.
- Support `file.tags` on list rows to access note-level tags, while `list.tags`
  accesses item-level tags.
- On list rows, item-level inline fields take precedence over note frontmatter.
- Add task priority as a fixed enum: lowest, low, normal, medium, high, highest.
  Stored as optional, with missing priority remaining absent.
- Add task lifecycle dates: created, scheduled, start, due, done, cancelled.
  Support both emoji syntax and inline field syntax. Parse dates into
  `DateValue`.
- Add task config section with `tag_filters: Vec<Tag>` and
  `statuses: TaskStatusMap`. Config entries normalize `#tag` and `tag`.
- Compute `fully_complete` on Task items by checking the task itself and
  recursively checking task children only. Plain bullets and checkboxes are
  ignored.
- `ListText.raw` excludes only the leading `[<char>] ` prefix, preserving all
  other inline syntax. `ListText.clean` strips markers, task tag filters, date
  syntax, priority emojis, and inline task fields.
- `render_task_list` in `src/query/format.rs` outputs the exact
  `TaskStatusSymbol` character inside `"- [{}] "`, preserving `[/]`, `[-]`,
  and `[!]`. It indents nested tasks by `list.depth * 2` spaces.
- Specialize `traces task` CLI command:
  - Add `--line-numbers` (`-l`) to render `({path}:{line})` coordinates.
  - Add convenience filter shortcuts: `--todo`, `--done`, `--status <char>`.
  - Flatten `SortArgs` to support `--sort <field>`, `--asc`, and `--desc`.
  - Add `--table` with default columns (`Task`, `Status`, `Due`, `Priority`,
    `File`) and custom `--column` overrides.
  - Add `--count` to output only the matching row count.
  - Omit `--json` and `--plain`.
- Expand `--from` source selector to accept direct `.md` file paths alongside
  tags, folders, and File Classes.
- Expose dual template globals `lists` and `tasks` in MiniJinja, parameterized
  via `QueryMode` (`Pages`, `Lists`, `Tasks`). Replace `TaskFields` with
  `ListFields` implementing `minijinja::value::Object`.

## Testing Decisions

- Prefer high-level integration testing over unit test duplication: use
  `test_support::TestProject` to verify full vault lifecycles, index
  persistence invariance without `LISTS`, query execution across `lists` and
  `tasks` modes, CLI output fidelity, and template rendering.
- Test index persistence invariance: assert that loading from `index.redb`
  without a `LISTS` table reconstructs identical list and task query results
  without reparsing Markdown.
- Test that `render_task_list` preserves custom status markers (`[/]`, `[!]`)
  and indents nested items accurately.
- Test that queries accept canonical `list.<field>` paths and reject
  `task.<field>`.
- Add dedicated Criterion benchmarks in `benches/`:
  - `benches/query_sort.rs`: Assert zero heap allocations during list row
    sorting across text, due date, priority, and status fields (`SortKey<'a>`).
  - `benches/memory_footprint.rs`: Assert average memory footprint under 100
    bytes per `ListItem` across 50,000 items in `FileIndex`.
  - `benches/query_execution.rs`: Measure query throughput scaling across task
    densities (1, 10, 100 tasks per note).

## Out of Scope

- Mutating task status from the CLI.
- Status cycling and next-status transitions.
- Editing Notes to complete, cancel, or reschedule tasks.
- Recurrence rules.
- Task dependencies and dependency IDs.
- On-completion behaviors.
- Urgency scoring.
- Machine-readable `--json` and `--plain` CLI flags.
- Nested tag matching for task classification.
- Backward compatibility aliases for `task.<field>`.
- Splitting or redesigning the FileIndex.
