# Research: Rust LSP framework/transport crate landscape

Resolves ticket [07-research-rust-lsp-framework-crates](../issues/07-research-rust-lsp-framework-crates.md).

Sources: rust-docs-mcp against `tower-lsp` (0.20.0), `tower-lsp-server` (0.23.0), `lsp-server` (0.10.0), `lsp-types` (0.97.0), `async-lsp` (0.2.4); web research on ecosystem adoption/maintenance/community consensus (added 2026-09-03 after this file was found to be skewed — see the correction note below).

## Correction note (2026-09-03)

The first version of this file answered a purely technical sub-question (what does each crate's *code* demand — async or sync, how does cancellation work) and let that single axis stand in for the whole "what exists and what it demands" question the ticket actually asked. It never checked real-world adoption, documentation quality, or practitioner consensus — the socio-technical half of "what exists" — because the research instructions only pointed at `rust-docs-mcp` and crate READMEs, not at how the Rust community actually uses and recommends these crates today. That's a real gap: **`tower-lsp-server` is the current, actively-maintained, de facto standard recommendation** for building a Rust LSP server, per multiple independent 2025/2026 sources (below) — a fact the original version of this file mentioned only in passing ("0.23.0, maintained fork") without conveying its weight. This rewrite adds that dimension as a first-class finding, not a footnote.

## Overview

Two genuinely separate questions, each with its own answer — conflating them (as the first draft did) understates the case for `tower-lsp-server`:

1. **What does each crate's runtime model demand of the calling code?** Sharp, verifiable split: `tower-lsp`/`tower-lsp-server`/`async-lsp` require an async runtime end-to-end; `lsp-server` is synchronous, channel-based, zero runtime requirement.
2. **What does the Rust ecosystem actually reach for, and why?** `tower-lsp-server` — not `lsp-server` — is the standard, most-recommended, most-documented choice for teams building an LSP server from scratch, specifically *because* of its higher-level ergonomics.

Both are real inputs to ticket 09. Neither one should silently decide the ticket.

## Findings

### Runtime/cancellation model (rust-docs-mcp, crate source)

**`tower-lsp` (0.20.0) / `tower-lsp-server` (0.23.0)**
- API: `LanguageServer` trait, every method `async fn` via `#[rpc(name = "...")]` macros; dispatch hidden behind an internal `Router` — you implement trait methods, the framework does JSON-RPC parsing, serialization, and request routing for you.
- Runtime: end-to-end async — the whole binary needs an async runtime (typically `tokio`). A synchronous core must be pushed through `spawn_blocking`.
- Cancellation: automatic via a `Cancel` layer — on `$/cancelRequest`, drops the underlying `Future` via `futures::future::abortable`. Verified directly against the unpacked crate source (`service/pending.rs`): `pub struct Pending(Arc<DashMap<Id, future::AbortHandle>>)`, `future::abortable(fut)`. **Real, confirmed mismatch for a sync core**: dropping a `Future` wrapping a `spawn_blocking` `JoinHandle` does not interrupt the blocking OS thread underneath — cancellation silently doesn't cancel synchronous work, only the outer future. This is a genuine cost, not invented, but it is a cost *of pairing tower-lsp-server with a synchronous core specifically* — it disappears entirely if the core itself is async (see ticket 09, which already treats that as a live option).

**`lsp-server` (0.10.0) + `lsp-types`**
- API: minimal — a `Connection { sender, receiver }` over `crossbeam-channel`; no trait, no router, no request routing, no serialization convenience — you write the `while let Ok(msg) = receiver.recv()` loop and every method dispatch by hand.
- Runtime: zero async requirement.
- Cancellation: fully manual — `$/cancelRequest` bookkeeping is provided (`ReqQueue::cancel`, verified in source), but actually *interrupting* in-flight work is entirely the implementer's job, every time, for every request type.

**`lsp-types` (0.97.0)** — pure protocol DTOs, usable independent of transport choice regardless of which framework is picked. 3.18 features gated behind an unstable `proposed` flag.

**`async-lsp` (0.2.4)** — `tower::Service`-based, forces an async runtime, same drop-based cancellation mismatch as `tower-lsp`.

### Ecosystem adoption & practitioner consensus (web research, added on correction)

- **The original `ebkalderon/tower-lsp` is genuinely unmaintained** — multiple independent sources (crates.io, `tower-lsp-community/tower-lsp-server` issue #1, libhunt comparison) confirm the community organized a fork specifically because of it. This part of the original research held up.
- **`tower-lsp-server`, not the original, is now the standard answer.** It's maintained by the dedicated `tower-lsp-community` GitHub organization, receives regular releases (0.23.x track), and is described across independent sources as "the recommended crate for new projects." (`github.com/tower-lsp-community`, `crates.io/crates/tower-lsp-server`, `libhunt.com/compare-tower-vs-tower-lsp`)
- **Current tutorials and practitioner discussion converge on `tower-lsp`/`tower-lsp-server` as the default recommendation for a first LSP server**, precisely because of the ergonomics `lsp-server` deliberately omits: no manual JSON-RPC parsing, no manual dispatch loop, no manual request-routing — you implement `LanguageServer` trait methods and the framework does the rest. A 2025/2026 web sweep (a technical deep-dive at `aroy.sh/posts/lsp-deep-dive`, a March-2026 walkthrough at `codeinput.com/blog/lsp-server`, an r/rust "trying to make an LSP for the first time" thread) consistently frames `lsp-server` as the *lower-level, more control, steeper-learning-curve, "from scratch"* option chosen when a team has a specific reason to want manual control (rust-analyzer's reason: full ownership of the event loop and salsa-integrated cancellation, per the rust-analyzer-precedent research) — not as the generally-recommended default.
- `lsp-server` is what `rust-analyzer` itself publishes and uses — real, notable, but it is evidence of one specific, unusually-demanding project's choice (a project that also built its own incremental query engine, `salsa`, precisely because it needed control `tower-lsp`-style abstractions don't offer), not evidence that `lsp-server` is the broader ecosystem's default.

## Key takeaway for the map — corrected

This is a genuine two-factor trade-off, not a "clean, decisive" answer to hand ticket 09:

- **On pure runtime-model fit with Traces' code as it stands today**, `lsp-server` requires zero change to the synchronous Index/Query/Schema/Template call chain, and its manual cancellation is at least honest about being manual (no false confidence that `$/cancelRequest` works when it silently doesn't, the way `tower-lsp-server` + `spawn_blocking` would).
- **On ecosystem maturity, documentation, community support, and implementation velocity**, `tower-lsp-server` is the stronger choice by a wide margin — it is the standard, actively-maintained, most-tutorialed path, and choosing it removes an entire category of hand-written JSON-RPC/dispatch/serialization code that `lsp-server` requires the team to write and maintain itself.
- These two factors point in **different directions**, and which one should dominate depends on a decision ticket 09 has not made yet: whether Traces' core (Index/Query/Schema/Template) moves to async at all. If it does, `tower-lsp-server`'s cancellation mismatch disappears (real `.await` points can be genuinely interrupted) and its ecosystem advantage becomes decisive with no offsetting cost. If the core stays synchronous, `lsp-server`'s fit is real but comes at the cost of writing and maintaining the JSON-RPC/dispatch layer `tower-lsp-server` would have supplied for free.
- **This file does not pick a winner.** Ticket 09 needs to weigh: (a) is Traces' core moving to async at all (a question this file's *original* draft implicitly foreclosed by treating "stays sync" as settled), and only then (b) which framework fits the resulting shape — not the reverse.
