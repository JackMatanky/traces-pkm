# Research: rust-analyzer host/snapshot architecture precedent

Type: research
Status: resolved

## Question

Investigate rust-analyzer's architecture as a precedent for a Rust-native language server built around an existing synchronous, non-async analysis core — read https://github.com/rust-lang/rust-analyzer/blob/master/docs/dev/architecture.md directly (and linked docs it references, e.g. the "analysis" vs "analysis-host" split, salsa usage) via `read`.

This grounds (does not decide) the prior architectural hypotheses already on the table: "a long-lived analysis host and immutable/queryable snapshots" and "parse-once/project-many semantic analysis". Establish and cite for each:

- The precise Host/Snapshot split: what `AnalysisHost` owns (mutable, single-writer) vs what `Analysis` (the snapshot) exposes (immutable, cheaply cloned, freely shared with request-handling threads).
- How incremental recomputation works (salsa query system) at a level useful for a *much simpler* system that has no query-engine today — what's the minimum viable subset of this idea (e.g. "just an Arc-swapped immutable snapshot rebuilt on each edit" vs a full incremental query DB) and what tradeoffs separate them.
- How cancellation works when a new request/edit arrives while a snapshot-based computation is in flight (their `Canceled` mechanism).
- How rust-analyzer handles the open-editor-buffer vs on-disk-file distinction (their VFS layer) — this is directly analogous to Traces' "unsaved editor buffers must be reflected in live language intelligence without requiring writes to disk" constraint.
- Threading model: how many threads, what each does (main loop, background indexing, request handlers).
- Any explicitly documented lessons/pitfalls (their docs are unusually candid about mistakes) relevant to a team building this for the first time.

Write findings to `.scratch/md-pkm-lsp/research/rust-analyzer-precedent.md`, citing each claim's source (file + section). This ticket answers "what does the pattern look like and what does it cost", not "should Traces adopt it" — that decision belongs to the architecture tickets it unblocks.

## Answer

AnalysisHost (mutable)/Analysis (immutable snapshot) split enables parse-once/project-many. Salsa's core lesson — isolate local/volatile state from global/stable state so local edits don't invalidate global data — is separable from adopting the full salsa query engine. Cancellation is panic-based (Canceled::throw + catch_unwind), an alternative to lsp-server's cooperative-flag approach. VFS treats files as opaque IDs with the editor as sole truth for open buffers — directly reusable for the live-buffer-overlay decision. Parser must never fail (returns (T, Vec<Error>), not Result) — relevant to Note parsing and template static analysis both needing to tolerate malformed/partial input.

Full findings: [`research/rust-analyzer-precedent.md`](../research/rust-analyzer-precedent.md)
