# 12 — Task and list performance and allocation benchmarks

**Category:** enhancement
**Status:** done

**What to build:** Implement rigorous Criterion performance and memory
allocation benchmarks validating the design guarantees of the task and list
subsystem across `benches/query_sort.rs`, `benches/memory_footprint.rs`,
`benches/query_execution.rs`, and `benches/common/content.rs`.
In `benches/query_sort.rs`, benchmark list row sorting across text, due date,
priority, and status fields to verify zero heap allocations during comparator
evaluation. In `benches/memory_footprint.rs`, benchmark the memory footprint
of 50,000 flat `ListItem`s in `WorkspaceIndex` using `Region::new(GLOBAL)`, verifying
gross memory per item remains bounded under 220 bytes per item (demonstrating
a >50% reduction against the pre-compaction >450 bytes/item layout). In
`benches/query_execution.rs`, upgrade task density benchmarking to sweep
`QueryService::run` across varying task densities (1, 10, 100 tasks per note)
and workspace file counts `[100, 1_000, 10_000]`. Ensure all benchmark functions
include expected and unexpected outcome doc comments matching repo conventions.

**Blocked by:** 10 (resolved: merged to `main@12391e9`), 11 (pending merge: implemented in `.worktrees/task-11-integration-tests@e663866`; integration tests only, zero `src/` changes).

## Acceptance Criteria

- [x] Add `task_sort_fixture_source` in `benches/common/content.rs` generating
  notes with varied list/task items across statuses (`[ ]`, `[/]`, `[x]`, `[-]`,
  `[!]`), priorities (`⏫`, `🔼`, `🔽`, `⏬`), due dates (`📅 YYYY-MM-DD`), and text.
- [x] Add `bench_sort_list_rows_by_text` in `benches/query_sort.rs` benchmarking
  sorting by `list.text` (borrowed string slices).
- [x] Add `bench_sort_list_rows_by_due` in `benches/query_sort.rs` benchmarking
  sorting by `list.due` (`DateValue` promoted to `DateTimeValue` for comparison).
- [x] Add `bench_sort_list_rows_by_priority` in `benches/query_sort.rs` benchmarking
  sorting by `list.priority` (alphabetical `as_str()` string comparison).
- [x] Add `bench_sort_list_rows_by_status` in `benches/query_sort.rs` benchmarking
  sorting by `list.status` (borrowed status name string slices).
- [x] Add `bench_list_items_memory_footprint` in `benches/memory_footprint.rs`
  measuring gross bytes allocated per `ListItem` via `Region::new(GLOBAL)` over
  50,000 flat list items in `WorkspaceIndex`, asserting gross memory per item stays
  under 220 bytes (validating the 152-byte struct layout + heap string storage,
  achieving >50% reduction over pre-compaction >450 bytes/item).
- [x] Build every fixture through existing helpers — `project::build_index_arc_from_note_source`
  (in-memory, wraps `test_support::build_test_index` → `WorkspaceIndex::new_test`, zero
  disk I/O) for `query_sort.rs`/`query_execution.rs`/`memory_footprint.rs`'s new
  list-item benchmark, and the new `task_sort_fixture_source` note generator for
  content. Do not add ad-hoc `IndexerService`/filesystem setup, hand-rolled Markdown
  strings inline in bench functions, or parallel note-building helpers.
- [x] Add `bench_query_tasks_density` in `benches/query_execution.rs` sweeping
  1, 10, and 100 tasks per note across workspace file counts `[100, 1_000, 10_000]`,
  upgrading the legacy `bench_run_tasks_density`.
- [x] Document all benchmark groups with explicit expected and unexpected
  outcome doc comments per codebase guidelines.
- [x] Register all new benchmark functions in their respective `criterion_group!`
  macros.
- [x] Verify all benchmarks compile and pass smoke testing under `cargo bench --no-run`
  and `mise run bench --mode test`.
- [x] All checks pass under `mise run verify`.

## Key Benchmark Suites

- **Zero-Allocation Sorting (`benches/query_sort.rs`):**
  Benchmark sorting `n = 20_000` list rows using `SortKey<'a>` projection:
  - Sort by `list.text` (borrowed string slices).
  - Sort by `list.due` (4-byte `DateValue` comparison).
  - Sort by `list.priority` (enum discriminant comparison).
  - Sort by `list.status` (borrowed status name string slices).
  - Expected: Zero heap allocations during comparator evaluation; throughput
    scales as O(N log N) dominated by comparator cache hits.
  - Unexpected: Heap allocations detected in comparison loops, or performance
    diverging significantly from primitive integer sorting.

