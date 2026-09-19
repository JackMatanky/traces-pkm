# 11 — Task and list system integration test suite

**Category:** enhancement
**Status:** ready-for-agent

**What to build:** Expand and complete the integration and end-to-end test
suites across the existing module-aligned test files in `tests/integration/` and
`tests/e2e/`. Rather than creating an uncohesive, cross-cutting kitchen-sink file,
distribute high-value end-to-end scenario coverage into their natural module
homes: task classification & multi-note lifecycle into `tests/integration/task_tag_filters.rs`
(or generalized to `task_lifecycle.rs`), index persistence invariance into
`tests/integration/index_persistence_roundtrip.rs`, query modes (`lists` vs `tasks`)
& canonical syntax into `tests/integration/index_query.rs`, template pipelines
(`lists.from()`, `tasks.from()`) into `tests/integration/template_render.rs`, and
process-boundary CLI fidelity into `tests/e2e/dispatch.rs` (under `query_commands`).
Strictly avoid duplicating low-level unit tests.

**Blocked by:** 09 (resolved: merged to `main@e945c9e`), 10 (pending merge: implemented in `.worktrees/task-10-cli-enhancements-and-from-expansion@1f25022`).

## Acceptance Criteria

- [ ] **Task Lifecycle (`tests/integration/task_tag_filters.rs`):** Test
  multi-note vault lifecycle using `TestProject`: custom and extended status
  markers (`[/]`, `[-]`, `[!]`, and unknown single-char markers `[?]`), tag
  filters (`#task`), mixed list outlines (plain bullets, non-task checkboxes,
  top-level tasks, and nested subtasks with dates and priorities), and
  `fully_complete` computation (parent task with all task children done evaluates
  to `fully_complete == true`; parent with an incomplete/in-progress child evaluates
  to `false`; non-task checklist items and plain bullets are ignored).
- [ ] **Persistence Invariance (`tests/integration/index_persistence_roundtrip.rs`):**
  Test redb persistence invariance without `LISTS` table (ADR 0005): build,
  persist to redb (`NOTES` and `FILES` tables only), reload from a fresh
  `IndexerService` simulating a cold process restart, and assert identical query
  outcomes and complete list metadata without reparsing Markdown.
- [ ] **Query Modes & Namespaces (`tests/integration/index_query.rs`):**
  Test public query evaluation across modes: `QueryBuilder::lists` yields all
  items (bullets, checkboxes, tasks) with structural fields (`depth`, `line`,
  `parent`), while `QueryBuilder::tasks` yields only tag-matching status items;
  verify acceptance of canonical `list.<field>` and diagnostic rejection of
  obsolete `task.<field>`; verify note frontmatter inheritance on list rows and
  inline field overrides.
- [ ] **Template Pipelines (`tests/integration/template_render.rs`):**
  Test template rendering under `TemplateService` with `lists.from(...)` and
  `tasks.from(...)` pipelines, verifying transforms (`where`, `sort`, `limit`) and
  terminal formatters (`task_list`, `table`, `count`) with inherited note
  frontmatter and inline field overrides.
- [ ] **CLI Output Fidelity (`tests/e2e/dispatch.rs`):**
  Add process-boundary CLI tests under `mod query_commands` using `Sandbox`:
  verify `traces task` preserves custom status markers (`[/]`, `[-]`, `[!]`, `[?]`),
  indents nested tasks by `list.depth * 2` spaces, formats clickable coordinates
  with `--line-numbers` (`-l`), filters with shortcuts (`--todo`, `--done`,
  `--status <char>`), orders with `--sort` (`--asc`/`--desc`), renders tables with
  default and custom columns via `--table`, outputs counts via `--count`, and
  resolves bare `.md` paths via `--from`.
- [ ] **No Unit Test Duplication:** Low-level parser edge cases (bracket
  variations, malformed emojis, date parsing errors) remain exclusively in unit
  suites (`src/note/`, `src/task/`, `src/query/`).
- [ ] All checks pass under `mise run verify`.

## Key Integration Workflows (Combined by Module)

- **Task Lifecycle & Outline Classification (`tests/integration/task_tag_filters.rs`):**
  Exercise an isolated test vault with `[tasks] tag_filters = ["#task"]`.
  Populate multiple notes with mixed outlines: plain bullets, non-task checkboxes,
  top-level tasks, and nested subtasks with dates (`📅`, `[due:: ...]`) and
  priorities (`🔺`, `[priority:: ...]`). Prove classification correctly isolates
  tasks, preserves custom markers (`[/]`, `[-]`, `[!]`, `[?]`), and computes
  `fully_complete` across parent-child hierarchies.

- **Persistence Invariance Without `LISTS` Table (`tests/integration/index_persistence_roundtrip.rs`):**
  Persist the index to disk using `TestProject::persist_index`. Load a fresh
  `IndexerService` from the same database file (simulating cold process restart),
  execute list and task queries, and assert identical results to the in-memory
  index, proving `NOTES` and `FILES` tables alone faithfully reconstruct the
  entire list and task domain without reparsing Markdown (ADR 0005).

