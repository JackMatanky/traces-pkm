# 09 — Task-Level Queries

**What to build:** Task-level queries operate on individual tasks instead of pages. The CLI and Template surfaces can query tasks while preserving task status and parent Note metadata.

**Blocked by:** 05 — QueryOutcome Filtering and Ordering

**Status:** ready-for-agent

- [ ] Task-level QueryOutcome values represent individual tasks.
- [ ] Task records expose completion status and task text.
- [ ] Task records retain parent Note metadata for filtering and display.
- [ ] `traces task` runs task-level queries and prints task output.
- [ ] Templates can use a task-level query source.
- [ ] Task query tests prove task-level filtering does not operate on page rows by mistake.
