# Research: Performant Rust LSP runtime models beyond rust-analyzer

Type: research
Status: resolved

## Question

Ticket 09 (LSP framework/transport crate & runtime model) is leaning on rust-analyzer as its sole architectural precedent for "stay fully synchronous, no async runtime." Rust-analyzer's shape is also unusual — it predates `tower-lsp-server`'s rise and was shaped by building its own incremental (salsa) engine from scratch, not necessarily representative of what a newer, genuinely performance-obsessed Rust LSP would choose today.

Investigate the runtime/concurrency architecture of other well-regarded, performance-oriented Rust-based language servers to broaden the evidence base:

- **Astral's `ty`** (Rust-based Python type checker + LSP; may currently live inside the `astral-sh/ruff` monorepo or a separate `astral-sh/ty` repo — confirm current location first via web/GitHub search, naming has shifted, formerly known as "red-knot")
- **`ruff server`** (`ruff_server`, Astral's Rust-native Python linter/formatter LSP, inside `astral-sh/ruff`)
- **Biome** (`biomejs/biome` — Rust-based JS/TS/CSS/JSON toolchain with an LSP, marketed heavily on speed)
- **Taplo** (`tamasfe/taplo` — Rust-based TOML LSP)

For each, determine:
- Which async runtime (if any) it depends on — `tokio`, `async-std`, none — and how far async reaches: transport-loop-only (sync worker threads/pool for the actual analysis work) vs whole-process-async (core analysis functions are `async fn`).
- Concurrency model for the analysis work itself: thread pool, `rayon`, actor-model, single-threaded, or something else.
- How it handles LSP request cancellation (`$/cancelRequest`) — cooperative checkpoints, dropped `Future`, or something else.
- Whether its core workload is I/O-bound or CPU-bound (parsing/analysis, like Traces), and whether that shaped the runtime choice.
- Any explicit design rationale documented (README, blog post, design doc, maintainer commentary) for why that runtime/concurrency shape was chosen — especially anything framed around performance.

Per the map's generalized rule for technology-comparison questions: answer both axes separately — what the source/design docs actually demand or state, **and** what the wider community/benchmarks/practitioner discussion say about whether these tools are in fact considered fast, and whether that reputation is attributed to the runtime model or to something else (algorithms, data structures, avoiding unnecessary work). Do not let one axis stand in for the other.

## Answer

Investigated four Rust-based LSPs with genuine speed reputations, reading actual server source (not just crate names) via four parallel `librarian` subagents:

- **Astral `ty`** (`crates/ty_server`) and **`ruff server`** (`crates/ruff_server`) — both fully synchronous, zero async runtime, `lsp-server`+`lsp-types` (rust-analyzer's own crates), thread-pool concurrency, cooperative/salsa-checkpoint cancellation. Speed is attributed by maintainers and community entirely to the analysis engine (salsa incrementality for `ty`; native Rust + no subprocess overhead for `ruff_server`) — never to the runtime choice.
- **Biome** — `tokio`+`tower-lsp-server`, but **strictly transport-loop-only**: every `Workspace` trait method in the core `biome_service` engine is a plain synchronous function, never `async fn`. `rayon` handles CPU-bound multi-file parallelism; `salsa` handles incrementality. Async exists specifically because the LSP layer has real concurrent I/O to manage (multi-client sessions, file-watch events, debounced edit coalescing) — an explicit, documented architectural choice, not an accident.
- **Taplo** — `tokio` via a hand-rolled, unmaintained async stub; core parsing/formatting library is fully synchronous. The maintainer's own retrospective (`tamasfe/taplo#715`) calls the custom stub "a mess" and states a preference to rebase onto `tower-lsp` — i.e. even Taplo's own author's verdict favors Biome's shape over what Taplo actually shipped. Not a counterexample to anything; a caution against hand-rolling the transport layer, which was never on ticket 09's table anyway.

**Zero precedent, across all four, for making the core analysis engine itself `async fn`** (ticket 09's option (c)) — every tool with a genuine speed reputation keeps parsing/analysis synchronous regardless of what the LSP transport layer does. The real split is between two viable shapes: fully synchronous end-to-end (`ty`/`ruff_server`, matching rust-analyzer) when the process has no non-trivial concurrent I/O beyond request/response, versus `tokio`+`tower-lsp-server` at the transport boundary with a synchronous core underneath (Biome) when the process does have real concurrent I/O to juggle — file-watch events, multi-client sessions, background scanning.

Full findings, per-project detail, and source citations: [research/performant-rust-lsp-runtime-models.md](../research/performant-rust-lsp-runtime-models.md).
