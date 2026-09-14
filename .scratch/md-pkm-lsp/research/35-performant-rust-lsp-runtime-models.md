# Research: Performant Rust LSP runtime models beyond rust-analyzer

Resolves ticket [35 — Research: performant Rust LSP runtime models beyond rust-analyzer](../issues/35-research-performant-rust-lsp-runtime-models.md), fired to broaden ticket 09's evidence base beyond rust-analyzer alone (raised mid-grilling-session 2026-09-04 after user pushback that leaning on rust-analyzer as the sole precedent was too conservative).

Sources: four parallel `librarian` subagents reading actual source (Cargo.toml, server main-loop/scheduler code, cancellation handling) plus web search for community/practitioner reputation, per the map's standing rule that technology-comparison questions must check both axes separately.

## Astral `ty` (`astral-sh/ruff`, `crates/ty_server`)

- **Runtime**: no async runtime at all. `crossbeam` channels + `jod-thread` for thread management, `lsp-server`+`lsp-types` for transport — the same synchronous crates rust-analyzer uses.
- **Concurrency**: custom `Scheduler` dispatching to a worker thread pool (`crates/ty_server/src/server/schedule.rs`), analysis engine built on `salsa` (query-based incremental computation, memoized, safe concurrent reads across threads).
- **Cancellation**: cooperative — a `$/cancelRequest` sets a token; in-flight work also gets interrupted by `salsa::Cancelled` panics when a concurrent edit bumps the database revision, caught and translated into either a retry (if cancelled by a DB write) or a silent drop (if cancelled by the client).
- **Workload**: heavily CPU-bound (parsing, name resolution, type inference). Design rationale is explicit: incrementality (salsa) is the performance lever, not I/O concurrency.
- **Community reputation**: very strong ("10-100x faster than mypy/Pyright"), attributed specifically to salsa's fine-grained incrementality — not to runtime/concurrency choice.

## `ruff server` (`astral-sh/ruff`, `crates/ruff_server`)

- **Runtime**: none. No `tokio`/`async-std` anywhere in the crate; `lsp-server`+`crossbeam`, same family as `ty`.
- **Concurrency**: two explicit thread pools (`crates/ruff_server/src/server/schedule.rs`) — a single-threaded `fmt_pool` for formatting and a CPU-core-sized `background_pool` for linting/diagnostics, scheduled with latency-sensitive vs. worker priorities. No `salsa` (that's `ty`'s engine, not the linter's).
- **Cancellation**: cooperative but coarse — an `AtomicBool`-backed token checked only *before* a task starts (`crates/ruff_server/src/server/api.rs`), not threaded into the parsing/linting loop itself.
- **Workload**: strictly CPU-bound, sub-millisecond-to-millisecond tasks. Explicit rationale: `ruff_server` replaced a prior Python wrapper (`ruff-lsp`) that spawned the `ruff` CLI as a subprocess per keystroke; the native server's whole point was eliminating that serialization/subprocess overhead by linking the Rust crates directly and keeping state in-process — async was never part of the value proposition.
- **Community reputation**: "blazingly fast," attributed to native Rust + avoiding Python/subprocess overhead + hand-optimized parsing; the runtime/concurrency choice is essentially undiscussed by practitioners, i.e. not a factor in the reputation either way.

## Biome (`biomejs/biome`, `crates/biome_lsp` + `crates/biome_service`)

- **Runtime**: `tokio` + `tower-lsp-server` (current maintained fork, v0.23.0) — the one genuine divergence among the four.
- **Reach**: strictly transport-loop-only. `biome_lsp` handlers are `async fn` to satisfy `tower-lsp`'s trait contract, but they immediately delegate through a synchronous wrapper (`catch_lsp_operation`) into the `Workspace` trait (`crates/biome_service/src/workspace.rs`), whose methods (`format_file`, `pull_diagnostics`, etc.) are plain synchronous functions returning `Result<T, WorkspaceError>` — never `Future`s.
- **Concurrency**: hybrid — `rayon` (`rayon::scope`, `rayon::ThreadPoolBuilder`) for multi-file work-stealing parallelism (e.g. `scan_project`), `salsa` for incremental computation, single-file requests processed synchronously on the invoking thread.
- **Cancellation**: cooperative via `salsa::Cancelled::catch` wrapping the synchronous core calls — a cancelled salsa query panics with a typed payload, caught and translated to an LSP cancellation error. Not a dropped `Future`.
- **Workload**: CPU-bound (AST parsing/linting/formatting); file scanning has some I/O but it's not the bottleneck.
- **Design rationale — explicit and load-bearing**: the standard Rust-ecosystem split is followed deliberately — `tokio` for the I/O-bound LSP transport/session layer (multi-client handling, file-watch events, debounced `didChange` coalescing), `rayon` for CPU-bound parallel work, keeping the core `biome_service` synchronous specifically so CPU-bound tasks never stall the `tokio` executor.
- **Community reputation**: attributed to Rust's memory model (no V8/Node GC pauses), parsing once and reusing the AST for both linting and formatting (vs. ESLint+Prettier's double-parse), and `rayon`-driven parallelism — never to the async runtime.

