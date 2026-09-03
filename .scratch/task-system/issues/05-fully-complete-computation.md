Status: implemented

**Date**: 2026-09-03
**Implemented in**: `632cc17`..`0f75ec2`, branch `task-system/05-fully-complete-computation`
(worktree `.worktrees/task-system-05/`)
# 05 — TaskListItem and fully-complete computation

**What to build:** Introduce `TaskListItem` struct to hold task-specific data inside `ListItemType::Task`, replacing the bare `TaskStatus`. Compute `fully_complete` as a precomputed field on `TaskListItem` — a parent task is fully_complete when it and every descendant task (at any depth) has a complete or cancelled status. Plain bullet children and non-task checkbox children are ignored.

**Blocked by:** 02 (needs `ListItemType` to identify task children).

## Why TaskListItem

The grilling session (Q76/Q79) designed `TaskListItem` as the composed data inside `ListItemType::Task(...)`. Issue 01 shipped `ListItemType::Task(TaskStatus)` as a simplification. This creates a structural problem: when issue 06 adds priority and dates, task data would scatter across `ListItem` fields that are meaningless on Plain/Checkbox items. `TaskListItem` co-locates task data with the task variant and provides a natural home for `fully_complete`.

## Key interfaces

- `TaskListItem` struct — carries `status: TaskStatus` and `fully_complete: bool`
- `ListItemType::Task(TaskListItem)` — replaces `ListItemType::Task(TaskStatus)`
- `TaskListItem::new(status, fully_complete) -> Self`
- `TaskListItem::status(&self) -> &TaskStatus`
- `TaskListItem::fully_complete(&self) -> bool`
- `fully_complete` computation — recursive over `Task` descendants only; Plain and Checkbox children ignored; cancelled counts as resolved

## Fully-complete algorithm

The computation runs inside `end_item` after children are built (no second pass). For a given list item being closed:

1. If the item is not a Task (`Plain` or `Checkbox`), skip — no `fully_complete` concept.
2. If the item is a Task, walk its `children: Vec<List>` recursively:
   - For each child list, iterate its `items`.
   - For each child item:
     - If `ListItemType::Plain` or `ListItemType::Checkbox` → skip (ignored for calculation).
     - If `ListItemType::Task(task)`:
       - If `task.status().kind().completed()` returns `Some(false)` (incomplete) → parent is **not** fully_complete, short-circuit return `false`.
       - Otherwise (done or cancelled) → continue.
     - Recurse into the child task's own children.
   - After all children processed without short-circuiting → this subtree is resolved.
3. If no `ListItemType::Task` descendants exist (leaf task or only non-task children) → vacuously `fully_complete = true`.

Cancelled tasks (`TaskStatusType::Cancelled`, `completed() → None`) count as resolved — they are terminal and do not block parent completion.

## Checklist

### TaskListItem struct

- [x] `TaskListItem` struct with `status: TaskStatus` and `fully_complete: bool` fields
- [x] `TaskListItem::new(status: TaskStatus, fully_complete: bool) -> Self`
- [x] `TaskListItem::status(&self) -> &TaskStatus`
- [x] `TaskListItem::is_fully_complete(&self) -> bool`
- [x] `ListItemType::Task(TaskListItem)` replaces `ListItemType::Task(TaskStatus)`

### Parser integration

- [x] `end_item` constructs `TaskListItem::new(status, fully_complete)` instead of bare `Task(status)`
- [x] `fully_complete` computed in `end_item` after children are built — no second pass
- [x] Non-task items (`Plain`, `Checkbox`) have no `fully_complete` concept
- [x] `ListItem::kind()` accessor exists (returns `&ListItemType`) — verify or add

### Fully-complete computation

- [x] Computation checks the task itself and recursively checks task children only
- [x] Plain bullet children (`ListItemType::Plain`) ignored for fully_complete
- [x] Non-task checkbox children (`ListItemType::Checkbox`) ignored for fully_complete
- [x] Only `ListItemType::Task` descendants participate in the calculation
- [x] Cancelled status counts as resolved (subtree is resolved when all tasks are done or cancelled)
- [x] A task with no task children is vacuously `fully_complete = true`
- [x] Short-circuits on first incomplete descendant (no wasted traversal)

### Downstream updates

All `ListItemType::Task(status)` pattern matches updated to destructure through `TaskListItem`:

- [x] `src/note/parser/list.rs` — `end_item` construction
- [x] `src/query/record.rs` — `with_task_item` accesses `task.status()`
- [x] `src/note/model.rs` — test construction wraps in `TaskListItem`
- [x] `src/note/lists.rs` — test helper `done_task()` wraps in `TaskListItem`
- [x] `src/note/parser.rs` — test assertions destructure through `TaskListItem`
- [x] `src/note/parser/list.rs` — test assertions access `task.status()` and `task.fully_complete()`

### Tests

- [x] Unit test: single task with no children → fully_complete = true
- [x] Unit test: parent with all done children → fully_complete = true
- [x] Unit test: parent with cancelled child → fully_complete = true
- [x] Unit test: parent with mixed done + cancelled children → fully_complete = true
- [x] Unit test: parent with incomplete child → fully_complete = false
- [x] Unit test: parent with plain bullet children ignored (only non-task children → true)
- [x] Unit test: parent with non-task checkbox children ignored (only non-task children → true)
- [x] Unit test: deeply nested tasks (3+ levels) all done → fully_complete = true
- [x] Unit test: deeply nested — intermediate done task has incomplete grandchild → false
- [x] Unit test: `TaskListItem::status()` and `TaskListItem::fully_complete()` accessors
- [x] `mise run verify` passes

