# LSP Performance Targets — Research

Resolves performance-targets research task.

Sources: Existing research in `.scratch/md-pkm-lsp/research/`, codebase digests, web search results (GitHub issues, forum posts, official benchmarks, blog posts).

---

## Per-tool findings

### Markdown Oxide

**Known bottlenecks:**
- **Full vault rebuild on filesystem events** (`did_change_watched_files` → `reconstruct_vault` drops entire vault and rebuilds from disk). Documented bug where external FS events wipe unsaved buffer state.
- **Runaway CPU usage** reported in Zed (#23296) — process eats increasingly large amounts of CPU even after closing all editor windows.
- **No persistence** — every editor restart pays the full WalkDir + parallel parse cost. No cold-start cache.
- **Regex-only parsing** (no formal AST) — every feature re-runs regex extraction from raw text.
- **MAX_INDEXED_LINES = 10,000 per file** (added v0.25.12 as a performance band-aid for large files).

**Concrete numbers:**
- No published benchmarks or latency numbers found.
- 10k line cap suggests files beyond that cause noticeable slowdown.

**Memory:**
- In-memory: `Vault` struct = `HashMap<PathBuf, MDFile>` + `HashMap<PathBuf, Rope>` per note. No cap on total vault size.

**Patterns to adopt:** PKM-specific features (daily notes, block references, tag hierarchies).
**Patterns to avoid:** Full vault rebuild on FS events, no persistence, regex-only parsing, no cancellation tokens.

---

### Marksman

**Known bottlenecks:**
- **Full re-parse per edited document** — no incremental parsing within a single document's AST. Workspace-level index is incremental (add/remove one document), but per-document catalog is rebuilt from scratch on each `didChange`.
- **No persistence** — eager scan + full parse on every editor restart.
- **F# / .NET runtime** — GC pauses possible on very large vaults.

**Concrete numbers (from `Benchmarks/Program.fs`, Apple M1 Pro):**

| Operation | Vault Size | Mean Latency |
|-----------|-----------|-------------|
| gotoDefTime | 10 docs | 1.565 µs |
| gotoDefTime | 50 docs | 2.455 µs |
| gotoDefTime | 250 docs | 2.925 µs |
| findRefsTime | 10 docs | 9.587 µs |
| findRefsTime | 50 docs | 50.685 µs |
| findRefsTime | 250 docs | 339.520 µs |

- goto-def is essentially O(1) (suffix tree lookup).
- find-refs scales linearly with vault size (~1.4 µs/doc).
- At 1000 docs: ~1.4ms for find-refs — still sub-5ms, acceptable.

**Memory:** In-memory suffix tree + `docsBySlug` map. No published memory numbers.

**Patterns to adopt:** Suffix-tree index for O(1) goto-def, MailboxProcessor actor model for concurrency.
**Patterns to avoid:** Full re-parse per edit (no incremental parsing), no disk persistence.

---

### rumdl

**Known bottlenecks:**
- **LSP server re-scans entire workspace on initialization** — no disk cache used by the LSP (only the CLI has cache).
- **Second independent file index** when running alongside another LSP — different processes, no shared index opportunity.
- **Regex-heavy lint rules** — some rules (e.g., MD013 line length with block-building) can be quadratic on pathological input.

**Concrete numbers (from `rumdl.dev`, Rust Book repo, 478 files):**

| Linter | Mean Time | Relative |
|--------|-----------|----------|
| rumdl | 217 ms | 1.0× |
| markdownlint-cli2 | 2.2 s | 10.2× |
| markdownlint-cli | 2.7 s | 12.5× |

- Cold start: 217ms for 478 files.
- With caching: subsequent runs skip unchanged files (blake3 content hash).
- LSP: in-memory `WorkspaceIndex` behind `Arc<RwLock<>>`, 100ms debounce on file changes.

**Benchmarks directory** (`benches/`): 8 dedicated benchmark files covering rule performance, link parsing, range operations, fix performance, config lookup, and a full perf audit.

**Memory:** Not published, but the binary is stripped and uses jemalloc/mimalloc.

**Patterns to adopt:** blake3 content hashing for cache invalidation, 100ms debounce on index updates, debounced `IndexUpdate` events, cross-file dependency tracking via heading/link diffs.
**Patterns to avoid:** Running a second independent index alongside another LSP.

---

### rust-analyzer

**Known bottlenecks:**
- **Cold startup: 30s–10 minutes** depending on project size. First launch with no cache: "5-10 minutes using high CPU before it is at all usable" for large projects.
- **Memory: 154MB–605MB** for the RA repo itself (226 KLOC); **2.5GB–13GB** for large workspaces with 800+ dependencies.
- **5 seconds per 100 crates** loading time (waiting on rustc).
- **Blocking target directory** — cargo check during startup prevents user from running cargo commands.
- **Macro-heavy projects** cause extreme memory usage (AttrsQuery: 6063MB in one report).

**Concrete numbers:**
- Database loaded: 9.57s for a brand-new empty project on Windows (due to `Command::output` latency).
- Cache warm-up disabled: 154MB for RA repo. Enabled: 605MB.
- PGO optimization of editor builds: 15-20% performance improvement.
- Parallel VFS loading prototype: 876 crates in 1.5s (vs. 1+ minute serial).
- **Goal: sub-100ms autocomplete latency** on larger projects (stated in #17491).

**Memory budget (industry knowledge):**
- VS Code extension host: ~3072MB default (TSServer).
- rust-analyzer typical: 500MB–2GB for medium projects.
- RA self-reports: 154MB–605MB.

**Patterns to adopt:** Salsa incremental computation graph, VFS with content hashing, LRU caching, parallel loading, PGO-optimized builds.
**Patterns to avoid:** Serial VFS loading, repeated `Command::output` calls during startup.

---

## Industry-standard LSP latency targets

### From real benchmarks (solidity-language-server, p95 latency)

| Method | Latency |
|--------|---------|
| initialize | 22.4 ms |
| textDocument/definition | 8.3 ms |
| textDocument/hover | 7.3 ms |
| textDocument/references | 5.3 ms |
| textDocument/completion | 59.9 ms |
| textDocument/diagnostics | 2.8 ms |
| textDocument/rename | 15.1 ms |
| textDocument/documentSymbol | 7.7 ms |
| textDocument/formatting | 43.4 ms |
| workspace/symbol | 7.2 ms |

### gopls (Go LSP) — completion budget

- **Soft latency goal: 100ms** for completion requests. Dynamically reduces search scope as budget is consumed.

### General community consensus (from forum posts, blog posts, VS Code docs)

| Operation | Target | Stretch |
|-----------|--------|---------|
| **Hover** | < 50ms | < 20ms |
| **Completion** | < 100ms | < 50ms |
| **Go-to-definition** | < 50ms | < 10ms |
| **Find-references** | < 200ms | < 50ms |
| **Diagnostics (per-file)** | < 100ms | < 30ms |
| **Rename** | < 500ms | < 100ms |
| **Document symbols** | < 100ms | < 30ms |
| **Initialize (cold)** | < 5s | < 2s |
| **Initialize (warm)** | < 1s | < 500ms |

### VS Code constraints

- **Extension host memory limit**: ~3072MB (hardcoded in Electron utilityProcess, not configurable at runtime).
- **Large file threshold**: 50MB — above this, many editor features are disabled.
- **Language servers run in separate processes** — avoids blocking the editor, but means the LSP has its own memory budget.
- **TSServer memory**: defaults to 3072MB `max-old-space-size`, no dynamic pressure resolution.

---

## Summary: Recommended targets for Traces LSP

### Startup

| Scenario | Target | Rationale |
|----------|--------|-----------|
| Cold start (no cache) | < 3s for 1000 files | 5× faster than Markdown Oxide's full scan; Traces already has persisted redb index |
| Warm start (cache exists) | < 500ms | Load redb index + diff filesystem; rumdl CLI does 217ms for 478 files |
| Warm start (10,000 files) | < 2s | Scale linearly; redb index lookup is fast |

### Interactive latency (p95)

| Operation | Target | Stretch | Notes |
|-----------|--------|---------|-------|
| Hover | < 30ms | < 10ms | Pure in-memory lookup |
| Completion | < 50ms | < 20ms | Wiki-link + heading completion from in-memory index |
| Go-to-definition | < 20ms | < 5ms | In-memory hash map lookup (Marksman: 1.5–3µs) |
| Find-references | < 100ms | < 20ms | Linear scan of in-memory link index (Marksman: ~1.4µs/doc) |
| Diagnostics (per-file) | < 50ms | < 15ms | Re-parse + lint one file |
| Document symbols | < 30ms | < 10ms | Headings from in-memory index |
| Rename | < 200ms | < 50ms | Find refs + apply edits |

### Memory

| Scenario | Budget | Rationale |
|----------|--------|-----------|
| Idle (vault indexed) | < 100MB | Markdown vaults are smaller than code projects; keep it lean |
| Active editing (10 open buffers) | < 200MB | Buffer content + index + diagnostics |
| Hard cap | < 500MB | Leave room for VS Code + other extensions in 3GB host |
| Per-file cap | 10,000 lines | Match Markdown Oxide's cap; reject/lazy-load beyond this |

### Indexing throughput

| Metric | Target | Notes |
|--------|--------|-------|
| Parse + index per file | < 5ms | pulldown-cmark: 0.20ms for 48KB; extraction overhead on top |
| 1000 files initial index | < 3s | Parallel via rayon |
| Incremental re-index (single file) | < 10ms | Parse + diff structural data + update in-memory index |
| Debounce window | 100ms | Match rumdl; avoids re-indexing on every keystroke |

### Patterns to adopt from research

1. **rumdl**: blake3 content hashing for cache invalidation, 100ms debounce, cross-file dependency tracking via heading/link diffs, `Arc<RwLock<WorkspaceIndex>>` shared between server and background worker.
2. **Marksman**: Suffix-tree or hash-map index for O(1) goto-def, MailboxProcessor/actor pattern for sequential state mutation.
3. **rust-analyzer**: Salsa-style incremental computation (if complexity warrants it), parallel file loading, LRU caching, PGO-optimized builds.
4. **Traces (existing)**: redb persistence (already ahead of all other PKM LSPs), `IndexDelta::compute` for incremental updates, `refresh()` warm-start path.

### Patterns to avoid

1. **Markdown Oxide**: Full vault rebuild on `did_change_watched_files`, no persistence, regex-only parsing, no cancellation.
2. **Marksman**: Full re-parse per edit with no incremental parsing within a document.
3. **rust-analyzer**: Serial VFS loading, repeated `Command::output` during startup, unbounded memory growth.
4. **General**: Running a second independent file index alongside rumdl (coordinate via init_options instead).
