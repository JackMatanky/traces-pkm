Status: ready-for-agent

# 05 — Fully-complete computation

**What to build:** Compute `fully_complete` on Task list items. A parent task is fully_complete when it and every descendant task (at any depth) has a complete or cancelled status — the subtree is resolved. Plain bullet children and non-task checkbox children are ignored. Stored as a flattened indexed field for O(1) query access.

**Blocked by:** 02 (needs `ListItemType` to identify task children).

- [ ] `ListItem` gains `fully_complete: bool` field
- [ ] Computation checks the task itself and recursively checks task children only
- [ ] Plain bullet children (`ListItemType::Plain`) ignored for fully_complete
- [ ] Non-task checkbox children (`ListItemType::Checkbox`) ignored for fully_complete
- [ ] Only `ListItemType::Task` descendants participate in the calculation
- [ ] Cancelled status counts as resolved (subtree is resolved when all tasks are done or cancelled)
- [ ] Computation runs during list item construction (after children are built)
- [ ] `fully_complete` stored as flattened indexed field for O(1) query access
- [ ] Unit tests: single task with no children
- [ ] Unit tests: parent with all done children → fully_complete = true
- [ ] Unit tests: parent with cancelled child → fully_complete = true
- [ ] Unit tests: parent with incomplete child → fully_complete = false
- [ ] Unit tests: parent with plain bullet children ignored
- [ ] Unit tests: parent with non-task checkbox children ignored
- [ ] Unit tests: deeply nested tasks (3+ levels) all done → fully_complete = true
