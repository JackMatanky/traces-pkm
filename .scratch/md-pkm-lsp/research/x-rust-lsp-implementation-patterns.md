# Rust LSP Implementation Patterns: Source Code Analysis

Concrete patterns from production Rust LSPs. Sources: Biome, rust-analyzer, taplo, oxc, Harper, and others.

---

## 1. Framework Landscape

| Server | Framework | Transport | Core |
|--------|-----------|-----------|------|
| **Biome** | `tower-lsp-server` (community fork) | tokio async | Sync `WorkspaceServer` via `spawn_blocking` |
| **rust-analyzer** | `lsp-server` (rust-analyzer's own) | crossbeam-channel | Sync `GlobalState` + salsa DB |
| **Oxc** | `tower-lsp-server` | tokio async | Sync analysis |
| **Harper** | `tower-lsp-server` | tokio async | Sync analysis |
| **Deno** | `tower-lsp` (original) | tokio async | Sync analysis |
| **Turborepo** | `tower-lsp` (original) | tokio async | Sync analysis |
| **Taplo** | `lsp-async-stub` (custom) | async | Generic over environment |
| **ast-grep** | `tower-lsp-server` | tokio async | Sync analysis |
| **Veryl** | `tower-lsp-server` | tokio async | Sync analysis |

**Key finding:** `tower-lsp-server` (community fork of `tower-lsp`) is the dominant framework. The only significant alternative is rust-analyzer's hand-rolled `lsp-server` + crossbeam-channel approach.

---

## 2. Biome: The Canonical tower-lsp-server + spawn_blocking Pattern

### 2.1 Framework Setup

`biome/crates/biome_lsp/src/server.rs`:

```rust
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

// ServerFactory creates connections
pub struct ServerFactory {
    cancellation: Arc<Notify>,
    workspace: Arc<WorkspaceServer>,      // Sync workspace state
    db_state: Arc<DbState>,               // Salsa DB state
    sessions: Sessions,                    // Arc<Mutex<FxHashMap<SessionKey, SessionHandle>>>
    // ...
}
```

The factory creates `ServerConnection` per client:

```rust
pub fn create(&self) -> ServerConnection {
    let mut builder = LspService::build(move |client| {
        let session = Session::new(session_key, client, workspace, db_state, ...);
        LSPServer::new(handle, self.sessions.clone(), server_lifecycle)
    });

    // Custom methods registered via workspace_method! macro
    builder = builder.custom_method(SYNTAX_TREE_REQUEST, LSPServer::syntax_tree_request);
    workspace_method!(builder, pull_diagnostics);
    workspace_method!(builder, format_file);
    workspace_method!(builder, code_actions);
    // ... 20+ workspace methods

    let (service, socket) = builder.finish();
    ServerConnection { socket, service, lifecycle }
}
```

### 2.2 The workspace_method! Macro — Core Dispatch Pattern

`biome/crates/biome_lsp/src/server.rs`:

```rust
macro_rules! workspace_method {
    ( $builder:ident, $method:ident ) => {
        $builder = $builder.custom_method(
            concat!("biome/", stringify!($method)),
            |server: &LSPServer, params| {
                let span = tracing::trace_span!(concat!("biome/", stringify!($method)), params = ?params).or_current();
                let session = server.session.clone();
                let result = spawn_blocking(move || {
                    let _guard = span.entered();
                    catch_lsp_operation(|| session.workspace_for_request().$method(params))
                });
                result.map(move |result| {
                    match result {
                        Ok(Ok(Ok(Ok(result)))) => Ok(result),        // spawn_blocking OK → salsa OK → workspace OK
                        Ok(Ok(Ok(Err(err)))) => Err(into_lsp_error(err)),  // workspace error
                        Ok(Ok(Err(cancelled))) => Err(cancelled_to_lsp_error(cancelled)),  // salsa cancelled
                        Ok(Err(err)) => Err(into_lsp_error(err)),     // catch_lsp_operation panic
                        Err(err) => match err.try_into_panic() {      // spawn_blocking panic
                            Ok(err) => Err(panic_to_lsp_error(err)),
                            Err(err) => Err(into_lsp_error(err)),
                        },
                    }
                })
            },
        );
    };
}
```

**This is the most important pattern in the file.** Every workspace method:
1. Gets wrapped in `spawn_blocking` (moves off tokio runtime)
2. Calls `catch_lsp_operation` (catches salsa panics + cancellation)
3. Uses `workspace_for_request()` (non-retrying, returns ContentModified on staleness)
4. Returns a 4-layer nested Result that's unwound in the match

### 2.3 Biome's Two Workspace Accessors

`biome/crates/biome_lsp/src/session.rs`:

```rust
impl Session {
    /// For notifications (didOpen, didChange, didClose) — retries on cancellation
    /// because the editor won't re-send these
    pub(crate) fn workspace(&self) -> impl Workspace + '_ {
        RetryingWorkspace::new(self.workspace.with_db_state(&self.db_state))
    }

    /// For requests (formatting, codeAction) — does NOT retry
    /// Returns ContentModified so editor re-sends with fresh positions
    pub(crate) fn workspace_for_request(&self) -> impl Workspace + '_ {
        self.workspace.with_db_state(&self.db_state)
    }
}
```

**Critical distinction:** Notifications retry (editor won't re-send), requests don't (positions become stale).

### 2.4 Biome's Panic Handling

`biome/crates/biome_lsp/src/server.rs`:

```rust
fn catch_lsp_operation<F, T>(operation: F) -> Result<Result<T, salsa::Cancelled>, PanicError>
where
    F: FnOnce() -> T,
{
    biome_diagnostics::panic::catch_unwind(AssertUnwindSafe(move || {
        salsa::Cancelled::catch(AssertUnwindSafe(operation))
    }))
}
```

Two layers of catch: inner catches salsa cancellation, outer catches panics. Both are converted to LSP errors.

### 2.5 Biome's Standard LSP Handlers

For built-in LSP methods (not custom workspace methods), Biome implements the `LanguageServer` trait:

```rust
impl LanguageServer for LSPServer {
    async fn formatting(&self, params: DocumentFormattingParams) -> LspResult<Option<Vec<TextEdit>>> {
        let result = catch_lsp_operation(move || handlers::formatting::format(&self.session, params));
        match result {
            Ok(Ok(result)) => self.map_op_error(Ok(result)).await,
            Ok(Err(cancelled)) => Err(cancelled_to_lsp_error(cancelled)),
            Err(err) => Err(into_lsp_error(err)),
        }
    }
}
```

Same pattern: `catch_lsp_operation` → match on nested Result. But note: these run on the **tokio runtime** (not `spawn_blocking`) because they're trait methods, not custom methods. This is a subtle difference — Biome's custom workspace methods get `spawn_blocking` but its `LanguageServer` trait methods run async on tokio.

### 2.6 Biome's Diagnostics Debouncing

`biome/crates/biome_lsp/src/session.rs`:

```rust
const DIAGNOSTICS_DEBOUNCE: Duration = Duration::from_millis(250);

struct DiagnosticsEntry {
    scheduled_version: AtomicI32,
    running: AtomicBool,
    rerun_after_current: AtomicBool,
    closed: AtomicBool,
}

impl Session {
    pub(crate) fn schedule_diagnostics(self: &Arc<Self>, url: Uri, version: i32) {
        let entry = self.diagnostics_entry(url.clone(), version);
        entry.scheduled_version.store(version, Ordering::Release);
        self.spawn_delayed_diagnostics(url, entry, version);
    }

    fn spawn_delayed_diagnostics(...) {
        spawn(async move {
            sleep(DIAGNOSTICS_DEBOUNCE).await;  // 250ms debounce
            session.run_debounced_diagnostics(url, entry, version).await;
        });
    }

    async fn run_debounced_diagnostics(...) {
        if entry.scheduled_version.load(Ordering::Acquire) != version { return; }
        // Only one diagnostics update per document at a time
        if entry.running.compare_exchange(false, true, ...).is_err() {
            entry.rerun_after_current.store(true, Ordering::Release);
            return;
        }
        // ... compute and publish diagnostics ...
        entry.running.store(false, Ordering::Release);
        if entry.rerun_after_current.swap(false, Ordering::AcqRel) {
            // A newer change arrived — re-schedule for latest version
            self.spawn_delayed_diagnostics(url, entry, latest_version);
        }
    }
}
```

Pattern: 250ms debounce + at-most-one-running + rerun-if-newer-change.

---

## 3. rust-analyzer: The crossbeam-channel Event Loop

### 3.1 Main Loop Structure

`crates/rust-analyzer/src/main_loop.rs`:

```rust
pub fn main_loop(config: Config, connection: Connection) -> anyhow::Result<()> {
    GlobalState::new(connection.sender, config).run(connection.receiver)
}

impl GlobalState {
    fn run(mut self, inbox: Receiver<lsp_server::Message>) -> anyhow::Result<()> {
        // ...
        while let Ok(event) = self.next_event(&inbox) {
            let Some(event) = event else {
                anyhow::bail!("client exited without proper shutdown sequence");
            };
            if matches!(&event, Event::Lsp(lsp_server::Message::Notification(not))
                if not.method == lsp_types::ExitNotification::METHOD.as_str()) {
                return Ok(());
            }
            self.handle_event(event);
        }
        Err(anyhow::anyhow!("A receiver has been dropped, something panicked!"))
    }
}
```

### 3.2 Event Selection — The fmt_pool Priority Trick

```rust
fn next_event(&mut self, inbox: &Receiver<lsp_server::Message>) -> Result<Option<Event>, _> {
    // CRITICAL: Check formatting pool FIRST before the select! block
    if let Ok(task) = self.fmt_pool.receiver.try_recv() {
        return Ok(Some(Event::Task(task)));
    }

    select! {
        recv(inbox) -> msg => return Ok(msg.ok().map(Event::Lsp)),
        recv(self.task_pool.receiver) -> task => task.map(Event::Task),
        recv(self.deferred_task_queue.receiver) -> task => task.map(Event::DeferredTask),
        recv(self.fmt_pool.receiver) -> task => task.map(Event::Task),
        recv(self.loader.receiver) -> task => task.map(Event::Vfs),
        recv(self.flycheck_receiver) -> task => task.map(Event::Flycheck),
        recv(self.test_run_receiver) -> task => task.map(Event::TestResult),
        recv(self.discover_receiver) -> task => task.map(Event::DiscoverProject),
        recv(self.fetch_ws_receiver...) -> _instant => { ... },
    }
    .map(Some)
}
```

**The `try_recv` before `select!` is the key trick** — formatting results are checked first so they never wait behind other events. This is what prevents editor freezing on format-on-type.

### 3.3 Event Coalescing

```rust
fn handle_event(&mut self, event: Event) {
    match event {
        Event::DeferredTask(task) => {
            self.handle_deferred_task(task);
            // Drain up to 50ms of additional deferred tasks
            while loop_start.elapsed() < Duration::from_millis(50)
                && let Ok(task) = self.deferred_task_queue.receiver.try_recv()
            {
                self.handle_deferred_task(task);
            }
        }
        Event::Task(task) => {
            self.handle_task(&mut prime_caches_progress, task);
            // Coalesce up to 50ms of additional tasks
            while loop_start.elapsed() < Duration::from_millis(50)
                && let Ok(task) = self.task_pool.receiver.try_recv()
            {
                self.handle_task(&mut prime_caches_progress, task);
            }
        }
        Event::Vfs(message) => {
            self.handle_vfs_msg(message, &mut last_progress_report);
            // Coalesce up to 50ms of VFS events
            while loop_start.elapsed() < Duration::from_millis(50)
                && let Ok(message) = self.loader.receiver.try_recv()
            {
                self.handle_vfs_msg(message, &mut last_progress_report);
            }
        }
        // Same for Flycheck, TestResult, DiscoverProject
    }
}
```

**50ms coalescing window** applied to all non-LSP events. LSP messages are never coalesced (they need immediate response).

### 3.4 Request Dispatching

`crates/rust-analyzer/src/handlers/dispatch.rs`:

```rust
pub(crate) struct RequestDispatcher<'a> {
    pub(crate) req: Option<lsp_server::Request>,
    pub(crate) global_state: &'a mut GlobalState,
}

impl RequestDispatcher<'_> {
    /// On main thread, mutable access (rare)
    pub(crate) fn on_sync_mut<R>(&mut self, f: fn(&mut GlobalState, R::Params) -> anyhow::Result<R::Result>) -> &mut Self {
        let (req, params, panic_context) = self.parse::<R>()?;
        let result = f(self.global_state, params);
        self.global_state.respond(result_to_response::<R>(req.id, result));
        self
    }

    /// On main thread, snapshot (for latency-sensitive reads)
    pub(crate) fn on_sync<R>(&mut self, f: fn(GlobalStateSnapshot, R::Params) -> anyhow::Result<R::Result>) -> &mut Self {
        let world = self.global_state.snapshot();
        let result = panic::catch_unwind(move || f(world, params));
        self.global_state.respond(thread_result_to_response::<R>(req.id, result));
        self
    }

    /// On thread pool (default for most requests)
    pub(crate) fn on<const ALLOW_RETRYING: bool, R>(&mut self, f: fn(GlobalStateSnapshot, R::Params) -> ...) -> &mut Self {
        if !self.global_state.vfs_done {
            // Return default while VFS isn't ready
            self.global_state.respond(lsp_server::Response::new_ok(id, R::Result::default()));
            return self;
        }
        self.on_with_thread_intent::<false, ALLOW_RETRYING, R>(ThreadIntent::Worker, f, ...)
    }

    /// Latency-sensitive thread pool requests (completion, hover)
    pub(crate) fn on_latency_sensitive<const ALLOW_RETRYING: bool, R>(&mut self, f: ...) -> &mut Self {
        self.on_with_thread_intent::<false, ALLOW_RETRYING, R>(ThreadIntent::LatencySensitive, f, ...)
    }

    /// Formatting gets its own pool (never blocks on worker availability)
    pub(crate) fn on_fmt_thread<R>(&mut self, f: ...) -> &mut Self {
        self.on_with_thread_intent::<true, false, R>(ThreadIntent::LatencySensitive, f, ...)
    }

    fn on_with_thread_intent<const RUSTFMT: bool, const ALLOW_RETRYING: bool, R>(...) {
        let world = self.global_state.snapshot();
        let pool = if RUSTFMT { &mut self.global_state.fmt_pool.handle }
                   else { &mut self.global_state.task_pool.handle };
        pool.spawn(intent, move || {
            let result = panic::catch_unwind(move || f(world, params));
            match thread_result_to_response::<R>(req.id.clone(), result) {
                Ok(response) => Task::Response(response),
                Err(_cancelled) if ALLOW_RETRYING => Task::Retry(req),
                Err(_cancelled) => Task::Response(Response { id: req.id, result: None, error: Some(on_cancelled()) }),
            }
        });
    }
}
```

**Three dispatch tiers:**
1. `on_sync_mut` → main thread, mutable (workspace symbol, shutdown)
2. `on_sync` → main thread, snapshot (inlay hints, join lines)
3. `on` → worker pool (most requests)
4. `on_latency_sensitive` → latency-sensitive pool (completion, hover, highlight)
5. `on_fmt_thread` → dedicated formatting pool (never blocked by workers)

### 3.5 Salsa Cancellation → Retry

```rust
fn thread_result_to_response<R>(id, result) -> Result<Response, HandlerCancelledError> {
    match result {
        Ok(result) => result_to_response::<R>(id, result),
        Err(panic) => {
            if let Ok(cancelled) = panic.downcast::<Cancelled>() {
                return Err(HandlerCancelledError::Inner(*cancelled));
            }
            // ... normal panic handling
        }
    }
}

fn result_to_response<R>(id, result) -> Result<Response, HandlerCancelledError> {
    match result {
        Ok(resp) => Ok(Response::new_ok(id, &resp)),
        Err(e) => match e.downcast::<Cancelled>() {
            Ok(cancelled) => return Err(HandlerCancelledError::Inner(cancelled)),
            Err(e) => Ok(Response::new_err(id, ErrorCode::InternalError, e.to_string())),
        },
    }
}
```

When `ALLOW_RETRYING = true` and salsa cancels, the request is re-queued as `Task::Retry(req)`. When `ALLOW_RETRYING = false`, it returns `ContentModified` error to the editor.

---

## 4. Other Rust LSPs

### 4.1 Oxc

Uses `tower-lsp-server` like Biome. From `oxc/crates/oxc_language_server/README.md`:
- Text Document Sync: FULL
- Workspace folders: true
- Diagnostic mode: pull (preferred) or push
- Each workspace folder has its own configuration

Architecture follows Biome's pattern: async trait methods + sync analysis core.

### 4.2 Taplo

Uses `lsp-async-stub` (its own framework). From taplo docs:
> "The taplo-lsp crate exposes a language server implementation that uses lsp-async-stub, it is generic over its environment so it is possible to embed it in your own software."

`lsp-async-stub` is a lower-level async LSP framework that gives more control than tower-lsp but requires more boilerplate.

### 4.3 Harper

Uses `tower-lsp-server`. Known for being fast and lightweight.

### 4.4 SQL LSPs

- `sql-lsp` (Rust): Multi-dialect SQL LSP, uses tower-lsp-server
- `sqls` (Go): The original sqls is Go-based, not Rust

---

## 5. Common Patterns Extracted

### 5.1 The Async Transport + Sync Core Bridge

Every production Rust LSP uses this pattern:

```
Editor ←→ [async I/O layer] ←→ [sync analysis core]
              tower-lsp-server        WorkspaceServer
              or lsp-server            + salsa DB
```

**The bridge happens via:**
1. `spawn_blocking` (Biome) — moves sync work off tokio runtime
2. Snapshot + thread pool spawn (rust-analyzer) — takes immutable snapshot, sends to thread pool
3. Both catch panics and salsa cancellation at the boundary

### 5.2 Request Cancellation

No LSP implements `$/cancelRequest` handling at the server level. Instead:

| Strategy | How | Used by |
|----------|-----|---------|
| Salsa invalidation | New VFS change invalidates DB → in-flight queries return `Err(Cancelled)` | rust-analyzer |
| `catch_lsp_operation` | Catch salsa cancellation + panics, return error to client | Biome |
| Stale result discarding | Complete old request, client ignores stale response | Most tower-lsp servers |
| `ContentModified` | Return error code -32801 when document changed | rust-analyzer (when ALLOW_RETRYING=false) |

**The practical approach:** Don't try to cancel in-flight requests. Instead, detect staleness and either:
- Return `ContentModified` (editor re-sends)
- Retry with fresh state (if request is idempotent)
- Discard (if result is naturally stale)

### 5.3 Diagnostics Refresh

Two models:
1. **Push:** Server publishes diagnostics after every change (Biome's `update_diagnostics`)
2. **Pull:** Editor requests diagnostics (rust-analyzer's `textDocument/diagnostic`, Biome supports both)

Biome uses push with 250ms debounce + at-most-one-computation-per-file.
rust-analyzer uses push with generation-based invalidation (only publish if generation matches).

### 5.4 Snapshot Pattern

Both rust-analyzer and Biome use an immutable snapshot for request handling:

```rust
// rust-analyzer
let world = self.global_state.snapshot();
pool.spawn(intent, move || { f(world, params) });

// Biome
let session = server.session.clone();
spawn_blocking(move || session.workspace_for_request().$method(params))
```

The snapshot is `Send + 'static` — it can be moved to a thread pool. The analysis core operates on the snapshot, not mutable state. This is what makes concurrent request handling safe.

### 5.5 Event Coalescing

Applied to non-request events (VFS changes, diagnostics, background tasks):
- rust-analyzer: 50ms window
- Biome: 250ms debounce for diagnostics
- CIRCT: Configurable debounce (min/max quiet time)

**Never applied to LSP requests** — those need immediate response.

---

## 6. Key Source File References

| File | What it contains |
|------|-----------------|
| `biome/crates/biome_lsp/src/server.rs` | `workspace_method!` macro, `ServerFactory`, `LSPServer` impl, `LanguageServer` trait impl |
| `biome/crates/biome_lsp/src/session.rs` | `Session` state, `workspace()` vs `workspace_for_request()`, diagnostics debouncing |
| `rust-analyzer/crates/rust-analyzer/src/main_loop.rs` | `GlobalState::run`, `next_event` (crossbeam select), `handle_event` (coalescing) |
| `rust-analyzer/crates/rust-analyzer/src/handlers/dispatch.rs` | `RequestDispatcher`, `NotificationDispatcher`, thread intent routing |
| `rust-analyzer/crates/rust-analyzer/src/global_state.rs` | `GlobalState` struct, snapshot creation |
| `tower-lsp-server/src/server.rs` | Tower service implementation, request spawning |
| `async-lsp/src/server.rs` | Alternative framework (lifecycle middleware) |

---

## 7. Recommendations for Your Implementation

Based on the patterns above:

1. **Use `tower-lsp-server`** unless you need fine-grained control over the event loop (in which case use `lsp-server` + crossbeam like rust-analyzer).

2. **Wrap sync analysis in `spawn_blocking`** (Biome pattern) for the simplest async/sync bridge.

3. **Implement `catch_lsp_operation`** to catch both panics and salsa cancellation at the sync/async boundary.

4. **Use `workspace_for_request()` vs `workspace()`** distinction — requests should not retry (positions become stale), notifications should.

5. **Debounce diagnostics** with 250ms delay + at-most-one-running-per-file.

6. **Don't implement `$/cancelRequest`** — instead, detect staleness and return `ContentModified`.

7. **Snapshot your state** for thread pool work — never share `&mut` across threads.

8. **Consider a separate formatting pool** if formatting is latency-sensitive (rust-analyzer's `fmt_pool` trick).
