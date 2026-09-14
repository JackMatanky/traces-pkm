# Research: Actor Model vs Arc-Swap for Analysis Host and MCP Extensibility

Resolves ticket 36.

## Overview
This document investigates whether an actor/message-passing model provides material benefits over an `Arc<FileIndex>` shared-snapshot model for Traces' analysis host, specifically concerning the ease of adding a future MCP (Model Context Protocol) server alongside the LSP.

## Findings on Precedents

1. **Marksman (Actor Model Precedent):**
   - Uses F#'s built-in `MailboxProcessor` to manage state mutations sequentially without explicit mutexes. 
   - While effective in F#, it's worth noting that Marksman does a full re-parse per edited document and keeps the index entirely in-memory.

2. **rust-analyzer, Biome, ruff_server, ty (Snapshot/Thread-Pool Precedent):**
   - **None** of the highly performant Rust LSPs researched use an actor-model core to coordinate their indexes.
   - **rust-analyzer** explicitly uses a Host/Snapshot split: `AnalysisHost` manages mutable state and produces an `Analysis` snapshot. Multiple request-handling threads query this snapshot concurrently and lock-free.
   - **Biome, ruff_server, ty** all rely on synchronous core functions, thread pools, and shared-immutable state (often via `salsa` query engines), entirely avoiding the serialization of reads that a pure actor model would impose.

## Rust Actor Ecosystem

If Traces were to adopt an actor model, the Rust ecosystem offers several paths, though most are overkill for a single analysis host:
- **Hand-rolled `tokio::sync::mpsc`:** The most idiomatic Rust approach (as recommended in Tokio's "Actors with Tokio" tutorial). A spawned task loops over a channel, processing enums. Replies require packaging a `oneshot::Sender` inside the message.
- **actix / ractor:** Mature and battle-tested, but heavy, Erlang-inspired, and strongly opinionated. Overkill for a local file-index coordinator.
- **xtra / kameo:** Lighter, async-first actor frameworks. Easier to integrate but still require defining message structs and handler traits.

*Maturity & Adoption:* While the frameworks are mature, the broader Rust ecosystem heavily favors hand-rolled Tokio MPSC loops for simple message passing, and standard `Arc`/`RwLock` or lock-free data structures for read-heavy shared state.

## The MCP Extensibility Question

**Question:** Does an actor-model core make it materially easier to later expose the same coordinator to an MCP server without duplicating logic, compared to an `Arc`-swap facade?

**Answer: No. In fact, an `Arc`-swap facade is both simpler and more performant for multi-frontend architectures.**

1. **Concurrency and Read Bottlenecks:** 
   An LSP (and a future MCP server) is overwhelmingly read-heavy (hover, definitions, references, semantic tokens, context queries). In a pure actor model, every query must be sent as a message to the single actor task. This serializes all reads through a single channel queue, bottlenecking the system. To fix this, the actor would have to reply with an `Arc<Snapshot>` anyway—at which point the actor is just a slow, message-based `RwLock`.
   
2. **Multi-Frontend Integration:** 
   "One coordinator, N protocol adapters calling its public API" is trivially satisfied by the `Arc`-swap model. The LSP server and the MCP server simply both hold a clone of the `Arc<Coordinator>`. When a request comes in from either protocol, the adapter calls ordinary Rust methods on the coordinator (e.g., `coordinator.query_references(...)`). Neither frontend needs to know the other exists.
   
3. **No Unique Actor Benefits for Extensibility:**
   An actor model provides built-in sequential mutation (preventing write races) and can provide backpressure via bounded channels. However, file changes (writes) in an LSP usually arrive via a single source of truth (the editor's `didChange` events), making write-contention minimal. Adding an MCP server mostly adds *readers*, not concurrent *writers* of source text. Thus, the actor model's primary benefit (safe concurrent writes) is misaligned with the actual workload, while its primary drawback (serialized reads) actively harms it.

## Concrete Recommendation

**Reject the actor model (c) and proceed with the `Arc<FileIndex>` swap-on-refresh facade (a).**

- **Performance:** `Arc`-swap allows N threads (LSP handlers, MCP handlers) to read the index concurrently and lock-free.
- **Simplicity:** Exposing a standard Rust `impl Coordinator { pub fn query(...) }` API is far less boilerplate than defining message enums and oneshot response channels.
- **Extensibility:** To add an MCP server later, you simply instantiate the MCP transport loop and pass it a clone of the `Arc<Coordinator>`, exactly as you do for the LSP transport loop. No internal logic needs to be duplicated or rearchitected.
- **Precedent:** This aligns directly with `rust-analyzer`'s proven Host/Snapshot architecture, which handles exact same concurrency requirements at a massive scale.
