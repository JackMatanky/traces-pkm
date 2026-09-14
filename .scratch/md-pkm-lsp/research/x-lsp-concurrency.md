# Rust LSP Concurrency & Cancellation Patterns — Research Summary

## 1. tower-lsp / tower-lsp-server Concurrency Model

### Architecture
- **tower-lsp** (ebkalderon, v0.20): The original. Each incoming request is spawned as a separate tokio task. The `LspService` implements `tower::Service<Request>`.
- **tower-lsp-server** (tower-lsp-community, v0.22-0.23): Community fork. Same fundamental model. `Server::new(stdin, stdout, socket).serve(service)` drives the event loop.

### Concurrency Strategy
Messages are read sequentially from stdin. Each message is routed to an async handler. **Pending tasks are buffered and executed concurrently**, with a configurable `concurrency_level` (default: 4 concurrent tasks). The framework processes client-to-server and server-to-client messages concurrently with each other.

### Known Issue — Concurrent Execution Bug (Issue #284)
- **URL**: https://github.com/ebkalderon/tower-lsp/issues/284
- The core problem: tower-lsp unconditionally executes pending async tasks concurrently **without regard for execution correctness**. While outgoing message *ordering* is preserved, the *execution order* of handlers is not guaranteed.
- Example: A `self.client.log_message().await` call in one handler might yield, allowing the next request to start executing concurrently, causing state drift between client and server.
- This was flagged as a **bug** and remains open. The recommended fix: execute client-to-server handlers **sequentially** (in order received), but process client-to-server and server-to-client messages concurrently with each other.
- **Impact for design**: If using tower-lsp-server, the sequential-by-default behavior is the safer choice. The concurrency_level setting exists but should be used carefully.

### $/cancelRequest Handling
- `LspService` docs state: "Pending requests can be canceled by issuing a $/cancelRequest notification."
- tower-lsp-server internally tracks pending requests by ID and can cancel them. The `CancellationToken` from `tower-lsp-server` is passed to request handlers.
- However, cancellation is **cooperative** — the handler must check the token. There's no forced kill.

### Actionable Pattern for tower-lsp-server
```rust
// Typical tower-lsp-server usage
let (service, socket) = LspService::new(|client| MyServer::new(client));
Server::new(stdin, stdout, socket).serve(service).await;

// The server receives CancellationToken in request params
// Must check token.is_cancelled() at logical boundaries
```

---

## 2. Biome's Concurrency Model