## Implementation notes

### Where it landed

| File | Purpose |
|---|---|
| `src/note/lists.rs` | `TaskListItem` struct (`status: TaskStatus`, `fully_complete: bool`), `ListItemType::Task(TaskListItem)` payload, accessor methods `status(&self)`, `is_fully_complete(&self)`, and `fully_complete(&self)` alias. Unit tests for construction, accessors, and trait invariants. |
| `src/note/parser/list.rs` | `is_descendant_tree_complete` recursive helper function, `end_item` construction of `TaskListItem::new(status, fully_complete)`, unit tests covering all 10 subtree completion permutations. |
| `src/note/mod.rs` | Public re-export of `TaskListItem`, gated conditional re-export of `TaskIter`. |
| `src/lib.rs` | Root crate re-export of `TaskListItem` under `traces_pkm::TaskListItem`. |
| `src/query/results.rs` | `with_task_item` updated to destructure `ListItemType::Task(task)` and access `task.status()`. |
| `src/note/model.rs` | `task()` test helper updated to construct `TaskListItem`. |
| `src/note/parser.rs` | Test assertions updated to destructure through `TaskListItem`. |

### Key design and algorithmic decisions

1. **Bottom-up $O(\text{children})$ precomputation**:
   Markdown parser list stacks naturally finalize children before their parents in `end_item`. When evaluating a parent `Task`, all direct child `Task` items already have their own `is_fully_complete` state computed and stored. `is_descendant_tree_complete` checks `task.status().kind().completed() != Some(false) && task.is_fully_complete()`, which runs in $O(\text{direct children})$ without repeatedly re-traversing subtrees.

2. **Intermediate non-task nodes**:
   `ListItemType::Plain` and `ListItemType::Checkbox` items carry no completion concept. However, they may contain nested task grandchildren (e.g. a plain bullet point grouping subtasks). `is_descendant_tree_complete` recurses through `item.children()` when encountering plain or checkbox items so that any task descendants at arbitrary depth participate in the calculation.

3. **Terminal cancelled tasks**:
   Cancelled tasks (`TaskStatusType::Cancelled`, where `completed() -> None`) are considered terminal and resolved. `completed() == Some(false)` evaluates to `false` for `None`, allowing cancelled child tasks to satisfy parent completion.

4. **Vacuous completion for leaf tasks**:
   A task with no children or only non-task children with no nested tasks evaluates to `is_fully_complete = true` vacuously (`is_descendant_tree_complete(&[])` returns `true`).

5. **Principle of least privilege & encapsulation**:
   - `TaskListItem` fields (`status: TaskStatus`, `fully_complete: bool`) are private.
   - Read-only access is provided via getters `status(&self) -> &TaskStatus` and `is_fully_complete(&self) -> bool`.
   - `is_descendant_tree_complete` is private (`fn`) in `src/note/parser/list.rs`.
   - `TaskIter` re-export is restricted to `#[cfg(any(test, feature = "test-utils"))]` in `src/note/mod.rs` and `src/lib.rs` to keep production public API surface minimal.

6. **Idiomatic trait implementations and constructors**:
   - `TaskListItem` derives `#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]` (derives `Eq` following `api-common-traits`).
   - `TaskListItem::new` is marked `pub const fn` with `#[inline]` and `#[must_use]`.
   - `is_fully_complete(&self) -> bool` follows the standard `is_` boolean naming convention (`name-is-has-bool`).
   - `fully_complete(&self) -> bool` is provided as an inline alias for exact compatibility with issue specifications and downstream Issue 07 consumers.

7. **Zero-allocation subtree traversal**:
   `is_descendant_tree_complete(&[List]) -> bool` borrows lists and items via slice iteration, performing zero heap allocations.

### Test coverage summary

Unit test suites in `src/note/lists.rs` and `src/note/parser/list.rs` verify:
- Single leaf task with no children (`is_fully_complete = true`)
- Parent task with all done children (`is_fully_complete = true`)
- Parent task with cancelled child (`is_fully_complete = true`)
- Parent task with mixed done + cancelled children (`is_fully_complete = true`)
- Parent task with incomplete todo child (`is_fully_complete = false`)
- Parent task with in-progress child (`is_fully_complete = false`)
- Parent task with unknown marker child (`[?]`) (`is_fully_complete = false`)
- Parent task with plain bullet children ignored (`is_fully_complete = true`)
- Parent task with non-task checkbox children ignored (`is_fully_complete = true`)
- Deeply nested tasks (3+ levels) all done (`is_fully_complete = true` across all levels)
- Deeply nested tasks with incomplete grandchild (`is_fully_complete = false` for parents, `true` for leaf)
- Deeply nested incomplete task under intermediate plain bullet (`is_fully_complete = false` for parent)
- `TaskListItem::new`, `TaskListItem::status()`, `TaskListItem::is_fully_complete()`, and `TaskListItem::fully_complete()` accessors and trait invariants

## Out of scope

- Priority and dates on `TaskListItem` — issue 06 extends the struct
- LISTS persistence and `ListRecord` — issue 07 reads `fully_complete` from `TaskListItem`
- Query record enrichment — issue 08
- Template `tasks.*` namespace changes — issue 09
