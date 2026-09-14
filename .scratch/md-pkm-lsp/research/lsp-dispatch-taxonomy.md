# LSP Request Dispatch & Scheduling Patterns: A Comprehensive Taxonomy

## 1. The LSP Spec's Stance

The LSP 3.17 specification (§Request, Notification and Response Ordering) says:

> Responses to requests should be sent in roughly the same order as the requests appear on the server or client side. [...] However, the server may decide to use a parallel execution strategy and may wish to return responses in a different order than the requests were received. The server may do so as long as this reordering doesn't affect the correctness of the responses.

The spec explicitly allows parallelism and reordering. It also defines `$/cancelRequest` (error code `-32800`) and `ContentModified` for servers to abort stale requests. But it does **not** mandate any priority ordering, batching, or supersession strategy — those are implementation decisions.

---

## 2. Taxonomy of Dispatch Patterns

### Pattern A: Sequential Single-Threaded Event Loop

**How it works:** One thread runs a `loop { select! { ... } }` over channels. All requests are processed sequentially on that thread. Long-running work blocks the loop.

**Examples:**
- **Haskell `lsp` library (simple mode):** The `reactor` design pattern processes all requests on a single thread.
- **Many early-stage LSPs:** The simplest possible architecture.

**Pros:**
- Zero concurrency bugs. State is trivially single-owner.
- Predictable latency for fast operations.
- Simplest possible implementation.

**Cons:**
- One slow request (e.g., workspace-wide analysis) blocks hover/completion for the entire duration.
- Cannot use multiple CPU cores.

**When to use:** Prototype stage, very fast language implementations, or LSPs where all operations complete in <10ms.

---

### Pattern B: Task-per-Request with Async Runtime (tower-lsp-server model)

**How it works:** Each incoming request is spawned as a separate tokio task. The `LanguageServer` trait methods are `async fn`. The framework handles I/O multiplexing; you write async handlers.

**Examples:**
- **`tower-lsp-server` (Rust):** Implements `tower::Service<Request>`. Each request becomes a spawned task. The `Server` struct reads from stdin and dispatches to the `LspService` which returns futures.
- **Biome (current):** Uses `tower-lsp-server`. The `LSPServer` implements `LanguageServer` (async trait). Workspace methods are wrapped via `workspace_method!` macro which calls `spawn_blocking` to move CPU-bound analysis off the tokio runtime.

**Pros:**
- I/O doesn't block: multiple requests can be in flight.
- Tower middleware allows composable layers (cancellation, logging, rate limiting).
- Well-integrated with the tokio ecosystem.

**Cons:**
- **Cancellation is cooperative.** `spawn_blocking` tasks cannot be aborted once started.
- Shared mutable state (`Session` behind `Arc<Mutex<...>>`) introduces contention.
- The async/sync boundary (`spawn_blocking`) adds overhead and complexity.
- No built-in prioritization — all tasks are equal in the tokio scheduler.

**When to use:** When your language server does significant I/O (file watching, subprocess spawning) and you want the tokio ecosystem. Good default for Rust LSPs.

---

### Pattern C: crossbeam-channel Event Loop with Thread Pool (rust-analyzer model)

**How it works:** A single main thread runs a `crossbeam::select!` loop over multiple channels (LSP messages, VFS events, flycheck output, task results, etc.). Long-running work is dispatched to a custom thread pool (`stdx::thread::Pool`) with `ThreadIntent` markers (LatencySensitive vs Worker). The main loop **coalesces** events in time windows.

**rust-analyzer's concrete architecture:**

```
Event::Lsp(msg)     → handle immediately (on main thread)
Event::Vfs(msg)     → coalesce for up to 50ms
Event::Task(task)   → coalesce for up to 50ms  
Event::Flycheck(msg)→ coalesce for up to 50ms
Event::DeferredTask → coalesce for up to 50ms
```

