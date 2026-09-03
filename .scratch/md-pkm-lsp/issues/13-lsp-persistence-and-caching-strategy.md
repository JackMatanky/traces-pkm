# LSP-specific persistence & caching strategy

Type: grilling
Blocked by: 10, 11

## Question

The existing `redb`-backed index (`src/index/store.rs`) persists three tables — `FILES`, `NOTES` (parsed AST/frontmatter/links, not raw text), `LINKS` (target→source multimap) — with no source spans stored (per ticket 11's findings) and no filesystem-watch-driven invalidation (refresh is always a manual full-tree diff against mtime/size).

Decide, once tickets 10 and 11 have settled the host and span model:

- Whether LSP-derived data (source spans if persisted per ticket 11, a reference-resolution graph richer than the existing target-path-keyed `LINKS` multimap, e.g. one that also tracks unresolved/ambiguous targets for diagnostics) gets added as new `redb` tables alongside the existing three, or kept purely in-memory and rebuilt every process start.
- Cache invalidation granularity for LSP mode: does an in-editor keystroke-level `didChange` invalidate only that file's derived data (cheap), or does it fall back to the existing whole-file re-parse-on-change-detected behavior (`src/index/delta.rs`) which was designed for CLI-invocation-time full-tree diffing, not per-keystroke editor latency.
- Whether the existing `IndexDelta`/merge-join diffing algorithm (`src/index/delta.rs:97-112`, O(N) path-sorted merge) is reused as-is for LSP-triggered refreshes, or whether a different incremental path is needed for single-file-changed-while-editing (which doesn't need a full directory rescan at all — the changed path is already known from the `didChange` notification).
- Whether an on-disk format/schema version bump (if spans get persisted per ticket 11) requires an index migration path or a plain rebuild-on-mismatch policy (check whether `src/index/codec.rs` already has any versioning field to hook into).

Blocks: 28(file operations/cascading edits), 33(performance targets).
