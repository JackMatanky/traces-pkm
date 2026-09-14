# LSP-specific persistence & caching strategy

Type: grilling
Blocked by: 10, 11
Assigned: agent
Status: resolved

## Question

The existing `redb`-backed index (`src/index/store.rs:32-75`) persists eight tables — `FILES`, `NOTES` (parsed AST/frontmatter/links, not raw text), `LINKS` (target→source multimap), `LISTS`, `PATHS_BY_TAG`, `PATHS_BY_FILE_CLASS`, `TAGS_BY_PATH`, `FILE_CLASSES_BY_PATH`. Source spans stay transient, never in redb (per ticket 11's findings). Refresh is always a manual full-tree diff via `RefreshPlan::collect` in `src/index/sync.rs`, which launches `IndexerService::scan` in parallel with store open, then does an O(N) path-sorted merge-join delta via `IndexDelta::compute` — designed for CLI's "process start → scan → exit" model, not persistent editor sessions. redb's `open_table` creates-if-not-exists, so adding new tables is schema-compatible with no migration needed. The `ByteTracker` and position model from ticket 11 now live in `src/position.rs`; the note parser at `src/note/parser.rs` uses `into_offset_iter()` but only `range.start` survives.

Decide, now that tickets 10 and 11 have settled the host and span model:

- Whether LSP-derived data (a reference-resolution graph richer than the existing target-path-keyed `LINKS` multimap, e.g. one that also tracks unresolved/ambiguous targets for diagnostics) gets added as new `redb` tables alongside the existing eight, or kept purely in-memory and rebuilt every process start.
- Cache invalidation granularity for LSP mode: does an in-editor keystroke-level `didChange` invalidate only that file's derived data (cheap), or does it fall back to the existing full-tree scan + merge-join diffing (`src/index/delta.rs`, `src/index/sync.rs`) which was designed for CLI-invocation-time full-tree diffing, not per-keystroke editor latency.
- Whether the existing `IndexDelta`/merge-join diffing algorithm (`src/index/delta.rs:97-112`, O(N) path-sorted merge) is reused as-is for LSP-triggered refreshes, or whether a different incremental path is needed for single-file-changed-while-editing (which doesn't need a full directory rescan at all — the changed path is already known from the `didChange` notification).
- Whether an on-disk schema version bump is needed if new tables are added (noting that redb's `open_table` is create-if-not-exists, so new tables are schema-compatible without migration — the question is whether any existing table schema changes require a rebuild-on-mismatch policy).

Blocks: 28(file operations/cascading edits), 33(performance targets).

## Answer

### 1. No new redb tables

No markdown/PKM LSP persists its workspace index to disk — rumdl LSP, Markdown Oxide, and Marksman all keep their index in-memory and re-scan on startup. Traces' existing 8 redb tables already store everything the LSP needs: parsed ASTs (frontmatter/tags/links/lists), inlink multimap, tag/file-class lookup indices. Derived LSP data (reference-resolution graph, diagnostic state) is cheap to reconstruct from these at startup. redb persists as a warm-start cache, not as the query source. No schema versioning changes needed — redb's `open_table` creates-if-not-exists, and `check_rebuild_needed` already handles schema mismatches via wipe-and-recreate.

### 2. In-memory mutable index (editor buffer is authoritative)

The LSP maintains a `FileIndex`-like structure in memory as the working copy for all queries. On `didChange`, reparse from editor-sent content, update the in-memory index, diff structural data (links/headings/tags), recompute affected backlinks. redb persists periodically or on shutdown for warm-start next launch. This matches the universal pattern (rumdl's `IndexWorker`, Marksman's `Doc.withText`) and aligns with ticket 10's `Arc<FileIndex>` swap-on-refresh architecture. Design rule: editor buffer is authoritative for open documents — `did_change` uses editor-sent content, not disk reads; `did_change_watched_files` reads disk only for non-open files.

### 3. Expand `RefreshPlan` with single-file input mode (not a separate method)

Rather than a standalone `update_buffer` method that bypasses `RefreshPlan`, extend the existing sync infrastructure: add a `RefreshPlan` constructor that accepts a single changed path + editor content (bypassing filesystem scan), while keeping `reconcile`/`persist`/`into_index` shared. This ensures both `didChange` (editor buffer) and a future filesystem watcher (ticket 37's `traces watch` CLI mode) flow through the same reconciliation pipeline with different delta sources. The existing `RefreshPlan::collect` stays for CLI/startup/external-change full resync. The O(N) merge-join in `IndexDelta::compute` is reused for full resync; single-file mode computes a delta of one.

### 4. Debounce `didChange` at 50–100ms

Matching rumdl's 100ms debounce pattern. Markdown reparse is cheap per-file but not free; debouncing avoids redundant work during rapid typing without perceptible latency. The exact value is tunable. Cross-file recomputation (backlinks, tag queries) fires after the debounce settles, not per-keystroke.

### Summary

| Decision                    | Resolution                              | Key reasoning                                                              |
| --------------------------- | --------------------------------------- | -------------------------------------------------------------------------- |
| New redb tables             | None                                    | Existing 8 tables sufficient; ecosystem re-scans on startup; redb is warm-start cache |
| Index model                 | In-memory mutable index                 | Editor buffer authoritative; redb write-behind for next-launch warm-start  |
| didChange path              | Expand `RefreshPlan` with single-file constructor | Unified sync infrastructure for editor + future filesystem watcher         |
| Debouncing                  | 50–100ms                                | Matches rumdl; avoids redundant reparse during rapid typing                |
