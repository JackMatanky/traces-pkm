# 11 — Task and list system integration test suite

**Status:** ready-for-agent

**What to build:** Implement a focused, high-value integration test suite in
`tests/integration/` verifying the end-to-end task and list system across all
subsystems. Using `test_support::TestProject`, test the public boundaries:
configuration loading with custom status registries and tag filters, note
parsing and indexing, index persistence invariance without a separate `LISTS`
table, query execution across `lists` and `tasks` modes, CLI command execution
with fidelity formatting, and template pipeline rendering. Strictly avoid
duplicating low-level unit tests.

**Blocked by:** 09 (needs template namespaces), 10 (needs CLI task command).

## Key Integration Workflows

- **Full Vault Lifecycle:**
  Create an isolated test vault with custom statuses (`[/]` for In Progress,
  `[!]` for Attention) and tag filters (`#task`). Populate multiple notes with
  mixed outlines: plain bullets, checkboxes, top-level tasks, and nested
  subtasks with dates and priorities. Verify `QueryService::run` returns all
  items in `QueryMode::Lists`, but only tag-matching status items in
  `QueryMode::Tasks`.

- **Persistence Invariance (Without `LISTS` Table):**
  Persist the index to disk using `TestProject::persist_index`. Load a fresh
  `IndexerService` from the same database file, execute list and task queries,
  and assert identical results, proving `NOTES` and `FILES` tables alone
  faithfully reconstruct the entire list and task domain without reparsing
  Markdown.

- **CLI Execution & Output Fidelity:**
  Exercise the `Task` CLI command across realistic workflows:
  - Verify `render_task_list` preserves custom markers (`[/]`, `[!]`) and
    indents nested tasks by `list.depth * 2`.
  - Verify `--line-numbers` produces clickable `({path}:{line})` coordinates.
  - Verify `--todo`, `--done`, and `--status` filter shortcuts narrow output
    accurately.
  - Verify `--table` produces a formatted Markdown table with resolved
    columns.

- **Template Pipeline Execution:**
  Render templates via `TemplateService` containing `lists.from()` and
  `tasks.from()` pipelines. Verify transforms (`where`, `sort`, `limit`,
  `group_by`) and renderers (`task_list`, `table`, `count`) execute cleanly
  with inherited note metadata and inline field overrides.

## Acceptance Criteria

- [ ] Create `tests/integration/task_system_lifecycle.rs` using
  `TestProject`.
- [ ] Test multi-note vault lifecycle: custom statuses, tag filters, mixed list
  items, and query mode separation (`QueryMode::Lists` vs `QueryMode::Tasks`).
- [ ] Test index persistence invariance: build, persist to redb, reload from
  fresh service, and assert identical query outcomes without reparsing.
- [ ] Test CLI execution with output fidelity: custom marker preservation,
  nested indentation, clickable coordinates (`--line-numbers`), and filter
  shortcuts (`--todo`, `--done`, `--status`).
- [ ] Test CLI `--table` output with default columns and `--sort` ordering.
- [ ] Test template pipeline rendering with `lists.from()` and `tasks.from()`
  under `TemplateService`.
- [ ] Verify zero duplicate unit tests: low-level parser edge cases are
  excluded from the integration suite.
- [ ] All checks pass under `mise run verify`.
