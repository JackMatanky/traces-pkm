# LSP Persistence & Caching Patterns — Research

Resolves ticket [13-persistence-caching-strategy](../issues/13-persistence-caching-strategy.md).

Sources: `docs/digests/lsp_rvben-rumdl-digest.txt` (rumdl LSP), `docs/digests/lsp_artempyanykh-marksman-digest.txt` (Marksman), `docs/digests/lsp_feel-ix-343-markdown-oxide-src-digest.txt` (Markdown Oxide), `src/index/store.rs`, `src/index/delta.rs`, `src/index/sync.rs`, `src/index/service.rs`, existing research in `.scratch/md-pkm-lsp/research/`.

---

## Per-tool findings

### rumdl

**Persistence model:** Two distinct layers:

1. **CLI lint cache** (`src/cache.rs`): File-level JSON cache of lint results keyed by `(blake3(content_hash), blake3(config_hash), blake3(rules_hash), dependency_fingerprint)`. Stored at `.rumdl_cache/{version}/{hash}.json`. Caches `Vec<LintWarning>` per file. Version-pinned — entries are discarded when the rumdl version changes. Atomic writes via temp-file + rename. Configurable via `--cache-dir`, `RUMDL_CACHE_DIR`, or `cache-dir` in config. Disabled via `--no-cache`.

2. **Workspace index** (`workspace_index.rs`, persisted as `.rumdl_cache/workspace_index.bin`): Binary-serialized cross-file heading/link index. Loaded at startup via `WorkspaceIndex::load_from_cache`, saved after each CLI run via `workspace_index.save_to_cache`. Contains `FileIndex` per file (heading anchors, cross-file links). Used for MD051 (link fragment) and MD057 (relative link) rules.

**In-memory (LSP):** The LSP server (`src/lsp/`) keeps a `WorkspaceIndex` behind `Arc<RwLock<WorkspaceIndex>>` shared between the server and a background `IndexWorker`. No disk persistence from the LSP — the LSP relies entirely on in-memory state and re-scans on workspace open.

**Cache invalidation:** Per-file content hash (blake3) for CLI cache. For the workspace index: debounced `IndexUpdate::FileChanged` events (100ms debounce) trigger `update_single_file`. The worker diffs `extracted_data_differs` (heading anchors + links) against the previous `FileIndex` to decide whether dependent files need re-linting. `IndexUpdate::FileRemoved` immediately removes the entry. `FullRescan` rebuilds the entire in-memory index.

**Startup handling:** CLI: loads workspace index cache, skips files whose content hash matches, lints only stale files, then saves the updated cache. LSP: full workspace scan on initialization (no disk cache used by the LSP server itself).

**Schema versioning:** The CLI cache version-pins entries by `CARGO_PKG_VERSION`. The workspace index is a simple binary blob with no explicit version field — a version bump silently invalidates the cache (old `.bin` files are simply overwritten).

**File changed while editing:** For the LSP, each `did_change` sends the full new content to the `IndexWorker` via channel. The worker debounces, then does a full re-parse of the changed file's content into a new `FileIndex`, diffs against the previous entry, and triggers re-linting only if cross-file data (headings, links) actually changed. For the CLI, the content hash comparison handles this: if the file on disk matches the cached hash, it's skipped.

### Markdown Oxide

**Persistence model:** Entirely in-memory. No on-disk cache. `Vault` struct holds `HashMap<PathBuf, MDFile>` and `HashMap<PathBuf, Rope>`. No persistence to redb, JSON, or any other format.

**Cache invalidation:** `did_change` → `update_vault` mutates just that file's in-memory entry. `did_change_watched_files` → `reconstruct_vault`, which **drops the entire vault and rebuilds from disk** on all watched files. This is a known deficiency (documented in the existing research) — a filesystem event wipes all in-memory buffer state.

**Startup handling:** Full `WalkDir` at startup, parsed in parallel via `rayon`, results stored entirely in memory. `MAX_INDEXED_LINES = 10_000` per file cap. No cold-start cache; every editor restart pays the full scan cost.

**Schema versioning:** N/A — no persistent schema.

**File changed while editing:** `did_change` does a targeted in-memory update for the specific file. External filesystem changes (via `did_change_watched_files`) trigger a full vault reconstruction, which is the documented bug where unsaved buffer state can be lost.

### Marksman

**Persistence model:** Entirely in-memory. `SuffixTree<CanonDocPath, Doc>` for path-based lookups, `docsBySlug: Map<Slug, Set<Doc>>` for title-based lookups. No on-disk persistence.