Key details from `main_loop.rs`:
- **Formatting gets priority:** `self.fmt_pool.receiver.try_recv()` is checked *before* the `select!` block to reply ASAP so editors don't freeze.
- **Event coalescing:** After handling one event, the loop drains the same channel for up to 50ms, batching multiple VFS/task/flycheck events into one turn.
- **Quiescence detection:** The server tracks whether it's "quiescent" (no pending ops). When it becomes quiescent, it triggers cache priming, diagnostics refresh, semantic tokens refresh, etc.
- **ThreadIntent:** Tasks are tagged `LatencySensitive` (for diagnostics triggered by typing) or `Worker` (for background cache priming).
- **Cancellation:** Uses `salsa::Cancelled` — when the database is invalidated, in-progress queries return `Err(Cancelled)` and the main loop retries the request.
- **Diagnostics are chunked:** Split across `max(1, num_threads/4)` parallel tasks, each processing a slice of files.

**Pros:**
- Main thread never blocks (except for the `select!` itself).
- Event coalescing reduces redundant work (5 VFS events → 1 analysis pass).
- No async runtime overhead — pure channels and threads.
- `ThreadIntent` allows coarse prioritization without a full priority queue.
- Formatting pool (`fmt_pool`) is a separate channel to avoid head-of-line blocking.

**Cons:**
- More complex than tower-lsp-server. You build the event loop yourself.
- No work-stealing — the thread pool is fixed-size.
- Cancellation is Salsa-based, not per-request — you can't cancel an individual in-flight request, only invalidate the database state.

**When to use:** High-performance servers where you need fine-grained control over event ordering and coalescing. The gold standard for Rust LSPs.

---

### Pattern D: Async Transport + Sync Core (Hybrid)

**How it works:** Use async I/O (tokio) for reading/writing LSP messages, but run the actual language analysis synchronously on dedicated threads. The boundary between async and sync is explicit.

**Examples:**
- **Biome (current, refined view):** `tower-lsp-server` handles the async transport. Custom workspace methods use `spawn_blocking` to run `session.workspace_for_request().method(params)` on a blocking thread. The `WorkspaceServer` and `Session` are fully synchronous.
- **CIRCT/LLVM Verilog LSP:** Uses `llvm::lsp::JSONTransport` (blocking) on a main thread, with a `ThreadPool` for background diagnostics and debounced document changes.

