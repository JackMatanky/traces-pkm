# Concurrency & cancellation model

Type: grilling
Blocked by: 09, 10
Status: resolved

## Question

Given the runtime model (ticket 09) and analysis-host design (ticket 10), decide the concrete concurrency model for handling LSP requests:

- Thread-per-request vs a bounded worker pool vs a single-threaded event loop reusing the existing all-synchronous Index/Query/Schema code paths directly.
- How `$/cancelRequest` (see `docs/refs/lsp_spec.md`'s Cancellation section) propagates into a synchronous, non-async call stack — Rust has no forced preemption, so cancellation must be cooperative (checked at loop boundaries in e.g. a linear index scan) or coarse (drop the whole request's thread/task and discard its result on completion). Ground this against `QueryArch` findings: source-expression resolution is currently a linear `(0..index.entries().len())` scan (`src/query/service.rs:132-139`) with no existing cancellation checkpoints — decide whether/where checkpoints get added, or whether requests are just cheap enough (target workspace sizes, see ticket 25) that cancellation mid-scan is unnecessary and only "supersede in the queue before starting" cancellation is implemented.
- Read/write exclusivity around index refresh: can queries run concurrently with an in-progress refresh (reading the previous immutable snapshot until the new one swaps in), or does refresh block new requests.
- Interaction with `$/progress` (see spec) for long-running operations like full workspace reindex on startup.

Blocks: 33(performance targets, since concurrency model bounds achievable latency).

## Answer

### 1. Request dispatch: tower-lsp-server with `concurrency_level(1)`

tower-lsp-server's dispatch uses `buffer_unordered(N)` on a single task. With N=1, only one handler future is polled at a time — true sequential execution. Sync handlers (no `.await`) run inline, blocking the pipeline until they return. This is not a limitation; it's the correct model for a single-client LSP with a sync core.

Why not N>1: tower-lsp-server has a known ordering bug (#284) where `buffer_unordered(N>`1`) executes handler futures out of arrival order, causing state drift when handlers share mutable state. The framework provides no mechanism for custom dispatch, per-method concurrency modes, or request prioritization. Sync handlers at N>1 block the `buffer_unordered` pipeline anyway (they never yield at `.await` points), so concurrent buffering provides no benefit.

Why not `spawn_blocking` per handler: every handler would need manual `tokio::task::spawn_blocking` wrapping, losing tower-lsp-server's `$/cancelRequest` support (spawn_blocking threads can't be interrupted), adding per-handler bridge overhead, and introducing sync/async boundary complexity (Biome's `catch_lsp_operation` pattern catches panics and Salsa cancellation at this boundary — non-trivial code).

Why not lsp-server (crossbeam-channel): ticket 09 evaluated this. `lsp-server` gives full dispatch control but requires implementing JSON-RPC framing, request routing, progress, and cancellation manually. The ~125-line scaffold is the floor; a full-featured LSP is significantly more. Tower-lsp-server provides all of this. The framework choice stands.

`concurrency_level(1)` also implicitly disables `$/cancelRequest` support (per tower-lsp-server docs), which aligns with the stale-result discard strategy (decision 2).

### 2. Cancellation: stale-result discard

No production Rust LSP implements cooperative `$/cancelRequest` handling. rust-analyzer detects staleness via salsa's revision counter and returns `ContentModified`. Biome catches cancellation at the sync/async boundary. The industry-standard pattern: let the handler finish, discard the result if stale.

With N=1 and <100ms handler budgets, there is no mid-execution cancellation to implement. When the user types quickly, multiple completion/hover requests fire — the old one finishes, its result is discarded, the new one runs. When the document changes mid-handler, return `ContentModified` error.

No `CancellationToken` infrastructure is needed. Cooperative cancellation (checking a token at loop boundaries in e.g. `QueryService::execute`'s linear scan) is premature — add only if ticket 33's performance work proves handlers exceed the budget at target scale.

### 3. Read/write exclusivity during index refresh: sequential

With N=1 sequential dispatch, refresh and queries never overlap. The refresh handler (triggered by `workspace/didChangeWatchedFiles`) performs the full cycle — filesystem diff, parallel rayon parse, redb persist, `Arc<FileIndex>` swap — then returns. The next handler sees the new `FileIndex`.

The swap is an atomic `Arc` pointer replacement (instantaneous, happens between handlers). No `RwLock` needed — `FileIndex` is immutable after construction (ticket 10). Queries before the swap see the old snapshot; queries after see the new one. With sequential dispatch, there is no "mid-swap" state.

If refresh time becomes a problem at large vault scale, background refresh can be layered on later (kick off diff/parse/persist on a `spawn_blocking` thread, return immediately, swap when complete). But the initial model should be the simplest correct thing.

### 4. `$/progress`: not implemented initially

Startup reindex is a one-time cost. Adding `$/progress` requires threading progress callbacks through `IndexerService::refresh()`, which is invasive for a feature that may never be visibly used (most LSP clients show their own loading state during initialization). If ticket 33 shows startup reindex exceeds an acceptable threshold (e.g. >2s for a 1000-note vault), `$/progress` can be added as a targeted follow-up. Not a concurrency decision — UX polish.

### Summary

| Decision                | Resolution                           | Key reasoning                                                        |
| ----------------------- | ------------------------------------ | -------------------------------------------------------------------- |
| Request dispatch        | tower-lsp-server, `concurrency_level(1)` | Sequential execution; eliminates ordering bug; sync handlers native  |
| Cancellation            | Stale-result discard                 | Industry standard; no production Rust LSP does cooperative cancel    |
| Refresh exclusivity     | Sequential (refresh before next)     | N=1 makes this natural; simplest correct model                       |
| `$/progress`            | Not initially                        | UX polish, not a concurrency decision                                |