**Cache invalidation:** Workspace-level: `withDoc`/`withoutDoc` incrementally add/remove entries from the suffix tree. Per-document: full re-parse on every edit (`Doc.withText` feeds the whole buffer to `Parser.parse`, then `Index.ofCst` rebuilds the document's catalog). No incremental parsing within a single document.

**Startup handling:** Eager scan respecting `.gitignore`/`.ignore`. All documents parsed in memory. No disk cache.

**Schema versioning:** N/A — no persistent schema.

**File changed while editing:** Full re-parse of the document on every `did_change`. The workspace-level index is updated incrementally (one entry at a time), but the per-document catalog is rebuilt from scratch each time. No attempt at incremental parsing within a document.

### rust-analyzer (referenced via community knowledge)

**Persistence model:** Two-tier: Salsa query database (in-memory incremental computation graph) + on-disk `target/debug/.fingerprint` and `target/debug/incremental/` directories for incremental compilation results. Also optionally persists `rust-analyzer.json` settings.

**Cache invalidation:** Salsa's red-green incremental computation: each query result is tracked with inputs; when an input changes, only dependent queries are recomputed. On-disk: file content hashes via `vfs::Vfs` (virtual filesystem). Each file change invalidates only the queries that transitively depend on that file.

**Startup handling:** Loads on-disk incremental compilation cache, then validates it against current file content hashes. Changed files trigger incremental re-computation, not full rebuild.

**Schema versioning:** The on-disk incremental cache has a version header; version mismatches trigger a full rebuild. The Salsa database itself is purely in-memory and rebuilt from scratch on each session.

**File changed while editing:** Salsa handles this natively — the changed file's VFS entry is updated, dependent queries are marked dirty, and recomputation happens lazily or eagerly depending on the query type.

---

## Cross-cutting patterns

### What persists vs in-memory

| Tool | Persists to disk | Keeps in-memory |
|------|-----------------|-----------------|
| **rumdl** (CLI) | Lint result cache (JSON), workspace index (binary blob) | N/A — CLI exits |
| **rumdl** (LSP) | Nothing from the LSP server | WorkspaceIndex, document store, config resolver |
| **Markdown Oxide** | Nothing | Entire Vault (MDFile + Rope per note) |
| **Marksman** | Nothing | SuffixTree + docsBySlug |
| **rust-analyzer** | Incremental compilation results, file fingerprints | Salsa query database, VFS |
| **Traces** (current) | redb: FILES, NOTES, LINKS, LISTS, PATHS_BY_TAG, PATHS_BY_FILE_CLASS, TAGS_BY_PATH, FILE_CLASSES_BY_PATH | FileIndex (files + notes + inlinks) |

**Key insight:** Of the markdown/PKM LSPs researched, only Traces and rumdl (CLI mode) persist anything. The LSP-mode servers (rumdl LSP, Markdown Oxide, Marksman) all keep their index entirely in-memory. rust-analyzer is the only tool that persists a rich incremental computation graph. The pattern is: **LSP servers typically don't persist their workspace index** — they re-scan on startup.

### Cache invalidation strategies

| Tool | Strategy | Granularity |
|------|----------|-------------|
| **rumdl** (CLI cache) | Content hash + config hash + rules hash + version | Per-file |
| **rumdl** (LSP index) | Debounced `did_change` → full re-parse of changed file → diff extracted data → re-lint dependents | Per-file with cross-file dependency tracking |
| **Markdown Oxide** | `did_change`: targeted file update; `did_change_watched_files`: full vault rebuild | Per-file for edits, full rebuild for FS events |
| **Marksman** | `didChange`: full re-parse of changed document; workspace-level `withDoc`/`withoutDoc` | Per-document (no incremental parsing) |
| **rust-analyzer** | Salsa incremental computation + VFS content hashes | Per-query (transitive dependency tracking) |
| **Traces** (current) | `IndexDelta::compute` on `FileBase` timestamps; content-only path → patch inlinks only; path-set change → full recompute | Per-file for scan; per-link-edge for inlinks |

**Key insight:** The common pattern is **per-file invalidation at the document level** (re-parse the whole file on each edit), with cross-file invalidation handled by diffing extracted structural data (headings, links, anchors). No tool does incremental parsing within a single markdown document. Traces' approach of comparing `FileBase` metadata (path + size + timestamps) to detect changes, then parsing only changed files, is consistent with what all other tools do.

### Startup handling

| Tool | Cold start | Warm start (cache exists) |
|------|-----------|--------------------------|
| **rumdl** (CLI) | Full scan + parse + persist | Load workspace index, skip files with matching content hash |
| **rumdl** (LSP) | Full workspace scan, in-memory index | Same — LSP doesn't use disk cache |
| **Markdown Oxide** | Full WalkDir + parallel parse | Same — no cache |
| **Marksman** | Full scan + parse | Same — no cache |
| **rust-analyzer** | Full Salsa rebuild | Load on-disk incremental cache, validate against VFS |
| **Traces** (current) | `build()` → full scan + parse + persist | `refresh()` → load persisted index, diff against filesystem, re-parse only changed files |

**Key insight:** Traces is already ahead of the other PKM LSPs in startup performance: its `refresh()` path loads the persisted redb index and only re-parses changed files. Markdown Oxide and Marksman pay full scan + parse cost on every editor restart. rust-analyzer's approach is the gold standard (load incremental cache + validate), which Traces' `refresh()` approximates.

### Schema versioning

| Tool | Versioning approach |
|------|-------------------|
| **rumdl** (CLI cache) | Version-pinned by `CARGO_PKG_VERSION`; entries discarded on version change |
| **rumdl** (workspace index) | No version field; silently overwritten on version change |
| **rust-analyzer** | On-disk cache has version header; mismatch triggers full rebuild |
| **Traces** (current) | redb schema mismatch → `check_rebuild_needed` detects `TableTypeMismatch`/`TypeDefinitionChanged`/`Corrupted` → wipe-and-recreate |

**Key insight:** Traces' approach (detect schema mismatch at open time, wipe and rebuild) is the most robust of the markdown tools. rumdl's approach of silently overwriting is simpler but less explicit. rust-analyzer's version header is more deliberate but requires manual migration logic. Traces should keep its current approach — it's already handling the schema evolution problem correctly.

### File changed while editing (buffer vs filesystem)

| Tool | Approach | Known issues |
|------|----------|-------------|
| **rumdl** (LSP) | Editor sends full content on `did_change`; worker re-indexes from the editor's content, not disk | None documented |
| **Markdown Oxide** | `did_change` updates in-memory; `did_change_watched_files` does full vault rebuild from disk, wiping buffer state | Documented: external FS events lose unsaved buffer index state |
| **Marksman** | `didChange` re-parses from editor buffer content; workspace mode scans disk | A rename is `didClose(old)` + `didOpen(new)` |
| **Traces** (current) | N/A — no LSP server yet; the CLI always reads from disk | N/A |

**Key insight:** The correct approach (used by rumdl and Marksman) is: **the editor's buffer is authoritative for open documents**. On `did_change`, the server should use the content sent by the editor, not re-read from disk. Only `did_change_watched_files` (external changes to files not currently open in the editor) should trigger disk reads. Markdown Oxide's bug of doing a full vault rebuild on any FS event is the anti-pattern to avoid.

---

## Implications for ticket 13

### What to persist

Based on the cross-tool analysis, Traces should persist:

1. **Current redb tables** (already exist): `FILES`, `NOTES`, `LINKS`, `LISTS`, `PATHS_BY_TAG`, `PATHS_BY_FILE_CLASS`, `TAGS_BY_PATH`, `FILE_CLASSES_BY_PATH`. These are the right data to persist — they represent the parsed metadata and derived indices that are expensive to recompute.

2. **No new persistence needed for the LSP workspace index**: All other markdown LSPs keep their workspace index in-memory. Traces should follow this pattern — the LSP server loads the redb index at startup and keeps it in-memory, with the redb file serving as the warm-start cache.

### Cache invalidation for the LSP

The LSP should follow rumdl's pattern:

1. **`did_change`**: Use the editor-sent content (not disk). Re-parse the document, update the in-memory index, diff structural data (links, tags, headings), and trigger re-computation of dependent data (backlinks, tag queries).

2. **`did_change_watched_files`**: For files not open in the editor, re-read from disk and update. For files open in the editor, skip (the editor buffer is authoritative).

3. **Debouncing**: 100ms debounce on `did_change` events (matching rumdl's approach) to avoid re-indexing on every keystroke.

### Startup flow

1. `IndexStore::open()` — open or create the redb database (already implemented).
2. `read_all()` or `read_files_and_links()` — load persisted state into memory (already implemented).
3. Scan the filesystem, compute `IndexDelta` against persisted state (already implemented).
4. Re-parse only changed files (already implemented in `service.rs`).
5. Persist the delta (already implemented).
6. The in-memory `FileIndex` is now the working copy for all LSP queries.

This is exactly what `IndexerService::refresh()` already does. The LSP just needs to call this at startup and keep the result in memory.

### Schema versioning

Keep the current approach: redb's `check_rebuild_needed` detects schema mismatches and triggers a wipe-and-recreate. No additional versioning logic is needed. The `is_rebuild_trigger` predicate already handles `TableTypeMismatch`, `TypeDefinitionChanged`, and `Corrupted`.

### File changed while editing (for future LSP implementation)

Design rule: **editor buffer is authoritative for open documents**.

- `did_open`: Add document to in-memory store, trigger initial diagnostics.
- `did_change`: Update in-memory store with editor content, re-parse, re-index, re-lint.
- `did_close`: Remove from in-memory store (or mark as "read from disk on next access").
- `did_change_watched_files`: For non-open files, re-read from disk and update. For open files, ignore (editor is authoritative).

This avoids the Markdown Oxide bug where external filesystem events wipe unsaved buffer state.
