Status: ready-for-agent

# 09 — Template tasks namespace field paths

**What to build:** Expose enriched task fields through the template `tasks` namespace so templates can filter, sort, and render task views with full metadata. The existing pipeline (`where`/`filter`, `sort`, `limit`, `group_by`, `flatten`, `table`, `list`, `task_list`, `count`) already works for task rows — this adds richer field paths, not new pipeline mechanics.

**Blocked by:** 08 (needs enriched query records).

- [ ] Template engine exposes `task.status` field path
- [ ] Template engine exposes `task.priority` field path
- [ ] Template engine exposes `task.due` field path
- [ ] Template engine exposes `task.tags` field path
- [ ] Template engine exposes `task.parent` field path
- [ ] Template engine exposes `task.fully_complete` field path
- [ ] Template engine exposes `task.line` field path
- [ ] Template engine exposes `task.depth` field path
- [ ] Old `task.completed` and `task.text` field paths remain available
- [ ] `tasks.from_tags()`, `tasks.from_folder()`, `tasks.from_class()` work with enriched fields
- [ ] `tasks.from_class()` uses transitive is-a matching (Schema Extends)
- [ ] Terminal renderers (`task_list`, `table`, `list`, `count`) work for task rows
- [ ] Integration test: template with `tasks.where(task.status == "Done").table(...)` renders correctly
- [ ] Integration test: template with `tasks.from_class("Project").task_list()` works