### Architecture
- Biome uses a **server-client daemon architecture** (https://next.biomejs.dev/internals/architecture).
- A long-running daemon process handles requests from the editor and CLI.
- The architecture is described as "inspired by rust-analyzer."
- Parser uses an internal fork of `rowan` (Green/Red tree pattern) for full-fidelity CST.

### Concurrency Approach
- Biome's official docs state: "Biome uses a server-client architecture to run its tasks." The Daemon section is marked "Work in progress."
- From the architecture page: The daemon is a long-running server that processes requests. The key insight is that Biome's **core analysis (parsing, linting, formatting) is synchronous** — the daemon handles I/O and request dispatching.
- The "tokio at transport, sync core underneath" pattern (cited in the user's context) is: tokio handles the LSP transport (stdio/TCP), but the actual linting/formatting work runs synchronously on a rayon thread pool or similar.
- Biome 2.0 added "better LSP support" and the roadmap mentions multi-file analysis.

### Key Insight
Biome avoids the complexity of async cancellation entirely by keeping the core synchronous. The tokio runtime only handles transport. This is simpler and avoids cooperative cancellation complexity.

---

## 3. rust-analyzer's Concurrency Model

### Architecture (from https://github.com/rust-lang/rust-analyzer/blob/master/docs/dev/architecture.md)
- **GlobalState** holds the server state. The `main_loop` accepts requests and sends responses.
- Requests that modify state or might block user typing are handled **on the main thread**.
- All other requests are processed **in background threads**.
- Uses **salsa** (incremental computation framework) for query-based analysis.

### Cancellation Model
This is the gold standard for LSP cancellation:

1. **salsa maintains a global revision counter**. When applying a change, salsa bumps this counter and waits until all other threads using salsa finish.
2. If a thread doing salsa-based computation notices the counter has been incremented, it **panics with a special value** (`Canceled::throw`).
3. The `ide` crate catches this panic and transforms it into `Result<T, Cancelled>`.
4. **rust-analyzer requires unwinding** — panics are used as the cancellation mechanism.
5. Each LSP request is protected by `catch_unwind`.

### Snapshot-Based Concurrency
- `AnalysisHost` is the mutable state holder (like Clojure's atom).
- `Analysis` is an **immutable snapshot** of the world state at a point in time.
- All analysis queries run against the snapshot. The snapshot is immutable, so queries can run concurrently without locking.
- When a change arrives, a new snapshot is created. Old queries continue on their old snapshot.
- This is the "immutable snapshot" pattern: **no locks needed for reads**, and writes create new snapshots rather than mutating in place.

### VFS (Virtual File System)
- VFS provides consistent snapshots of the file system.
- Files are identified by opaque `FileId`, not paths.
- Current issue (#18948): VFS is being redesigned — the current setup is "brittle and tends to break very easily."

### Key Design Principles
- "The server is stateless, a-la HTTP." Each request should include enough info to recreate context from scratch.
- "Typing inside a function's body never invalidates global derived data."
- "rust-analyzer should be partially available even when the build is broken."

### Latency Targets
- Issue #17491 ("A Plan for Making Rust Analyzer Faster") targets **sub-100ms autocomplete latencies** on larger projects.
- Parallel VFS loading is planned.

---

## 4. LSP Cancellation Patterns in General

### LSP Spec (3.17/3.18)
- **URL**: https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.18/specification.md
- Client sends `$/cancelRequest` notification with the request ID to cancel.
- **A canceled request still must return a response** — it cannot be left open/hanging.
- Error code `RequestCancelled` (-32800) is the standard response for canceled requests.
- The spec explicitly says: "if the server implementation uses a single threaded synchronous programming language then there is little a server can do to react to a $/cancelRequest notification."

### Error Codes
- `-32800` `RequestCancelled`: Client canceled the request
- `-32801` `ContentModified`: Document content modified outside normal conditions
- `-32802` `ServerCancelled`: Server cancelled the request (since 3.17)
- `-32803` `RequestFailed`: Request failed but was syntactically correct (since 3.17)

### Common Approaches

**Approach 1: Cooperative Cancellation (Standard)**
- Pass a `CancellationToken` to each request handler.
- Check `token.is_cancelled()` at loop boundaries / await points.
- On cancellation, return `RequestCancelled` error.
- Used by: rust-analyzer (via salsa panics), clangd, ElixirLsp, most production LSPs.

**Approach 2: Let Cancelled Requests Complete**
- Simplest: don't check for cancellation at all. Let the request finish and send the response.
- The client will discard the response anyway.
- Used by: Simple LSPs where requests are fast (< 100ms).
- **This is viable for fast LSPs** where the cost of completing is less than the complexity of cooperative cancellation.

**Approach 3: Supersede (Implicit Cancellation)**
- When a new request of the same type arrives (e.g., new hover request), implicitly cancel the old one.
- The server doesn't need $/cancelRequest — it just stops caring about the old result.
- rust-analyzer does this: "typing any symbol will cancel outgoing requests."

**Approach 4: Content-Modified Detection**
- When a request completes, check if the document has been modified since the request was started.
- If modified, return `ContentModified` error or discard the result.
- Simple and effective for fast requests.

### clangd's Approach
- **URL**: https://reviews.llvm.org/D52004
- All LSP methods run in cancelable scopes managed by `JSONRPCDispatcher`.
- Cancellation request processing is in `JSONRPCDispatcher`.
- Cancelable scopes wrap each handler, allowing cancellation at any await point.

### ElixirLsp Approach
- **URL**: https://elixir-lsp.hexdocs.pm/readme.md
- Router DSL with `with_cancel/2` and `check_cancel!/1` helpers.
- Cooperative: handlers explicitly call `check_cancel!()` at logical boundaries.

---

## 5. Performance Considerations

### Latency Targets for Interactive LSP Operations
Based on the lsp-bench framework (https://github.com/mmsaki/lsp-bench):
- **Completion**: < 50ms (p50), < 100ms (p95) — users expect instant feedback
- **Hover**: < 100ms (p50), < 200ms (p95) — slightly more tolerant
- **Go to Definition**: < 100ms (p50), < 300ms (p95)
- **Diagnostics/Linting**: < 200ms (p50) for single file
- **Find References**: < 500ms (p50) — most tolerant
- **Initialize**: < 1000ms — one-time cost, acceptable

### General Principles
- **Interactive operations** (completion, hover, signature help): must be < 100ms
- **Navigation** (goto def, find refs): can be up to 300ms
- **Background operations** (linting, diagnostics): can be slower but should stream results
- **The 100ms rule**: If a response takes > 100ms, the user notices a delay. If > 300ms, it's annoying.

### How Fast LSPs Handle Thoroughness vs Speed
- **Incremental computation** (salsa pattern): Only recompute what changed
- **Snapshot isolation**: Don't block reads on writes
- **Debouncing**: Batch rapid changes (e.g., typing) before processing
- **Prioritization**: Process requests in priority order (completion > hover > background linting)
- **Partial results**: Return early with partial data, update later (e.g., streaming diagnostics)

---

## 6. Read/Write Lock Patterns for LSP

### The Immutable Snapshot Pattern (rust-analyzer)
This is the dominant pattern in Rust LSPs:

```
AnalysisHost (mutable, single writer)
  └─ apply_change() → creates new Analysis snapshot
  
Analysis (immutable, shared reader)
  └─ All queries run against this snapshot
  └─ No locking needed — snapshot is immutable
  └─ Multiple queries can run concurrently
```

### SharedIndexState Pattern (rumdl)
```rust
pub(crate) struct SharedIndexState {
    pub(crate) workspace_index: Arc<RwLock<WorkspaceIndex>>,
    pub(crate) index_state: Arc<RwLock<IndexState>>,
    pub(crate) workspace_roots: Arc<RwLock<Vec<PathBuf>>>,
    pub(crate) documents: Arc<RwLock<HashMap<Url, DocumentEntry>>>,
}
```
- Uses `Arc<RwLock<T>>` for shared mutable state.
- Multiple readers, single writer.
- The index worker runs on a separate tokio task, communicating via `mpsc` channels.

### Common Patterns

**Pattern 1: RwLock with Snapshot**
- Writer acquires write lock, updates state, releases lock.
- Readers acquire read lock, take snapshot, release lock.
- Subsequent reads work on the snapshot without holding the lock.
- Pros: Simple, well-understood. Cons: Write lock blocks readers.

**Pattern 2: Channel-Based State Updates**
- State changes flow through `mpsc` channels.
- A single task owns the state and processes updates sequentially.
- Readers get state via channel response or shared snapshot.
- Pros: No locking, natural serialization. Cons: Adds latency for state reads.

**Pattern 3: Double-Buffered State (rust-analyzer style)**
- Two copies of state: "front buffer" (read by queries) and "back buffer" (written by main thread).
- On update: write to back buffer, then swap atomically.
- Queries that started before the swap continue on the old front buffer.
- Pros: Zero contention between reads and writes. Cons: Memory cost of two copies.

**Pattern 4: COW (Copy-on-Write)**
- State is `Arc<T>`. On mutation, clone the data, modify the clone, swap the Arc.
- Readers hold old Arc (immutable, no lock). Writer creates new Arc.
- Pros: No locking at all. Cons: Clone cost on mutation.

---

## 7. rumdl's Concurrency Model

### Architecture
- rumdl uses **tower-lsp v0.20** (the original, not the community fork).
- Tokio runtime for async I/O.
- LSP module at `src/lsp/` with: server, linting, index_worker, completion, configuration, relint, symbols.

### Concurrency Structure
Based on the digest:

1. **Main LSP Server** (`RumdlLanguageServer`): Handles incoming LSP requests via tower-lsp's async dispatch.

2. **Index Worker** (separate tokio task): Runs its own event loop via `tokio::select!`, receiving `IndexUpdate` messages over an `mpsc` channel. Has debouncing (100ms). Manages workspace index, document state, and configuration.

3. **Shared State** via `Arc<RwLock<T>>`:
   - `workspace_index: Arc<RwLock<WorkspaceIndex>>`
   - `index_state: Arc<RwLock<IndexState>>`
   - `workspace_roots: Arc<RwLock<Vec<PathBuf>>>`
   - `documents: Arc<RwLock<HashMap<Url, DocumentEntry>>>`

4. **Relint Worker** (separate tokio task): Receives `RelintRequest` messages over `mpsc`, runs linting on a separate task to avoid blocking the main server.

5. **Config Resolver** (`ConfigResolver`): Shared configuration state.

### Threading Model
- Tokio multi-threaded runtime (features = ["full"]).
- Index worker and relint worker run as separate tokio tasks.
- The linting itself uses rumdl's core library (synchronous), likely run via `tokio::task::spawn_blocking` or rayon.
- rumdl's core library has a `parallel` feature using rayon for CLI parallelism.

### Key Observations
- rumdl follows the "Biome-like" pattern: tokio for transport, sync core for analysis.
- Uses channel-based communication between components rather than direct shared state mutation.
- Debouncing prevents rapid re-indexing during typing.
- The `Arc<RwLock<T>>` pattern is used for state that needs to be read by multiple tasks.

---

## Summary of Actionable Design Patterns

### For a New LSP Server in Rust (2025-2026)

1. **Use `tower-lsp-server`** (community fork, actively maintained). Set `concurrency_level(1)` for safety, or be very careful with concurrency.

2. **For cancellation**, one of:
   - **Simple LSP (fast requests)**: Skip cancellation entirely. Let requests complete. Return results or discard. This is fine if all requests are < 100ms.
   - **Complex LSP (slow requests)**: Use cooperative cancellation with `CancellationToken`. Check at loop boundaries and major await points.
   - **If using salsa**: Follow rust-analyzer's panic-based cancellation pattern.

3. **For concurrency**, prefer:
   - **Immutable snapshot pattern**: Create snapshots of state, run queries against them. No locking needed for reads.
   - **Channel-based communication**: Use `mpsc` channels between components rather than shared mutable state.
   - **Debouncing**: Batch rapid changes before processing.

4. **For performance targets**:
   - Completion/hover: < 50ms (p50)
   - Navigation: < 100ms (p50)
   - Diagnostics: < 200ms (p50)
   - Use incremental computation where possible

5. **The "Biome pattern"** (tokio transport + sync core) is the safest and simplest approach for new LSPs:
   - tokio handles stdio/TCP transport
   - Core analysis is synchronous, runs on rayon thread pool or `spawn_blocking`
   - No async cancellation complexity in the core
   - Easy to reason about and debug

### Sources
- tower-lsp Issue #284: https://github.com/ebkalderon/tower-lsp/issues/284
- tower-lsp-server docs: https://docs.rs/tower-lsp-server/latest/tower_lsp_server/
- Biome Architecture: https://next.biomejs.dev/internals/architecture
- rust-analyzer Architecture: https://github.com/rust-lang/rust-analyzer/blob/master/docs/dev/architecture.md
- rust-analyzer Performance Plan: https://github.com/rust-lang/rust-analyzer/issues/17491
- LSP Spec 3.18: https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.18/specification.md
- LSP Cancellation Discussion: https://github.com/microsoft/language-server-protocol/issues/185
- clangd Cancellation: https://reviews.llvm.org/D52004
- lsp-bench: https://github.com/mmsaki/lsp-bench
- ElixirLsp: https://elixir-lsp.hexdocs.pm/readme.md
- rumdl digest: /Users/jack/Documents/41_personal/traces-pkm/docs/digests/lsp_rvben-rumdl-digest.txt
