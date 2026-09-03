Status: ready-for-agent

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

- [ ] `TaskListItem` struct with `status: TaskStatus` and `fully_complete: bool` fields
- [ ] `TaskListItem::new(status: TaskStatus, fully_complete: bool) -> Self`
- [ ] `TaskListItem::status(&self) -> &TaskStatus`
- [ ] `TaskListItem::fully_complete(&self) -> bool`
- [ ] `ListItemType::Task(TaskListItem)` replaces `ListItemType::Task(TaskStatus)`

### Parser integration

- [ ] `end_item` constructs `TaskListItem::new(status, fully_complete)` instead of bare `Task(status)`
- [ ] `fully_complete` computed in `end_item` after children are built — no second pass
- [ ] Non-task items (`Plain`, `Checkbox`) have no `fully_complete` concept
- [ ] `ListItem::kind()` accessor exists (returns `&ListItemType`) — verify or add

### Fully-complete computation

- [ ] Computation checks the task itself and recursively checks task children only
- [ ] Plain bullet children (`ListItemType::Plain`) ignored for fully_complete
- [ ] Non-task checkbox children (`ListItemType::Checkbox`) ignored for fully_complete
- [ ] Only `ListItemType::Task` descendants participate in the calculation
- [ ] Cancelled status counts as resolved (subtree is resolved when all tasks are done or cancelled)
- [ ] A task with no task children is vacuously `fully_complete = true`
- [ ] Short-circuits on first incomplete descendant (no wasted traversal)

### Downstream updates

All `ListItemType::Task(status)` pattern matches updated to destructure through `TaskListItem`:

- [ ] `src/note/parser/list.rs` — `end_item` construction
- [ ] `src/query/record.rs` — `with_task_item` accesses `task.status()`
- [ ] `src/note/model.rs` — test construction wraps in `TaskListItem`
- [ ] `src/note/lists.rs` — test helper `done_task()` wraps in `TaskListItem`
- [ ] `src/note/parser.rs` — test assertions destructure through `TaskListItem`
- [ ] `src/note/parser/list.rs` — test assertions access `task.status()` and `task.fully_complete()`

### Tests

- [ ] Unit test: single task with no children → fully_complete = true
- [ ] Unit test: parent with all done children → fully_complete = true
- [ ] Unit test: parent with cancelled child → fully_complete = true
- [ ] Unit test: parent with mixed done + cancelled children → fully_complete = true
- [ ] Unit test: parent with incomplete child → fully_complete = false
- [ ] Unit test: parent with plain bullet children ignored (only non-task children → true)
- [ ] Unit test: parent with non-task checkbox children ignored (only non-task children → true)
- [ ] Unit test: deeply nested tasks (3+ levels) all done → fully_complete = true
- [ ] Unit test: deeply nested — intermediate done task has incomplete grandchild → false
- [ ] Unit test: `TaskListItem::status()` and `TaskListItem::fully_complete()` accessors
- [ ] `mise run verify` passes

## Out of scope

- Priority and dates on `TaskListItem` — issue 06 extends the struct
- LISTS persistence and `ListRecord` — issue 07 reads `fully_complete` from `TaskListItem`
- Query record enrichment — issue 08
- Template `tasks.*` namespace changes — issue 09
