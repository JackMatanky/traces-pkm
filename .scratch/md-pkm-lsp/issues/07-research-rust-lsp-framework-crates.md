# Research: Rust LSP framework/transport crate landscape

Type: research
Status: resolved

## Question

Traces today has **zero async runtime dependency** (confirmed: no tokio/async-std anywhere in `src/`, fully synchronous single-shot CLI process). Using `rust-docs-mcp` as the primary source (cache each crate via `cache_crate`, then `structure`/`search_items_preview`/`get_item_docs`/`get_item_source`), and the crates' repositories as secondary source, evaluate the Rust LSP server framework/transport landscape:

- `tower-lsp` (and its maintained fork(s) — check for `tower-lsp-server` or similar; the original `tower-lsp` has known maintenance-gap history, verify current status) — API shape, whether it *requires* an async runtime (tokio) end-to-end or can be driven from a sync core via a thin async shell, cancellation support, how it maps JSON-RPC methods to trait methods.
- `lsp-server` + `lsp-types` (the rust-analyzer-authored pair) — API shape, whether it's synchronous/thread-based (no forced async runtime), how request/response/notification dispatch works, how cancellation is exposed to the handler.
- `lsp-types` alone as a protocol-DTO-only dependency (usable regardless of which transport/framework is chosen) — version, coverage of LSP 3.18 features (semantic tokens, inline completion, etc.), `serde` integration.
- Any other actively-maintained Rust LSP framework worth considering (check crates.io recency).
- For each candidate: does adopting it force Traces to take on an async runtime dependency (tokio) for the *entire* LSP binary/crate, or can request handling stay synchronous with only the transport loop being async/threaded? This directly matters given Traces' current all-synchronous core (Index/Query/Schema/Template are all sync, and Template rendering synchronously blocks on interactive `DialogProvider` I/O — an async-locked-in framework would create an awkward sync-in-async boundary).
- Cancellation-token propagation: how each framework surfaces `$/cancelRequest` to handler code, and whether that maps cleanly onto Rust's cooperative-cancellation limitations (no forced preemption).

Write findings to `.scratch/md-pkm-lsp/research/rust-lsp-framework-crates.md`, citing rust-docs-mcp findings and crate versions precisely. This ticket answers "what exists and what it demands", not "which one to pick" — the pick is architecture ticket 09 (LSP framework & runtime model), which this ticket blocks.

## Answer

**Corrected 2026-09-03** (the answer originally posted here over-weighted one axis and omitted the other; see the research file's correction note for the full story):

Two separate axes, not one. (1) Runtime demands, verified against crate source: `tower-lsp`/`tower-lsp-server`/`async-lsp` force an async runtime end-to-end and cancel `$/cancelRequest` by dropping a `Future`, which does not interrupt `spawn_blocking`'d synchronous work; `lsp-server`+`lsp-types` (the pair rust-analyzer publishes and uses) is a minimal synchronous crossbeam-channel API with zero async requirement and fully manual cancellation. (2) Ecosystem adoption, verified by web research after the original answer was found to have skipped this axis entirely: `tower-lsp-server` (the actively-maintained `tower-lsp-community` fork of the now-abandoned original `tower-lsp`) is the current de facto standard and most-recommended choice across 2025/2026 tutorials and practitioner discussion — `lsp-server`'s low-level, write-everything-yourself shape is what rust-analyzer specifically chose for its own unusual needs (it also built its own incremental query engine), not evidence of the broader ecosystem default. `lsp-types` is usable standalone regardless of framework; 3.18 features are behind an unstable `proposed` flag.

Neither axis alone picks a winner for ticket 09 — axis (1) favors `lsp-server` only if Traces' core stays synchronous; axis (2) favors `tower-lsp-server` regardless, and becomes decisive with no offsetting cost if the core moves to async.

Full findings: [`research/rust-lsp-framework-crates.md`](../research/rust-lsp-framework-crates.md)
