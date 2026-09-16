# 09 — Template lists and tasks namespaces

**Status:** ready-for-agent

**What to build:** Expose `lists` (generic list items) and `tasks` (list items
pre-filtered to `list.is_task == true`) globals in MiniJinja templates,
evaluated over the canonical `list.<field>` namespace. Parameterize `QueryOps`
by `QueryMode` (`Pages`, `Lists`, `Tasks`) so templates can query, filter,
sort, and render outlines, checklists, and tasks with full metadata. Replace
`TaskFields` with `ListFields` implementing `minijinja::value::Object`,
exposing all universal and task fields.

**Blocked by:** 08 (needs zero-allocation `QueryRow` and `ListField`).

## Key Interfaces and Models

- **Template Globals:**
  - `lists.from(...)` runs `QueryMode::Lists`, yielding rows for all list items
    (plain bullets, checkboxes, tasks).
  - `tasks.from(...)` runs `QueryMode::Tasks`, yielding rows for list items
    where `list.is_task == true`.
  - Both globals share the identical underlying pipeline and evaluation engine.

- **`QueryOps` Parameterization:**

  ```rust
  pub(super) struct QueryOps {
      name: &'static str,
      root: Arc<Path>,
      service: QueryService,
      mode: QueryMode,
  }
  ```

  `QueryOps::page`, `QueryOps::list`, and `QueryOps::task` construct the
  respective dispatchers.

- **`ListFields` Object Implementation:**
  Replaces `TaskFields` in `src/template/engine/query.rs`. Implements
  `minijinja::value::Object` to project all `list.*` fields into template
  values:
  - Universal fields: `text`, `raw_text`, `line`, `parent`, `depth`, `tags`,
    `is_task`, `kind`, `is_ordered`.
  - Task fields: `status`, `status_type`, `status_symbol`, `completed`,
    `priority`, `due`, `done`, `created`, `start`, `scheduled`, `cancelled`,
    `fully_complete`.
  - Missing or non-applicable task fields on non-task items project to
    MiniJinja's `none` (empty value), matching `NoteFieldValue::Null`.

- **Pipeline Capabilities:**
  All existing transforms (`where`/`filter`, `sort`, `limit`, `group_by`,
  `flatten`) and terminal renderers (`table`, `list`, `task_list`, `count`)
  function across both `lists` and `tasks` streams using `list.<field>` paths.

## Acceptance Criteria

- [ ] Parameterize `QueryOps` by `QueryMode` in `src/template/engine/query.rs`.
- [ ] Register both `lists` and `tasks` globals in the MiniJinja environment.
- [ ] Implement `ListFields` wrapping `Arc<QueryRow>` to expose all universal
  and task fields.
- [ ] Support `lists.from()`, `lists.from_tags()`, `lists.from_folder()`, and
  `lists.from_class()` for querying unfiltered list items.
- [ ] Support `tasks.from()`, `tasks.from_tags()`, `tasks.from_folder()`, and
  `tasks.from_class()` for querying task list items.
- [ ] Enable filtering on `list.*` fields in templates, e.g.,
  `tasks.where("list.status == \"Done\"")` and
  `lists.where("list.is_task == false")`.
- [ ] Enable sorting on `list.*` fields in templates, e.g.,
  `tasks.sort("list.due", true)`.
- [ ] Terminal renderers (`task_list`, `table`, `list`, `count`) execute
  cleanly on list and task query sets.
- [ ] Unit tests for `lists` and `tasks` template evaluation in
  `src/template/engine/query.rs`.
- [ ] Unit tests verifying non-task fields return `none` when queried on plain
  bullets in templates.
- [ ] All checks pass under `mise run verify`.
