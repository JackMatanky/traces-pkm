# lsp-server Crate Analysis

> Deep analysis of rust-analyzer's own LSP transport scaffold.  
> Crate: `lsp-server` v0.10.0 | Maintainer: rust-analyzer team | License: MIT/Apache-2.0

---

## 1. API Surface

### Exported Types

```rust
pub struct Connection { pub sender: Sender<Message>, pub receiver: Receiver<Message> }
pub struct IoThreads { /* private */ }
pub struct Request { pub id: RequestId, pub method: String, pub params: serde_json::Value }
pub struct Notification { pub method: String, pub params: serde_json::Value }
pub struct Response { pub id: RequestId, pub response_result: Result<serde_json::Value, ResponseError> }
pub struct RequestId(/* private */)
pub struct ResponseError { pub code: i32, pub message: String, pub data: Option<serde_json::Value> }
pub struct ReqQueue<I, O> { pub incoming: Incoming<I>, pub outgoing: Outgoing<O> }
pub struct Incoming<I> { /* private: HashMap<RequestId, I> */ }
pub struct Outgoing<O> { /* private: next_id, HashMap<RequestId, O> */ }
pub enum Message { Request(Request), Response(Response), Notification(Notification) }
pub enum ExtractError<T> { MethodMismatch(T), JsonError { method: String, error: serde_json::Error } }
pub enum ErrorCode { ParseError, InvalidRequest, MethodNotFound, InvalidParams, InternalError,
    ServerNotInitialized, UnknownErrorCode, RequestCanceled, ContentModified,
    ServerCancelled, RequestFailed }
pub struct ProtocolError(String)
```

All types derive `Debug`. `Message`, `Request`, `Notification`, `Response`, `RequestId`, `ResponseError` derive `Serialize`/`Deserialize` and `Clone`.

### Transport Constructors

```rust
impl Connection {
    pub fn stdio() -> (Connection, IoThreads)                    // stdin/stdout
    pub fn connect<A: ToSocketAddrs>(addr: A) -> io::Result<(Connection, IoThreads)>  // TCP client
    pub fn listen<A: ToSocketAddrs>(addr: A) -> io::Result<(Connection, IoThreads)>   // TCP server
    pub fn memory() -> (Connection, Connection)                  // in-memory pair for tests
}
```

### Initialization

```rust
impl Connection {
    // Simple: wait for `initialize`, respond with capabilities, return params
    pub fn initialize(&self, server_capabilities: serde_json::Value)
        -> Result<serde_json::Value, ProtocolError>

    // Advanced: two-phase — separate start/finish for custom flow
    pub fn initialize_start(&self) -> Result<(RequestId, serde_json::Value), ProtocolError>
    pub fn initialize_finish(&self, id: RequestId, resp: serde_json::Value) -> Result<(), ProtocolError>

    // Variants that process messages during initialization (for background work)
    pub fn initialize_while<F>(&self, server_capabilities: serde_json::Value, f: F)
        -> Result<serde_json::Value, ProtocolError>
        where F: FnMut(Message) -> Option<Message>

    pub fn initialize_start_while<F>(&self, f: F) -> ...
    pub fn initialize_finish_while<F>(&self, id: RequestId, resp: serde_json::Value, f: F) -> ...
}

impl Connection {
    pub fn handle_shutdown(&self, req: &Request) -> Result<bool, ProtocolError>
}
```

### Message Extraction (typed deserialization)

```rust
impl Request {
    pub fn new(id: RequestId, method: String, params: serde_json::Value) -> Self
    pub fn extract<P: DeserializeOwned>(self, expected_method: &str)
        -> Result<(RequestId, P), ExtractError<Self>>
}

impl Notification {
    pub fn new(method: String, params: serde_json::Value) -> Self
    pub fn extract<P: DeserializeOwned>(self, expected_method: &str)
        -> Result<P, ExtractError<Self>>
}

impl Response {
    pub fn new_ok(id: RequestId, result: serde_json::Value) -> Self
    pub fn new_err(id: RequestId, code: ErrorCode, message: String) -> Self
}
```

### Request Queue (cancellation support)

```rust
impl<I> Incoming<I> {
    pub fn register(&mut self, id: RequestId, data: I)
    pub fn cancel(&mut self, id: RequestId) -> Option<Response>  // returns error Response
    pub fn complete(&mut self, id: &RequestId) -> Option<I>     // removes and returns data
    pub fn is_completed(&self, id: &RequestId) -> bool
    pub fn has_pending(&self) -> bool
}

impl<O> Outgoing<O> {
    pub fn register<P: Serialize>(&mut self, method: String, params: P, data: O) -> Request
    pub fn complete(&mut self, id: &RequestId) -> Option<O>
    pub fn has_pending(&self) -> bool
}
```

### IoThreads

