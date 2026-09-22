# LSP Performance Best Practices

Research compiled from rust-analyzer, TypeScript/tsserver, clangd, gopls, Biome, ruff, Rumdl, Marksman, Markdown Oxide, and domain-driven LSP implementations. Organized by category with actionable recommendations for a Markdown/PKM LSP.

---

## 1. Startup & Cold-Start

### 1.1 Persisted Index with Lazy Revalidation

**Pattern:** Persist the workspace index to disk. On startup, load it and only re-parse files whose content hash changed since last session.

**Who uses it:**
- **rumdl** (CLI mode): JSON cache at `.rumdl_cache/{version}/{hash}.json` keyed by `(blake3(content_hash), blake3(config_hash), blake3(rules_hash), dependency_fingerprint)`. Binary-serialized workspace index at `.rumdl_cache/workspace_index.bin`. `WorkspaceIndex::load_from_cache` at startup, skip files with matching hash.
- **clangd**: Per-file `*.idx` shards in `.cache/clangd/index/`. Before indexing each file, checks for cached shard. Avoids reindexing on startup if nothing changed.
- **gopls**: Persistent file-based key/value store via `filecache` package. Serializable indexes (`xrefs`, `methodsets`, `typerefs`) for fast restart.
- **rust-analyzer**: On-disk incremental compilation cache with version header; version mismatch triggers full rebuild.

**Why it works:** Editors restart frequently (config changes, crashes, updates). Full workspace scans are O(n) in file count. Persisted index makes warm start O(changed_files).

**PKM application:** Traces already has `IndexerService::refresh()` that loads redb and diffs against filesystem. This is exactly the right pattern — keep it.

### 1.2 Parallel File Loading

**Pattern:** Load/index files in parallel using a thread pool during startup.

