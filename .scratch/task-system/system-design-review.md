# Architecture Review: Dataview Parity, List/Task Data Models, and Rust-Native Design

**Date**: 2026-09-17  
**Status**: Approved Architecture Review & Redesign Guide  
**Scope**: `src/note/lists.rs`, `src/note/parser/list.rs`,
`src/query/results.rs`, `src/query/grammar/field.rs`, `src/query/service.rs`,
`src/query/format.rs`, `src/index/store.rs`, `src/index/entry.rs`,
`src/date.rs`, `docs/digests/`, and `.scratch/task-system/`  

---

## 1. Executive Summary

A direct port of Obsidian Dataview's list and task data model into Rust
introduces severe architectural friction. Dataview was designed around the V8
JavaScript runtime, characterized by dynamic duck-typing (`[key: string]: any`),
cyclic references (`children: SListItem[]`, `parent?: number`), garbage
collection, and runtime DOM re-nesting (`nestItems`).

Porting these conventions directly to Rust causes several critical defects:

1. **Memory bloat:** Every single list item allocates multiple heap structures
   (`IndexMap`, two `String`s, and multiple `Box` slices), even for plain bullet
   points.
2. **Cache destruction:** Mutually recursive tree structures (`List ->
   ListItem -> List`) force depth-first pointer chasing and heap-allocated stack
   iterators.
3. **Query churn:** `QueryService::task_rows` clones row handles and allocates
   new owned `String`s for task text, directly breaking the zero-allocation
   sorting architecture established in recent refactors.
4. **Dead persistence:** The redb `LISTS` table is maintained on every note
   write, yet has zero production consumers. Queries read entirely from the
   in-memory `FileIndex`.
5. **Nomenclature confusion:** Attempting to match Dataview's exact property
   names while also supporting Obsidian Tasks emojis creates an inconsistent
   hybrid.
6. **Ticket backlog invalidation:** Tickets 08 through 11 were drafted against
   an invented type (`QueryRecord`), an unread persistence layer, and an obsolete
   `task.<field>` model.

By adopting a Rust-native architecture centered on flat contiguous storage,
borrowed positional indexing, memory compaction, and a unified `list.<field>`
query namespace without legacy aliases, the system achieves full Dataview and
Obsidian Tasks functional parity with zero allocations during query execution
and significantly reduced memory usage.

---

## 2. Upstream Precedent Analysis

Research across `docs/digests/obsidian_blacksmithgu-obsidian-dataview-digest.txt`,
`docs/digests/obsidian_obsidian-tasks-digest.txt`, and
`docs/digests/dataview-sql-precedent-research.md` reveals two distinct
architectural paradigms:

```text
┌───────────────────────────────────┐   ┌───────────────────────────────────┐
│         Obsidian Dataview         │   │          Obsidian Tasks           │
├───────────────────────────────────┤   ├───────────────────────────────────┤
│ • Generic List Item AST           │   │ • Task-centric Domain Model       │
│ • SListItem = SListEntry | STask  │   │ • Class Task extends ListItem     │
│ • Dynamic field bag: [key: string]│   │ • Explicit lifecycle dates (6)    │
│ • Recursive children: SListItem[] │   │ • 6-tier priority enum (emojis)   │
│ • Re-nesting algorithm (nestItems)│   │ • Custom status workflow registry │
│ • 5 date shorthands; no priorities│   │ • Dual serializers: Tasks/Dataview│
└───────────────────────────────────┘   └───────────────────────────────────┘
```

### Dataview: Structural Generalization

- **Core Truth:** A Task is a specialized variant of a List Item
  (`SListItem = SListEntry | STask`). Both share fundamental properties: text,
  line, position, parent, children, tags, and outlinks.
- **Flaws to Avoid:**
  - Dynamic property bags on every list item.
  - Flattening queries only to reconstruct hierarchies later with $O(N)$ map
    algorithms (`nestItems`).
  - Deprecated legacy properties: `subtasks`, `real`, `header`, and mutable
    `visual` strings.
  - Incomplete task support: Dataview only recognizes five date shorthands
    (`due`, `completion`, `created`, `start`, `scheduled`). It lacks task
    priority emojis, cancelled dates, and custom status workflows.

### Obsidian Tasks: Rich Workflow Specialization

- **Core Truth:** Tasks require an explicit workflow model, including a
  structured `StatusRegistry` (`TODO`, `IN_PROGRESS`, `DONE`, `ON_HOLD`,
  `CANCELLED`, `NON_TASK`), a 6-tier priority enum, and complete lifecycle
  dates (`created`, `start`, `scheduled`, `due`, `done`, `cancelled`).
