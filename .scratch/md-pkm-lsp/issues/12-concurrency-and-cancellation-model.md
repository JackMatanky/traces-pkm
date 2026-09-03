# Concurrency & cancellation model

Type: grilling
Blocked by: 09, 10

## Question

Given the runtime model (ticket 09) and analysis-host design (ticket 10), decide the concrete concurrency model for handling LSP requests:

- Thread-per-request vs a bounded worker pool vs a single-threaded event loop reusing the existing all-synchronous Index/Query/Schema code paths directly.
- How `$/cancelRequest` (see `docs/refs/lsp_spec.md`'s Cancellation section) propagates into a synchronous, non-async call stack — Rust has no forced preemption, so cancellation must be cooperative (checked at loop boundaries in e.g. a linear index scan) or coarse (drop the whole request's thread/task and discard its result on completion). Ground this against `QueryArch` findings: source-expression resolution is currently a linear `(0..index.entries().len())` scan (`src/query/service.rs:132-139`) with no existing cancellation checkpoints — decide whether/where checkpoints get added, or whether requests are just cheap enough (target workspace sizes, see ticket 25) that cancellation mid-scan is unnecessary and only "supersede in the queue before starting" cancellation is implemented.
- Read/write exclusivity around index refresh: can queries run concurrently with an in-progress refresh (reading the previous immutable snapshot until the new one swaps in), or does refresh block new requests.
- Interaction with `$/progress` (see spec) for long-running operations like full workspace reindex on startup.

Blocks: 33(performance targets, since concurrency model bounds achievable latency).
