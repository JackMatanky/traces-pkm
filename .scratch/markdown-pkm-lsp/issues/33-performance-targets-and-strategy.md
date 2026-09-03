# Performance targets & strategy

Type: grilling
Blocked by: 10, 12, 13

## Question

Grounding: the only existing performance signal is `benches/index_lifecycle.rs`, which benchmarks CLI-scale refresh latency up to 1,000 files and asserts near-instantaneous no-op refresh; `benches/index_codec.rs` benchmarks `postcard` (de)serialization cost per read/write. Indexing/query execution is single-threaded throughout (no `rayon`), and query source-resolution is a linear scan of the `FileIndex` (`src/query/service.rs:132-139`), not an inverted index. No numbers exist today for interactive per-keystroke LSP latency, large-workspace (10k+ note) scale, or memory footprint.

Once the analysis-host (10), concurrency (12), and persistence/caching (13) decisions are made, set concrete, falsifiable targets and the strategy to hit them:

- Initial full-workspace indexing time targets at stated workspace sizes. **The "1k / 10k / 50k notes" figures in this ticket's own earlier draft were illustrative placeholders I invented while charting the map, not evidence of actual or expected Traces vault sizes** — before setting a falsifiable target, find or establish a real basis for target scale (e.g. ask the user directly, check for any existing user/vault-size data, or look at what comparable PKM tools cite as "large vault" in their own docs/issue trackers — Markdown Oxide's `MAX_INDEXED_LINES = 10_000`-per-file cap and any vault-size discussion in the zk/Marksman research are a starting point, not a substitute). Then decide whether the existing single-threaded, linear-scan approach needs parallelization (e.g. `rayon` — not used elsewhere in Traces today, but that's not a reason to withhold it here if the LSP's indexing hot path genuinely benefits; evaluate it on its own merits for this workload) or an inverted index (tag→files, folder→files) to hit the target, or whether the current approach is provably sufficient at target scale.
- Incremental single-edit (`didChange`) latency budget (interactive-feel threshold, typically sub-100ms for completion) and what specifically must stay off that hot path (e.g. full-tree redb diffing must not run per keystroke — ties directly to ticket 13's per-keystroke-vs-per-save invalidation granularity decision).
- Interactive request latency budgets per capability (hover, completion, definition should be near-instant; workspace-wide rename/diagnostics may tolerate higher latency, potentially with `$/progress` reporting per ticket 12).
- Memory budget for large PKM workspaces — does keeping the full parsed `Note` AST (not just `FileBase`) resident in memory for every file (as `FileIndex` does today) remain viable at 50k+ notes, or does LSP mode need a different residency policy (e.g. lazily-loaded/evictable parsed-note cache) than the CLI's always-full-index model.
- Which existing benchmarks get extended/adapted for LSP scenarios (e.g. a new `benches/lsp_incremental.rs`) vs which need a genuinely new benchmark harness (e.g. simulated keystroke-latency benchmarks).
