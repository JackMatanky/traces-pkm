# Performance targets & strategy

Type: grilling
Blocked by: 10, 12, 13
Status: resolved

## Question

Grounding (reconciled 2026-09-23): the existing performance signals are the decomposed `benches/index_*.rs` suite — `index_lifecycle.rs` was split into `index_refresh.rs`, `index_build.rs`, `index_store.rs`, `index_inlinks.rs`, etc.; `benches/index_codec.rs` benchmarks `postcard` (de)serialization cost per read/write. Metadata scan and note parsing are already `rayon`-parallel (`src/index/service.rs`), but query source-resolution is still a linear scan of the `WorkspaceIndex` (`src/query/service.rs:132-139`), not an inverted index, and the merge-join delta is single-threaded. No numbers exist today for interactive per-keystroke LSP latency, large-workspace (10k+ note) scale, or memory footprint.

Once the analysis-host (10), concurrency (12), and persistence/caching (13) decisions are made, set concrete, falsifiable targets and the strategy to hit them:

- Initial full-workspace indexing time targets at stated workspace sizes. **The "1k / 10k / 50k notes" figures in this ticket's own earlier draft were illustrative placeholders I invented while charting the map, not evidence of actual or expected Traces vault sizes** — before setting a falsifiable target, find or establish a real basis for target scale (e.g. ask the user directly, check for any existing user/vault-size data, or look at what comparable PKM tools cite as "large vault" in their own docs/issue trackers — Markdown Oxide's `MAX_INDEXED_LINES = 10_000`-per-file cap and any vault-size discussion in the zk/Marksman research are a starting point, not a substitute). Then decide whether the existing linear-scan query approach (scan/parse are already `rayon`-parallel; `rayon` is already a dependency — reconciled 2026-09-23) needs an inverted index (tag→files, folder→files) to hit the target, or whether the current approach is provably sufficient at target scale.
- Incremental single-edit (`didChange`) latency budget (interactive-feel threshold, typically sub-100ms for completion) and what specifically must stay off that hot path (e.g. full-tree redb diffing must not run per keystroke — ties directly to ticket 13's per-keystroke-vs-per-save invalidation granularity decision).
- Interactive request latency budgets per capability (hover, completion, definition should be near-instant; workspace-wide rename/diagnostics may tolerate higher latency, potentially with `$/progress` reporting per ticket 12).
- Memory budget for large PKM workspaces — does keeping the full parsed `Note` AST (not just `FileBase`) resident in memory for every file (as `WorkspaceIndex` does today) remain viable at 50k+ notes, or does LSP mode need a different residency policy (e.g. lazily-loaded/evictable parsed-note cache) than the CLI's always-full-index model.
- Which existing benchmarks get extended/adapted for LSP scenarios (e.g. a new `benches/lsp_incremental.rs`) vs which need a genuinely new benchmark harness (e.g. simulated keystroke-latency benchmarks).

## Answer

### Scale basis

Target vault sizes from `benches/common/mod.rs:63`: `WORKSPACE_FILE_COUNTS` = [50, 200, 1_000, 5_000, 20_000] (reconciled 2026-09-23 — the earlier-drafted longer list was stale), with extrapolation to 50K/100K via `bench-model` script. The 20K anchor is the primary design target; 50K/100K are extrapolation checkpoints, not hard commitments. Markdown files average 5–20KB; typical PKM vaults range 100–5,000 notes; power users reach 10K–20K. No PKM LSP publishes vault-size benchmarks — Traces will set the standard.

### 1. Startup targets (cold & warm)

| Scenario | Target | Strategy |
|----------|--------|----------|
| Cold start (no redb cache) | **<2s for 20K files** | `rayon` parallel scan + parse; parallelism breakeven at ~1K items for parsing workloads |
| Warm start (redb exists) | **<500ms for 20K files** | Load redb index → filesystem diff → reparse only changed files (existing `refresh()` path) |
| Warm start (1K files) | **<100ms** | Same path, smaller scan |

**Why parallelize**: Markdown Oxide already uses `rayon::par_iter` for startup. rust-analyzer's parallel VFS prototype cut scan from 46s to 6–20s. pulldown-cmark parse is CPU-bound and independent per file — ideal for work-stealing. The existing `IndexerService::scan` metadata walk and `parse_notes` are already `rayon`-parallel (`src/index/service.rs`; reconciled 2026-09-23) — remaining cold-start work is verifying against the <2s@20K target (with `with_min_len` tuned to 2–4× CPU core count if the parallelism breakeven under-delivers), not introducing parallelism from scratch.

### 2. Incremental single-edit (didChange) latency

| Operation | Target | Strategy |
|-----------|--------|----------|
| Single-file reparse + index update | **<10ms** | pulldown-cmark: 0.20ms/48KB; extraction overhead on top; full re-parse per edit (universal pattern — no incremental parsing within markdown documents) |
| Structural diff (links/headings/tags) | **<1ms** | Compare extracted structural data against previous WorkspaceIndex entry; skip cascade if unchanged |
| Backlink recomputation | **<5ms** | Update inlink multimap for changed file + its neighbors; O(link-count), not O(vault-size) |
| Debounce window | **100ms** (provisional — ticket 25 owns final tiers) | Matches rumdl; avoids re-indexing on every keystroke |
| Full-tree redb diff | **Never on hot path** | Ticket 13: expand `RefreshPlan` with single-file constructor; `didChange` bypasses `RefreshPlan::collect` entirely |

### 3. Interactive request latency (p95)

| Capability | Target | Stretch | Strategy |
|------------|--------|---------|----------|
| Hover | **<10ms** | <5ms | In-memory hash-map lookup; metadata always resident |
| Completion | **<20ms** | <10ms | Wiki-link + heading completion from inverted index; no linear scan |
| Go-to-definition | **<5ms** | <2ms | O(1) inverted index lookup (Marksman achieves 1.5–3µs) |
| Find-references | **<20ms** | <5ms | Inverted index: file→outgoing-links multimap; O(refs), not O(vault) |
| Document symbols | **<10ms** | <5ms | Headings from in-memory index |
| Diagnostics (per-file) | **<20ms** | <10ms | Re-parse + lint one file; debounced, not per-keystroke |
| Rename | **<100ms** | <30ms | Find refs + apply edits; may use `$/progress` for large workspaces |

**Inverted index design** (ticket 13's "in-memory mutable index" made concrete):

```
wiki_link_index: HashMap<String, Vec<PathBuf>>   // file stem/title → paths
tag_index: HashMap<String, Vec<PathBuf>>          // tag → files
file_class_index: HashMap<String, Vec<PathBuf>>   // file class → files
outgoing_link_index: HashMap<PathBuf, Vec<Link>>  // file → its outgoing links
```

Metadata (paths, tags, links, headings, file-classes, frontmatter field names) always resident — ~100 bytes/file × 20K = ~2MB. AST bodies in LRU cache. Schema definitions (field names, types, constraints per FileClass) are small and static — always resident, never evicted.

### 4. Memory budget

| Scenario | Budget | Strategy |
|----------|--------|----------|
| Idle (vault indexed) | **<100MB** | Metadata always resident; AST bodies in LRU cache |
| Active editing (10 open buffers) | **<200MB** | Buffer content + index + diagnostics + schema definitions |
| Hard cap | **<500MB** | Leave room for VS Code + other extensions in 3GB host |
| Per-file cap | **10,000 lines** | Match Markdown Oxide's cap; reject/lazy-load beyond |

**Memory model** — evictable LRU cache with three tiers:

| Tier | Contents | Residency | Size estimate |
|------|----------|-----------|---------------|
| **Always resident** | File paths, tags, links, headings, file-classes, frontmatter field names, schema definitions | Never evicted | ~100 bytes/file × 20K = ~2MB + schema KBs |
| **LRU cache** | Parsed Note ASTs (full body) | Evicted under memory pressure; re-parse on demand from redb/disk | Configurable max (e.g. 256 entries) |
| **Transient** | Editor buffer snapshots, diagnostic results, in-flight query results | Dropped on completion | Bounded by concurrent requests |

**Degradation modes** (perl-lsp-rs pattern):
- **Normal**: full LRU cache, background indexing active
- **Warning** (>300MB): stop background indexing, reduce LRU capacity
- **Critical** (>400MB): evict LRU entries aggressively, serve only from always-resident metadata

**Frontmatter/schema intelligence impact**: Schema definitions (`HashMap<FileClass, SchemaDefinition>`) are small (field names + types + constraints per class) and always resident. Schema-aware completion/validation is a hash-map lookup + type check — O(1), negligible memory and compute cost. This is the major differentiator: Traces can do field-name completion, type-aware value completion, and schema validation for frontmatter because the schema index is always in memory and always cheap to query. No other Markdown/PKM LSP does this.

### 5. Benchmark plan

**Extend existing** (`benches/index_refresh.rs` — the former `benches/index_lifecycle.rs`, decomposed into `index_*.rs` files; reconciled 2026-09-23):
- Add `WorkspaceIndex::refresh/single-file-didchange` group — reparse one file + update in-memory index + recompute affected backlinks at each workspace size
- Add `WorkspaceIndex::refresh/parallel-cold-start` group — baseline is the current parallel scan/parse (`rayon`) plus the single-threaded merge-join delta; verify the <2s@20K target rather than parallelizing from scratch (reconciled 2026-09-23)
- Extend `WorkspaceIndex::build` to track peak memory via `jemalloc` stats or `std::mem::size_of_val`

**New file** (`benches/lsp_latency.rs`):
- **Interactive latency group**: simulated hover/completion/gotodef/find-refs at 1K/5K/10K/20K files; pure in-memory lookups against the inverted index
- **Memory footprint group**: measure `WorkspaceIndex` memory at each workspace size; track LRU cache hit/miss rates
- **Debounce effectiveness group**: simulate 100ms keystroke bursts; measure how debounce reduces reparse count
- **Schema intelligence group**: frontmatter completion/validation latency with 5/20/50/100 field schemas

### Summary

| Decision | Resolution | Key reasoning |
|----------|------------|---------------|
| Cold-start strategy | Parallelize with `rayon`, <2s at 20K | Markdown Oxide/rust-analyzer precedent; parsing is CPU-bound and independent |
| Query strategy | Inverted index now, <5ms gotodef | Linear scan of 20K entries is fast but not O(1); inverted index eliminates it entirely |
| Memory model | Evictable LRU with 3 tiers | Best balance of performance (metadata always resident) and system strain (ASTs evictable); schema definitions always resident for frontmatter intelligence |
| Benchmark plan | Extend index_refresh.rs + new lsp_latency.rs (reconciled 2026-09-23: `index_lifecycle.rs` decomposed) | Both refresh-path additions and interactive latency need dedicated benchmarks |