- **Flaws to Avoid:**
  - Isolating tasks from list items: treating tasks as a completely separate
    line-based system prevents querying mixed outlines or generic checklist
    items.

### Synthesis for Traces

- **Structural Model (from Dataview):** A Task is a subset of a List Item. All
  items share the same structural representation and query interface.
- **Workflow Model (from Tasks):** Statuses follow an explicit state machine;
  priorities and dates are strongly typed.
- **Query Surface:** Standardize on `list.<field>` as the single, canonical
  namespace. Do not alias `task.<field>`.

---

## 3. Current Codebase State & Recent Evolution

Recent updates across `src/` establish patterns that the list and task subsystem
must align with:

- **Unified Date System (`src/date.rs`):** `DateValue` wraps `NaiveDate` in a
  transparent, 4-byte, `Copy` newtype with ISO-8601 enforcement. `DateTimeValue`
  provides a 12-byte `Copy` UTC timestamp.
- **Typed Values & Borrowed Projections (`src/note/field.rs`):** `NoteFieldValue`
  provides 10 strongly typed variants. `NoteFieldValueRef<'a>` enables
  zero-allocation borrowed views.
- **Zero-Allocation Sorting (`src/query/sort.rs`):** `SortKey<'a>` and
  `SortKeys<'a>` project sort terms without heap allocations by borrowing string
  slices directly.
- **Positional Query Rows (`src/query/results.rs`):** `QueryRow` holds an
  `Arc<FileIndex>` and a `RowIndex`, resolving file metadata via $O(1)$ index
  lookups.
- **In-Memory Store (`src/index/`):** `FileIndex` holds all indexed files in
  memory as `Box<[FileEntry]>`, assembled from the `NOTES`, `FILES`, and `LINKS`
  tables.

---

## 4. Critical Findings & Flaws in the Current Plan

### Finding 1: The `LISTS` redb table is dead code causing write amplification

In `src/index/store.rs`, the `LISTS` table is defined, written during cold
indexing, and synchronized on every note update. However, **no query or
production path reads from `LISTS`**.

- `QueryService::task_rows` iterates `note.tasks()`, walking in-memory
  `Note.lists` within `FileIndex`.
- `read_lists`, `read_lists_for_path`, and `read_all_lists` are invoked
  exclusively by their own tests.
- Every note update incurs 100% redundant write amplification: updating a note
  with 200 items requires 200 separate redb key-value operations, even though
  the note and its lists are already serialized inside `NOTES`.
- **Governance Violation:** ADR 0005 (accepted 2026-07-27) explicitly specified
  two tables: `file_records` and `note_metadata`. The grilling session confirmed
  lists and tasks belong inside `note_metadata`. Issue 07 introduced the third
  table without an ADR amendment.
- **Decision:** Applying the Deletion Test from `codebase-design`: deleting
  `LISTS` breaks zero production code. Revert to ADR 0005's accepted two-table
  design (`FILES` and `NOTES`).

### Finding 2: Structural memory bloat across `ListItem`

`ListItem` currently measures ~220 bytes on 64-bit platforms:

- `text: ListText` holds two `String`s (`raw` and `clean`) = 48 bytes.
- `fields: IndexMap<FieldKey, Box<[NoteFieldValue]>>` = 56 bytes.
- `tags: Box<[Tag]>` = 16 bytes.
- `children: Box<[List]>` = 16 bytes.
- `kind: ListItemType` = ~72 bytes.
- `position: ListItemPosition` = 12 bytes.

In a vault with 50,000 list items, more than 95% of items have no inline fields,
no subtasks, and no task markers (`raw == clean`). The system allocates ~11 MB
of struct overhead and over 200,000 distinct heap buffers for empty collections
and duplicate strings.

Empirical testing documented in earlier reviews showed that serializing nested
trees directly caused an $O(\text{depth}^2)$ storage blowup on disk (3.2x byte
inflation on a 6-item chain), which required introducing
`without_children()` as a mitigation.

### Finding 3: Query pipeline allocation churn breaks Phase 1 sorting guarantees

`QueryService::task_rows` constructs query rows by iterating:

```rust
for item in note.tasks() {
    out.push(base.clone().with_task_item(item));
}
```

`with_task_item` allocates an owned `String` via `item.clean_text().to_owned()`
and clones `TaskStatus`. Phase 1 went to great lengths to make `SortKey`
zero-allocation by borrowing string slices. Creating owned strings on every row
during row expansion defeats this optimization.