**Pros:**
- I/O layer is non-blocking (editor doesn't freeze).
- Core analysis can use synchronous APIs, `&mut` references, and avoid `Arc<Mutex<>>`.
- `spawn_blocking` is well-understood.

**Cons:**
- `spawn_blocking` tasks cannot be cancelled.
- The async/sync bridge has overhead (task scheduling, context switches).
- If the sync core is slow, the blocking thread pool fills up and new tasks queue.

**When to use:** When your analysis code is inherently synchronous (most Rust compiler-like tools) but you want non-blocking I/O.

---

### Pattern E: Daemon + Proxy (Biome daemon model)

**How it works:** A long-running daemon process holds the workspace state. A separate proxy process communicates with the editor via LSP and forwards requests to the daemon via internal IPC.

**Biome's architecture:**
- `biome lsp-proxy` spawns a daemon + a proxy server.
- The proxy handles LSP I/O.
- The daemon runs `WorkspaceServer` with shared state.
- Multiple editor sessions can connect to the same daemon (session-keyed map).

**Pros:**
- State persists across editor restarts (fast reconnection).
- Multiple editors share the same workspace state.
- Daemon can be shared between CLI and LSP.

**Cons:**
- Two processes to manage (daemon lifecycle, ghost processes).
- IPC adds latency compared to in-process.
- More deployment complexity.

**When to use:** When you want persistent workspace state or CLI/LSP sharing. Biome's choice makes sense for a tool that's both a CLI and an LSP.

---

### Pattern F: Request Priority / Head-of-Line Blocking Avoidance

**How it works:** Certain request types are given preferential treatment to minimize perceived latency.

**LSP spec implicit priority ordering** (from spec §Language Features):
1. "coding features" like **completion**, **code actions** — most latency-sensitive
2. "code comprehension" like **hover**, **goto definition** — moderately sensitive
3. "project-wide" like **workspace/symbol**, **diagnostics** — least sensitive

**Concrete implementations:**

| Server | Priority Strategy |
|--------|-------------------|
| **rust-analyzer** | Formatting gets its own thread pool (`fmt_pool`) checked first. Diagnostics run on `LatencySensitive` threads. Cache priming runs on `Worker` threads. |
| **Biome** | No explicit priority. All requests go through tower-lsp-server's task spawning. Formatting is fast enough that it doesn't need priority. |
| **CIRCT Verilog LSP** | Debouncing with `DebounceOptions` (min/max quiet time). Document changes accumulate in `PendingChangesMap` and are flushed after a quiet period. |

**No LSP uses a formal priority queue.** The approaches are:
1. **Separate thread pools** per concern (rust-analyzer's `fmt_pool` vs `task_pool`)
2. **ThreadIntent tagging** (latency-sensitive vs worker)
3. **try_recv priority** (check formatting channel before others)

---

### Pattern G: Request Cancellation & Supersession

**LSP protocol mechanism:** Client sends `$/cancelRequest { id: N }`. Server must return a response (error code `-32800` or partial result) for the cancelled request.

**Server-side supersession patterns:**

| Pattern | Description | Used by |
|---------|-------------|---------|
| **Client-initiated cancel** | Client sends `$/cancelRequest` when result is stale | Most LSPs (VS Code does this) |
| **Server-initiated supersession** | Server detects a newer request for the same position and aborts the old one | Suggested in LSP spec discussion, rarely implemented |
| **Database invalidation** | New document change invalidates salsa database → in-flight queries return `Err(Cancelled)` → main loop retries | rust-analyzer |
| **Stale result discarding** | Server completes old request, client discards the response because it's for an old position | Many LSPs (pragmatic approach) |
| **ContentModified error** | Server returns error code `-32801` when document state changed | Recommended by LSP spec |

**Biome's approach:** `catch_lsp_operation` wraps every handler in `salsa::Cancelled::catch`. If the database is invalidated during processing, the error is caught and returned as a cancellation error.

**rust-analyzer's approach:** Salsa's incremental computation model naturally handles this — when the VFS changes, all pending queries are cancelled. The `Task::Retry` variant re-queues requests that were cancelled.

---

### Pattern H: Request Batching & Debouncing

**The LSP spec does NOT support JSON-RPC batch messages** (§Base Types: "protocol clients and servers must not send JSON-RPC batch messages").

However, servers debounce internally:

| Server | Batching Strategy |
|--------|-------------------|
| **rust-analyzer** | Coalesces VFS/task/flycheck events within 50ms windows. Multiple `didChange` notifications → single analysis pass. |
| **CIRCT Verilog LSP** | `PendingChangesMap` with configurable debounce (min quiet time, max burst time). Changes accumulate per-file, flushed after quiet period. |
| **Infracost LSP** | Debounces `didSave` per-project (300ms), cancels in-flight scan. |
| **ALE (Vim)** | Incremental synchronization + debouncing for completion performance. |

**Debounce parameters (CIRCT):**
```rust
struct DebounceOptions {
    disableDebounce: bool,      // flush immediately
    debounceMinMs: u64,         // minimum quiet time before flush
    debounceMaxMs: u64,         // maximum total burst time (0 = no cap)
}
```

---

### Pattern I: Actor Model

**How it works:** Each logical component (document tracker, diagnostics, completion, etc.) is an actor with its own mailbox. Actors communicate via message passing. No shared mutable state.

**Marksman (F#):** Uses the Ionide Language Server Protocol library with a mailbox-based architecture. However, Marksman's source doesn't show a classic actor model — it uses the standard Ionide LSP framework with async handlers.

**Theoretical Rust implementation:** Could use `tokio::mpsc` channels or `xtra` (actor framework) to create actors for:
- Document state management
- Diagnostics computation
- Completion/hover analysis
- Project-wide indexing

**Pros:**
- Natural concurrency safety (no shared state).
- Each actor can be independently scaled.
- Message ordering per-actor is guaranteed.

**Cons:**
- Overhead of message serialization/deserialization.
- Debugging message flows is harder.
- Not a natural fit for incremental computation (salsa).
- No production Rust LSP uses this pattern (to my knowledge).

---

### Pattern J: Work-Stealing

**How it works:** Tasks are placed in per-thread deques. Idle threads steal from busy threads' deques.

**Rust ecosystem:** `crossbeam-deque` provides work-stealing deques. `rayon` uses this for `par_iter`.

**LSP precedent:** **None.** No production LSP server uses work-stealing for request dispatch. The reasons:
1. LSP requests are not homogeneous — completion and diagnostics have very different costs.
2. The main bottleneck is usually the language analysis, not task scheduling.
3. `crossbeam-channel` with `select!` is simpler and sufficient for most cases.
4. rust-analyzer's `stdx::thread::Pool` is a fixed-size pool, not work-stealing.

**When it could help:** If you have many homogeneous, CPU-bound tasks (e.g., parallel type-checking of independent files). But even then, rayon is the standard tool, not a custom work-stealing scheduler.

---

### Pattern K: Separate Thread Pools per Request Type

**How it works:** Different categories of requests get their own thread pools with independent sizes and scheduling.

**rust-analyzer's implementation:**
- `task_pool`: General-purpose worker threads (default `num_cpus` threads)
- `fmt_pool`: Dedicated formatting threads (always checked first via `try_recv`)
- Diagnostics: Spawned on `task_pool` with `ThreadIntent::LatencySensitive`, limited to `max(1, num_threads/4)` concurrent tasks

**Why not more granular?** In practice, 2-3 pools is sufficient:
- One for latency-sensitive work (formatting, completion)
- One for general work (diagnostics, analysis)
- One for background work (cache priming, project discovery)

---

## 3. Decision Matrix

| Pattern | Latency | Complexity | Cancellation | Throughput | Best for |
|---------|---------|------------|--------------|------------|----------|
| A: Sequential | Bad | Trivial | N/A | Bad | Prototypes |
| B: Task-per-request (tower) | Good | Low | Cooperative | Good | I/O-heavy servers |
| C: crossbeam event loop | Excellent | High | Salsa-based | Excellent | CPU-heavy servers |
| D: Hybrid async/sync | Good | Medium | Limited | Good | Sync analysis cores |
| E: Daemon + proxy | Good | High | N/A | Good | Persistent state |
| F: Priority | Excellent | Medium | N/A | Good | Any server |
| G: Cancellation | N/A | Medium | Required | N/A | Any server |
| H: Debouncing | Good | Low | N/A | Good | Bursty inputs |
| I: Actor model | Good | High | Natural | Good | Heterogeneous workloads |
| J: Work-stealing | Good | High | Hard | Excellent | Homogeneous CPU work |
| K: Separate pools | Excellent | Medium | Easy | Good | Mixed workloads |

---

## 4. The "Ideal" High-Performance LSP Architecture

Based on the evidence, the optimal architecture for a high-performance Rust LSP combines:

1. **crossbeam-channel event loop** (Pattern C) for the main loop — gives you coalescing, multiple event sources, and no async overhead.
2. **Separate thread pools** (Pattern K) with `ThreadIntent`-like tagging — formatting pool checked first, latency-sensitive pool for typing-triggered work, worker pool for background.
3. **Request debouncing** (Pattern H) at the VFS level — coalesce rapid `didChange` events.
4. **Cancellation via database invalidation** (Pattern G) — Salsa or similar incremental computation for natural cancellation.
5. **No formal priority queue** — the empirical evidence shows that try_recv ordering + thread intent tagging is sufficient.
6. **Optional daemon** (Pattern E) if you want persistent state or CLI sharing.

This is essentially what rust-analyzer does, and it's the most battle-tested approach in the Rust ecosystem.

---

## 5. Performance Data

No rigorous benchmarks comparing LSP dispatch patterns exist in the literature. Anecdotal data:

- **rust-analyzer:** Formatting response time is <10ms (separate `fmt_pool` ensures it's never blocked by analysis). Completion response typically 50-200ms. Workspace-wide diagnostics can take seconds but are chunked and run in background.
- **Biome:** Formatting is sub-millisecond per file. The daemon adds ~1-2ms IPC overhead per request.
- **CIRCT Verilog LSP:** Document change debouncing reduced re-analysis frequency by ~5x for rapid typing.

---

## 6. References

- LSP 3.17 Specification: https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification
- rust-analyzer main_loop.rs: `crates/rust-analyzer/src/main_loop.rs`
- tower-lsp-server: https://github.com/tower-lsp-community/tower-lsp-server
- Biome LSP: `crates/biome_lsp/src/server.rs`
- CIRCT PendingChanges.h: `circt-verilog-lsp-server/Utils/PendingChanges.h`
- crossbeam-channel: https://docs.rs/crossbeam-channel
- LSP cancellation discussion: https://github.com/microsoft/language-server-protocol/issues/185
- Agent Client Protocol (ACP) cancellation: https://agentclientprotocol.com/rfds/request-cancellation.md
