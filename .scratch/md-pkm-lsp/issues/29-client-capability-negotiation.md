# Client capability negotiation & graceful degradation

Type: grilling
Blocked by: 09

## Question

Once the framework/runtime (ticket 09) is chosen, decide the capability-negotiation strategy per `docs/refs/lsp_spec.md`'s `initialize`/`ClientCapabilities`/`ServerCapabilities` sections:

- Which server capabilities are unconditionally registered (statically, at `initialize` response time) vs dynamically registered later (`client/registerCapability`) based on what the client actually advertises supporting.
- Minimum viable client: what happens for a client with a sparse `ClientCapabilities` (e.g. no `workspace/didChangeWatchedFiles` support, no pull-diagnostics, no `positionEncoding` negotiation) — does every feature degrade gracefully, or are some features simply unavailable and the server reports that clearly.
- `positionEncoding` negotiation (LSP 3.17+, ties to ticket 11's UTF-8/UTF-16 decision): request `utf-8` when the client advertises support, falling back to `utf-16` otherwise.
- Version/capability floor: does Traces target LSP 3.17 baseline features only, or does it opportunistically use 3.18 features (per `docs/refs/lsp_spec.md`'s "What's new in 3.18") where advertised, decide the policy once, not per-feature ad hoc.