- **Query Mode Separation & Canonical Grammar (`tests/integration/index_query.rs`):**
  Verify `QueryService::run` with `QueryBuilder::lists` returns all list items
  with structural metadata (`list.depth`, `list.line`, `list.parent`), while
  `QueryBuilder::tasks` returns only tag-matching tasks. Verify canonical
  `list.<field>` syntax succeeds, obsolete `task.<field>` produces an actionable
  diagnostic hint, and inline fields on list items override note frontmatter.

- **Template Pipeline Execution (`tests/integration/template_render.rs`):**
  Render templates via `TemplateService` containing `lists.from()` and
  `tasks.from()` pipelines. Verify transforms (`where`, `sort`, `limit`,
  `group_by`) and renderers (`task_list`, `table`, `count`) execute cleanly
  with inherited note metadata and inline field overrides.

- **CLI Execution & Output Fidelity (`tests/e2e/dispatch.rs`):**
  In `tests/e2e/dispatch.rs` under `mod query_commands` using `Sandbox::trusted()`,
  exercise the compiled `traces` binary across realistic task workflows:
  - Verify `traces task` preserves custom and fallback markers (`[/]`, `[-]`,
    `[!]`, `[?]`) and indents nested tasks by `list.depth * 2` spaces.
  - Verify `--line-numbers` (`-l`) produces clickable `({path}:{line})`
    coordinates.
  - Verify `--todo`, `--done`, and `--status <char>` filter shortcuts narrow
    output accurately.
  - Verify `--sort` with `--asc` and `--desc` correctly orders output rows.
  - Verify `--table` produces formatted Markdown table with default columns
    (`Task`, `Status`, `Due`, `Priority`, `File`) and custom `--column` overrides.
  - Verify `--count` produces single numeric integer on stdout with zero stderr.
  - Verify `--from <path.md>` directly queries specified note files.

## Comments

> *Triaged and verified against task-system spec, codebase architecture, and ADR 0005.*
> *Architectural decision: aligned with the repository's modular test layout.*
> *Tests are combined into their respective subsystem files (`task_tag_filters.rs`,*
> *`index_persistence_roundtrip.rs`, `index_query.rs`, `template_render.rs`, and*
> *`tests/e2e/dispatch.rs`) instead of introducing a redundant, monolithic file.*

## Agent Brief

**Category:** enhancement
**Summary:** Build out end-to-end integration and CLI test coverage for the task
and list system across the existing module-organized test suites.

**Current behavior:**
Integration tests in `tests/integration/` are split by subsystem module but currently
lack full task-system coverage:
- `task_tag_filters.rs` only tests basic tag filter classification without custom
  status markers, deep hierarchies, dates/priorities, or `fully_complete`.
- `index_persistence_roundtrip.rs` has a preliminary flat list check, but does not
  prove full persistence invariance of query outcomes against reloaded redb stores.
- `index_query.rs` only exercises page-level queries, with zero coverage for
  `QueryMode::Lists` vs `QueryMode::Tasks`, canonical `list.*` paths, or inline field
  overrides.
- `template_render.rs` only tests `query.from()`, lacking `lists.from()` and
  `tasks.from()` pipeline rendering.
- `tests/e2e/dispatch.rs` only checks basic `traces task` without marker preservation,
  depth indentation, `--line-numbers`, shortcuts, sorting, or `--table`.

**Desired behavior:**
Distribute focused, end-to-end task integration coverage into existing test suites:
1. `tests/integration/task_tag_filters.rs`: multi-note vault lifecycle, custom markers
   (`[/]`, `[-]`, `[!]`, `[?]`), mixed list outlines, dates, priorities, and
   `fully_complete` computation.
2. `tests/integration/index_persistence_roundtrip.rs`: persistence invariance proving
   cold `IndexerService::load` recovers identical list/task query outcomes without
   reparsing Markdown (ADR 0005, no `LISTS` table).
3. `tests/integration/index_query.rs`: `QueryMode::Lists` vs `QueryMode::Tasks`,
   canonical `list.<field>` validation, obsolete `task.<field>` rejection, and
   field override precedence.
4. `tests/integration/template_render.rs`: `lists.from()` and `tasks.from()` pipelines,
   method transforms, and terminal filters (`task_list`, `table`, `count`).
5. `tests/e2e/dispatch.rs`: process-boundary CLI tests for `traces task` covering
   marker preservation, depth indentation, clickable line coordinates (`-l`),
   filter shortcuts (`--todo`, `--done`, `--status`), sorting, `--table`, and `--count`.

**Key interfaces:**

- `traces_pkm::TestProject`: Scaffold isolated test vaults with custom config,
  write notes, and persist/reload indices in `tests/integration/`.
- `traces_pkm::QueryService`: Evaluate queries across `QueryMode::Lists` and
  `QueryMode::Tasks`.
- `traces_pkm::TemplateService`: Render templates containing `lists.from()` and
  `tasks.from()` pipelines.
- `tests/e2e/support::Sandbox`: Spawn isolated child processes running the real
  `traces` binary to assert stdout, stderr, and exit codes.

**Acceptance criteria:**
Refer to the single Acceptance Criteria checklist at the top of this ticket.

**Out of scope:**

- Criterion performance and allocation benchmarks (addressed in Issue 12).
- Adding new CLI flags, parser features, or mutating task statuses.