- **Compacted Memory Footprint (`benches/memory_footprint.rs`):**
  Measure gross allocated bytes and allocation calls for `WorkspaceIndex`
  construction over vaults with 50,000 list items:
  - Exercise notes with sparse inline fields and shared raw/clean text.
  - Expected: Average gross memory consumption under 220 bytes per list item
    (reflecting compiled `size_of::<ListItem>()` of 168 bytes plus single raw
    heap string), achieving a >50% reduction compared to the pre-compaction
    >450 bytes/item layout.
  - Unexpected: Allocation count scaling linearly with empty inline field maps
    or duplicate clean text allocations.

- **Query Execution Throughput (`benches/query_execution.rs`):**
  Sweep `QueryService::run` with `QueryBuilder::tasks` across varying task
  densities:
  - 1 task per note, 10 tasks per note, 100 tasks per note across workspace
    file counts `[100, 1_000, 10_000]`.
  - Expected: Row construction throughput scales linearly with task count;
    positional index lookup overhead is negligible.
  - Unexpected: Non-linear degradation as task density increases, indicating
    inefficient slice traversal in `Note::tasks()`.

- **Shared Sorting Fixture (`benches/common/content.rs`):**
  Add `task_sort_fixture_source(note_index: usize, count: usize) -> String`
  providing deterministic, high-entropy distributions of status markers,
  due dates, priority emojis, and text so sorting comparators exercise real
  branch permutations rather than uniform tie-breaks.

- **Fixture Reuse (`benches/common/`, `test_support`):**
  All three new benchmark suites build fixtures exclusively through existing
  `benches/common` wrappers over the crate's `test_support` module (feature-gated
  `test-utils`, exposed from `src/lib.rs`):
  - `project::build_index_arc_from_note_source(note_count, |i, n| ...)` — wraps
    `test_support::build_test_index` → `WorkspaceIndex::new_test`. Zero disk I/O.
    Already the established pattern in `query_sort.rs` and `query_execution.rs`
    (`bench_sort_task_rows`, `bench_run_tasks_density`); reuse it verbatim rather
    than reaching for `project::create_project` + `IndexerService::build()` (the
    disk-backed path `memory_footprint.rs` uses for `bench_file_index_footprint`,
    which conflates filesystem I/O with the `ListItem` struct's own density and
    is not appropriate for isolating list-item memory footprint).
  - `common::content::{task_note_source, task_triplet_note_source}` remain the
    baseline task generators; the new `task_sort_fixture_source` sits alongside
    them in `benches/common/content.rs`, following the module's existing
    `*_note_source` naming convention (see `benches/common/mod.rs` usage rules).
  - Do not import `traces_pkm::test_support::*` directly inside a `benches/*.rs`
    file; go through the `benches/common` wrapper so fixture logic stays
    centralized per the module's stated usage rules.

## Comments

> *This was generated by AI during triage.*

### Triage Notes

**What we've established:**

1. **Dependency Resolution & Runtime Readiness:**
   - All functional task system capabilities (tickets 01–10) are merged on `main`.
   - Ticket 11 (integration test suite) is implemented in `.worktrees/task-11-integration-tests` (commit `e663866`). Crucially, ticket 11 modifies only `tests/` and does not touch `src/` or `benches/`.
   - All query pipelines (`QueryBuilder::lists`, `QueryBuilder::tasks`), sorting comparators (`SortKey<'a>`), and `WorkspaceIndex` APIs required by ticket 12 are available and fully functional in `main`.

2. **Memory Footprint Grounding:**
   - Analysis of `ListItem` structure:
     - `ListText`: `raw: String` (24 bytes) + `clean: Option<String>` (24 bytes) = 48 bytes.
     - `kind: ListItemType`: `Task(TaskListItem)` is 72 bytes (containing `TaskDates` 24 bytes, `TaskStatus` 40 bytes, `priority` 1 byte, `fully_complete` 1 byte).
     - Positioning & hierarchy: `depth` 1 byte, `line` 4 bytes, `parent` 4 bytes, `is_ordered` 1 byte.
     - Metadata & tags: `fields` 8 bytes (`Option<Box<...>>`), `tags` 16 bytes (`Box<[Tag]>`).
     - Total `size_of::<ListItem>()` is 168 bytes.
     - In memory, 50,000 items with shared clean text and heap `raw: String` (~24 bytes) consume ~192 bytes resident heap per item.
   - The pre-compaction layout occupied >450-500 bytes per item (inline `IndexMap` 56 bytes + hash table bucket allocations + separate `clean: String` heap alloc + 72-byte `NaiveDate` array + separate `LISTS` redb table rows).
   - The original ticket AC of "under 100 bytes" was an ungrounded napkin estimate that is physically lower than the 168-byte struct definition. The target is updated to "under 220 bytes gross allocation per item", which proves the required >50% reduction (>55% actual reduction).
   - **Correction (implementation, 2026-09-20):** the compiled layout is
     `size_of::<ListItem>() == 152` bytes (not 168), and the clone probe
     measures 2 heap allocations per item (~29 bytes total), for ~180 gross
     bytes/item against the 220-byte budget. The bench now prints
     `size_of::<ListItem>()` alongside its gross-bytes report.

