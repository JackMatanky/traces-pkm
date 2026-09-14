# LSP-specific persistence & caching strategy

Type: grilling
Blocked by: 10, 11
Assigned: agent
Status: in_progress

## Question

The existing `redb`-backed index (`src/index/store.rs:32-75`) persists eight tables — `FILES`, `NOTES` (parsed AST/frontmatter/links, not raw text), `LINKS` (target→source multimap), `LISTS`, `PATHS_BY_TAG`, `PATHS_BY_FILE_CLASS`, `TAGS_BY_PATH`, `FILE_CLASSES_BY_PATH`. Source spans stay transient, never in redb (per ticket 11's findings). Refresh is always a manual full-tree diff via `RefreshPlan::collect` in `src/index/sync.rs`, which launches `IndexerService::scan` in parallel with store open, then does an O(N) path-sorted merge-join delta via `IndexDelta::compute` — designed for CLI's "process start → scan → exit" model, not persistent editor sessions. redb's `open_table` creates-if-not-exists, so adding new tables is schema-compatible with no migration needed. The `ByteTracker` and position model from ticket 11 now live in `src/position.rs`; the note parser at `src/note/parser.rs` uses `into_offset_iter()` but only `range.start` survives.

Decide, now that tickets 10 and 11 have settled the host and span model:

- Whether LSP-derived data (a reference-resolution graph richer than the existing target-path-keyed `LINKS` multimap, e.g. one that also tracks unresolved/ambiguous targets for diagnostics) gets added as new `redb` tables alongside the existing eight, or kept purely in-memory and rebuilt every process start.
- Cache invalidation granularity for LSP mode: does an in-editor keystroke-level `didChange` invalidate only that file's derived data (cheap), or does it fall back to the existing full-tree scan + merge-join diffing (`src/index/delta.rs`, `src/index/sync.rs`) which was designed for CLI-invocation-time full-tree diffing, not per-keystroke editor latency.
- Whether the existing `IndexDelta`/merge-join diffing algorithm (`src/index/delta.rs:97-112`, O(N) path-sorted merge) is reused as-is for LSP-triggered refreshes, or whether a different incremental path is needed for single-file-changed-while-editing (which doesn't need a full directory rescan at all — the changed path is already known from the `didChange` notification).
- Whether an on-disk schema version bump is needed if new tables are added (noting that redb's `open_table` is create-if-not-exists, so new tables are schema-compatible without migration — the question is whether any existing table schema changes require a rebuild-on-mismatch policy).

Blocks: 28(file operations/cascading edits), 33(performance targets).
