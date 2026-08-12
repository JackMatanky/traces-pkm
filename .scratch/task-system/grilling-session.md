# Task System Grilling Session — Design Document

> **Date:** 2026-08-13
> **Method:** Domain-modeling grilling session (19 rounds)
> **Goal:** Improve task handling across CLI, query commands, template engine, and note parsing
> **Status:** Design complete through data model; implementation pending

---

## Current State

### Parsing (`src/note/`)

- `lists.rs`: `TaskStatus` enum (`Incomplete`/`Complete`), `ListItem` with optional `task_status`, `List` and `ListContainer` traits
- `model.rs`: `Note` has `tasks()` method returning `TaskIter` — depth-first iterator over task list items
- Tasks are derived from lists, not stored separately
- `ListItem` already has `children` for nested subtasks

### Indexing (`src/index/`)

- `query.rs`: `IndexRecord` has `TaskInfo { completed: bool, text: String }` for task-level rows
- `FileIndex::query_tasks()` expands each Note into one row per task item
- `QueryOutcome::task_list()` renders task records as markdown checkboxes
- Tasks stored in redb `note_metadata` table as part of Note serialization

### CLI (`src/cli/`)

- `task.rs`: `traces task` command with `--from` and `--where` flags
- `mod.rs`: `refresh_task_query()` and `apply_filter()` shared helpers
- Output format: `- [ ] text (path)` or `- [x] text (path)`
- No `--sort` flag (unlike `traces list` and `traces table`)
- `--from` only supports tag and folder sources

### Template Engine (`src/template/engine/`)

- `query.rs`: `tasks` namespace registered as `QueryOps` with `query: FileIndex::query_tasks`
- Methods: `all()`, `from_tags()`, `from_folder()`, `from_class()`
- Terminal: `task_list()`, `table()`, `list()`, `count()`
- Non-terminal: `where()`/`filter()`, `sort()`, `limit()`, `group_by()`, `flatten()`

### Domain Model (`CONTEXT.md`)

- `Task` mentioned implicitly through `task.completed` / `task.text` fields
- No explicit "Task" term defined in CONTEXT.md

---

## Reference Materials