3. **Benchmark Module Alignment:**
   - `benches/query_sort.rs`: Remains dedicated to CPU throughput, profiling, and cache-hit scaling. Comparator zero-allocation guarantees are documented in the function expected/unexpected doc comments.
   - `benches/memory_footprint.rs`: Houses the `StatsAlloc` `Region::new(GLOBAL)` memory verification measuring gross allocated bytes and allocation calls.
   - `benches/query_execution.rs`: Replaces legacy `bench_run_tasks_density` (`&[1, 3, 10, 20]`) with `bench_query_tasks_density` sweeping `[1, 10, 100]` tasks per note across workspace file counts.
   - `benches/common/content.rs`: Centralizes the varied task fixture generator to prevent code duplication across benchmark targets.

- 2026-09-20 (implementation): Done on `feat/task-12-performance-benchmarks`
  (`84058b7` benches, `0864a41` measurement hygiene, follow-up docs fix). Two
  AC comparator premises were corrected in place during implementation:
  `list.due` sorts via `DateTimeValue` promotion and `list.priority` sorts
  alphabetically by `as_str()` — the bench docs document the actual
  comparators. Zero-allocation comparator verification remains external
  (DHAT/valgrind) per Triage Notes §3, as instrumenting the sort benches'
  global allocator would perturb their timing.

## Agent Brief

**Category:** enhancement
**Summary:** Implement Criterion benchmarks validating zero-allocation list sorting, memory footprint reduction, and task query throughput scaling.

**Current behavior:**
`benches/query_sort.rs` only has `bench_sort_task_rows` sorting by `"list.completed"`, with no coverage for `list.text`, `list.due`, `list.priority`, or `list.status`. `benches/memory_footprint.rs` lacks a benchmark measuring `WorkspaceIndex` memory consumption for 50,000 list items. `benches/query_execution.rs` has a legacy `bench_run_tasks_density` that only sweeps `{1, 3, 10, 20}` tasks per note on 1,000 files without multi-file-count scaling.

**Desired behavior:**
1. Implement `task_sort_fixture_source` in `benches/common/content.rs` generating high-entropy task distributions (varied markers `[ ]`, `[/]`, `[x]`, `[-]`, `[!]`, due dates, priorities, and text).
2. Add `bench_sort_list_rows_by_text`, `bench_sort_list_rows_by_due`, `bench_sort_list_rows_by_priority`, and `bench_sort_list_rows_by_status` in `benches/query_sort.rs`.
3. Add `bench_list_items_memory_footprint` in `benches/memory_footprint.rs` using `Region::new(GLOBAL)` to assert that 50,000 list items in `WorkspaceIndex` allocate <220 bytes gross per item (>50% reduction over pre-compaction layout).
4. Upgrade `benches/query_execution.rs` to `bench_query_tasks_density` sweeping `[1, 10, 100]` tasks per note across workspace sizes `[100, 1_000, 10_000]`.
5. Ensure all benchmark functions include expected and unexpected outcome doc comments per repo standards.

**Key interfaces:**
- `benches/common/project.rs`: `build_index_arc_from_note_source(note_count, note_source_fn)` —
  wraps `test_support::build_test_index` → `WorkspaceIndex::new_test` (in-memory, zero disk
  I/O). Use for all three new benchmarks; do not call `test_support`/`IndexerService`
  directly from a `benches/*.rs` file.
- `benches/query_sort.rs`: `QueryBuilder::tasks(SourceSelector::All).sort("list.<field>", false)`
- `benches/memory_footprint.rs`: `stats_alloc::Region::new(GLOBAL)`
- `benches/query_execution.rs`: `QueryService::run` with `QueryBuilder::tasks` across density matrix
- `benches/common/content.rs`: `task_sort_fixture_source` (new), alongside existing
  `task_note_source`/`task_triplet_note_source`

**Acceptance criteria:**
Refer to the single Acceptance Criteria checklist at the top of this ticket.

**Out of scope:**
- Altering production query/sort logic in `src/`.
- Functional test suites (covered in Issue 11).
