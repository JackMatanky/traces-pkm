# Rust LSP Frameworks — Landscape Analysis

Research date: 2026-09-14

## Overview

The Rust LSP ecosystem has **7 distinct approaches** to building a Language Server, ranging from full-featured async frameworks to raw JSON-RPC framing from scratch. Here's the full landscape.

---

## 1. tower-lsp (Original) — `tower-lsp`

| Attribute | Detail |
|-----------|--------|
| **Crate** | `tower-lsp` v0.20.0 |
| **Repo** | [ebkalderon/tower-lsp](https://github.com/ebkalderon/tower-lsp) |
| **Status** | **Unmaintained** (last release Aug 2023, 1359 GitHub stars) |
| **Downloads** | ~7M all-time, ~1.8M/90d (still high due to legacy) |
| **Handler trait** | `LanguageServer` with `&self` (immutable) |
| **Async model** | `#[async_trait]` — all handlers async |
| **Notification ordering** | **Async** (semantically incorrect, causes out-of-order issues) |
| **Tower integration** | Built on tower but **no custom Layer support** — Service interface is builtin |
| **Concurrency** | Built-in, not customizable |
| **Lifecycle** | Built-in, not customizable |
| **Client support** | Server only |
| **Cancellation** | Not built-in |
| **Progress** | Not built-in (planned, never shipped) |
| **LSP spec version** | 3.16, proposed 3.18 via feature flag |
| **LOC for basic server** | ~30 lines |

**Key limitation**: `&self` on all handlers forces `Arc<RwLock<...>>` for mutable state. Notifications handled asynchronously introduces ordering bugs. No way to plug in custom tower middleware.

---

## 2. tower-lsp-server (Community Fork) — `tower-lsp-server`

| Attribute | Detail |
|-----------|--------|
| **Crate** | `tower-lsp-server` v0.23.0 |
| **Repo** | [tower-lsp-community/tower-lsp-server](https://github.com/tower-lsp-community/tower-lsp-server) |
| **Status** | **Actively maintained** (Dec 2025, 219 stars, 665 commits since Nov 2024) |
| **Downloads** | ~1.6M all-time, ~918K/90d |
| **Handler trait** | `LanguageServer` with `&self` (same as original) |
| **Async model** | Native async (removed `async_trait` requirement in 0.21.1) |
| **Notification ordering** | **Still async** (same fundamental issue as original) |
| **Tower integration** | Same limitations as original |
| **Key changes from original** | Switched from `gluon-lang/lsp-types` to own `ls-types` fork (codegen-based), `async_trait` no longer required, `stream_select` fix, `UriExt` improvements |
| **LSP spec** | 3.18 support, proposed features |
| **Projects using it** | **Biome, Oxc, Harper, Polarity, Veryl, django-language-server, pytest-language-server, SystemD-LSP, Amber LSP, ast-grep** |
| **LOC for basic server** | ~30 lines |

**Verdict**: The de facto "easy" choice. Most production Rust LSPs use this. Same architectural limitations as the original (immutable state, no custom layers), but actively maintained and Biome-level projects ship on it.

---

## 3. async-lsp — `async-lsp`

| Attribute | Detail |
|-----------|--------|
| **Crate** | `async-lsp` v0.2.4 |
| **Repo** | [oxalica/async-lsp](https://github.com/oxalica/async-lsp) |
| **Status** | **Actively maintained** (May 2025, by oxalica — nix ecosystem) |
| **Downloads** | ~1.3M all-time, ~484K/90d |
| **SLoC** | ~2.1K |
| **Handler trait** | `LspService` — a tower `Service` for requests + notification handler |
| **Dispatch model** | `Router` for routing, `MainLoop` as driver |
| **Handler signature** | `&mut self` for both requests and notifications |
| **Request futures** | Return `Future` **without borrowing self** — enables true concurrent requests |
| **Notification ordering** | **Synchronous** (correct per LSP spec) |
| **Tower integration** | **First-class** — uses `tower_layer::Layer` for all middleware |
| **Built-in middleware** | `Concurrency` (multiplexing + cancellation), `CatchUnwind`, `Tracing`, `Lifecycle`, `ClientProcessMonitor`, `Router` |
| **Custom middleware** | Full support — timeout, metering, request transformation all possible |
| **Client support** | **Both server and client** (symmetric) |
| **Cancellation** | Built-in via `Concurrency` middleware |
| **Progress** | Not built-in |
| **LSP spec** | 3.17 (follows `lsp-types` Request/Notification traits) |
| **LOC for basic server** | ~50-80 lines (more ceremony, more control) |

**Key architecture**:
- Requests: tower `Service` — can be `tower::Service::call()` concurrently
- Notifications: synchronous handler — processed in order, can control main loop via `ControlFlow`
- Middleware stack is fully composable via tower `Layer`
- Two API styles: builder API (more flexible) or omnitrait `LanguageServer`/`LanguageClient` (similar to tower-lsp)

**Why `&mut self` matters**: No `Arc<RwLock<...>>` needed. You mutate state directly in notification handlers (like `didChange`), and request handlers return futures that don't borrow self. This is the correct model for LSP.

**Adoption**: Used by oxalica's tooling (rust-overlay, etc.). Not as widely adopted as tower-lsp-server, but architecturally superior.

---

## 4. lsp-server (rust-analyzer) — `lsp-server`

| Attribute | Detail |
|-----------|--------|
| **Crate** | `lsp-server` v0.10.0 |
| **Repo** | [rust-analyzer/lsp-server](https://github.com/rust-analyzer/lsp-server) (also vendored into rust-analyzer) |
| **Status** | **Actively maintained** (by rust-analyzer team) |
| **Downloads** | ~309K/90d |
| **Architecture** | **Synchronous**, crossbeam-channel based |
| **Handler model** | **You write the dispatch loop yourself** |
| **What it provides** | Protocol handshaking, message parsing, `Connection` (pair of channels) |
| **What you provide** | Main loop, request routing, response sending |
| **Tower integration** | None |
| **Cancellation** | Manual |
| **Progress** | Manual |
| **Client support** | Server only |
| **LSP spec** | N/A (just types + framing) |
| **LOC for basic server** | ~40 lines |

**Who uses it**: rust-analyzer itself, and projects that want maximum control. The `Connection` struct gives you `sender`/`receiver` and you write `loop { match receiver.recv() { ... } }`.

**Key insight**: This is a "bring your own main loop" scaffold. You get JSON-RPC framing + LSP handshake, but everything else is manual. This is what rust-analyzer uses — it has a complex custom main loop with salsa integration, event loops, and internal task scheduling.

---

## 5. lsp-types / ls-types / lspt / lsp (Type-only crates)

These provide **only type definitions** — no server scaffolding, no framing, no dispatch.

### lsp-types (gluon-lang)
- **Crate**: `lsp-types` v0.97.0 (most popular, 33M all-time downloads)
- **LSP spec**: 3.16 with proposed 3.17
- **Status**: Effectively unmaintained (last release Jun 2024), but still the most depended-upon
- **Used by**: tower-lsp, most frameworks

### ls-types (tower-lsp-community)
- **Crate**: `ls-types` v0.0.6 (1M all-time downloads)
- **LSP spec**: 3.18
- **Status**: Active — maintained by tower-lsp-community, plans for codegen from LSP metamodel
- **Used by**: tower-lsp-server v0.22+

### gen-lsp-types
- **Crate**: `gen-lsp-types` v0.11.0 (334K all-time downloads)
- **Generated from**: Official LSP metamodel
- **Always up-to-date** with latest LSP spec
- **Status**: Active (Jul 2026)

### lspt
- **Crate**: `lspt` v0.5.0
- **Features**: Configurable URI (String vs url::Url), FxHashMap/indexmap, method macros
- **Status**: Active (Jul 2026)

### lsp (macmv)
- **Crate**: `lsp` v0.1.1
- **Generated from**: LSP meta model via lsp-generator
- **Status**: New (2026)

---

## 6. Other Frameworks

### lspower — `lspower`
| Attribute | Detail |
|-----------|--------|
| **Crate** | `lspower` v1.5.0 |
| **Status** | **Unmaintained** (last release 2021, fork of tower-lsp) |
| **Improvements over tower-lsp** | Semantic tokens, cancellation tokens, WASM support, runtime-agnostic, SIMD-accelerated parsing |
| **Adopted by** | Few projects (superseded by tower-lsp-server) |
| **SLoC** | 3.4K |

### language-server (latex-lsp)
| Attribute | Detail |
|-----------|--------|
| **Repo** | [latex-lsp/language-server](https://github.com/latex-lsp/language-server) |
| **Status** | Low activity (65 commits, 37 stars) |
| **Architecture** | Async, executor-independent (`async_executors`), full client+server |
| **Used by** | latex-lsp project |

### lspf
| Attribute | Detail |
|-----------|--------|
| **Crate** | `lspf` (Aug 2026) |
| **Description** | "Extensible LSP framework, async-only, stand up a working server in very little code" |
| **Status** | Very new, unproven |

---

## 7. Production Rust LSPs — What Framework They Use

| Project | Framework | Notes |
|---------|-----------|-------|
| **rust-analyzer** | `lsp-server` (custom main loop) | Uses crossbeam channels, custom dispatch, salsa integration |
| **Biome** | `tower-lsp-server` (community fork) | Previously used original tower-lsp |
| **Oxc** | `tower-lsp-server` | `oxc_language_server` crate |
| **Harper** | `tower-lsp-server` | |
| **Deno** | `tower-lsp` (original, still) | |
| **Turborepo** | `tower-lsp` (original, still) | |
| **Taplo** | `lsp-async-stub` (custom crate) | Uses its own `taplo-lsp` crate with `lsp-async-stub` internally |
| **Veryl** | `tower-lsp-server` | |
| **ast-grep** | `tower-lsp-server` | |
| **async-rust-lsp** | `tower-lsp` (original) | New project (Apr 2026) |
| **crates-lsp** | Tower-based (custom) | |
| **rlsp** | Custom | New YAML LSP project |
| **RLS** (deprecated) | Custom (jsonrpc-core) | Replaced by rust-analyzer |

---

## 8. Custom LSP from Scratch (lsp-types only)

### How viable is it?

**The implementation surface for a minimal LSP server:**

1. **Read Content-Length header** from stdin (~10 lines)
2. **Read JSON body** (~5 lines)
3. **Parse JSON-RPC** message (request/notification/response) (~20 lines)
4. **Handle initialize handshake** (~30 lines)
5. **Dispatch to handlers** (~50 lines)
6. **Send responses** with Content-Length framing (~10 lines)

**Total**: ~125 lines of boilerplate for a working skeleton.

**Libraries needed**:
- `lsp-types` (or `lspt`, `gen-lsp-types`) for type definitions
- `serde_json` for serialization
- A Content-Length reader (custom or `lsp-server`)

**Tutorials**: No dedicated tutorials exist. The best reference is `lsp-server`'s `examples/goto_def.rs` and rust-analyzer's `main_loop.rs`.

**When to go custom**:
- Need synchronous notification processing with complex state machine
- Need custom I/O (TCP, Unix sockets, not just stdio)
- Need multiplexed internal event loops (e.g., rust-analyzer's salsa integration)
- Performance-critical with custom scheduling

**When NOT to go custom**:
- Standard LSP server with straightforward request handling
- You want middleware, cancellation, progress without writing it yourself
- Team unfamiliar with JSON-RPC/LSP protocol details

---

## 9. Framework Comparison Matrix

| Feature | tower-lsp | tower-lsp-server | async-lsp | lsp-server | Custom (lsp-types) |
|---------|-----------|-------------------|-----------|------------|---------------------|
| **Sync handlers** | No | No | Yes (notifications) | Yes (everything) | Yes |
| **Async handlers** | Yes (all) | Yes (all) | Yes (requests) | Manual | Manual |
| **Handler self** | `&self` | `&self` | `&mut self` | N/A (manual) | N/A |
| **Custom dispatch** | No | No | Via `Router` + Layers | Full control | Full control |
| **Cancellation built-in** | No | No | Yes | No | No |
| **Progress built-in** | No | Yes | No | No | No |
| **Middleware/layers** | No | No | Yes (tower Layer) | No | No |
| **Client+Server** | Server only | Server only | Both | Server only | Either |
| **Notification ordering** | Async (wrong) | Async (wrong) | Synchronous (correct) | You decide | You decide |
| **Runtime** | tokio | tokio | tokio/async-std/smol | Any (sync) | Any |
| **WASM support** | No | No | Theoretically | No | Yes |
| **LSP spec** | 3.16+proposed | 3.18+proposed | 3.17 | N/A | N/A |
| **Downloads/90d** | 1.8M | 918K | 484K | 309K | 8M (types only) |
| **Production users** | Deno, Turborepo | Biome, Oxc, Harper | oxalica | rust-analyzer | All of them |
| **LOC (basic server)** | ~30 | ~30 | ~50-80 | ~40 | ~125 |

---

## 10. Recommendations

### For most new projects: **tower-lsp-server**
- Best balance of ease-of-use and production validation
- Biome, Oxc, Harper prove it works at scale
- Active maintenance, LSP 3.18 support
- Trade-off: `&self` means `Arc<RwLock<...>>` for mutable state

### For maximum correctness: **async-lsp**
- Correct `&mut self` model, synchronous notifications
- Full tower middleware stack
- Best architecture for complex servers
- Trade-off: More ceremony, less ecosystem adoption

### For maximum control: **lsp-server** or **lsp-types only**
- rust-analyzer's approach
- You write the main loop
- Best for complex state machines, custom I/O, performance-critical dispatch
- Trade-off: More code, more responsibility

### Avoid: **tower-lsp** (original), **lspower**
- Unmaintained
- Use tower-lsp-server or async-lsp instead

---

## 11. Key Architectural Insight: The `&self` vs `&mut self` Divide

The fundamental design tension in Rust LSP frameworks:

**`&self` (tower-lsp, tower-lsp-server)**:
- All handlers take `&self`
- Concurrency is safe by construction
- Mutable state requires `Arc<RwLock<T>>`
- Notifications handled asynchronously (out-of-order bug)
- Simpler API surface

**`&mut self` (async-lsp)**:
- Request handlers return futures that don't borrow self → concurrent
- Notification handlers take `&mut self` → synchronous, ordered
- No `Arc`/`RwLock` needed for state
- More complex API surface
- Correct per LSP specification

The LSP spec says notifications must be processed in order (they change state that affects later requests). `async-lsp` is the only framework that gets this right by design.
