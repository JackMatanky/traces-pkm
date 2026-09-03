# Live editor buffer vs filesystem overlay model

Type: grilling
Blocked by: 10

## Question

Standing constraints: "Filesystem Markdown is the authoritative persistent source of truth" AND "Unsaved editor buffers must be reflected in live language intelligence without requiring writes to disk." These two constraints are in tension and need a precise reconciliation model, informed by ticket 10's host design and the rust-analyzer VFS precedent (ticket 06).

Decide:

- The overlay data structure: an in-memory map from URI to unsaved buffer content (+ version) that LSP requests consult *instead of* the persisted `FileIndex`'s view of that file, falling back to persisted/on-disk content for every other file.
- Whether an open-with-unsaved-changes file gets fully re-parsed into a transient `Note` (via the existing `src/note/parser.rs` entry point directly, bypassing `IndexerService`/redb entirely) on every `didChange`, or debounced.
- How cross-file features (backlinks, unresolved-reference diagnostics, rename cascades) reconcile a stale-on-disk view of file A with a live-buffer view of file B that references A — e.g. does the derived Inlink graph (`src/index/inlinks.rs`, currently eagerly computed `HashMap<PathBuf, Vec<PathBuf>>`) get a transient per-request overlay merge, or does every affected derived structure get recomputed against a synthetic combined view.
- What happens on `didClose` without save (discard overlay, revert to persisted view) vs `didSave` (write already happened by the client; persisted index refresh picks it up through the normal `IndexerService::refresh` path).
- Whether buffers for files *not yet indexed* (a brand-new untitled/unsaved note, or a note outside any currently-scanned root) are supported at all, and if so how they participate in link resolution before they exist on disk.
