Status: ready-for-agent

# 08 — Query record enrichment with task fields

**What to build:** Enrich `QueryRecord` with full task fields so queries can filter, sort, and display by status, priority, dates, tags, parent, depth, line, and fully-complete state. Cancelled tasks map to `None` for `completed`. Task item fields override inherited Note metadata. Old `task.completed` and `task.text` fields remain available for backward compatibility.

**Blocked by:** 07 (needs persisted `ListRecord` fields).

- [ ] `QueryRecord` exposes `task.status` (status name string)
- [ ] `QueryRecord` exposes `task.completed` as `Option<bool>` — `Some(true)` for done, `Some(false)` for incomplete, `None` for cancelled
- [ ] `QueryRecord` exposes `task.priority` (optional enum name)
- [ ] `QueryRecord` exposes `task.due` (optional date)
- [ ] `QueryRecord` exposes `task.tags` (task-level tags)
- [ ] `QueryRecord` exposes `task.parent` (parent line number)
- [ ] `QueryRecord` exposes `task.fully_complete` (boolean)
- [ ] `QueryRecord` exposes `task.line` (source line number)
- [ ] `QueryRecord` exposes `task.depth` (nesting depth)
- [ ] Old `task.completed` and `task.text` fields remain available
- [ ] Task item fields override inherited Note metadata when the item has a non-null value for that field
- [ ] `TaskField` enum extended with new variants for all task fields
- [ ] `TaskField` enum variants: `Status`, `Completed`, `Priority`, `Due`, `Tags`, `Parent`, `FullyComplete`, `Line`, `Depth`
- [ ] Query filters on `completed == false` exclude cancelled tasks
- [ ] Query filters on `completed == true` also exclude cancelled tasks
- [ ] Unit tests for all new task field paths on task rows
- [ ] Unit tests for task fields returning None on page-level records
- [ ] Unit tests for item fields overriding inherited Note metadata