**Who uses it:**
- **rust-analyzer**: VFS loads files serially today (known issue #17373), but a parallel loader prototype reduced Buck2's 909-crate scan from 46s to 6–20s.
- **clangd**: Background index uses a thread pool; compilation database entries are queued for parallel indexing.
- **Markdown Oxide**: `WalkDir` at startup, parsed in parallel via `rayon`.
- **rumdl**: `rayon` parallel iteration for linting.

**Why it works:** Disk I/O and parsing are independent per file. Parallelism saturates SSD throughput and uses multiple cores.

**PKM application:** Use `rayon` for the startup scan. Markdown files are small and parsing is CPU-bound — parallel iteration is nearly free.

### 1.3 Deferred Validation

**Pattern:** Don't validate everything at startup. Provide syntactic features immediately; defer semantic validation to background.

**Who uses it:**
- **TypeScript**: `useSyntaxServer: 'auto'` spawns a lightweight syntax-only server for fast startup, with a full semantic server running in background.
- **pyright**: Open files get immediate validation; background files are validated lazily.
- **DomainLang proposal (R7)**: `{ validation: false }` at startup; validate on first access.

**Why it works:** Users see syntax highlighting and basic completions within milliseconds. Full diagnostics appear seconds later.

**PKM application:** On startup, load the index and immediately serve completions/gotodef from it. Run diagnostics in background after a short delay.

---

## 2. Incremental Updates

### 2.1 Per-File Re-Parsing (No Incremental Parsing Within Documents)

**Pattern:** On `didChange`, re-parse the entire document from scratch. Don't attempt incremental parsing within a single markdown file.

**Who uses it:**
- **Marksman**: `Doc.withText` feeds the whole buffer to `Parser.parse`, then `Index.ofCst` rebuilds the document's catalog. Full re-parse on every `didChange`.
- **rumdl LSP**: `IndexWorker` re-parses changed file's content into a new `WorkspaceIndex`, diffs structural data.
- **Markdown Oxide**: `Vault::update_vault` mutates just that file's in-memory entry.
- **rust-analyzer**: Even with salsa, `parse()` re-runs on every file change (but downstream queries may short-circuit via early cutoff).

**Why it works:** Markdown files are small (typically <100KB). Full re-parse takes <1ms. Incremental parsing within a document adds complexity for negligible gain. The expensive part is cross-file dependency tracking, not per-file parsing.

**PKM application:** Parse the full document on each edit. Spend optimization effort on cross-file index updates, not intra-document parsing.

### 2.2 Structural Diff for Cross-File Invalidation

**Pattern:** After re-parsing a file, diff the extracted structural data (headings, links, anchors) against the previous version. Only trigger re-computation of dependent files if the diff is non-empty.

**Who uses it:**
- **rumdl LSP**: `extracted_data_differs` compares heading anchors + links between old and new `WorkspaceIndex`. Only re-lints dependent files if cross-file data actually changed.
- **rust-analyzer**: Salsa's "early cutoff" — if a query's result is unchanged despite a changed input, dependent queries are not recomputed.
- **gopls**: Compares export data (package interface) before invalidating dependents. Only re-analyzes files that imported from the changed file.
- **pyright**: Tracks import-level dependencies; only re-analyzes files that imported from the changed file.

**Why it works:** Most edits (typo fixes, whitespace changes, body edits) don't change a document's exported structure. Structural diff avoids cascading invalidation.

**PKM application:** After re-parsing, compare extracted links/tags/headings. If unchanged, skip re-computation of backlinks, tag queries, and diagnostics for other files.

### 2.3 Export-Signature Diffing

**Pattern:** Before propagating changes to dependents, compare the set of exported symbols (headings, links, tags) before and after the change. If exports are unchanged, skip transitive invalidation.

**Source:** DomainLang LSP requirements doc (R2), gopls package interface comparison.

**PKM application:** A file's "exported" data for PKM purposes is: its headings (for wiki-link targets), its outgoing links (for backlink computation), and its tags. If none of these changed, no other file needs re-indexing.

---

## 3. Debouncing & Batching

### 3.1 Diagnostics Debouncing

**Pattern:** Don't recompute diagnostics on every keystroke. Use a debounce window (200–500ms) so bursts of edits collapse into a single diagnostics run.

**Who uses it:**
- **Markdown Oxide**: `DIAGNOSTICS_DEBOUNCE_MS = 300`. Uses `AtomicU64` generation counter — each edit bumps the counter and spawns a task that waits; only the task whose generation is still current computes diagnostics.
- **TypeScript**: Debounced `GetErr` requests; the server batches semantic diagnostics.
- **rumdl LSP**: 100ms debounce on `IndexUpdate::FileChanged` events.

**Why it works:** Diagnostics are O(open_files × references × referenceables). Without debouncing, each keystroke triggers a full pass. With fast typing, passes pile up faster than they complete, filling the message queue and blocking stdin.

**PKM application:** Use a generation-counter debounce (like Markdown Oxide's `schedule_diagnostics`). 300ms is a good default. This is critical for PKM where notebooks can have hundreds of files.

### 3.2 Request Cancellation

**Pattern:** Support `$/cancelRequest` so in-flight work is abandoned when a newer request arrives.

**Who uses it:**
- **rust-analyzer**: Salsa's global revision counter. When a concurrent edit bumps the database revision, in-flight queries panic with `salsa::Cancelled`, caught and translated into a silent drop or retry.
- **TypeScript**: `cancellationPipeName` mechanism — client creates named pipes to signal cancellation.
- **Biome**: `salsa::Cancelled::catch` wrapping synchronous core calls.

**Why it works:** Without cancellation, stale work blocks the server from handling newer (more relevant) requests. Users see laggy completions because the server is still computing results for a position they've already moved past.

**PKM application:** Use salsa or a generation-counter approach. On `didChange`, bump a revision; check it periodically during long operations.

### 3.3 Full Document Sync (Not Incremental)

**Pattern:** Use `TextDocumentSyncKind::FULL` — the editor sends the entire document content on each change, not just the diff.

**Who uses it:**
- **Markdown Oxide**: `TextDocumentSyncKind::FULL`.
- **Marksman**: `didChange` re-parses from full buffer content.

**Why it works for markdown:** Markdown files are small. The overhead of sending the full content is negligible compared to the complexity of applying incremental patches. Incremental sync is essential for large source files (C++, TypeScript) but counterproductive for small markdown files.

**PKM application:** Use `FULL` sync. Markdown files average <50KB. The simplicity win is worth the bandwidth cost.

---

## 4. Memory Management

### 4.1 Memory Budget with Degradation Modes

**Pattern:** Track memory usage and degrade gracefully under pressure (e.g., disable expensive features, evict caches).

**Who uses it:**
- **perl-lsp-rs**: `MemoryBudget` struct with `warning_threshold_bytes` (512MB), `critical_threshold_bytes` (1GB), `ast_cache_max_bytes` (128MB). `MemoryMonitor` tracks allocations via atomics. Pressure levels: `Normal`, `Warning`, `Critical`. At `Warning`, disable background indexing. At `Critical`, evict AST cache.
- **TypeScript**: `maxTsServerMemory` setting (default ~3GB). V8 heap limit.
- **Deno LSP**: Idle memory release — after 5s of no requests, fires `low_memory_notification()` to force V8 GC and return pages to OS. Reclaims ~69MB (15%) of idle heap.

**Why it works:** LSP servers are long-running. Without memory management, heap grows monotonically. PKM vaults can be large (thousands of files). Memory budgets prevent OOM kills.

**PKM application:** Implement `MemoryBudget` with configurable thresholds. Track `rss` via `std::mem::size_of_val` or `jemalloc` stats. At warning, stop indexing new files. At critical, evict LRU cache entries.

### 4.2 LRU Caches with Bounded Size

**Pattern:** Use LRU caches for expensive computations, with explicit maximum sizes to prevent unbounded growth.

**Who uses it:**
- **rust-analyzer**: Query results cached via salsa with explicit LRU capacities:
  | Query | LRU Capacity |
  |-------|-------------|
  | File Text | 16 |
  | Parse | 128 |
  | Borrowck | 2024 |
- **TypeScript**: Completion data cached on server with `cacheId` — sends lightweight reference to client, resolves on demand.

**Why it works:** Unbounded caches grow until OOM. LRU eviction keeps the hottest data in memory while bounding total usage.

**PKM application:** Cache parsed document ASTs and link resolution results in an LRU with a configurable max (e.g., 256 entries for a PKM vault of 1000 files).

### 4.3 Idle Memory Release

**Pattern:** When the server goes idle (no requests for N seconds), trigger a GC or memory release to return freed pages to the OS.

**Source:** Deno LSP (`denoland/deno#34727`): After 5s idle, synthesize internal `$releaseMemory` request → `Isolate::low_memory_notification()` → compacting GC. Reclaims ~69MB reproducibly.

**Why it works:** Runtime GCs (V8, jemalloc) don't return memory to the OS unless explicitly asked. Idle release prevents the LSP from holding peak memory indefinitely.

**PKM application:** After a configurable idle period (default 5s), call `malloc_trim` (Linux) or `jemallax` to release memory. This is especially important for PKM servers that may sit idle for minutes between edits.

---

## 5. Architecture & Runtime

### 5.1 Synchronous Core, Async Transport

**Pattern:** The analysis/parsing core is synchronous. Only the LSP transport layer (stdin/stdout, JSON-RPC) uses async. CPU-bound work never blocks the async runtime.

**Who uses it:**
- **ruff server**: No async runtime at all. `crossbeam` channels + `lsp-server`+`lsp-types`.
- **Biome**: `tokio` + `tower-lsp` at transport layer only. Core `Workspace` methods are plain synchronous functions.
- **rust-analyzer**: `lsp-server`+`lsp-types`, thread pools, cooperative cancellation.
- **Astral `ty`**: Same pattern as ruff — synchronous core, no async.

**Why it works:** CPU-bound work (parsing, linting, type checking) doesn't benefit from async. Making it `async` adds overhead (Future state machines, poll scheduling) with no benefit. The transport layer IS I/O-bound (stdin/stdout) and benefits from async.

**PKM application:** Use `tower-lsp` for transport. Keep all parsing/indexing/query logic synchronous. Use `tokio::task::block_in_place` for CPU-bound vault operations (as Markdown Oxide already does).

### 5.2 Thread Pool for CPU-Bound Work

**Pattern:** Use a dedicated thread pool (via `rayon`) for parallel CPU-bound work, separate from the async runtime.

**Who uses it:**
- **Biome**: `rayon::scope` + `rayon::ThreadPoolBuilder` for multi-file work-stealing parallelism.
- **ruff server**: Two thread pools — `fmt_pool` (single-threaded for formatting) + `background_pool` (CPU-core-sized for linting).
- **Markdown Oxide**: `rayon::par_iter` for parallel vault construction and diagnostics.
- **clangd**: Background index thread pool with configurable `-j` parallelism.

**Why it works:** `tokio`'s default worker pool is sized for I/O concurrency, not CPU parallelism. A separate `rayon` pool with `num_cpus` threads maximizes CPU utilization for parsing/indexing.

**PKM application:** Use `rayon` for workspace scanning and diagnostics. Keep the tokio runtime for LSP transport only.

---

## 6. Caching Strategies

### 6.1 Content-Hash Based Cache Keys

**Pattern:** Cache results keyed by content hash (not file path or mtime). Content hashing is deterministic and filesystem-independent.

**Who uses it:**
- **rumdl**: `blake3(content_hash)` for CLI cache.
- **Beacon.rs**: `DefaultHasher` for scope-level content hashing.
- **gopls**: Content hashing for persistent cache validation.
- **Serena**: MD5 hash of file contents for symbol cache invalidation.

**Why it works:** `mtime` is unreliable on NFS, Docker volumes, and some macOS edge cases. Content hashing always detects actual changes.

**PKM application:** Already using blake3 for content hashing in redb. Continue this pattern for any new caches.

### 6.2 Two-Tier Cache (Raw + Processed)

**Pattern:** Cache both the raw LSP response and the processed/normalized version. Invalidation checks the raw tier first.

**Source:** Serena LSP: `raw_document_symbols.pkl` (raw JSON-RPC response) + `document_symbols.pkl` (processed `UnifiedSymbolInformation`). Content-based invalidation on the raw tier; processed tier is invalidated transitively.

**Why it works:** Some operations need the raw response (e.g., forwarding to client). Others need processed data (e.g., internal analysis). Caching both avoids redundant processing.

**PKM application:** Cache raw parsed AST (for re-parsing on edit) and derived index data (links, tags, headings) separately. Invalidate derived data only when raw parse output changes structurally.

### 6.3 Versioned Cache with Automatic Invalidation

**Pattern:** Stamp cache entries with a version number. On version mismatch, discard the entire cache.

**Who uses it:**
- **rumdl**: CLI cache version-pinned by `CARGO_PKG_VERSION`.
- **Serena**: `__cache_version` in pickle files.
- **rust-analyzer**: On-disk cache has version header; mismatch triggers full rebuild.

**Why it works:** When the data format changes between releases, stale caches cause subtle bugs. Version pinning is the simplest correct invalidation strategy.

**PKM application:** Already handled by redb schema versioning (`check_rebuild_needed`). Keep this.

---

## 7. Editor Buffer vs. Filesystem

### 7.1 Editor Buffer is Authoritative for Open Documents

**Pattern:** On `didChange`, use the content sent by the editor (not re-read from disk). Only `didChangeWatchedFiles` (external changes to non-open files) should trigger disk reads.

**Who uses it:**
- **rumdl LSP**: Editor sends full content on `didChange`; worker re-indexes from editor's content.
- **Marksman**: `didChange` re-parses from editor buffer content.
- **Markdown Oxide**: `did_change` updates in-memory vault with editor content.

**Anti-pattern (Markdown Oxide bug):** `did_change_watched_files` triggers `reconstruct_vault`, which **drops the entire vault and rebuilds from disk**, losing unsaved buffer state. This is documented as a known deficiency.

**Why it works:** The editor has the most up-to-date content (including unsaved changes). Re-reading from disk would lose the user's in-progress edits.

**PKM application:** `did_open` → add to in-memory store. `did_change` → update with editor content, re-parse, re-index. `did_close` → mark as "read from disk on next access". `did_change_watched_files` → for non-open files, re-read from disk; for open files, ignore.

### 7.2 File Watch Events (didChangeWatchedFiles)

**Pattern:** Use `workspace/didChangeWatchedFiles` for external filesystem changes, but debounce and be selective.

**Who uses it:**
- **TypeScript**: `useClientFileWatcher` option (TS 5.4+) — editor provides file watch events instead of tsserver watching all files directly. Reduces filesystem watchers in large workspaces.
- **rumdl LSP**: Debounced `IndexUpdate::FileChanged` (100ms) triggers `update_single_file`.

**PKM application:** Watch `**/*.md` and config files. Debounce events. For files not open in editor, re-read and update. For open files, skip (editor is authoritative).

---

## 8. Query Optimization

### 8.1 Memoization with Early Cutoff

**Pattern:** Memoize query results. When an input changes, check if the query result actually changed before propagating to dependents.

**Source:** Salsa (rust-analyzer):
- Global version number incremented on each input change (O(1)).
- On query revalidation: flood forward to inputs, check if any changed. If not, just increment version numbers (no recomputation). If yes, flood backward, recompute, stop at early cutoff.
- **Durability system**: Divide inputs into durability levels (volatile/normal/durable). User code is volatile; dependencies are durable. When a volatile input changes, skip re-checking all durable queries entirely.

**PKM application:** For a PKM vault, tags and file classifications are "durable" (rarely change). Links and headings are "volatile" (change with edits). A link edit shouldn't trigger re-evaluation of tag queries for unrelated files.

### 8.2 Lazy Evaluation

**Pattern:** Don't compute results until they're requested. Expensive operations (diagnostics, cross-reference resolution) should be lazy.

**Who uses it:**
- **Salsa**: All work is done when a fresh result is requested, not eagerly on input change.
- **BSL Language Server**: `Lazy<T>` wrapper for deferred computation. Each field in `DocumentContext` is `Lazy` — computed on first access, cached thereafter.
- **DomainLang proposal**: Defer validation until first access instead of eagerly at startup.

**PKM application:** Backlink computation, tag aggregation, and diagnostics should be lazy. Compute on first query, cache, invalidate on structural change.

---

## 9. Concurrency & Locking

### 9.1 RwLock for Shared State

**Pattern:** Use `RwLock` (not `Mutex`) for the workspace index. Multiple readers (completions, gotodef, hover) can run concurrently; only writes (edits, file creation) take exclusive lock.

**Who uses it:**
- **Markdown Oxide**: `Arc<RwLock<Option<Vault>>>`.
- **Marksman**: Implicit in F# async model.
- **rumdl LSP**: `Arc<RwLock<WorkspaceIndex>>`.

**Why it works:** LSP requests are overwhelmingly reads. A RwLock allows concurrent read access, only blocking on writes.

**PKM application:** Already using `Arc<RwLock<Option<Vault>>>` pattern. Continue this.

### 9.2 Lock-Free Atomics for Coordination

**Pattern:** Use `AtomicU64`/`AtomicBool` for lightweight coordination (debounce counters, cancellation tokens) instead of locks.

**Who uses it:**
- **Markdown Oxide**: `AtomicU64` generation counter for diagnostics debouncing.
- **ruff server**: `AtomicBool`-backed cancellation token.
- **rust-analyzer**: Salsa's global revision counter is atomic.

**Why it works:** Atomics are lock-free and don't cause contention. Perfect for coordination signals that don't carry data.

**PKM application:** Use atomics for debounce counters, cancellation tokens, and revision tracking.

---

## 10. Specific PKM Considerations

### 10.1 Wiki-Link Resolution as a First-Class Index

**Pattern:** Build a dedicated index for wiki-link resolution (file stems → paths, headings → anchors) that's separate from the general file index.

**Source:** Marksman uses `SuffixTree<CanonDocPath, Doc>` for path-based lookups and `docsBySlug: Map<Slug, Set<Doc>>` for title-based lookups. This two-level index enables O(1) wiki-link resolution.

**PKM application:** Maintain a `HashMap<String, Vec<PathBuf>>` mapping file stems and titles to paths. Update incrementally on file create/delete/rename. This is the core of wiki-link completion and gotodef.

### 10.2 Tag Index as a Separate Concern

**Pattern:** Tags (`#tag`, YAML frontmatter tags) should be indexed separately from links. Tag queries (find all files with tag X) are a different access pattern than link queries (find all files linking to Y).

**Source:** Traces already has `PATHS_BY_TAG` and `TAGS_BY_PATH` tables in redb. This is the right design.

**PKM application:** Keep the dual tag index. Update incrementally when tags change in a file. Tag changes don't affect link resolution, so they should be independently updateable.

### 10.3 Batch Operations for Workspace Scans

**Pattern:** When scanning the workspace, batch reads and writes to minimize lock contention and I/O overhead.

**Source:** clangd's background index uses a thread pool with a work queue. Each thread reads one file, builds its shard, and writes it. No cross-thread coordination except the shared output.

**PKM application:** During `refresh()`, collect all changed paths, then batch-parse them in parallel via `rayon`, then batch-upsert into redb in a single write transaction.

---

## 11. Anti-Patterns to Avoid

### 11.1 Full Vault Rebuild on Any Filesystem Event
**Source:** Markdown Oxide's `reconstruct_vault` on `did_change_watched_files`. Drops all in-memory state, rebuilds from disk, loses buffer content.

### 11.2 No Debouncing on Diagnostics
**Source:** Markdown Oxide's pre-fix behavior — diagnostics recomputed on every keystroke, causing server hangs with many open files.

### 11.3 Making Core Logic Async
**Source:** Taplo's hand-rolled async stub — described by its own maintainer as "a mess." The core parser/formatter should be synchronous.

### 11.4 Eager Validation at Startup
**Source:** DomainLang LSP — `loadAdditionalDocuments()` builds all imported documents with `{ validation: true }`, delaying workspace readiness.

### 11.5 mtime-Based Cache Invalidation
**Source:** DomainLang LSP — `readAndCacheManifest()` compares `stat.mtimeMs`. Unreliable on NFS, Docker, and some macOS edge cases. Use content hashing instead.

---

## 12. Summary: Recommended Architecture for a PKM LSP

```
┌─────────────────────────────────────────────────┐
│  Editor (stdin/stdout)                          │
│  tower-lsp transport layer (async, tokio)       │
├─────────────────────────────────────────────────┤
│  Request handlers (sync, block_in_place)        │
│  - completions, gotodef, hover, diagnostics     │
│  - debounced via AtomicU64 generation counter   │
├─────────────────────────────────────────────────┤
│  In-memory workspace index                      │
│  - HashMap<PathBuf, ParsedDoc>                  │
│  - HashMap<String, Vec<PathBuf>> (wiki-links)   │
│  - HashMap<String, Vec<PathBuf>> (tags)         │
│  - RwLock for concurrent reads                  │
├─────────────────────────────────────────────────┤
│  Persistent store (redb)                        │
│  - warm-start cache for refresh()               │
│  - content-hash based invalidation              │
│  - version-pinned schema                        │
├─────────────────────────────────────────────────┤
│  Background workers (rayon thread pool)         │
│  - workspace scanning                           │
│  - diagnostics computation                      │
│  - cross-file dependency tracking               │
└─────────────────────────────────────────────────┘
```

### Key Design Rules

1. **Editor buffer is authoritative** for open documents.
2. **Parse full document** on each edit; spend effort on cross-file diff, not intra-document incremental parsing.
3. **Debounce diagnostics** (300ms, generation counter).
4. **Structural diff** for cross-file invalidation — only cascade when headings/links/tags actually change.
5. **Persist index** for warm-start; validate against filesystem on startup.
6. **Memory budget** with warning/critical thresholds and degradation modes.
7. **Synchronous core** for parsing/indexing; async only for transport.
8. **Parallel everything** via `rayon` — scanning, parsing, diagnostics.
9. **Lazy computation** — compute backlinks/tags/diagnostics on first query, cache, invalidate on structural change.
10. **Content-hash** for all cache keys; never use mtime.