### Finding 4: Custom status markers are erased during task list rendering

In `src/query/format.rs:145-149`, `render_task_list` hardcodes output formatting
based on `task_completed()`:

```rust
out.push_str(match row.task_completed() {
    Some(true) => "- [x] ",
    Some(false) => "- [ ] ",
    None => "- [-] ",
});
```

Any custom status with `completed() == Some(false)` (such as `[/]` for In
Progress or `[!]` for On Hold) is rendered as `- [ ] `. Unknown single-character
markers (`[?]`) are also rendered as `- [ ] `. This directly violates User
Stories 2 and 4 from `spec.md`, which require preserving custom task marker
symbols.

### Finding 5: Temporal domain type disconnect

`src/date.rs` established `DateValue` as the canonical 4-byte `Copy` date type.
However, `TaskDates` in `src/note/lists.rs` was implemented using
`chrono::NaiveDate`. The parser parses candidate strings with
`DateValue::parse_iso`, converts them to `NaiveDate` via `into_inner()`, and
query resolution later converts `NaiveDate` back into `DateValue` to construct
`NoteFieldValueRef::Date`.

### Finding 6: The four-way "list" naming collision

The term "list" is heavily overloaded across the existing codebase:

1. **CLI:** `traces list` (`src/cli/list.rs`) queries *notes/pages*, not list
   items.
2. **Template filter:** `| list` (`src/template/engine/query.rs`) renders any
   result set as a Markdown bullet list.
3. **Domain model:** `List` / `ListItem` (`src/note/lists.rs`) represents the
   parsed syntax tree.
4. **Query surface:** `list.<field>` and `QueryMode::Lists` represent list item
   rows.

Documentation and diagnostics must consistently use the term **"List Item"**
for query rows and accessors (never bare "list") to avoid confusion with
`traces list` (pages) and `| list` (the rendering filter).

### Finding 7: Ticket backlog built on phantom types and obsolete assumptions

Auditing the ticket backlog in `.scratch/task-system/issues/` reveals that
pending tickets cannot be implemented as drafted:

- **Issue 08 (`08-query-record-enrichment.md`):** Targets `QueryRecord` (a type
  that does not exist in the codebase; the actual type is `QueryRow`). Assumes
  rows read from the dead `LISTS` table via `ListRecord`. Restricts fields to
  `task.<field>` on a dedicated `TaskRow`. Claims "backward compatibility" for a
  pre-release feature.
- **Issue 09 (`09-template-tasks-namespace.md`):** Restricts template access to
  the `tasks.*` namespace and `task.<field>` syntax, preventing templates from
  querying generic list items.
- **Issue 10 (`10-cli-enhancements-and-from-expansion.md`):** Adds `--sort` and
  `--table` to `traces task` using `task.*` fields, missing the status-marker
  erasure bug in `render_task_list` and providing no CLI command for non-task
  list items.
- **Issue 11 (`11-integration-tests.md`):** Plans tests asserting persistence
  of the dead `LISTS` table and tests the obsolete `task.<field>` syntax.

---

## 5. Stress-Tested Architectural Decisions

### 1. `parent` Representation: `Option<SourceLine>`

- **Evaluation:** Using `Option<SourceLine>` is strictly superior to a raw `u32`
  slice offset.
- **Semantic Grounding:** In Markdown, lines are the canonical coordinate
  system. Dataview defines `item.parent: number` as the parent item's source
  line. Users querying `list.parent` expect the source line in their file
  (`list.parent == 14`), not an internal slice index.
- **Zero Overhead:** `SourceLine` is a `NonZeroU32` newtype. Through Rust's
  niche optimization, `Option<SourceLine>` takes exactly 4 bytes, identical to a
  bare `u32`.
- **Fast Traversal:** Items in `Note` are stored in strictly increasing line
  order. Finding a parent from a child's `Option<SourceLine>` is an $O(\log N)$
  binary search across the note's items.

### 2. Canonical Namespace: `list.<field>` (No `task.<field>` Alias)

- **Evaluation:** Dataview was correct in defining tasks as a subset of list
  items. Every task is a list item; not every list item is a task.
- **Unified Engine:** A single `ListRow` model handles all list items. Universal
  properties evaluate on every item; task-specific properties evaluate on tasks
  and return `Null` on plain bullets or non-task checkboxes.
- **No Aliasing:** Reject `task.<field>` as an alias. An alias creates dual
  documentation, duplicate test matrices, and ambiguity in query error
  reporting. Clean cutover to `list.<field>` enforces a consistent conceptual
  model across the entire toolchain.

