# Analysis host architecture & integration with IndexerService/QueryService/SchemaService

Type: grilling
Blocked by: 09

## Question

Decide how the LSP process wraps and reuses the existing synchronous services rather than building parallel models, per the standing constraint "Reuse existing Traces semantics and services rather than building parallel LSP-only models."

Grounding facts already gathered:
- `FileIndex` (`src/index/entry.rs:23`) is an immutable, single-owned (not `Arc`-shared today), `Clone`-able snapshot: `entries: Box<[FileEntry]>` + `delta: IndexDelta`. `IndexerService` (`src/index/service.rs`) exposes `build`/`refresh`/`persist`/`load`, all synchronous, all manually triggered (no filesystem watcher exists anywhere in the codebase today).
- `QueryService::execute(&self, index: &Arc<FileIndex>, builder: QueryBuilder) -> QuerySet` (`src/query/service.rs:84`) already expects an `Arc<FileIndex>` at the call boundary, even though `IndexerService` itself doesn't produce one internally — note this seam.
- `SchemaService` (`src/schema/service.rs`) is a load-once, in-memory-cached registry (`IndexMap<SchemaName, Arc<Schema>>`), resolved once at startup, not re-resolved per query.
- Indexing/refresh today is single-threaded with no `rayon`/parallelism.

Decide:
- Whether the LSP host wraps a single `Arc<FileIndex>` behind a swap-on-refresh cell (à la prior "immutable/queryable snapshots" hypothesis) shared read-only across concurrent request handlers, versus some other sharing strategy.
- What triggers an index refresh in LSP mode: `didSave`, `didChangeWatchedFiles` (LSP file-watching capability — consult `docs/refs/lsp_spec.md`), a debounced timer, or something else — given there's no existing filesystem-watcher crate/dependency in Traces today, decide whether one gets added (and which — check via rust-docs-mcp if not already covered by the chosen LSP framework) or whether LSP-side didChange/didOpen/didSave notifications are sufficient without OS-level file watching.
- Whether `QueryService`/`SchemaService` get wrapped by a thin analysis-host facade or invoked directly per-request.
- How this interacts with the rust-analyzer host/snapshot precedent (ticket 06) — confirm, refine, or reject that hypothesis for Traces, including whether Traces adopts an incremental query engine (salsa-style or otherwise) alongside the host/snapshot split, or whether a coarser whole-file-invalidation model is sufficient; don't presume the answer either way before weighing it against ticket 33's performance targets.

Blocks: 12(concurrency), 13(persistence/caching), 14(live-buffer overlay), 30(multi-root), 34(packaging).
