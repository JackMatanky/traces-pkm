# Research: rust-analyzer host/snapshot architecture precedent

Resolves ticket [06-research-rust-analyzer-precedent](../issues/06-research-rust-analyzer-precedent.md).

Source: `https://raw.githubusercontent.com/rust-lang/rust-analyzer/master/docs/book/src/contributing/architecture.md` (rust-analyzer's own architecture documentation, unusually candid/didactic).

## Overview

rust-analyzer is an "IDE backend": independent library crates for parsing, incremental computation (`salsa`), and LSP wiring, built around a purely in-memory, query-based model. Ground truth (file text) flows in from the client; a semantic model is lazily derived on demand. This grounds — confirms, with real engineering detail — the map's prior "long-lived analysis host and immutable/queryable snapshots" and "parse-once/project-many" hypotheses; it does not itself decide whether Traces should adopt the full pattern.

## Findings

**Host/Snapshot split** (`architecture.md:72-73, 217-219`)
- `AnalysisHost`: the mutable, single-owner state; exposes a transactional `apply_change` to ingest edits/project updates.
- `Analysis`: an **immutable, cheaply-cloneable snapshot** of state at a point in time — "parse-once/project-many": many request-handling threads query an `Analysis` snapshot concurrently, lock-free, while `AnalysisHost` prepares the next state on the main thread.

**Incremental recomputation (salsa)** (`architecture.md:145-181`)
- `salsa` = a key-value store computing derived values via specified functions; strictly separates **input queries** (ground facts, e.g. file text) from **derived queries** (ASTs, semantic models).
- Core architecture invariant: *"typing inside a function's body never invalidates global derived data."* **Minimum-viable lesson for a simpler system with no query engine**: the real payoff isn't the query engine itself, it's the *discipline of separating local/volatile state (function bodies) from global/stable state (signatures, module structure)* so a small, local edit doesn't force wide recomputation — achievable in a much simpler system (e.g. per-file granularity: editing file A's body never invalidates file B's derived link-index entries unless A→B's specific reference actually changed) without adopting salsa itself. The cost of the full pattern is "rigorous discipline in how data is structured" (raw IDs, not object references) — a real engineering tax, not free.

**Cancellation mechanism** (`architecture.md:388-395`)
- A global revision counter on the salsa database; `apply_change` bumps it. Background threads doing salsa computations periodically check the counter; on an increment, they **panic** with a special `Canceled::throw` value — "rust-analyzer requires unwinding." The `ide` crate boundary catches this specific panic via `catch_unwind` and turns it into `Result<T, Cancelled>`. This is a genuinely unusual (panic-based, not Future-drop-based) cancellation mechanism worth weighing against the `lsp-server`/cooperative-`AtomicBool` approach found in ticket 07's research.

**VFS layer (open-buffer vs on-disk)** (`architecture.md:46-47, 156-158`)
- `base-db` has **no concept of `std::path::Path`** at all — files are opaque `FileId`s; "the analyzer keeps all input data in memory and never does any IO" for source code. The VFS is the sole sync boundary: an editor's `didChange` updates the VFS's in-memory buffer text and notifies salsa. This is a directly transferable pattern for Traces' "unsaved buffers without disk writes" constraint (ticket 14): treat the editor as the sole source of truth for open-file content, disk reads only populate files the editor hasn't opened.

**Threading model** (`architecture.md:239-246`)
- Explicit `main_loop` event loop (not open-ended futures/callbacks) over a closed `enum` of event kinds. State-modifying requests (or ones that might block typing) run **on the main thread**; all read-only requests run on **background threads** against immutable snapshots, preemptible via the cancellation mechanism above.

**Documented lessons/pitfalls** (`architecture.md:110-111, 244-246, 515-519`)
- *Serialization is a rigid boundary*: `ide`/`hir`/`base_db` types are deliberately **not** `Serialize` — adding it would force backward-compatibility guarantees on internal types; serialization is confined entirely to the outermost LSP-facing crate.
- *Server is stateless, à la HTTP*: cross-request context is never held server-side implicitly; the client resends what's needed.
- *Parsing must never fail*: the parser returns `(T, Vec<Error>)`, never `Result<T, Error>` — because an IDE constantly analyzes broken/incomplete code as the user types, so partial/recoverable syntax trees are a hard requirement, not a nice-to-have.
- *UI-facing API is POD, editor-vocabulary, not compiler-vocabulary*: the public `ide` API exposes plain structs using offsets/strings, deliberately not exposing internal syntax-tree/semantic-model types.

## Key takeaway for the map

This is real evidence, not just a name-check: the Host/Snapshot split (ticket 10) is a proven, load-bearing pattern at rust-analyzer's scale, but adopting salsa wholesale is a separate, much larger decision with real costs (raw-ID data modeling discipline) — ticket 10 should explicitly decide whether Traces needs the full salsa-style incremental query engine or whether the "immutable Arc-swapped `FileIndex` snapshot, coarse whole-file invalidation" model it already has today (per IndexArch findings) is sufficient at Traces' target scale (ticket 33 sets that scale). The VFS pattern (files-as-opaque-IDs, editor-buffer-as-sole-truth-for-open-files) is directly reusable for ticket 14 regardless of that choice. The "parsing must never fail, recoverable partial trees" invariant is directly relevant to ticket 11/22: Traces' `Note` parser and template static-analysis path should both tolerate malformed input gracefully rather than erroring out of producing any AST at all, since an LSP is constantly asked to analyze a document mid-edit.