### 3. Flat Storage in `Note`: `Box<[ListItem]>`

- **Evaluation:** Replace the mutually recursive `List -> ListItem -> List` tree
  with a flat, contiguous `Box<[ListItem]>` on `Note`.
- **Hierarchy Encoding:** Store `depth: u8` and `parent: Option<SourceLine>`
  directly on `ListItem`. Store `is_ordered: bool` on `ListItem` to preserve
  whether an item belongs to an ordered (`1. `) or unordered (`- `) list.
- **Cache Locality & Traversal:** In document order, all descendants of an item
  appear immediately after it with `depth > parent.depth`, ending at the first
  item with `depth <= parent.depth`. Collecting descendants is a contiguous
  slice take (`take_while`), requiring zero recursion and zero heap allocations.
- **Memory Impact:** Consolidates multiple heap allocations into a single array
  allocation per note.

### 4. Zero-Allocation `QueryRow` via Positional Indexing

- **Evaluation:** Replace `RowKind::Task(TaskRow)` with:

  ```rust
  pub(crate) enum RowKind {
      Page,
      List { item_idx: u32 },
  }
  ```

- **$O(1)$ Borrowing:** `QueryRow` accesses its item via:

  ```rust
  self.entry().note().unwrap().list_items()[item_idx as usize]
  ```

- **Zero Row Allocations:** Row construction clones no strings and no status
  structs. When sorting by `list.text`, `SortKey::Text(&'a str)` borrows string
  slices directly from the note in memory.

### 5. Metadata Precedence & Resolution Rules

- **`list.<field>`:** Resolves intrinsic list and task properties.
- **`file.<field>`:** Resolves parent file properties (`file.path`,
  `file.folder`, `file.cdate`, `file.mdate`, `file.size`, `file.tags`).
- **Bare Metadata Keys (`<key>`):**
  - On a `List` row: First inspects the item's own inline fields (`item.fields`).
    If present, returns that value. If absent, falls back to the parent note's
    frontmatter and page-level inline fields.
  - On a `Page` row: Resolves against the note's frontmatter and page-level
    inline fields.
- **Tags Resolution:**
  - `list.tags`: Item-level tags scanned from bullet text.
  - `file.tags`: Note-level tags from frontmatter and note body.
  - Bare `tags`: On list rows, resolves to `list.tags`. On page rows, resolves to
    `file.tags`.

---

## 6. Complete Field Specification: `list.<field>`

The `list.<field>` namespace provides a unified interface across all list items:

| Field Path | Return Type | Applicable To | Description |
|---|---|---|---|
| `list.text` | `String` | All Items | Normalized clean display text (markers, task tags, priority emojis, and dates stripped) |
| `list.raw_text` | `String` | All Items | Source text with only leading list/checkbox marker prefix stripped |
| `list.line` | `Number` | All Items | 1-indexed source line number |
| `list.parent` | `Number \| Null` | All Items | 1-indexed source line of immediate parent item, or null |
| `list.depth` | `Number` | All Items | 0-indexed nesting depth level |
| `list.tags` | `List<Tag>` | All Items | Item-level tags scanned from the item text |
| `list.is_task` | `Bool` | All Items | `true` if item is classified as a task; `false` for checkboxes and plain bullets |
| `list.kind` | `String` | All Items | Item classification: `"plain"`, `"checkbox"`, or `"task"` |
| `list.is_ordered` | `Bool` | All Items | `true` if item is part of a numbered list (`1. `); `false` for bulleted lists |
| `list.status` | `String \| Null` | Tasks Only | Configured display name of task status (`"Todo"`, `"Done"`, `"In Progress"`) |
| `list.status_type` | `String \| Null` | Tasks Only | Status workflow type: `"TODO"`, `"IN_PROGRESS"`, `"ON_HOLD"`, `"DONE"`, `"CANCELLED"`, `"NON_TASK"` |
| `list.status_symbol`| `String \| Null` | Tasks Only | Raw status character inside brackets (`" "`, `"x"`, `"/"`, `"-"`) |
| `list.completed` | `Bool \| Null` | Tasks Only | Tri-state completion: `Some(true)` for Done, `Some(false)` for Todo/InProgress/OnHold, `None` for Cancelled and non-task items |
| `list.priority` | `String \| Null` | Tasks Only | Priority name: `"highest"`, `"high"`, `"medium"`, `"low"`, `"lowest"`, or null |
| `list.due` | `Date \| Null` | Tasks Only | Due date (`📅`/`🗓️` or `[due:: ...]`) |
| `list.done` | `Date \| Null` | Tasks Only | Completion date (`✅` or `[done:: ...]` / `[completion:: ...]`) |
| `list.created` | `Date \| Null` | Tasks Only | Creation date (`➕` or `[created:: ...]`) |
| `list.start` | `Date \| Null` | Tasks Only | Start date (`🛫` or `[start:: ...]`) |
| `list.scheduled` | `Date \| Null` | Tasks Only | Scheduled date (`⏳` or `[scheduled:: ...]`) |
| `list.cancelled` | `Date \| Null` | Tasks Only | Cancellation date (`❌` or `[cancelled:: ...]`) |
| `list.fully_complete`| `Bool \| Null` | Tasks Only | `true` if this task and all descendant tasks in its subtree are resolved |

