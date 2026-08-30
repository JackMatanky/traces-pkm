Status: ready-for-agent

# 02 — Custom marker scanner and list item classification

**What to build:** Replace the `ENABLE_TASKLISTS` pulldown-cmark extension and `set_task_status` method with a custom marker scanner that is the only source of truth for task marker identity. Introduce a `ListItemType` enum (Plain, Checkbox, Task) replacing the current `task_status: Option<TaskStatus>` field on `ListItem`. The scanner recognizes `[x]`, `[X]`, `[/]`, `[-]`, `[!]`, `[?]`, configured custom markers, and unknown single-character markers. `[x]` and `[X]` are equivalent and both map to Done. Unknown markers behave as incomplete todos and are never downgraded to plain bullets. All existing task classification tests are rewritten against the scanner.

**Blocked by:** 01 (needs `TaskStatusMap` for symbol→status resolution).

- [ ] `ListItemType` enum with variants: Plain, Checkbox, Task
- [ ] `ListItem` stores `ListItemType` instead of `task_status: Option<TaskStatus>`
- [ ] Custom scanner recognizes `[` + any single non-`]` character + `]` + whitespace at item-leading position
- [ ] Scanner trims leading marker prefix exactly once, only at item-leading marker position
- [ ] `[x]` and `[X]` both resolve to Done status via `TaskStatusMap`
- [ ] `[/]`, `[-]`, `[!]` resolve to configured default statuses (in-progress, on-hold, non-task)
- [ ] Unknown markers (e.g. `[?]`) preserved and resolved as incomplete todo by default
- [ ] Unknown markers are never downgraded to plain bullets
- [ ] Later bracket text in item body is not trimmed as a task marker
- [ ] Remove `ENABLE_TASKLISTS` from pulldown-cmark options
- [ ] Remove `set_task_status` method and `ItemFrame.task_status` derivation path
- [ ] `Note.tasks()` filters by `ListItemType::Task` instead of `is_task()` / `task_status()`
- [ ] Unit tests for scanner recognizing all marker types
- [ ] Unit tests for marker scanning only accepting item-leading position
- [ ] Unit tests for unknown markers preserved and classified as incomplete todo
- [ ] Unit tests for `[x]`/`[X]` equivalence
- [ ] Rewrite existing task classification tests against new scanner