| Source | Key Takeaways |
|--------|---------------|
| [Obsidian Tasks Emoji Format](https://publish.obsidian.md/tasks/Reference/Task+Formats/Tasks+Emoji+Format) | Dates (➕⏳🛫📅✅❌), priority (⏬🔽🔼⏫🔺), recurrence (🔁), on-completion (🏁), dependencies (🆔⛔) |
| [Obsidian Tasks Global Filter](https://publish.obsidian.md/tasks/Getting+Started/Global+Filter) | Obsidian Tasks uses arbitrary string matching and warns against subtags under the global filter; traces-pkm deliberately narrows this to typed exact tag filters |
| [Obsidian Tasks Scripting/Task Properties](https://publish.obsidian.md/tasks/Scripting/Task+Properties) | Properties: status, priority, dates, tags, recurrence, dependsOn, isRecurring, description, subtasks, parent |
| [Dataview metadata-tasks](https://blacksmithgu.github.io/obsidian-dataview/annotation/metadata-tasks) | Tasks parsed from markdown lists with checkboxes; accessible via `task.fieldName`; fields: status, completed, subtasks, text, line, lineCount, section, tags, outlinks, list, blockId |

---

## Design Decisions

### Round 1

#### Q1 — Task Metadata Scope

**Decision:** Add dates and priority. Defer recurrence and urgency scoring.

| Category | Fields |
|----------|--------|
| Dates | created ➕, scheduled ⏳, start 🛫, due 📅, done ✅, cancelled ❌ |
| Priority | Fixed 6-level enum: lowest, low, normal, medium, high, highest |
| Tags | Per-task tags AND note-level tags (both) |
| Deferred | Recurrence, urgency scoring |

#### Q2 — Date Parsing

**Decision:** Support both emoji syntax and inline field syntax.

- **Emoji:** `📅 2026-08-15` — needs a small parser addition
- **Inline field:** `[due:: 2026-08-15]` — already parsed by existing infrastructure

#### Q3 — CLI `--sort` for Tasks

**Decision:** Add `--sort` and `--order desc` to `traces task`.

- Subtask ordering is a concern (child before parent)
- Consistent with `traces list` and `traces table`

#### Q4 — Task-Level Query Filters

**Decision:** Yes, filtering by `task.due`, `task.priority` etc. is crucial.

- No new filter syntax needed; existing engine handles new field paths
- Field paths like `task.due`, `task.priority` work out of the box

#### Q5 — CLI Output Format

**Decision:** Add richer output including table format.

- No `--format` flag needed
- Use `--table` flag with column array

#### Q6 — `--from` Expansion

**Decision:** Expand `--from` to handle tags, folders, file classes, and specific files.

- Instead of adding `--note`, make `--from` more general
- `--from "#tag"` → tag source
- `--from "folder"` → folder source
- `--from "class:value"` → class source (new)
- `--from "path/to/file.md"` → single file source (new, detected by `.md` extension)

#### Q7 — Task Completion

**Decision:** Defer mutation operations to future session.

---

### Round 2

#### Q8 — Global Filter Tag

**Decision:** Yes, configurable via config.toml.

```toml
[tasks]
tag_filters = ["task"]
```

- When set, only task items containing that tag are indexed as tasks
- Tag is stripped from `task.text` in output
- Superseded by Q108-Q114: field name is `tag_filters`, values are arrays, and config entries may include or omit `#` but normalize to validated `Tag` values.

#### Q9 — Task Tags vs Note Tags

**Decision:** Task tags are both per-task and note-level.

- Same tag appears in `task.tags` AND `note.tags`
- Consistent with Obsidian's model

#### Q10 — Priority Enum

**Decision:** Fixed enum: lowest, low, normal, medium, high, highest.

- Superseded by Q97: missing priority stays absent as `Option<TaskPriority>::None`, not `normal`
- Enables sorting by priority

#### Q11 — Subtask Ordering

**Decision:** Add `parent` field to tasks for parity with Dataview.

- Enables subtask ordering and `fully_complete` status
- User was unsure, leaning toward adding parent field
- `fully_complete` = all children (recursively) are done

#### Q12 — CLI Output for Tasks

**Decision:** `--table` flag followed by column array (no `--format` needed).

Default columns: `task.text`, `task.status`, `task.due`, `file.name`

#### Q13 — `--from` Expansion Details

| Syntax | Source Type |
|--------|-------------|
| `--from "#tag"` | Tag source (unchanged) |
| `--from "folder"` | Folder source (unchanged) |
| `--from "class:value"` | Class source (new) |
| `--from "path/to/file.md"` | Single file source (new, detected by `.md` extension) |

Apply to all CLI query commands: `list`, `table`, `task`.

#### Q14 — Task Dates in Templates

**Decision:** `FieldValue::Date` when emoji date is present and parseable (YYYY-MM-DD).

- Missing dates resolve to `FieldValue::Null`

#### Q15 — Cancelled Task Status

**Decision:** Yes, support cancelled status from `[-]`.

- Superseded by later status model: `TaskStatus` is now a struct and cancellation is represented by `TaskStatusType::Cancelled`.

---

### Round 3

#### Q16 — Custom Status Config Format

**Decision:** Default statuses with default order. Explicit next-status pushed to later.

```toml
[tasks]
tag_filters = ["task"]

[[tasks.statuses]]
symbol = "[ ]"
name = "Todo"
type = "TODO"

[[tasks.statuses]]
symbol = "[/]"
name = "In Progress"
type = "IN_PROGRESS"

[[tasks.statuses]]
symbol = "[x]"
name = "Done"
type = "DONE"

[[tasks.statuses]]
symbol = "[-]"
name = "Cancelled"
type = "CANCELLED"

[[tasks.statuses]]
symbol = "[!]"
name = "On Hold"
type = "ON_HOLD"
```

#### Q17 — Status Type Semantics

**Decision:** `task.completed` stays available as a derived query field; `task.status` is a new status field.

- `task.completed` = `true` for `DONE` / `CANCELLED`
- `task.status` = `"todo"`, `"in_progress"`, `"on_hold"`, `"done"`, `"cancelled"`
- Backward compatibility is not a design constraint because the project has not shipped yet
- Later rounds replaced the raw storage shape with `ListItemType` plus `TaskStatusType`; query fields can be derived from that model

#### Q18 — `fully_complete` Field

**Decision:** Resolved by later rounds: use a separate LISTS table and defer/remove ListView unless implementation proves it useful.

- A LISTS table could store list hierarchy info (parent relationships, completion status)
- Or create a view type for these queries
- Resolved by Q30/Q69/Q79: LISTS table is separate, `ListRecord` is flattened, and `fully_complete` is computed at index time for query access.

#### Q19 — Status Cycling CLI

**Decision:** Defer to future session (write operation with its own design tree).

#### Q20 — Default Columns for `traces task --table`

**Decision:**

- Default: `task.text`, `task.status`, `task.due`, `file.name`
- Users can override with column array after `--table` flag

#### Q21 — `--from` with Class Sources

**Decision:** Yes, transitive is-a matching consistent with template API.

#### Q22 — Task Text with Global Filter Stripped

**Decision:** Similar to Tasks plugin: task text should have full and clean views.

| View | Description |
|------|-------------|
| **Full** | Shows everything including emojis and tags |
| **Clean** | Strips global filter and other inline task fields |

---

### Round 4

#### Q23 — LISTS Table and ListView

**Decision:** Superseded by later rounds: use a LISTS table; defer/remove ListView unless implementation proves it useful.

- LISTS table: redb table storing list hierarchy data (parent, children, fully_complete)
- ListView was considered as a helper for populating LISTS but is not part of the initial design
- Final implementation should start without ListView and add it only if it removes real duplication

#### Q24 — Parent Field

**Decision:** Field called `task.parent`. Aim for line number, but may need byte offset due to pulldown-cmark parsing.

- `SourceLine(u32)` newtype for line numbers
- `ByteOffset(usize)` newtype for byte offsets
- Research found: `into_offset_iter()` provides byte offsets, `line_starts` vec with `partition_point` gives O(1) line lookup

#### Q25 — fully_complete Computation

**Decision:** Computed at index time.

#### Q26 — Task Text Full/Clean

**Decision:** `ListText` type with `raw` and `clean` fields.

#### Q27 — Custom Status Parsing (Stress Test)

**Decision:** Post-parse annotation won over lexer extension.

- Stress tested lexer extension vs post-parse annotation
- Post-parse annotation: cleaner separation, more testable, config-independent parser
- However, config coupling may be inevitable since ignore/file settings need config anyway
- Final: accept parser config coupling through `NoteConfigSpec`; the custom marker scanner and task annotation run during parsing.

#### Q28 — Research Completed

**Decision:** Subagent researched Tasks and Dataview data models.

- Tasks plugin: `ListItem` base class, `Task` extends `ListItem`, `StatusConfiguration` system, ID-based dependencies
- Dataview: `ListItem` with parent/children as line numbers, `fullyCompleted` boolean, implicit fields

#### Q29 — Tags on ListItem

**Decision:** Store tags directly on `ListItem`.

---

### Round 5

#### Q30 — LISTS Table Schema

**Decision:** Separate redb table with composite key `(path, line)`.

- `ByteOffset`/`SourceLine` newtypes
- `ListText { raw, clean }` type
- `TaskItem` for task-specific fields
- Tags as `Vec<Tag>` on `ListItem`
- `parent_line` named `parent`
- No list symbol needed
- Reuse `children` and `fields` from `ListItem`
- `TaskStatusMarker`/`TaskStatusSymbol` and `TaskStatusType` newtypes
- Rename current `TaskStatus` to `TaskCompletionStatus`

#### Q31 — ListView

**Decision:** Superseded by later rounds: do not include ListView in the initial design.

- `ListItem`, `ListItemType`, and flattened `ListRecord` provide the needed structures.
- Add ListView later only if implementation proves a real need.

#### Q32 — Tags on ListItem

**Decision:** Store directly on `ListItem`.

#### Q33 — Line Number vs Byte Offset

**Decision:** Prefer line number.

- Need to check pulldown-cmark feasibility
- Research found: `into_offset_iter()` provides byte offsets, `line_starts` vec with `partition_point` gives O(1) lookup

#### Q34 — src/task.rs

**Decision:** Root-level `src/task.rs` file. If larger, create `src/tasks/` module.

#### Q35 — Emoji Date Parsing

**Decision:** Lexer already handles task shorthands (confirmed by reading lexer.rs).

- `extract_task_inline_fields()` already handles 🗓️, ➕, 🛫, ⏳, ✅ emojis

---

### Round 6

#### Q36 — Config Struct

**Decision:** Superseded by Q46/Q49: `NoteConfigSpec` (renamed from `MarkdownParserInput`).

- Follows the codebase convention for focused input/spec structs.
- Final shape: `NoteConfigSpec { task: TaskConfig, frontmatter: FrontmatterConfig }`.

#### Q37 — TaskItem/ListItem Restructuring (MAJOR)

**Decision:** Superseded by Q74/Q101: `ListItem` uses `kind: ListItemType`; task-specific data is in `ListItemType::Task(TaskListItem)`.

- `TaskCompletionStatus` no longer lives as a separate `ListItem` field; it is held by `ListItemType::Checkbox(TaskCompletionStatus)` or derived from `TaskListItem.status`
- `ListText` goes on `ListItem` as `text: ListText`
  - If a task, compute `clean` text
- `parent` goes on `ListItem` with newtype for file lines
- `tags` goes on `ListItem`
- `TaskListItem` is composed inside `ListItemType::Task`, not a wrapper around `ListItem`
- `TaskStatus` is a struct (not enum) with `symbol`, `name`, `status_type` fields

#### Q38 — Priority Storage

**Decision:** Store as enum value.

- Emoji parsed during task annotation, mapped to enum
- Raw emoji not stored (presentation concern)

#### Q39 — LISTS Table Value

**Decision:** Most fields on `ListItem`.

- Need separate struct to add `path` field (note housing the list)
- `line` and `depth` on `ListItem` depends on cost of adding them to struct

#### Q40 — ByteOffset Tracking

**Decision:** Do NOT extend `SourceText` (only responsible for text).

- Create `ByteOffset`/`Span` newtype
- Create `ByteTracker` or cursor utility

#### Q41 — --table Columns

**Decision:** Already settled.

- Default columns: `task.text`, `task.status`, `task.due`, `file.name`
- No `--column` flag
- Array of column names after `--table` flag

---

## Research Findings

### pulldown-cmark Line Tracking

- Only provides byte offsets (`Range<usize>`) via `OffsetIter`, never line numbers
- `into_offset_iter()` is zero-overhead (same tree walk, just returns ranges)
- No built-in byte→line conversion utility
- Cannot get positions without consuming parser
- `SoftBreak`/`HardBreak` get byte ranges when using `OffsetIter`
- **Recommendation:** Switch to `into_offset_iter()` in `parser.rs`, build `line_starts` vec for O(1) conversion

### pulldown-cmark List Item Research

- Current parser already has a state machine via `ListTracker`
- `ItemFrame` serves as de facto `RawListItem`
- `Tag::Item` is unit variant (no data)
- `Event::TaskListMarker(bool)` only carries checked state
- Indentation level NOT exposed in events
- `RawListItem` is over-engineering — current approach is optimal

### Dataview Design Patterns

- Identity = line number (simple but fragile, accepted because markdown has no stable block IDs)
- Task as optional sub-object on `ListItem` (not inheritance)
- `fullyCompleted` is index-time derived
- Fields always arrays (`Map<string, Literal[]>`)
- Two date parsing paths (inline fields + emoji shorthands)
- Implicit field inheritance from pages
- `annotated` boolean for quick filtering
- `ListSerializationCache` for lazy resolution

### Tasks Plugin Design

- `Task` extends `ListItem` (inheritance, not composition)
- `ListItem` has 8 fields (markdown structure + hierarchy)
- `Task` adds 11 task-specific fields
- `StatusConfiguration` → `Status` → `StatusRegistry` three-layer design
- Parent-child via `parent`/`children` pointers (object references)
- Both `Task` and `ListItem` coexist in same hierarchy
- Two-phase parsing: structural → semantic
- Global filter checked during `Task.fromLine()`, falls back to `ListItem` if missing

---

### Round 7

#### Q43 — TaskListItem Composition Direction

**Decision:** `TaskListItem` is composed within `ListItemType::Task` (Dataview pattern, not Tasks plugin inheritance).

- User debated: TaskListItem wrapping ListItem vs ListItem wrapping TaskListItem
- Tasks plugin uses inheritance; Dataview uses composition
- Final: `ListItem` is the primary type with `kind: ListItemType`
- Final shape uses `ListItem { text, kind, children, fields, tags, parent, line, depth }`
- `TaskListItem` has `status`, `priority: Option<TaskPriority>`, and `dates`; `fully_complete` is a method plus an indexed `ListRecord` field

#### Q44 — ListRecord

**Decision:** Superseded by Q69: `ListRecord` is flattened for LISTS table persistence.

- Adds `path: PathBuf` for note path
- Duplicates query-relevant list fields at top level for easier LISTS table access
- Does NOT duplicate data unnecessarily beyond the flattened query record shape

#### Q45 — ByteTracker

**Decision:** User prefers ByteTracker over standalone functions.

- Zero-copy if possible
- Creates `line_starts` vec during construction
- Provides `byte_to_line()` method

#### Q46 — MarkdownParserInput

**Decision:** Renamed to `NoteConfigSpec`.

- Contains all input parameters for markdown parser
- Avoids constant re-allocation for config values
- Larger refactor: split FileIndex into separate file indexer (future, irrelevant to task system)

#### Q47 — Task Annotation Timing

**Decision:** During parsing (not indexing or query time).

- Status resolution, date parsing, and priority mapping happen during the parse step
- Post-parse annotation layer receives `NoteConfigSpec`

#### Q48 — ListView

**Decision:** Superseded by later rounds: do not include ListView in the initial design.

- `ListItem`, `ListItemType`, and flattened `ListRecord` provide the needed structures.
- Add ListView later only if implementation proves a real need.

---

### Round 8

#### Q49 — NoteConfigSpec Structure

**Decision:** Option A — composition.

```rust
pub struct NoteConfigSpec {
    pub task: TaskConfig,
    pub frontmatter: FrontmatterConfig,
}
```

- User asked about `FrontmatterTable` which doesn't exist
- Config model has `FrontmatterConfig` with `title`, `aliases`, `date_created`, `date_modified`

#### Q50 — ByteTracker Zero-Copy

**Decision:** User wants zero-copy if possible.

- pulldown-cmark research completed; byte→line via `line_starts` vec with `partition_point`

#### Q51 — FileIndex Split

**Decision:** Future refactor, irrelevant to task system.

#### Q52 — TaskListItem Naming

**Decision:** Confirmed as `TaskListItem`.

#### Q53 — ListRecord Path Field

**Decision:** Superseded by Q69: `ListRecord` is flattened and stores query-relevant fields directly.

- `ListRecord` includes `path` plus flattened list fields such as `line`, `depth`, `parent`, `text`, `tags`, kind/status data, and indexed `fully_complete`.

#### Q54 — TaskStatusMap

**Decision:** Built once at startup.

- Struct with `by_name`, `by_symbol`, `by_type` HashMaps
- Avoids repeated lookups during parsing

---

### Round 9

#### Q55 — Field Inheritance

**Decision:** Keep off ListItem/TaskListItem.

- Put on `ListRecord` or resolve via note metadata at query time
- Must ensure list/task items can access note metadata

#### Q56 — has_metadata

**Decision:** Deferred.

#### Q57 — TaskStatusMap by_type

**Decision:** Yes, add `by_type: HashMap<TaskStatusType, Vec<TaskStatus>>`.

- Enables lookups like "all statuses of type DONE"

#### Q58 — NoteConfigSpec

**Decision:** Reconfirmed Option A (composition).

```rust
pub struct NoteConfigSpec {
    pub task: TaskConfig,
    pub frontmatter: FrontmatterConfig,
}
```

#### Q59 — ListItem line/depth

**Decision:** Add both (`line: SourceLine`, `depth: u8`).

- Cost is ~8 bytes per item (negligible)
- Enables O(1) line lookups and hierarchy reconstruction

---

### Round 10

#### Q60 — ListItem as Primary Type

**Decision:** Superseded by Q74: ListItem is the primary type with `kind: ListItemType`.

- Adopt Dataview pattern (composition over inheritance)
- All common fields on `ListItem`
- Task-specific data is carried by `ListItemType::Task(TaskListItem)`

#### Q61 — ListRecord Structure

**Decision:** User wonders about flattening for easier access in LISTS multimap.

- Initial shape: `ListRecord { path: PathBuf, item: ListItem }`
- Flattening confirmed in Q69

#### Q62 — Field Inheritance

**Decision:** Option A: Full inheritance (item fields → note fields including frontmatter + inline).

- Fields resolve from list item → note frontmatter → note inline fields
- Confirmed: Full inheritance model

#### Q63 — TaskConfig

**Decision:**

- Do NOT add default status and priority (status always available, not every task has priority)
- Private fields with accessors
- Consider restricting global filter to tags
- If restricting to tags, move `tag.rs` to `src/` for use in both `config/` and `note/`
- `TaskConfig { statuses: TaskStatusMap, filter: Option<String> }` or `tag_filter: Option<Tag>`
- Superseded by Q108-Q111: `TaskConfig { statuses: TaskStatusMap, tag_filters: Vec<Tag> }`.

#### Q64 — Default Statuses

**Decision:** Yes, include default status but allow user to extend or override.

- Default statuses always available
- User config can add new statuses or override existing ones

#### Q65 — Global Filter and Task Detection

**Decision:** Superseded by Q71/Q74/Q98.

- Parser classifies item kind during construction with the custom scanner and exact tag filters
- `Note.tasks()` returns only `ListItemType::Task(_)`
- `Note.list_items()` is the unfiltered escape hatch

---

### Round 11

#### Q66 — RawListItem Shape

**Decision:** Current parser already has a state machine via `ListTracker`. `RawListItem` is over-engineering.

- Research: `ItemFrame` serves as de facto `RawListItem`
- `ListTracker` is already a state machine
- `Tag::Item` is a unit variant (no data)
- `Event::TaskListMarker(bool)` only carries checked state
- Indentation level NOT exposed in events
- Adding a `RawListItem` intermediate would be over-engineering — current approach is optimal
- **BUT:** Question remains open about parsing task status symbols from `TaskListMarker`

#### Q67 — fully_complete as Method

**Decision:** User questions why method requires tree walking if ListItem has children attribute.

- Children are already on `ListItem`
- `fully_complete` can check subtree only (no full tree walk needed)
- Moved to `ListRecord` (computed at index time)

#### Q68 — Tag Module Location

**Decision:** Confirmed: move `tag.rs` to `src/tag.rs`.

- Makes `Tag` public with validated constructor
- Usable in both `config/` and `note/` modules

#### Q69 — ListRecord Flattening

**Decision:** Confirmed: flatten `ListRecord` for LISTS multimap.

- All fields at top level for easier querying
- No nested `ListItem` in `ListRecord`

#### Q70 — Tag Module Path

**Decision:** Confirmed: `src/tag.rs`.

#### Q71 — Note.tasks() Filtering

**Decision:** User reiterates: `Note.tasks()` must be filtered.

- Cannot have unfiltered `Note.tasks()`
- Global filter must be accessible at Note level

---

### Round 12

#### Q72 — Note.tasks() Filtering Implementation

**Decision:** Option C: Flag during construction (confirmed).

- Superseded by Q74/Q75: parser stores the classification in `ListItemType`; no `matches_filter` boolean is kept
- `Note.tasks()` returns only items whose `kind` is `ListItemType::Task(_)`
- `NoteConfigSpec` carries task filter config to parser
- Superseded by Q108: filter config is `tag_filters: Vec<Tag>`.

#### Q73 — Escape Hatch Method

**Decision:** `Note.list_items()` (confirmed).

- Not `Note.all_list_items()` — name is simpler
- `Note.list_items()` returns all list items unfiltered
- Escape hatch for advanced users who want everything
- Avoids naming confusion with `Note.tasks()`

#### Q74 — ListItemType Enum

**Decision:** Single enum replacing separate booleans. Naming confirmed: `Plain` / `Checkbox` / `Task`.

```rust
pub enum ListItemType {
    /// Plain bullet item, no checkbox
    Plain,
    /// Checkbox item that doesn't match global filter
    Checkbox(TaskCompletionStatus),
    /// Checkbox item that matches global filter, has task data
    Task(TaskListItem),
}
```

- Alternatives discussed: `Regular`/`Checklist`/`Task`, `Bullet`/`Checkbox`/`Task`, `ListItem`/`CheckItem`/`TaskItem`
- Final naming: `Plain` / `Checkbox` / `Task`
- `Plain` holds no data
- `Checkbox(TaskCompletionStatus)` holds completion status only
- `Task(TaskListItem)` holds full task data
- Replaces `is_task: bool`, `is_completed: Option<TaskCompletionStatus>`, `task: Option<TaskListItem>` fields on `ListItem`

#### Q75 — matches_filter on ListItem

**Decision:** Not needed on `ListItem` itself when using `ListItemType` enum.

- The enum discriminant tells us if it's a task
- `ListItemType::Task(...)` implies `matches_filter == true`
- `ListItemType::Checkbox(...)` implies `matches_filter == false`
- `ListItemType::Plain` is neither
- No separate boolean field needed

#### Q76 — fully_complete as Method on TaskListItem

**Decision:** Method on `TaskListItem` taking `&ListItem` reference.

- Walks children (on `ListItem`), so takes `&ListItem` to access children
- O(subtree) not O(full tree) — only checks children, not the entire tree
- Computed at index time, result stored on `ListRecord`

#### Q77 — ListItemType Naming (Final)

**Decision:** Confirmed: `Plain` / `Checkbox` / `Task`.

- `Plain`: no data (bullet item with no checkbox)
- `Checkbox(TaskCompletionStatus)`: has checkbox, filter not matched
- `Task(TaskListItem)`: has checkbox, filter matched, full task data

#### Q78 — TaskCompletionStatus in Checkbox

**Decision:** `Checkbox(TaskCompletionStatus)` is correct.

- `Plain` has no data (unit variant)
- `Checkbox` holds `TaskCompletionStatus` (Incomplete/Complete)
- `Task` holds `TaskListItem` (full task data)

#### Q79 — fully_complete Location

**Decision:** Both method on `TaskListItem` AND attribute on `ListRecord`.

- Method on `TaskListItem`: for on-demand computation (takes `&ListItem` to access children)
- Attribute on `ListRecord`: for indexed queries (computed at index time)
- Method is O(subtree), attribute is O(1) lookup

#### Q80 — Note.list_items() Return Type

**Decision:** `&[ListItem]`.

- Returns a slice of all list items
- Unfiltered escape hatch

#### Q81 — Note.tasks() Filtering

**Decision:** `tasks()` skips `Checkbox`, returns only `Task` items.

- `list_items()` returns all items (Plain, Checkbox, Task)
- `tasks()` filters to only `ListItemType::Task(_)` items
- Filtering by global tag filter happens at construction time (Q72)

#### Q82 — Task Status Symbol Parsing

**Decision:** Superseded by Q98: during item-leading text extraction with one custom scanner.

- `ENABLE_TASKLISTS` is dropped, so `Event::TaskListMarker` is not part of the final classification path
- Scan item-leading text for `[<single non-]>]` followed by whitespace
- Map the extracted `TaskStatusSymbol` via `TaskStatusMap`
- Happens during the parse step (consistent with Q47)

---

### Round 13

#### Q77 — ListItemType Naming (Final Confirmation)

**Decision:** Confirmed: `Plain` / `Checkbox` / `Task`.

- `Plain`: unit variant, no data (bullet item with no checkbox)
- `Checkbox(TaskCompletionStatus)`: holds completion status, filter not matched
- `Task(TaskListItem)`: holds full task data, filter matched

#### Q78 — TaskCompletionStatus in Checkbox

**Decision:** `Checkbox(TaskCompletionStatus)` is correct.

- `Plain` has no data (unit variant)
- `Checkbox` holds `TaskCompletionStatus` (Incomplete/Complete)
- `Task` holds `TaskListItem` (full task data)

#### Q79 — fully_complete Location (Final)

**Decision:** Both method on `TaskListItem` AND attribute on `ListRecord`.

- Method on `TaskListItem`: for on-demand computation (takes `&ListItem` to access children)
- Attribute on `ListRecord`: for indexed queries (computed at index time)
- Method is O(subtree), attribute is O(1) lookup

#### Q80 — Note.list_items() Return Type

**Decision:** `&[ListItem]`.

- Returns a slice of all list items
- Unfiltered escape hatch

#### Q81 — Note.tasks() Filtering

**Decision:** `tasks()` skips `Checkbox`, returns only `Task` items.

- `list_items()` returns all items (Plain, Checkbox, Task)
- `tasks()` filters to only `ListItemType::Task(_)` items
- Filtering by global tag filter happens at construction time (Q72)

#### Q82 — Task Status Symbol Parsing

**Decision:** Option A: during text extraction.

- For list items with a `TaskListMarker`
- Scan text buffer for status character pattern
- Map via `TaskStatusMap`
- Happens during the parse step (consistent with Q47)

---

### Round 14

#### Q83 — Status Character Extraction

**Decision:** Store raw character in `ItemFrame` when receiving `Event::TaskListMarker`.

- Extract from byte range using `into_offset_iter()`
- pulldown-cmark's `Event::TaskListMarker(bool)` only gives checked state
- Status character is in brackets, NOT in text
- `Event::TaskListMarker` byte range covers `[/]` or `[x]` etc.
- Use `into_offset_iter()` to get byte ranges
- Extract char at `offset + 1` from `TaskListMarker` byte range
- Superseded by Q98: do not enable pulldown-cmark `ENABLE_TASKLISTS`; marker identity comes from the custom scanner.

#### Q84 — ItemFrame Fields

**Decision:** `ItemFrame` should have:

```rust
pub struct ItemFrame {
    is_checked: Option<bool>,    // from TaskListMarker
    status_char: Option<char>,   // raw character from [/], [x], etc.
    text_buffer: String,
    scan_buffer: String,
    // ... existing fields
}
```

- `is_checked: Option<bool>` — from `Event::TaskListMarker`
- `status_char: Option<char>` — raw character (space for `[ ]`, `x` for `[x]`, `/` for `[/]`)
- Separate fields because `is_checked` is the parsed boolean, `status_char` is the raw symbol for status resolution
- Superseded by Q101/Q106 naming: final field name is `task_status: Option<TaskStatusSymbol>`.

#### Q85 — Tag Filter Check Timing

**Decision:** During `ListItem` construction (at `Event::End(TagEnd::Item)`).

```
if has_task_status && any exact tag filter match → Task
if has_task_status && no exact tag filter match → Checkbox
if no_task_status → Plain
```

- Task status presence indicates a checkbox/task marker
- Exact tag filter check determines if it's a Task or just a Checkbox
- Plain items have no task status at all
- Superseded by Q103-Q105: multiple tag filters are allowed; check any tag on the item, exact match only.

#### Q86 — No Filter Configured

**Decision:** If no task filter is configured, ALL checkbox items become `ListItemType::Task`.

- Matches Obsidian Tasks plugin behavior
- Without a filter, every checkbox is treated as a task
- `has_status_char && no_filter → Task`

#### Q87 — Final Design Summary

**Decision:** Superseded by Q115; do not produce a final design summary yet.

- Finalize this grilling session document instead.
- Prepare it for later `/to-spec` conversion.

---

### Round 15

#### Q88 — Custom Status Marker Scanner

**Decision:** Implement a custom item-leading text marker scanner for statuses pulldown-cmark does not emit as `TaskListMarker`.

- New research correction: pulldown-cmark only recognizes `[ ]`, `[x]`, and `[X]` as `Event::TaskListMarker(bool)`.
- Custom statuses such as `[/]`, `[-]`, and `[!]` are not emitted as `TaskListMarker`; they remain in `Event::Text`.
- Therefore, relying only on `Event::TaskListMarker` byte ranges cannot support configured custom task status symbols.
- Q92 resolved scanner source: use item-leading `Event::Text` when `Event::TaskListMarker` did not fire.
- Scanner should mirror the useful shape of pulldown-cmark's task marker rules while accepting any single non-`]` marker character.

Scanner contract:

- Run on item-leading text.
- Require `[` at the item-leading marker position.
- Accept any single non-`]` character as the marker character, not only whitespace, `x`, or `X`.
- Require `]`.
- Require trailing whitespace after the closing bracket.
- Trim the marker prefix exactly once.
- Preserve unknown symbols for diagnostics/fallback.
- Do not duplicate pulldown-cmark task markers when `Event::TaskListMarker` already fired for `[ ]`, `[x]`, or `[X]`.
- Preserve clean/raw text semantics: raw text should keep source-visible task text semantics, while clean text strips only task marker/filter/inline task metadata according to the final rules.
- Superseded by Q98: drop pulldown-cmark `ENABLE_TASKLISTS` and use this custom marker scanner for both standard and custom markers.

#### Q89 — Trim Custom Marker Prefix

**Decision:** Trim the `[<char>] ` prefix from item text after custom status extraction.

- Because custom markers arrive in item-leading `Event::Text`, their literal marker prefix would otherwise remain in `ListText.raw` / task display text.
- After extracting the marker, remove the exact marker prefix from the text buffers used for list item text.
- The trim must be source-position aware enough to avoid removing a later textual `[!]` or `[/]` that is not the leading task marker.
- Clean/raw semantics still need a precise final definition: the parser must not accidentally make `raw` mean "post-normalized text" if future consumers need source-faithful text.

#### Q90 — `ItemFrame` Completion and Status Fields

**Decision:** Keep both `is_checked: Option<bool>` and `status_symbol: Option<TaskStatusSymbol>` in `ItemFrame`.

```rust
struct ItemFrame {
    is_checked: Option<bool>,
    status_symbol: Option<TaskStatusSymbol>,
    // existing text_buffer, scan_buffer, fields, children, line/depth/parent as designed
}
```

- `is_checked` preserves pulldown-cmark's completion boolean for recognized `[ ]`, `[x]`, and `[X]` markers.
- `status_symbol` preserves the raw status symbol for both built-in and custom markers.
- The split avoids conflating completion (`bool`) with status identity (`TaskStatusSymbol`).
- `TaskCompletionStatus` for `Checkbox` remains derived from built-in `is_checked` when present; task status semantics come from `TaskStatusMap` / fallback type.
- Superseded by Q101/Q106: with `ENABLE_TASKLISTS` dropped, `ItemFrame` only needs `task_status: Option<TaskStatusSymbol>`.

#### Q91 — No Filter Configured with Custom Statuses

**Decision:** If no filter is configured, all checkbox/custom-status items become `ListItemType::Task`.

- Applies to pulldown-cmark-recognized checkboxes (`[ ]`, `[x]`, `[X]`).
- Applies to custom configured statuses found by the custom scanner (`[/]`, `[-]`, `[!]`, etc.).
- If a tag filter is configured, only checkbox/custom-status items matching that filter become `Task`; non-matching checkbox/custom-status items remain `Checkbox`.

---

### Round 16

#### Q92 — Custom Marker Scanner Source

**Decision:** Superseded by Q98: use item-leading `Event::Text` for all task marker detection.

- Byte offsets remain useful for line tracking, not marker identity.
- Custom statuses should be found in the leading text content, not by depending on pulldown-cmark to classify them.
- This keeps source-offset tracking useful without making custom status parsing depend on pulldown-cmark internals.

#### Q93 — Unknown `[?]` Classification

**Decision:** Never silently downgrade an unknown bracket marker to `Plain`.

- With a tag filter configured, apply the same missing/present filter rules and classify as `Checkbox` or `Task` accordingly.
- With no filter configured, classify as `Task` with fallback `TaskStatusType::Todo`.
- Preserve the raw unknown symbol for diagnostics and fallback behavior.

#### Q94 — Scanner Acceptance vs Config Completeness

**Decision:** The custom scanner accepts any single non-`]` character inside brackets, then resolves it via `TaskStatusMap`.

- Parsing should not depend on config completeness.
- Unknown symbols remain valid parsed markers and are resolved through fallback status semantics.

#### Q95 — Tag Filter Exact vs Nested

**Decision:** Superseded by Q99/Q105: exact match only for task classification.

- Obsidian Tasks uses arbitrary string matching and warns against subtags under the global filter.
- traces-pkm deliberately uses exact normalized `Tag` matching because task filters are classification boundaries, not search.

#### Q96 — `ListText.clean` Scope

**Decision:** `ListText.clean` strips task-only syntax; `raw` remains source-like display.

`ListText.clean` strips:

- Task marker
- Tag filter
- Task dates
- Priority emojis
- Inline task fields

`ListText.raw` remains source-like display text after parser-level marker handling.

Final clean/raw rule for spec-writing: `clean` is for query/display normalization; `raw` is for source-like display and diagnostics, not for rebuilding exact source bytes.

#### Q97 — Task Priority Absence

**Decision:** `TaskListItem.priority: Option<TaskPriority>`.

- Priority absence should remain absent.
- Do not default missing priority to `TaskPriority::Normal` in the task model.

---

### Round 17

#### Q98 — Drop pulldown-cmark `ENABLE_TASKLISTS`

**Decision:** Drop pulldown-cmark `ENABLE_TASKLISTS`; use one custom marker scanner for standard and custom markers.

- Do not rely on `Event::TaskListMarker(bool)` for task detection.
- The custom scanner recognizes standard markers (`[ ]`, `[x]`, `[X]`) and custom markers (`[/]`, `[-]`, `[!]`, unknown single-character markers, etc.).
- This removes the split between built-in marker extraction and custom marker extraction.
- One scanner means one classification path, one trimming path, and one source of truth for `TaskStatusSymbol`.
- `into_offset_iter()` remains useful for source positions and line tracking, but not for task marker identity.

#### Q99 — Exact Tag Matching From First Principles

**Decision:** Exact tag matching is recommended because task filters are classification boundaries, not search.

- A task filter decides whether a checkbox/list item becomes a `Task` or remains a `Checkbox`.
- Classification boundaries should be predictable and explicit.
- Nested tag matching (`#task/foo` matching `#task`) should be explicitly configured if desired, not implied.
- This decision does not depend on Obsidian behavior; it follows from the domain role of filters.

#### Q100 — Restrict Filters to Tags

**Decision:** Restrict task filters to tags for now; do not support arbitrary string filters.

- Task classification should be based on parsed `Tag` values, not substring search.
- Arbitrary string filters are ambiguous and can match prose by accident.
- If future requirements need more classification inputs, add explicit typed filters then.

#### Q101 — Remove `is_checked` from `ItemFrame`

**Decision:** With `ENABLE_TASKLISTS` dropped, remove `is_checked`; `ItemFrame` only needs `task_status: Option<TaskStatusSymbol>`.

```rust
struct ItemFrame {
    task_status: Option<TaskStatusSymbol>,
}
```

- Completion is derived by resolving `task_status` through `TaskStatusMap`, not by storing a pulldown-cmark boolean.
- Unknown symbols preserve raw status and behave as incomplete TODOs (Q107).
- This supersedes Q90 and Q84's two-field `ItemFrame` design.

---

### Round 18

#### Q103 — Multiple Tag Filters

**Decision:** Multiple tag filters are allowed.

- `TaskConfig` stores `tag_filters: Vec<Tag>`.
- An item matches the task filter if any configured tag filter exactly matches any tag on the item.
- Empty `tag_filters` means no filter configured, so all status-marked items become tasks.

#### Q104 — Match Any Tag on the Item

**Decision:** Check any tag on the list item, not just the first tag.

- Task classification scans the full `Vec<Tag>` extracted from the list item.
- A task tag can appear anywhere in the item text.
- This prevents classification from depending on tag order.

#### Q105 — Exact Match Only

**Decision:** Exact match only; no nested matching for task classification.

- `#task` matches `#task`.
- `#task` does not match `#task/project` unless `task/project` or `#task/project` is also configured.
- Nested matching can be added later as an explicit config option if needed.

#### Q106 — `ItemFrame` Field Name

**Decision:** Field name is `task_status: Option<TaskStatusSymbol>`.

- Use `task_status`, not `status_symbol`, in parser frame sketches.
- The field stores the raw status symbol extracted from the marker.
- Resolution to a full `TaskStatus` happens through `TaskStatusMap`.

#### Q107 — Unknown Status Symbols

**Decision:** Unknown status symbols preserve raw status for diagnostics but behave as incomplete TODOs.

- Unknown markers such as `[?]` are still recognized as task/checklist markers.
- Preserve `TaskStatusSymbol` so diagnostics can report the source status.
- Fallback behavior: `TaskStatusType::Todo`, incomplete.
- Unknown status symbols are never downgraded to `Plain`.

---

### Round 19

#### Q108 — `TaskConfig` Tag Filters

**Decision:** `TaskConfig` uses `tag_filters: Vec<Tag>`.

```rust
pub struct TaskConfig {
    statuses: TaskStatusMap,
    tag_filters: Vec<Tag>,
}
```

- `tag_filters` replaces the earlier single `tag_filter: Option<Tag>` / string `filter` sketches.
- Empty vector means no tag filter is configured.
- Internal `Tag` values include the leading `#`.

#### Q109 — User-Friendly Config Entries

**Decision:** Config entries should be user-friendly and may omit `#`; parsing normalizes each tag filter entry before validating/constructing `Tag`.

```toml
[tasks]
tag_filters = ["task", "todo"]
```

- Users can write `"task"` or `"#task"`.
- Config parsing normalizes by adding `#` if absent before constructing `Tag`.
- Exact matching compares normalized `Tag`s.
- Internal `Tag` values still include the leading `#`.

#### Q110 — Config Field Name

**Decision:** Config field name is `tag_filters`.

- Use `tag_filters`, not `filter` or `tag_filter`.
- The plural name reflects Q103 multiple filter support.

#### Q111 — No Single-Filter Sugar

**Decision:** No single-filter sugar; only `tag_filters = [...]`.

- Do not support `tag_filter = "task"`.
- One config shape is enough and avoids migration/precedence rules.

#### Q112 — `Tag::is_exact_match(&Tag)`

**Decision:** Add `Tag::is_exact_match(&Tag)` convenience method, similar spirit to `FieldKey` helpers.

- Exact matching compares normalized `Tag` values.
- It should be a small convenience wrapper, not a new matching policy system.
- Nested matching remains out of scope for task classification.

#### Q113 — Config Tag Filter Entry Normalization

**Decision:** Config tag filter entries accept both `"task"` and `"#task"`; both normalize internally to `Tag("#task")`.

- User-facing config can stay concise: `tag_filters = ["task", "todo"]`.
- Leading-`#` entries are also accepted for users who think in markdown tag syntax.
- Internal `Tag` values still include the leading `#`.

#### Q114 — Invalid Config Tag Filters

**Decision:** After normalization, invalid entries fail config loading with the config location.

- Invalid examples include empty strings, whitespace-only strings, malformed tags, and entries that do not construct a valid `Tag`.
- The error should identify the offending `tag_filters` entry and its config location.
- Do not silently drop or coerce invalid entries.

#### Q115 — No Final Design Summary Yet

**Decision:** Do not create a final design summary yet; finalize this grilling session document and prepare it for later `/to-spec` conversion.

- This document should be reviewed for inconsistencies first.
- Later, run `/to-spec` to create `spec.md` from the finalized grilling session.

---

## Status Symbol Parsing Detail

### pulldown-cmark Event Mapping

pulldown-cmark's `Event::TaskListMarker(bool)` only provides:
- `true` if checkbox is checked (`[x]`, `[X]`)
- `false` if checkbox is unchecked (`[ ]`)
- Byte range covering the recognized `[ ]`, `[x]`, or `[X]` marker

pulldown-cmark does **not** emit `Event::TaskListMarker` for custom statuses such as `[/]`, `[-]`, or `[!]`. Those markers appear as normal `Event::Text` content.

Final decision from Q98: do not enable pulldown-cmark task list parsing for this feature. Treat both standard and custom task markers as item-leading text and parse them with one custom marker scanner.

### Extraction Strategy

Use one extraction path: run the custom marker scanner against item-leading text for every list item.

Custom marker scanner contract:

```text
run on item-leading text
require '[' at the item-leading marker position
accept any single non-']' character
require ']'
require trailing whitespace
trim marker prefix exactly once
preserve unknown symbols for diagnostics/fallback
```

Because `ENABLE_TASKLISTS` is not enabled, `Event::TaskListMarker` should not fire for task classification. The scanner is the single source of truth.

### TaskStatusMap Lookup

The extracted character is mapped via `TaskStatusMap`:
- `' '` → TaskStatus { symbol: "[ ]", name: "Todo", kind: Todo }
- `'x'` → TaskStatus { symbol: "[x]", name: "Done", kind: Done }
- `'X'` → TaskStatus { symbol: "[X]", name: "Done", kind: Done }
- `'/'` → TaskStatus { symbol: "[/]", name: "In Progress", kind: InProgress }
- `'-'` → TaskStatus { symbol: "[-]", name: "Cancelled", kind: Cancelled }
- `'!'` → TaskStatus { symbol: "[!]", name: "On Hold", kind: OnHold }
- `'?'` or any other unknown single-character marker → preserve raw symbol, fallback to `TaskStatusType::Todo`

After marker extraction, trim the leading `[<char>] ` prefix exactly once from the item text buffers used for task/list text. This trim must preserve clean/raw text semantics and must only affect the leading marker, not later bracket text in the item body.

---

## Construction Flow

### Event Sequence

```rust
// 1. Event::Start(Tag::Item) → push new ItemFrame
Event::Start(Tag::Item) => {
    self.frames.push(ItemFrame::new());
}

// 2. Event::Text(text) → scan item-leading text, then append normalized text
Event::Text(text) => {
    if let Some(frame) = self.frames.last_mut() {
        let text = if frame.is_item_leading_text() {
            match scan_task_marker(text.as_ref()) {
                Some(marker) => {
                    frame.task_status = Some(marker.task_status);
                    marker.text_after_prefix
                }
                None => text.as_ref(),
            }
        } else {
            text.as_ref()
        };
        frame.text_buffer.push_str(text);
        frame.scan_buffer.push_str(text);
    }
}

// 3. Event::End(TagEnd::Item) → construct ListItem
Event::End(TagEnd::Item) => {
    let frame = self.frames.pop().unwrap();

    // Extract tags from text_buffer
    let tags = extract_tags(&frame.text_buffer);

    // Extract inline fields from scan_buffer
    let fields = extract_inline_fields(&frame.scan_buffer);

    // Determine ListItemType
    let kind = match (frame.task_status, self.config.task.tag_filters.is_empty()) {
        (Some(_task_status), true) => {
            // Has status marker AND no filters → Task (Q91/Q103)
            let task_item = TaskListItem::from_frame(&frame, &self.status_map);
            ListItemType::Task(task_item)
        }
        (Some(_task_status), false) if tags_match_any_filter(&tags, &self.config.task.tag_filters) => {
            // Has status marker AND any item tag exactly matches any configured filter → Task
            let task_item = TaskListItem::from_frame(&frame, &self.status_map);
            ListItemType::Task(task_item)
        }
        (Some(task_status), false) => {
            // Has status marker but no exact tag filter match → Checkbox
            let completion = self.status_map.completion_or_incomplete(task_status);
            ListItemType::Checkbox(completion)
        }
        (None, _) => {
            // No status char → Plain
            ListItemType::Plain
        }
    };

    // If no filters configured AND has task_status → Task (Q91/Q93/Q103)
    // (handled by the first arm)

    let item = ListItem {
        text: ListText {
            raw: frame.text_buffer,
            clean: compute_clean_text(&frame.text_buffer, &self.config.task),
        },
        kind,
        children: frame.children,
        fields,
        tags,
        parent: frame.parent_line,
        line: frame.line,
        depth: frame.depth,
    };

    // Add to parent or root
    // ...
}
```

### No Filter → All Tasks

When `tag_filters` is empty:
- All status-marked items become `ListItemType::Task`
- This matches Obsidian Tasks plugin behavior
- The first arm `(Some(_task_status), true)` covers this case

---

## ListItemType Rules (Final)

| Condition | Result |
|-----------|--------|
| `has_task_status` AND any item tag exactly matches any configured tag filter | `ListItemType::Task(TaskListItem)` |
| `has_task_status` AND no item tag exactly matches any configured tag filter | `ListItemType::Checkbox(TaskCompletionStatus)` |
| `no_task_status` | `ListItemType::Plain` |
| `has_task_status` AND no filters configured | `ListItemType::Task(TaskListItem)` |

`has_task_status` includes standard `[ ]`/`[x]`/`[X]` markers, configured custom statuses, and unknown single-character markers found by the custom scanner.

Tag matching rules:

- Multiple `tag_filters` are allowed.
- Check every tag on the list item, not just the first tag.
- Exact match only: `#task` does not match `#task/project` unless `task/project` or `#task/project` is also configured.
- Internal `Tag` values include the leading `#`; config parsing accepts entries with or without `#`, normalizes before constructing `Tag`, and exact matching compares normalized `Tag` values.

---

## Open Questions for Next Session

1. ~~**Cost of adding line/depth to ListItem struct**~~ — Resolved (Q59): negligible ~8 bytes
2. ~~**Whether ListView is still needed**~~ — Resolved: do not include initially; add only if implementation proves a need
3. ~~**Full config schema for [tasks] section**~~ — Resolved: `NoteConfigSpec` composition
4. ~~**How parser config is threaded through the codebase**~~ — Resolved: `NoteConfigSpec`
5. ~~**Task annotation pipeline**~~ — Resolved (Q47): during parsing
6. ~~**Field inheritance**~~ — Resolved (Q62): Full inheritance model
7. ~~**Global filter and task detection**~~ — Resolved (Q71/Q74/Q98): parser classifies `ListItemType` at construction time with custom scanner + exact tag filters
8. ~~**Tag module location**~~ — Resolved (Q68/Q70): `src/tag.rs`
9. ~~**ListRecord flattening**~~ — Resolved (Q69): flattened for LISTS multimap
10. ~~**Note.tasks() filtering**~~ — Resolved (Q71): must be filtered
11. ~~**RawListItem vs state machine**~~ — Resolved (Q66): current ListTracker is sufficient
12. ~~**ListItemType design**~~ — Resolved (Q74): single enum replacing separate booleans
13. ~~**Note.list_items() naming**~~ — Resolved (Q73): `Note.list_items()` not `Note.all_list_items()`
14. ~~**fully_complete as method**~~ — Resolved (Q76): method on TaskListItem taking &ListItem
15. ~~**ListItemType variant naming**~~ — Resolved (Q77): `Plain` / `Checkbox` / `Task`
16. ~~**matches_filter on ListItem**~~ — Resolved (Q75): not needed, enum discriminant suffices
17. ~~**fully_complete location**~~ — Resolved (Q79): both method on TaskListItem AND attribute on ListRecord
18. ~~**Note.list_items() return type**~~ — Resolved (Q80): `&[ListItem]`
19. ~~**Note.tasks() filtering details**~~ — Resolved (Q81): skips Checkbox, returns only Task items
20. ~~**Task status symbol parsing**~~ — Resolved (Q82): during text extraction via TaskStatusMap
21. ~~**Status character extraction**~~ — Resolved (Q83): store raw char in ItemFrame from byte range
22. ~~**ItemFrame fields**~~ — Resolved (Q84), superseded by Q101/Q106 final naming: `task_status: Option<TaskStatusSymbol>`
23. ~~**Tag filter check timing**~~ — Resolved (Q85): during ListItem construction
24. ~~**No filter configured**~~ — Resolved (Q86): all checkbox items become Task
25. ~~**Final design summary**~~ — Superseded (Q115): do not create a final design summary yet; prepare this document for later `/to-spec` conversion
26. ~~**Custom status scanner exact implementation**~~ — Resolved (Q92/Q94): scan item-leading `Event::Text`, accept any single non-`]` marker, resolve through `TaskStatusMap`
27. ~~**`ItemFrame` status fields**~~ — Resolved (Q90/Q92), superseded by Q101/Q106: keep only `task_status`
28. ~~**No filter with custom statuses**~~ — Resolved (Q91): all checkbox/custom-status items become Task
29. **Task completion / mutation operations** — The entire write-side design (Q7, Q19)
30. ~~**Tag filter exact vs nested matching**~~ — Resolved (Q99/Q105): exact match only; nested matching requires explicit configuration
31. ~~**`ENABLE_TASKLISTS` parser option**~~ — Resolved (Q98): drop it and rely only on the custom scanner

---

## Config Structure Summary

```toml
[tasks]
tag_filters = ["task", "todo"]

[[tasks.statuses]]
symbol = "[ ]"
name = "Todo"
type = "TODO"

[[tasks.statuses]]
symbol = "[/]"
name = "In Progress"
type = "IN_PROGRESS"

[[tasks.statuses]]
symbol = "[x]"
name = "Done"
type = "DONE"

[[tasks.statuses]]
symbol = "[-]"
name = "Cancelled"
type = "CANCELLED"

[[tasks.statuses]]
symbol = "[!]"
name = "On Hold"
type = "ON_HOLD"

[frontmatter]
title = "title"
aliases = "aliases"

[frontmatter.date_created]
name = "date_created"
format = "%Y-%m-%dT%H:%M:%S"

[frontmatter.date_modified]
name = "date_modified"
format = "%Y-%m-%dT%H:%M:%S"
```

Maps to `NoteConfigSpec { task: TaskConfig, frontmatter: FrontmatterConfig }`.

`tag_filters` entries are user-friendly and may omit `#`; entries like `"#task"` are also accepted. Config parsing normalizes each entry by adding `#` if absent, then validates/constructs a `Tag`. Invalid entries fail config loading with the offending config location. Internal `Tag` values still include the leading `#`; exact matching compares normalized `Tag`s.

---

## Key Design Patterns

1. **ListItem as primary type** — All common fields on `ListItem`, task-specific data in `Option<TaskListItem>`
2. **Dataview pattern** — Composition over inheritance for task data
3. **Two-phase parsing** — Structural (`pulldown-cmark`) → semantic (task annotation)
4. **Index-time computation** — `fully_complete` computed during indexing
5. **NoteConfigSpec composition** — Combines task and frontmatter config
6. **ByteTracker utility** — Zero-copy line number tracking via `line_starts` vec
7. **Field inheritance** — Item fields inherit from note fields (frontmatter + inline)
8. **State machine parsing** — Internal state machine for task markers (zero-allocation)
9. **Tag as config type** — Move to `src/tag.rs`, make public with validated constructor
10. **Note.tasks() filtered** — Global filter must be accessible at Note level
11. **ListItemType enum** — Single enum (`Plain`/`Checkbox`/`Task`) replacing `is_task`, `is_completed`, `task` fields
12. **Filter at construction** — Parser receives `tag_filters` via `NoteConfigSpec`, marks items during parse
13. **Escape hatch via Note.list_items()** — Unfiltered access to all list items when needed (`&[ListItem]`)
14. **fully_complete dual form** — Method on `TaskListItem(&self, &ListItem)` (on-demand, O(subtree)) + attribute on `ListRecord` (indexed, O(1))
15. **Enum discriminant as filter** — `ListItemType::Task` implies matched filter; no separate `matches_filter` boolean
16. **Status parsing during parse** — One custom marker scanner handles standard, custom, and unknown markers from item-leading text, then maps via `TaskStatusMap` or fallback
17. **ItemFrame task_status** — Raw status symbol stored in `ItemFrame.task_status` from the custom marker scanner
18. **Tag filters determine ListItemType** — `has_task_status && any exact tag filter match → Task`, `has_task_status && no exact tag filter match → Checkbox`, `no_task_status → Plain`
19. **No filter = all tasks** — When `tag_filters` is empty, all status-marked items become `ListItemType::Task` (Obsidian Tasks behavior)
20. **Text marker extraction** — Task markers are found from item-leading text by accepting `[<single non-]>]` followed by whitespace
21. **Single marker source** — `ENABLE_TASKLISTS` is dropped, so the custom scanner is the only task marker classifier
22. **Marker prefix trimming** — Marker extraction trims the leading `[<char>] ` prefix exactly once while preserving clean/raw text semantics
23. **Unknown marker fallback** — Unknown single-character markers such as `[?]` are never downgraded to `Plain`; classify by filter rules and preserve the raw symbol with fallback `TaskStatusType::Todo`
24. **Priority absence preserved** — `TaskListItem.priority` is `Option<TaskPriority>`; missing priority stays absent

---

## Affected Files (Estimated)

| Area | Files |
|------|-------|
| Parsing | `src/note/parser.rs` (state machine for task markers, ItemFrame with `task_status`, custom item-leading text scanner, byte offsets for line tracking only), `src/note/lists.rs`, `src/note/model.rs` |
| New Task Module | `src/task.rs` (or `src/tasks/` if large) |
| Tag Module | `src/tag.rs` (moved from `src/note/tag.rs`) |
| Indexing | `src/index/store.rs` (LISTS table) |
| CLI | `src/cli/task.rs`, `src/cli/mod.rs` |
| Template Engine | `src/template/engine/query.rs` |
| Config | `src/config/model.rs` (TaskConfig addition) |
| Domain Model | CONTEXT.md (Task term definition) |
| LISTS table | New redb table definition |
| ByteTracker | Byte-to-line conversion utility (new) |
| NoteConfigSpec | Replaces MarkdownParserInput |
| ListItemType | New enum `Plain`/`Checkbox`/`Task` in `src/note/lists.rs` (replaces is_task, is_completed, task fields) |
| Note.tasks() | Filtered iterator in `src/note/model.rs` (returns only `Task` items) |
| Note.list_items() | Escape hatch method in `src/note/model.rs` (returns `&[ListItem]`) |
| TaskListItem | `fully_complete` method taking `&ListItem` reference (in `src/note/lists.rs` or `src/task.rs`) |
| ListRecord | `fully_complete: bool` attribute computed at index time |
| ItemFrame | Parser state struct with `task_status: Option<TaskStatusSymbol>` (in `src/note/parser.rs`) |
| Construction flow | Event-driven: Start(Item) → item-leading Text custom marker scan → End(Item) → construct ListItem with ListItemType |

---

## New Type Definitions

### ListItemType enum (finalized — replaces is_task, is_completed, task fields)

```rust
pub enum ListItemType {
    /// Plain bullet item, no checkbox
    Plain,
    /// Checkbox item that doesn't match global filter
    Checkbox(TaskCompletionStatus),
    /// Checkbox item that matches global filter, has task data
    Task(TaskListItem),
}
```

- `Plain`: unit variant, no data
- `Checkbox(TaskCompletionStatus)`: holds Incomplete/Complete, filter not matched
- `Task(TaskListItem)`: holds full task data, filter matched
- Naming alternatives discussed: `Regular`/`Checklist`/`Task`, `Bullet`/`Checkbox`/`Task`, `ListItem`/`CheckItem`/`TaskItem`
- Final naming confirmed: `Plain` / `Checkbox` / `Task` (Q77)

### ListItem (primary type — updated)

```rust
pub struct ListItem {
    pub text: ListText,
    pub kind: ListItemType,  // replaces is_task, is_completed, task
    pub children: Vec<List>,
    pub fields: Vec<InlineField>,
    pub tags: Vec<Tag>,
    pub parent: Option<SourceLine>,
    pub line: SourceLine,
    pub depth: u8,
}
```

- `kind: ListItemType` replaces `is_task`, `is_completed`, and `task` fields
- `ListItemType::Task(_)` implies item matches global filter (no separate `matches_filter` boolean)
- `ListItemType::Checkbox(_)` implies item has checkbox but doesn't match filter
- `ListItemType::Plain` is a bullet item with no checkbox

### TaskListItem (composed within ListItem)

```rust
pub struct TaskListItem {
    pub status: TaskStatus,
    pub priority: Option<TaskPriority>,
    pub dates: TaskDates,
}

impl TaskListItem {
    /// Check if this task and all its children are complete.
    /// Takes &ListItem to access children. O(subtree), not O(full tree).
    pub fn fully_complete(&self, item: &ListItem) -> bool {
        if !self.is_complete() {
            return false;
        }
        item.children.iter().all(|list| {
            list.items().iter().all(|child| match &child.kind {
                ListItemType::Task(t) => t.fully_complete(child),
                ListItemType::Checkbox(_) | ListItemType::Plain => true,
            })
        })
    }
}
```

- `fully_complete` is a method taking `&ListItem` to access children
- O(subtree) — only walks the direct children, not the full tree
- Computed at index time, result stored on `ListRecord.fully_complete`
- Also available as attribute on `ListRecord` for indexed O(1) queries (Q79)
- `priority: Option<TaskPriority>` preserves missing priority as absent (Q97)

### ListText

```rust
pub struct ListText {
    pub raw: String,
    pub clean: String,
}
```

### TaskStatus (struct, not enum)

```rust
pub struct TaskStatus {
    pub symbol: TaskStatusSymbol,
    pub name: String,
    pub kind: TaskStatusType,
}
```

### TaskCompletionStatus (renamed from TaskStatus)

```rust
pub enum TaskCompletionStatus {
    Incomplete,
    Complete,
}
```

### TaskStatusSymbol (newtype)

```rust
pub struct TaskStatusSymbol(char);
```

- Represents the marker character inside `[<char>]`, not the full bracketed marker string.
- Examples: `' '`, `'x'`, `'X'`, `'/'`, `'-'`, `'!'`.
- Used as the configured lookup key for both pulldown-cmark-recognized markers and custom-scanned markers.
- Unknown single-character markers are preserved for diagnostics/fallback instead of being treated as plain text.

### TaskStatusType

```rust
pub enum TaskStatusType {
    Todo,
    InProgress,
    OnHold,
    Done,
    Cancelled,
    NonTask,
}
```

### TaskPriority

```rust
pub enum TaskPriority {
    Lowest,
    Low,
    Normal,
    Medium,
    High,
    Highest,
}
```

### TaskDates

```rust
pub struct TaskDates {
    pub created: Option<FieldValue>,
    pub scheduled: Option<FieldValue>,
    pub start: Option<FieldValue>,
    pub due: Option<FieldValue>,
    pub done: Option<FieldValue>,
    pub cancelled: Option<FieldValue>,
}
```

### SourceLine (newtype)

```rust
pub struct SourceLine(u32);
```

### ByteOffset (newtype)

```rust
pub struct ByteOffset(usize);
```

### Span (newtype)

```rust
pub struct Span {
    pub start: ByteOffset,
    pub end: ByteOffset,
}
```

### ByteTracker

```rust
pub struct ByteTracker {
    line_starts: Vec<usize>,
}

impl ByteTracker {
    pub fn new(source: &str) -> Self { /* build line_starts from source */ }
    pub fn byte_to_line(&self, offset: usize) -> SourceLine { /* partition_point O(1) */ }
}
```

### ListRecord (for LISTS table — flattened)

```rust
pub struct ListRecord {
    pub path: PathBuf,
    pub text: ListText,
    pub kind: ListItemType,  // replaces is_task, is_completed
    pub tags: Vec<Tag>,
    pub parent: Option<SourceLine>,
    pub line: SourceLine,
    pub depth: u8,
    pub fully_complete: bool,  // computed during indexing via TaskListItem::fully_complete()
}
```

- Flattened for LISTS multimap — all fields at top level
- No nested `ListItem` in `ListRecord`
- `fully_complete` computed at index time via `TaskListItem::fully_complete(&self, item: &ListItem)`
- Dual form: method for on-demand (O(subtree)), attribute for indexed queries (O(1)) (Q79)

### NoteConfigSpec (renamed from MarkdownParserInput)

```rust
pub struct NoteConfigSpec {
    pub task: TaskConfig,
    pub frontmatter: FrontmatterConfig,
}
```

### TaskConfig

```rust
pub struct TaskConfig {
    statuses: TaskStatusMap,
    tag_filters: Vec<Tag>,
}
```

- Empty `tag_filters` means no task filter is configured.
- Config parsing accepts entries like `"task"` and `"#task"`, normalizes them to internal `Tag` values that include `#`, and rejects invalid entries with config location.

### Note (updated)

```rust
impl Note {
    /// Returns filtered task items (matches global tag filter).
    /// Skips Plain and Checkbox items, returns only Task items.
    pub fn tasks(&self) -> impl Iterator<Item = &ListItem> {
        self.items.iter().filter(|item| {
            matches!(item.kind, ListItemType::Task(_))
        })
    }

    /// Returns all list items unfiltered (escape hatch).
    /// Includes Plain, Checkbox, and Task items.
    pub fn list_items(&self) -> &[ListItem] {
        &self.items
    }
}
```

- `tasks()`: filtered iterator, only `ListItemType::Task(_)` items (Q81)
- `list_items()`: returns `&[ListItem]`, all items unfiltered (Q80)
- Filtering happens at construction time via parser (Q72)

### ItemFrame (updated — parser state machine)

```rust
pub struct ItemFrame {
    /// Status symbol extracted by the custom marker scanner
    task_status: Option<TaskStatusSymbol>,
    /// Accumulated text content
    text_buffer: String,
    /// Scan buffer for inline fields
    scan_buffer: String,
    /// Parent line number
    parent_line: Option<SourceLine>,
    /// Line number of this item
    line: SourceLine,
    /// Nesting depth
    depth: u8,
    /// Child lists
    children: Vec<List>,
}
```

- `task_status` — from the custom marker scanner for standard, custom, and unknown statuses
- `is_checked` is not needed because `ENABLE_TASKLISTS` is dropped and completion is derived from `TaskStatusMap`
- Custom scanner must trim the leading `[<char>] ` prefix exactly once after extraction without corrupting `ListText.raw` / `ListText.clean` semantics
- Unknown symbols are preserved for diagnostics/fallback and behave as incomplete TODOs

### TaskStatusMap

```rust
pub struct TaskStatusMap {
    by_name: HashMap<String, TaskStatus>,
    by_symbol: HashMap<TaskStatusSymbol, TaskStatus>,
    by_type: HashMap<TaskStatusType, Vec<TaskStatus>>,
}
```

---

## Reference Materials (Expanded)

| Source | Key Takeaways |
|--------|---------------|
| [Obsidian Tasks Emoji Format](https://publish.obsidian.md/tasks/Reference/Task+Formats/Tasks+Emoji+Format) | Dates (➕⏳🛫📅✅❌), priority (⏬🔽🔼⏫🔺), recurrence (🔁), on-completion (🏁), dependencies (🆔⛔) |
| [Obsidian Tasks Global Filter](https://publish.obsidian.md/tasks/Getting+Started/Global+Filter) | Configurable tag (e.g. `#task`) marks checklist items as tasks; tag stripped from task text |
| [Obsidian Tasks Scripting/Task Properties](https://publish.obsidian.md/tasks/Scripting/Task+Properties) | Properties: status, priority, dates, tags, recurrence, dependsOn, isRecurring, description, subtasks, parent |
| [Dataview metadata-tasks](https://blacksmithgu.github.io/obsidian-dataview/annotation/metadata-tasks) | Tasks parsed from markdown lists with checkboxes; accessible via `task.fieldName`; fields: status, completed, subtasks, text, line, lineCount, section, tags, outlinks, list, blockId |
| redb | TableDefinition with composite tuple keys, postcard serialization |
| pulldown-cmark | `into_offset_iter()` for byte offsets, `line_starts` vec with `partition_point` for O(1) line lookup |

---

## Next Steps

1. Review this grilling session document for inconsistencies.
2. Use `/to-spec` to create `spec.md` from the finalized grilling session.
3. Resolve remaining open questions — mutation operations (Q7, Q19).

---

## Spec-Readiness Review

The document has been cleaned up so superseded decisions are marked as such, but a few design details still need explicit resolution before `/to-spec` to avoid encoding ambiguity into the spec.

### 1. `ListRecord` Flattening vs `ListItemType`

The final design says `ListRecord` is flattened for the LISTS table, but the current type sketch still includes `kind: ListItemType`, which embeds `TaskListItem` for task rows. The spec should decide whether this is flat enough, or whether `ListRecord` needs a persistence-specific enum/fields such as `item_kind`, `completion`, `status`, `priority`, and `dates` at top level.

### 2. `Note.list_items() -> &[ListItem]` Requires Storage Shape

The final design says `Note.list_items()` returns `&[ListItem]`, but the current `Note` stores nested `Vec<List>`. Returning a slice of all list items requires either storing a flattened list-item vector on `Note` or changing the accessor to return an iterator. The spec should choose one.

### 3. `ListText.raw` Needs a Precise Contract

The final design says the custom marker scanner trims the leading `[<char>] ` prefix before text buffers are stored, while `ListText.raw` remains "source-like" but not byte-exact. The spec should define whether `raw` includes task metadata like dates, priority emojis, inline fields, and tag filters, and whether it excludes only the task marker.

### 4. Completion Mapping Needs a Table

The final design derives `TaskCompletionStatus` from resolved `TaskStatusType`, but the exact mapping should be written down. Expected mapping: `Done` and `Cancelled` are `Complete`; `Todo`, `InProgress`, `OnHold`, `NonTask`, and unknown fallback statuses are `Incomplete`, unless explicitly decided otherwise.

### 5. `fully_complete` Child Semantics Need One Sentence

The final design's `fully_complete` method treats `Plain` and `Checkbox` children as complete/ignored and recurses only into `Task` children. The spec should explicitly state whether non-task checklist children are ignored for task completion.

### 6. `TaskStatusMap.by_name` Normalization

The final design includes lookup by status name, but does not specify case sensitivity or canonicalization. The spec should decide whether names are exact strings, case-insensitive, or normalized through the same canonical form as field keys.