*Note: All task-specific fields evaluate to `Null` when queried on plain bullets
or non-task checkboxes.*

---

## 7. Actionable Redesign Roadmap & Ticket Overhaul

To implement these architectural decisions cleanly, the remaining issues in
`.scratch/task-system/issues/` must be restructured.

### Overhaul of Existing Issues

#### 1. Issue 07 Amendment: Clean Persistence & Note Model

- **Drop `LISTS` Table:** Remove `LISTS` table definitions, key codecs, and
  synchronization code from `src/index/store.rs` and `src/index/service.rs`.
  Revert database schema to ADR 0005.
- **Flatten List Storage in `Note`:** Replace recursive `Box<[List]>` with a
  flat `Box<[ListItem]>` on `Note`. Store `depth`, `parent: Option<SourceLine>`,
  and `is_ordered: bool` on `ListItem`.
- **Adopt `DateValue`:** Replace `chrono::NaiveDate` in `TaskDates` with
  `crate::DateValue`.
- **Memory Compaction:** Wrap `fields` in `Option<Box<...>>` and make
  `ListText.clean` lazy via `Option<String>`.

#### 2. Issue 08 Replacement: Zero-Allocation `QueryRow` & `list.<field>`

- **Retitle & Target Real Types:** Rename from "Query record enrichment" to
  "Zero-allocation `QueryRow` and `list.<field>` resolution". Target `QueryRow`
  directly.
- **Positional Indexing:** Replace `RowKind::Task(TaskRow)` with
  `RowKind::List { item_idx: u32 }`.
- **Canonical `list.<field>`:** Implement `ListField` in
  `src/query/grammar/field.rs`. Reject `task.<field>`.
- **Metadata Precedence:** Update `QueryRow::resolve_ref` to check `item.fields`
  before falling back to `note.metadata()`.
- **Add `FileField::Tags`:** Add `file.tags` to access note-level tags on list
  rows.

#### 3. Issue 09 Replacement: Template `list.<field>` Pipeline

- **Retitle & Scope:** Update to expose `list.<field>` across both `query` and
  `tasks` template namespaces.
- **Filtering & Sorting:** Enable `where`, `sort`, `limit`, `group_by`, and
  `flatten` over `list.*` fields in templates.

#### 4. Issue 10 Replacement: CLI Enhancements & Output Fidelity

- **Fix Status Marker Output:** Update `render_task_list` in
  `src/query/format.rs` to output the exact `TaskStatusSymbol` (`[{symbol}]`)
  instead of hardcoded `[x]` / `[ ]`. Respect `list.depth` for indentation.
- **CLI Commands:** Add sorting and table columns to `traces task` using
  `list.<field>`. Consider exposing `traces list-items` for unfiltered lists.

#### 5. Issue 11 Replacement: Comprehensive Task System Integration Suite

- **Revise Verification Matrix:** Remove assertions checking the dead `LISTS`
  table.
- **Verify New Invariants:** Assert zero-allocation sorting over list fields,
  canonical `list.<field>` queries, rejection of `task.<field>`, and custom
  marker output fidelity.

---

## 8. Domain Glossary Discipline (`CONTEXT.md`)

To prevent recurring naming confusion, update `src/note/CONTEXT.md` and
`src/query/CONTEXT.md` with strict vocabulary entries:

- **List Item:** The universal Markdown structural primitive, classified as a
  plain bullet, a checkbox, or a task.
- **Task:** A status-marked list item that matches configured task tag filters.
- **Checkbox:** A status-marked list item excluded from task queries by tag
  filters.
- **Plain Bullet:** An unmarked list item.
- **List Item Field:** An accessor under the `list.*` namespace (never refer to
  it as a bare "list field").
- **Page Field:** An accessor under the `file.*` namespace.
- **Fully Complete:** A task whose own status is resolved and whose descendant
  tasks at all depths are resolved.