## Taplo (`tamasfe/taplo`)

- **Runtime**: `tokio`, but via a **custom, unmaintained hand-rolled stub** (`lsp-async-stub`), not `tower-lsp`.
- **Reach**: transport/dispatch only — the core `taplo` crate (parsing, DOM, formatting) has zero async dependencies; `taplo-lsp` handlers are `async fn` but only `.await` a workspace read-lock before running the synchronous parser/formatter to completion on a single-threaded `tokio::task::LocalSet`.
- **Concurrency**: effectively single-threaded — the `LocalSet` model means CPU-bound synchronous parsing blocks the entire dispatch thread until it finishes; concurrency only exists during I/O-yield points (lock waits).
- **Cancellation**: cooperative in theory (`AtomicBool` token) but **practically inert** — the synchronous parser/formatter never checks the token, so cancellation has no effect once work starts.
- **Design rationale — a cautionary tale, not an endorsement**: per the maintainer's own wrap-up issue (`tamasfe/taplo#715`, "The future of the project"), the custom async-stub architecture was called a "learning experience" and "a mess"; the maintainer explicitly wanted to abandon it in favor of rebasing onto **`tower-lsp`** — i.e. Taplo's actual expert opinion on its own design is that it should have looked more like Biome's shape, not that hand-rolled single-threaded async was a deliberate performance choice.
- **Community reputation**: "near-instant," attributed to being a compiled Rust binary doing synchronous parsing — explicitly *not* to its async model. The one place async actually causes user-visible problems: network-bound schema fetches blocking/lagging the server, an I/O-bound task the sync-only tools (`ty`, `ruff_server`) don't have to contend with at all.

## Synthesis

Across all four, a clean, consistent pattern:

1. **The core analysis engine is synchronous in every single case, no exceptions.** No performant Rust LSP examined — regardless of whether it uses `tokio` at all — makes its parsing/linting/formatting/type-inference functions themselves `async fn`. Ticket 09's option **(c) whole-process-async has zero precedent among tools with a genuine speed reputation** and should be treated as disfavored by the evidence, not a live contender.
2. **Two real, viable shapes exist among tools actually reputed for speed**, splitting cleanly on whether the project has non-analysis I/O concerns to juggle:
   - **Fully synchronous, no runtime at all** (`ty`, `ruff_server`, and by extension rust-analyzer) — `lsp-server`+`lsp-types`, thread pools, cooperative/salsa-checkpoint cancellation. Chosen when the workload is purely CPU-bound with no meaningfully concurrent I/O to manage.
   - **`tokio`+`tower-lsp`(-family) at the transport layer only, synchronous core underneath** (Biome, and Taplo's own maintainer's stated preference) — chosen when the LSP layer itself has genuine I/O-bound concerns beyond just "read a request, write a response": multi-client sessions, file-watch event streams, debounced edit coalescing, network-bound auxiliary fetches (schema downloads).
3. **Taplo is a warning, not a counterexample**: its async stub is unmaintained, self-described as a mess by its own author, whose stated preference was to move toward `tower-lsp` (option (b))'s shape — not toward more async, and not toward staying fully sync either. It doesn't support "avoid async runtimes as a rule"; it supports "if you take on async, use a maintained framework (`tower-lsp-server`) instead of hand-rolling the transport loop," which is already a given for Traces (ticket 09's crate shortlist doesn't include hand-rolling a stub).
4. **The deciding factor across all four isn't "is speed a goal" (it is, for all of them) — it's whether the LSP process has non-trivial concurrent I/O beyond request/response.** `ty`/`ruff_server` don't (no file watching, no multi-client, no network fetches in the hot path); Biome does (file-watch events, project scanning, multi-file coordination) and picked (b) deliberately for exactly that reason.