```rust
impl IoThreads {
    pub fn join(self) -> thread::Result<()>  // blocks until IO threads finish
}
```

---

## 2. Dispatch Model

**Synchronous, crossbeam-channel based.** The design philosophy is deliberately minimal: `lsp-server` gives you the raw message channel and expects you to build the dispatch loop yourself.

### How it works

1. `Connection::stdio()` spawns **two background threads**: one reads from stdin, one writes to stdout. They communicate with the main thread via crossbeam channels.
2. The main thread receives `Message` values from `connection.receiver` — a `crossbeam_channel::Receiver<Message>`.
3. You iterate with `for msg in &connection.receiver` (blocking recv) or use `recv_timeout`/`try_recv` for polling.
4. Each `Message` is one of `Request`, `Notification`, or `Response`. You match and dispatch manually.

### Is it truly synchronous?

**Yes.** No async runtime. No futures. The blocking happens at the channel level. The IO threads handle the async parts (reading frames off stdin, writing frames to stdout), but the API surface is entirely synchronous. You can call `.recv()` and get a `Message` on the current thread.

This is a **deliberate design choice** — rust-analyzer's concurrency model is crossbeam thread pools + atomic cancellation tokens, not Tokio tasks.

### Typical dispatch pattern

```rust
for msg in &connection.receiver {
    match msg {
        Message::Request(req) => {
            if connection.handle_shutdown(&req)? { break; }
            match req.method.as_str() {
                "textDocument/completion" => { /* ... */ }
                "textDocument/hover" => { /* ... */ }
                _ => { /* respond MethodNotFound */ }
            }
        }
        Message::Notification(notif) => {
            match notif.method.as_str() {
                "textDocument/didOpen" => { /* ... */ }
                "textDocument/didChange" => { /* ... */ }
                "$/cancelRequest" => { /* ... */ }
                "exit" => { break; }
                _ => {}
            }
        }
        Message::Response(resp) => {
            // Handle responses to requests YOU sent (rare in server-only)
        }
    }
}
```

---

## 3. Cancellation

### What lsp-server provides

**Zero built-in cancellation logic.** It provides the building blocks:

1. **`ErrorCode::RequestCanceled`** (`-32800`) — the standard LSP error code for cancelled requests.
2. **`Incoming::cancel(&id) -> Option<Response>`** — removes the request from the pending queue and generates an error Response with `RequestCanceled` code.
3. **`Incoming::complete(&id)`** — removes and returns the user data attached to a request.

### What you must build yourself

The cancellation *mechanism* is entirely manual:

1. Parse the `$/cancelRequest` notification yourself.
2. Look up the request ID in your `ReqQueue`.
3. Signal the background task to stop (via `Arc<AtomicBool>`, a channel, or a custom cancellation token).
4. The background task must periodically check the token and abort early.
5. If the task completes after cancellation, you must check the token before sending the response.

### How rust-analyzer does cancellation

rust-analyzer implements a sophisticated cooperative cancellation model:

- Every incoming request is paired with a cancellation token (typically `Arc<AtomicBool>`).
- The token is stored in `ReqQueue::Incoming` as user data via `.register(id, token)`.
- When `$/cancelRequest` arrives, the main loop calls `req_queue.incoming.cancel(&id)`, which returns the `Option<Response>`.
- Background threads check the token between computation steps (e.g., inside Salsa query loops).
- If cancelled, the background thread drops the result; the main loop sends the error response.
- Additionally, rust-analyzer implements **implicit cancellation**: when new requests arrive, older in-flight requests for the same document may be cancelled automatically.

---

## 4. Progress Reporting

### What lsp-server provides

**Nothing built-in.** There is no `ProgressReporter` type, no `$/progress` helper, no token management.

### How to implement it

You must construct `$/progress` notifications manually and send them through `connection.sender`:

```rust
use lsp_types::ProgressParamsValue;
use serde_json::json;

// Start progress
let token = "my-operation-id";
connection.sender.send(Message::Notification(
    Notification::new(
        "$/progress".to_string(),
        json!({
            "token": token,
            "value": {
                "kind": "begin",
                "title": "Analyzing...",
                "cancellable": true
            }
        })
    )
))?;

// Report progress
connection.sender.send(Message::Notification(
    Notification::new(
        "$/progress".to_string(),
        json!({
            "token": token,
            "value": {
                "kind": "report",
                "message": "50% complete",
                "percentage": Some(50)
            }
        })
    )
))?;

// End progress
connection.sender.send(Message::Notification(
    Notification::new(
        "$/progress".to_string(),
        json!({
            "token": token,
            "value": { "kind": "end" }
        })
    )
))?;
```

Or construct the `ProgressParams` from `lsp-types` and serialize it.

---

## 5. Initialization Handshake

### What lsp-server handles

