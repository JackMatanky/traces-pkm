# Live editor buffer vs filesystem overlay model

Type: grilling
Blocked by: 10
Status: resolved

## Question

Standing constraints: "Filesystem Markdown is the authoritative persistent source of truth" AND "Unsaved editor buffers must be reflected in live language intelligence without requiring writes to disk." These two constraints are in tension and need a precise reconciliation model, informed by ticket 10's host design and the rust-analyzer VFS precedent (ticket 06).

Decide:

- The overlay data structure: an in-memory map from URI to unsaved buffer content (+ version) that LSP requests consult *instead of* the persisted `FileIndex`'s view of that file, falling back to persisted/on-disk content for every other file.
- Whether an open-with-unsaved-changes file gets fully re-parsed into a transient `Note` (via the existing `src/note/parser.rs` entry point directly, bypassing `IndexerService`/redb entirely) on every `didChange`, or debounced.
- How cross-file features (backlinks, unresolved-reference diagnostics, rename cascades) reconcile a stale-on-disk view of file A with a live-buffer view of file B that references A — e.g. does the derived Inlink graph (`src/index/inlinks.rs`, currently eagerly computed `HashMap<PathBuf, Vec<PathBuf>>`) get a transient per-request overlay merge, or does every affected derived structure get recomputed against a synthetic combined view.
- What happens on `didClose` without save (discard overlay, revert to persisted view) vs `didSave` (write already happened by the client; persisted index refresh picks it up through the normal `IndexerService::refresh` path).
- Whether buffers for files *not yet indexed* (a brand-new untitled/unsaved note, or a note outside any currently-scanned root) are supported at all, and if so how they participate in link resolution before they exist on disk.

## Answer

**Resolution model: transient replacement, not layered overlay.** All three studied markdown PKM LSPs (Marksman, rumdl, markdown-oxide) use the same pattern: editor content replaces disk content while the file is open. There is no layered overlay with fallback. For Traces, the "overlay" is a transient replacement map — the persisted `FileIndex` is the baseline; open files temporarily override entries in it. [Full research](../research/14-buffer-overlay-model.md).

**1. Buffer storage: `RwLock<HashMap<Url, BufferState>>`.** `BufferState { content: String, version: i32 }`. No `parsed_note` field (computed on-demand — pulldown-cmark is fast at ~0.02ms/10KB, eliminates staleness vector). No DashMap (unnecessary with `concurrency_level(1)` sequential dispatch). No LRU eviction (typical PKM vaults have <100 open files, ~1MB total). Version guards reject stale `didChange` updates; `didClose` unconditionally removes regardless of version.

**2. Re-parse: debounced full re-parse.** 150ms debounce for content updates, 300ms for diagnostics publication. Full re-parse via `parse_markdown` on each debounced tick — no incremental parsing (pulldown-cmark's pull-parser design makes full re-parse cheaper than maintaining an incremental parse tree). Debounce implemented as `tokio::time::sleep` within the handler flow, preserving `concurrency_level(1)` sequential dispatch guarantees.

**3. Content resolution: `ContentResolver` facade.** Holds `&'a FileIndex`. Methods: `fn note_of(&self, uri: &Url) -> Cow<'a, Note>` (overlay `BufferState` content parsed on-demand, or `FileIndex`'s persisted `Note`), `fn text_of(&self, uri: &Url) -> Cow<'a, str>` (overlay content or disk content). Lifetime chain explicit: Cow borrows from `FileIndex` interior through the resolver's borrow. Analysis-host delegates to ContentResolver for content access; no duplication.

**4. Inlink reconciliation: per-request adjustment with cached LinkResolver.** Uses existing `patch_links` pattern (`src/index/sync.rs:216-226`). When a handler needs inlinks for target T: (a) start with `FileEntry.inlinks()` from disk baseline, (b) for each overlay file O, resolve O's current outlinks via `LinkResolver`, (c) add/remove O as source. `LinkResolver` cached between requests, rebuilt only on `didOpen`/`didClose`. Cost: `O(files)` for resolver build (amortized) + `O(overlay_count × outlinks_per_overlay)` per request. **Requires two API additions**: `FileIndex::files() -> &[FileBase]` and a method to reconstruct mutable inlink data from `FileEntry.inlinks()`. Overlay layer maintains its own `LinkResolver` over `(FileIndex files ∪ DocumentStore overlay files)` for cross-overlay link resolution (two unsaved files referencing each other).

**5. Lifecycle.** `didClose` → remove `DocumentStore` entry, publish cleared diagnostics (synchronous, no debounce). `didSave` → call `IndexerService::refresh` directly (not via `didChangeWatchedFiles` — that's a client-to-server notification the server cannot inject). `didChangeWatchedFiles` reserved for client-detected external file changes. `FileIndex` is never mutated; overlay awareness is additive to the facade and query layer.

**6. Unsaved/new files: `DocumentStore`-only entries.** Supported via `FileBase::new_buffer(path, size)` constructor (public, not test-gated). Synthetic entries participate in outlink resolution and inlink computation but don't persist to redb. On `didClose` without save: discard. On `didSave`: file hits disk, `IndexerService::refresh` picks it up, becomes normal indexed file. Links between two overlay-only files resolve via the overlay layer's own `LinkResolver`.

**Key API additions required:**
- `FileIndex::files() -> &[FileBase]` (trivial — entries are sorted by path)
- `FileBase::new_buffer(path, size)` constructor (public, not test-gated)
- Mutable inlink reconstruction from `FileEntry.inlinks()` for per-request patching
