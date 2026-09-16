# 12 — Task and list performance and allocation benchmarks

**Status:** ready-for-agent

**What to build:** Implement rigorous Criterion performance and memory
allocation benchmarks validating the design guarantees of the task and list
subsystem. In `benches/query_sort.rs`, benchmark list row sorting across text,
due date, priority, and status fields to verify zero heap allocations during
comparator evaluation. In `benches/memory_footprint.rs`, benchmark the memory
footprint of 50,000 flat `ListItem`s in `FileIndex` using `Region::new(GLOBAL)`,
verifying an average memory footprint under 100 bytes per item. In
`benches/query_execution.rs`, sweep `QueryService::run` across varying task
densities (1, 10, 100 tasks per note) and workspace file counts. Ensure all
benchmark functions include expected and unexpected outcome doc comments.

**Blocked by:** 11 (needs working integration and query pipelines).

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
  Measure gross allocated bytes and allocation calls for `FileIndex`
  construction over vaults with 50,000 list items:
  - Exercise notes with sparse inline fields and shared raw/clean text.
  - Expected: Average gross memory consumption under 100 bytes per list item,
    achieving a >50% reduction compared to the pre-compaction layout.
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

## Acceptance Criteria

- [ ] Add `bench_sort_list_rows_by_text` in `benches/query_sort.rs`.
- [ ] Add `bench_sort_list_rows_by_due` in `benches/query_sort.rs`.
- [ ] Add `bench_sort_list_rows_by_priority` in `benches/query_sort.rs`.
- [ ] Add `bench_sort_list_rows_by_status` in `benches/query_sort.rs`.
- [ ] Add `bench_list_items_memory_footprint` in `benches/memory_footprint.rs`
  measuring gross bytes allocated per `ListItem` via `Region::new(GLOBAL)`.
- [ ] Add `bench_query_tasks_density` in `benches/query_execution.rs` sweeping
  1, 10, and 100 tasks per note.
- [ ] Document all benchmark groups with explicit expected and unexpected
  outcome doc comments per codebase guidelines.
- [ ] Verify all benchmarks compile and run under `cargo bench --no-run`.
- [ ] All checks pass under `mise run verify`.