1. **Frame-level protocol**: Content-Length headers, JSON-RPC framing — fully handled.
2. **Message parsing**: Deserializing JSON-RPC into `Request`/`Notification`/`Response` — handled.
3. **`initialize`/`initialized` flow**: The `Connection::initialize()` method:
   - Blocks until the client sends `initialize`.
   - Returns `(RequestId, serde_json::Value)` — the request ID and raw `InitializeParams`.
   - You respond by calling `initialize_finish(id, capabilities)` which sends the response.
   - After that, you're expected to wait for `initialized` notification (you handle this yourself).
4. **`shutdown`/`exit` flow**: `handle_shutdown(&req)` detects the `shutdown` request, sends an empty success response, and returns `Ok(true)`. You then break your loop and wait for `exit`.

### What you implement

- Deserializing `InitializeParams` from the raw `serde_json::Value`.
- Building and returning `ServerCapabilities`.
- Handling the `initialized` notification (typically a no-op, but some servers use it to trigger initial diagnostics).
- Enforcing the LSP lifecycle state machine (no requests before `initialize`, proper shutdown sequence).

### Two-phase initialization

For advanced scenarios (e.g., logging messages during init):

```rust
let (id, params) = connection.initialize_start()?;
// ... do work, send log messages ...
connection.initialize_finish(id, capabilities)?;
```

Or with message processing:

```rust
let (id, params) = connection.initialize_start_while(|msg| {
    // Process messages during initialization (e.g., handle concurrent notifications)
    None
})?;
```

---

## 6. What You Must Build Yourself

Compared to `tower-lsp`/`tower-lsp-server`:

| Feature | `lsp-server` | `tower-lsp` |
|---------|-------------|-------------|
| **Transport / framing** | Built-in (stdio, TCP) | Built-in (stdio, TCP) |
| **Message types** | Raw `Request`/`Notification`/`Response` | Typed per-method |
| **Request routing** | Manual `match` | `LanguageServer` trait auto-dispatch |
| **Concurrency control** | Manual (crossbeam threads) | Tower `Service` + Tokio |
| **Middleware** | None | Tower `Layer` stack |
| **Cancellation** | Manual (`ReqQueue` + tokens) | Built-in cancellation tokens |
| **Progress reporting** | Manual `$/progress` | Built-in `ProgressReporter` |
| **Client handle** | Manual `connection.sender` | `Client` proxy struct |
| **Service trait** | Not implemented | `tower_service::Service` |
| **State machine** | Manual (no init guard) | Built-in lifecycle |
| **Outgoing requests** | Manual `Outgoing::register` | `Client::send_request` |
| **Diagnostics push** | Manual notification | `Client::publish_diagnostics` |
| **Logging** | Manual `window/logMessage` | `Client::log_message` |

### What you must build with lsp-server

1. **Request dispatch table/routing** — match on `req.method` string.
2. **Background task spawning** — `std::thread::spawn`, crossbeam thread pool, or tokio.
3. **Cancellation tokens** — `Arc<AtomicBool>` or custom, checked in background loops.
4. **ReqQueue management** — register, complete, cancel manually.
5. **Client notifications** — construct `Notification::new(...)` and send through channel.
6. **Progress tokens** — manual `$/progress` message construction.
7. **VFS / document state** — maintain open document contents from `didOpen`/`didChange`.
8. **Lifecycle state machine** — enforce initialize-before-requests, shutdown sequence.

---

## 7. rust-analyzer's Actual Usage

### Main loop architecture

rust-analyzer runs a **centralized single-threaded main loop** in `main_loop.rs` that coordinates three event sources via crossbeam `select!`:

1. **LSP messages** from `connection.receiver`
2. **Background thread completions** from a crossbeam channel (type inference results, diagnostics, etc.)
3. **VFS/flycheck events** from file watchers and `cargo check` subprocesses

```rust
// Simplified from rust-analyzer's actual main_loop.rs
crossbeam::select! {
    recv(connection.receiver) -> msg => { /* handle LSP message */ }
    recv(task_receiver) -> task_result => { /* handle background task completion */ }
    recv(vfs_receiver) -> vfs_event => { /* handle file changes */ }
}
```

### Dispatch pattern

rust-analyzer uses typed dispatchers:

```rust
// RequestDispatcher pattern
req.dispatcher.on::<GotoDefinition>(|params| { ... });
req.dispatcher.on::<Completion>(|params| { ... });
// Unhandled requests get MethodNotFound response

// NotificationDispatcher pattern
notif.dispatcher.on::<DidOpenTextDocument>(|params| { ... });
notif.dispatcher.on::<DidChangeTextDocument>(|params| { ... });
notif.dispatcher.on::<CancelRequest>(|params| { ... });
```

### Background tasks

- Heavy queries (goto definition, completion, code actions) are **offloaded to a thread pool**.
- The main loop takes an **immutable snapshot** of the Salsa database (`GlobalStateSnapshot`) and passes it to the background thread.
- Background threads send results back via a crossbeam channel.
- The main loop processes results and sends LSP responses.

### Cancellation in practice

1. `$/cancelRequest` arrives → main loop matches on it.
2. Looks up the request in `req_queue.incoming`.
3. Retrieves the cancellation token (`Arc<AtomicBool>`).
4. Sets it to `true`.
5. Background thread checks token periodically → aborts if set.
6. Main loop sends error response with `RequestCanceled` code.

### Diagnostics

- `cargo check` runs as a subprocess (flycheck).
- Its JSON output streams back via channels.
- The main loop translates compiler diagnostics into LSP `Diagnostic` objects.
- Sends `textDocument/publishDiagnostics` notifications to the client.

---

## 8. Concurrency with lsp-server

### The crossbeam model (what rust-analyzer uses)

```
stdin reader thread ──crossbeam──> main loop thread ──crossbeam──> stdout writer thread
                                        │
                                   ┌────┴────┐
                                   │         │
                              background  background
                              thread 1    thread N
                                   │         │
                              ─────crossbeam──┘
```

**No Tokio. No async.** Everything is threads + channels.

### Patterns

1. **Single-threaded main loop** (simple servers): Process messages inline. OK for lightweight servers.
2. **Thread pool** (rust-analyzer pattern): Spawn `N` worker threads. Each gets a snapshot of state. Results sent back via channel.
3. **Tokio integration** (if you want): You *can* spawn tokio tasks and bridge to the crossbeam channels, but it's non-standard. The `crossbeam_channel::Receiver` implements `futures::Stream` via the `crossbeam-channel` feature, so you could run `tokio::spawn` to bridge, but it adds complexity.

### Tradeoffs

| Approach | Pros | Cons |
|----------|------|------|
| **Pure crossbeam threads** | Simple, no async complexity, rust-analyzer-proven | Manual thread management, no structured concurrency |
| **Crossbeam + tokio bridge** | Can use async libraries (reqwest, etc.) | Complexity, mixing paradigms |
| **Single-threaded** | Simplest, no concurrency bugs | Blocking = unresponsive server on heavy queries |

---

## 9. Crates.io Adoption

- **Version**: 0.10.0 (latest stable)
- **Maintainer**: rust-analyzer team (Lukas Wirth / Veykril, + contributors)
- **Repository**: https://github.com/rust-lang/rust-analyzer/tree/master/lib/lsp-server
- **Maintenance status**: Actively maintained as part of rust-analyzer's core infrastructure. Updated with rust-analyzer releases.
- **Dependents**: Widely used in the Rust LSP ecosystem. The exact count on crates.io fluctuates but includes dozens of direct dependents — custom language servers, auto-lsp framework, tree-sitter LSP generators, and educational/example projects. The reverse dependency graph includes hundreds of crates transitively.

---

## 10. Real-World LSPs Using lsp-server

### Production users

1. **rust-analyzer** — The canonical user. Powers Rust support in VS Code, Neovim, Helix, Emacs, Sublime Text, Zed.
2. **auto-lsp** — Generic framework for generating LSP servers from tree-sitter grammars. Uses `lsp-server` as its transport backend.
3. **Various custom language servers** — Developers building LSPs for proprietary DSLs, configuration formats, and template languages frequently choose `lsp-server` over `tower-lsp` when they want explicit control.

### Why they choose lsp-server over tower-lsp

- **Predictability**: No async runtime surprises. No cryptic Tower trait error messages.
- **Control**: Full visibility into the message loop. Easy to add custom logging, intercept raw payloads.
- **Performance**: Crossbeam channels are fast. No async overhead for a fundamentally synchronous workload (most LSP operations are CPU-bound, not IO-bound).
- **Proven**: rust-analyzer's architecture is battle-tested at scale with massive multi-crate workspaces.

---

## Key Tradeoffs Summary

| Choose `lsp-server` when... | Choose `tower-lsp` when... |
|-----------------------------|---------------------------|
| You want full control over dispatch | You want auto-dispatch from a trait |
| You prefer synchronous/threaded concurrency | You want async/Tokio integration |
| You're building a CPU-heavy server (compiler, analyzer) | You need middleware (tracing, rate-limiting) |
| You want to avoid Tower/futures type complexity | You want built-in progress/cancellation helpers |
| You need custom wire-protocol behavior | You want a `Client` proxy for push notifications |
| You're building something rust-analyzer-like | You're building a simpler server fast |

---

## References

- Source: https://github.com/rust-lang/rust-analyzer/tree/master/lib/lsp-server
- Docs: https://docs.rs/lsp-server/0.10.0/lsp_server/
- Crates.io: https://crates.io/crates/lsp-server
- LSP spec: https://microsoft.github.io/language-server-protocol/
