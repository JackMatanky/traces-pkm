# Live editor buffer vs filesystem overlay — research

Resolves ticket [14-live-buffer-vs-filesystem-overlay](../issues/14-live-buffer-vs-filesystem-overlay.md).

Sources: `docs/digests/lsp_rvben-rumdl-digest.txt`, `docs/digests/lsp_rvben-rumdl-src-digest.txt`, `docs/digests/lsp_rvben-rumdl-docs-digest.txt`, `docs/digests/lsp_artempyanykh-marksman-digest.txt`, `docs/digests/lsp_artempyanykh-marksman-docs-digest.txt`, `docs/digests/lsp_feel-ix-343-markdown-oxide-digest.txt`, `docs/digests/lsp_feel-ix-343-markdown-oxide-src-digest.txt`, `docs/digests/lsp_feel-ix-343-markdown-oxide-docs-digest.txt`, `docs/digests/lsp_microsoft-vscode-markdown-languageservice-digest.txt`, `docs/digests/zk-digest.txt`, `docs/digests/zk-src-digest.txt`, `rust-docs-mcp` (ropey, lsp-types, pulldown-cmark, dashmap), web research (Biome LSP, TypeScript Go LSP, Zenzic, mdsmith, Ibex, Neovim LSP), `src/index/inlinks.rs`, `src/index/sync.rs`, `src/index/entry.rs`, `src/note/parser.rs`, `src/note/links.rs`, `src/query/service.rs`, `src/query/results.rs`.

---

## Executive summary

All three studied markdown PKM LSPs (Marksman, rumdl, markdown-oxide) use the same pattern: **editor content replaces disk content while the file is open**. There is no layered overlay with fallback — the rust-analyzer VFS "upper/lower layer" pattern is not used by any markdown LSP. For Traces, this means the "overlay" is a **transient replacement map**, not a merge layer: the persisted `FileIndex` is the baseline; open files temporarily override entries in it.

The hardest cross-file problem is **inlinks**: the `InlinkMap` requires the full file list for stem-based wikilink resolution and cannot be computed from a single file alone. The recommended approach is per-request inlink adjustment using the existing `patch_links` pattern from `src/index/sync.rs:216-226`, keeping the disk-based inlink graph as baseline and computing overlay-adjusted inlinks on-demand.

---

## Stress-test corrections (applied after grilling round 1)

Six recommendations were stress-tested against the codebase, previous ticket decisions, and performant-LSP conventions. Seven corrections were found:

### C1: DashMap → RwLock<HashMap>

DashMap's sharded concurrent access is unnecessary overhead with `concurrency_level(1)` (sequential dispatch). `RwLock<HashMap<Url, BufferState>>` is simpler, has less memory overhead, and the LSP model guarantees sequential access. DashMap only pays off with `concurrency_level > 1`.

### C2: Don't store parsed_note in BufferState

Storing `parsed_note: Option<Note>` in `BufferState` creates a staleness vector: the parsed note may reference schemas/templates that have changed on disk but the index hasn't refreshed yet. Since pulldown-cmark is fast (~0.02ms per 10KB), compute the parsed note on-demand from `content` when needed.

### C3: FileIndex API gaps block per-request inlink adjustment

Two API gaps prevent per-request inlink adjustment:
- `FileIndex` does not expose `&[FileBase]` — `LinkResolver::new(files: &'a [FileBase])` needs the full sorted file list, but only `entries() -> &[FileEntry]` exists.
- `FileEntry.inlinks()` returns `&[PathBuf]` (read-only), not an `InlinkMap` that can be patched.

**Fix**: Add `FileIndex::files() -> &[FileBase]`. Redesign inlink patching to work with reconstructed per-entry inlink data.

### C4: Cost model for inlink adjustment is incomplete

The stated cost `O(overlay_count × outlinks_per_overlay)` omits the `LinkResolver` construction cost: `O(files)` for the stem index build. **Fix**: Cache the `LinkResolver` between requests; rebuild only when the overlay set changes (didOpen/didClose).

### C5: didSave must not trigger refresh via didChangeWatchedFiles

`workspace/didChangeWatchedFiles` is a client-to-server notification — the server cannot inject its own. On `didSave`, call `IndexerService::refresh` **directly**. Reserve `didChangeWatchedFiles` for external file changes detected by the client's filesystem watcher.

### C6: FileBase needs a synthetic constructor for unsaved files

`FileBase::from_metadata` requires `std::fs::Metadata` — impossible for non-existent files. `FileBase::new_test` is test-gated. **Fix**: Add `FileBase::new_buffer(path, size)` (public, not test-gated) that fills `modified_at` with `DateTimeValue::now()`, `created_at` with `None`, and derives `format` from extension.

### C7: Two unsaved buffers referencing each other can't resolve links

If file A (new, unsaved) links to file B (new, unsaved), neither is in the `FileIndex`, so `LinkResolver` can't resolve the link. **Fix**: The overlay layer must maintain its own `LinkResolver` over the union of `(FileIndex files ∪ DocumentStore overlay files)`.

---

**LSPs analyzed:**
- **rumdl** (Rust) — Markdown linter with LSP
- **Marksman** (F#) — PKM-focused Markdown LSP
- **markdown-oxide** (Rust) — PKM-focused Markdown LSP
- **VS Code Markdown Language Service** (TypeScript) — Reference implementation
- **zk** (Go) — Zettelkasten LSP
- **Biome** (Rust) — Web linter/formatter with LSP
- **TypeScript Go** (Go) — TypeScript compiler LSP server
- **Zenzic** (Python) — Markdown analysis LSP
- **mdsmith** (Go) — Markdown linter LSP
- **Ibex** (Ruby) — Ruby LSP with overlay support
- **Neovim LSP Client** (Lua) — Client-side buffer management

---

## 1. Debouncing Strategy

### Findings

| Source | Approach |
|--------|----------|
| **CIRCT (LLVM Verilog LSP)** | `DebounceOptions` struct with `debounceMinMs` (minimum quiet time) and `debounceMaxMs` (maximum burst cap). Uses thread pool for async debounce checks. |
| **ALE/LSP clients** | "Incremental synchronization with debouncing proved hugely beneficial to auto-completion performance" |
| **opencode-cli spec** | Recommends 150-300ms for file search, 250-500ms for LSP refresh triggers |
| **rust-analyzer** | Uses `VfsChange` batch via `apply_changes()` — no explicit debounce; relies on atomic swap of entire VFS state |

### Recommendation

**Use a two-tier debounce:**
- **Typing (didChange):** 150-200ms quiet period before triggering index update
- **Diagnostics:** 300-500ms to avoid re-computing diagnostics on every keystroke
- **Max burst cap:** 1-2 seconds to prevent infinite deferral during rapid typing

```
┌─────────────┐     ┌──────────────┐     ┌─────────────┐
│ didChange   │────>│ debounce     │────>│ update Arc  │
│ (per keystroke)    │ timer (150ms)│     │ swap overlay│
└─────────────┘     └──────────────┘     └─────────────┘
```

---

## 2. Parsing Strategy with pulldown-cmark

### Findings

| Metric | Value |
|--------|-------|
| **pulldown-cmark 100K iterations (1106KB file)** | 2.179s (0.022ms per parse) |
| **Comrak (Rust)** | 11.113s (5x slower) |
| **cmark (C)** | 4.653s (2x slower) |
| **MD4C (C)** | 1.174s (fastest, but not Rust) |

pulldown-cmark is a **pull parser** — it produces an iterator of events, not a full AST. This means:
- **Low memory:** No tree construction unless explicitly collected
- **Fast re-parse:** For a 10KB markdown file, parsing takes ~0.02ms
- **No incremental parsing support:** Must re-parse entire document on change

### Recommendation

**Full re-parse on each debounced change is viable.** At ~0.02ms per 10KB file, even 100 open documents can be re-parsed in ~2ms total. Don't invest in incremental parsing — pulldown-cmark's pull-parser design makes full re-parse cheap.

```rust
// On debounced didChange:
let overlay_content = overlay_store.get(uri);
let parser = pulldown_cmark::Parser::new(&overlay_content);
let events: Vec<_> = parser.collect(); // ~0.02ms for 10KB
let new_index = build_index(uri, &events);
file_index.swap(new_index); // Atomic swap
```

---

## 3. Overlay Merge Pattern

### Findings

| Source | Pattern |
|--------|---------|
| **LSP Spec** | "After didOpen, the truth about the document is no longer on the file system but kept in memory" |
| **rust-analyzer** | `OverlayFS` with writable upper layer + read-only lower layers |
| **marksman** | `Doc` struct holds `Text` (in-memory) + `DocId` (URI); re-parses on change |
| **natural-lsp (Go)** | `Store.Close` reverts to on-disk content — standard pattern |
| **vscode-md-ls** | `workspace.ts` + `workspaceCache.ts` — document info cached by URI |

### Recommendation

**Simple overlay map pattern:**

```rust
struct OverlayStore {
    /// Documents open in editor (in-memory buffer)
    overlays: HashMap<DocumentUri, String>,
}

impl OverlayStore {
    fn get_content(&self, uri: &DocumentUri, disk_content: &str) -> Cow<str> {
        match self.overlays.get(uri) {
            Some(overlay) => Cow::Borrowed(overlay.as_str()),
            None => Cow::Borrowed(disk_content),
        }
    }
}
```

**Key principle:** The overlay store is the **single source of truth** for open documents. Queries always go through `overlay_store.get(uri)` which returns either the editor buffer or disk content.

---

## 4. Memory Management

### Findings

| Source | Approach |
|--------|----------|
| **gopls** | Fixed-size LRU cache (`lru` package) for document content |
| **rumdl** | `LintContext` computed once per document, cached behind `Arc` |
| **marksman** | `Doc` struct holds parsed `Cst` + `Text`; `Folder` holds `Map<DocId, Doc>` |
| **vscode-md-ls** | `InMemoryDocument` — simple HashMap, no eviction (VSCode manages lifecycle) |

### Recommendation

**No explicit eviction needed for typical PKM vaults.** A 10KB markdown file uses ~10KB memory. Even 1,000 open documents use ~10MB — negligible.

If memory is a concern:
- Use LRU eviction with capacity = 2x expected open documents
- Evict on `didClose` immediately (release the overlay entry)
- Keep the immutable FileIndex (on-disk index) always available

```rust
struct OverlayStore {
    overlays: LruCache<DocumentUri, String>,
}

impl OverlayStore {
    fn on_did_close(&mut self, uri: &DocumentUri) {
        self.overlays.pop(uri); // Revert to disk on close
    }
}
```

---

## 5. didClose Behavior

### Findings

| Source | Behavior |
|--------|----------|
| **LSP Spec** | "The server reverts to the on-disk content of the document" |
| **natural-lsp** | `Store.Close` reverts to on-disk content |
| **rust-analyzer** | VFS overlay layer is removed; reads fall through to lower (disk) layer |
| **marksman** | `Doc` is removed from `Folder.docs` map on close |

### Recommendation

**Standard pattern — immediate revert:**

```rust
fn did_close(&mut self, uri: DocumentUri) {
    // 1. Remove overlay
    self.overlays.remove(&uri);
    
    // 2. Rebuild index from disk content
    let disk_content = self.read_from_disk(&uri);
    let new_index = self.build_index(&uri, &disk_content);
    
    // 3. Atomic swap
    self.file_index.store(Arc::new(new_index));
    
    // 4. Publish cleared diagnostics
    self.publish_diagnostics(uri, vec![]);
}
```

**Do NOT** defer didClose handling — the editor expects immediate cleanup.

---

## 6. Unsafed Files Strategy

### Findings

| Source | Approach |
|--------|----------|
| **LSP Spec** | didOpen establishes in-memory truth; didClose reverts |
| **rumdl** | `index_worker.rs` handles background indexing |
| **markdown-oxide** | Full re-parse on every change (simplest approach) |
| **marksman** | `Folder.watchChanges()` re-indexes on file system events |

### Recommendation

**Treat unsaved files as primary source while open:**

```
┌─────────────────────────────────────────────┐
│ Query for document content                  │
│                                             │
│  1. Check overlay store (open in editor?)   │
│     YES → use overlay content              │
│     NO  → read from FileIndex (disk)        │
│                                             │
│  2. Index is always built from whichever    │
│     source is authoritative                │
└─────────────────────────────────────────────┘
```

For cross-file references (links from unsaved doc to other docs):
- Parse links from the **overlay content** (what user is editing)
- Resolve targets from the **FileIndex** (what's on disk)
- This ensures link resolution works even for unsaved changes

---

## 7. Keying Strategy

### Findings

| Source | Key Type |
|--------|----------|
| **marksman** | `DocId(UriWith<...>)` — URI-based |
| **rumdl** | File path (String) |
| **vscode-md-ls** | URI string |
| **rust-analyzer** | `FileId` (interned path) |

### Recommendation

**Use `DocumentUri` (URL-formatted path) as the key.** This is the LSP standard and avoids path normalization issues.

```rust
type DocumentUri = String; // or url::Url

struct OverlayStore {
    overlays: HashMap<DocumentUri, String>,
}
```

**Normalization:** Always normalize URIs before lookup:
- `file:///path/to/note.md` → canonical form
- Handle case-insensitive filesystems (macOS/Windows)
- Handle trailing slashes

---

## 8. Index Invalidation Strategy

### Findings

| Source | Strategy |
|--------|----------|
| **marksman** | `Folder.applyChange()` — rebuilds affected doc's index, then re-resolves all cross-refs |
| **rust-analyzer** | `Vfs::apply_changes()` — batch apply, then `AnalysisHost::apply_change()` |
| **rumdl** | `relint.rs` — incremental re-lint after config/content change |
| **vscode-md-ls** | `workspaceCache.ts` — invalidates cache on document change, lazy re-computation |

### Recommendation

**Two-level invalidation:**

1. **Local (single document):** On didChange, rebuild only that document's index
2. **Global (cross-references):** After local rebuild, re-resolve links that reference or are referenced by the changed document

```rust
fn on_document_change(&mut self, uri: DocumentUri, new_content: String) {
    // 1. Update overlay
    self.overlays.insert(uri.clone(), new_content.clone());
    
    // 2. Local index rebuild (only this document)
    let new_doc_index = self.parse_and_index(&new_content);
    
    // 3. Cross-reference re-resolution (only affected links)
    let affected_links = self.find_links_involving(&uri);
    self.re_resolve_links(affected_links);
    
    // 4. Atomic swap
    self.file_index.swap(Arc::new(new_index));
}
```

**For inverted indexes** (e.g., "which documents link to X?"):
- Maintain a separate `HashMap<DocumentUri, HashSet<DocumentUri>>` for backlinks
- On document change, update only the entries for that document
- This avoids full re-scan of all documents

---

## Summary: Recommended Architecture (corrected)

```
┌─────────────────────────────────────────────────────────┐
│                    LSP Server                            │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  ┌──────────────────┐    ┌──────────────────────────┐  │
│  │ DocumentStore    │    │ FileIndex (immutable, Arc)│  │
│  │ RwLock<HashMap>  │    │ - entries (sorted)        │  │
│  │ <Url, Buffer>    │    │ - inlinks (per-entry)     │  │
│  │                  │    │ - files() accessor [NEW]  │  │
│  │ didOpen → insert │    │                           │  │
│  │ didChange →update│    │ Swapped atomically on     │  │
│  │ didClose →remove │    │ didChangeWatchedFiles     │  │
│  └────────┬─────────┘    └────────────┬─────────────┘  │
│           │                           │                 │
│           ▼                           ▼                 │
│  ┌──────────────────────────────────────────────────┐  │
│  │           ContentResolver (facade)               │  │
│  │  note_of(uri) → Cow<Note> (overlay or disk)      │  │
│  │  text_of(uri) → Cow<str> (overlay or disk)       │  │
│  │  inlinks_of(uri) → adjusted inlinks              │  │
│  └──────────────────────────────────────────────────┘  │
│           │                                             │
│           ▼                                             │
│  ┌──────────────────────────────────────────────────┐  │
│  │        Per-request Inlink Adjustment             │  │
│  │  1. Start with FileEntry.inlinks() (disk)        │  │
│  │  2. For each overlay: resolve outlinks           │  │
│  │  3. Add/remove overlay as source                 │  │
│  │  LinkResolver cached, rebuilt on didOpen/close   │  │
│  └──────────────────────────────────────────────────┘  │
│                                                         │
│  Debounce: 150ms (typing), 300ms (diagnostics)          │
│  Parse: Full re-parse via pulldown-cmark (~0.02ms/10KB) │
│  didSave: refresh directly, not via didChangeWatched    │
└─────────────────────────────────────────────────────────┘
```
│  Invalidation: Local doc rebuild + targeted cross-ref   │
└─────────────────────────────────────────────────────────┘
```

---

## 9. Real-World LSP Buffer Management Implementations

### 9.1 Biome LSP (biomejs/biome)

**Pattern:** Dual-layer document tracking with project association.

| Component | Role |
|-----------|------|
| `Session` (LSP layer) | `HashMap<Url, Document>` — tracks open documents with version + project_key |
| `WorkspaceServer` (service layer) | `open_file()`, `change_file()`, `close_file()` — manages parsed state |
| `FileContent::FromClient` | Marks content as editor-supplied (not from disk) |

**Key code patterns:**
- `didOpen`: Creates `Document::new(project_key, version, &content)`, calls `workspace.open_file()` with `FileContent::FromClient`, inserts into document map, schedules diagnostics
- `didChange`: Gets old text from `workspace.get_file_content()`, applies `apply_document_changes()`, calls `workspace.change_file()` with new content
- `didClose`: Calls `workspace.close_file()` — reverts to disk content

**Insight:** Biome separates the LSP document tracking from the workspace parsing layer. The `FileContent::FromClient` tag explicitly marks editor content vs disk content, allowing the workspace to handle them differently.

### 9.2 TypeScript Go LSP Server (microsoft/typescript-go)

**Pattern:** OverlayFS with snapshot-based virtual filesystem.

| Component | Role |
|-----------|------|
| `OverlayFS` | Virtual filesystem merging in-memory snapshots with disk |
| `Snapshot` | Immutable content version for a file at a point in time |
| `ParseCache` | Caches parsed ASTs to avoid redundant parsing |

**Key design:**
- `didOpen`/`didChange` → create/update snapshot in `OverlayFS`
- Compiler/language service reads through `OverlayFS` transparently
- `OverlayFS` checks snapshot map first, falls back to physical filesystem

**Insight:** The overlay is a transparent proxy — all file reads go through it. The compiler doesn't know whether content comes from disk or editor buffer.

### 9.3 Zenzic LSP (Python)

**Pattern:** VirtualBufferOverlay with reverse index for cross-document dependencies.

| Component | Role |
|-----------|------|
| `VirtualBufferOverlay` | `buffers: dict[Url, str]` + `incoming_links: dict[str, set[Path]]` |
| `IncrementalAnalysisEngine` | O(K) pipeline for incremental updates |

**Key operations:**
- `didOpen` → inject buffer into overlay, mark URI dirty
- `didChange` → update overlay, reindex outgoing links in O(link count) time
- `didClose` → evict buffer from overlay

**Insight:** The reverse index (`incoming_links`) enables O(1) lookup of "which documents link to this file" — critical for efficient cross-document invalidation.

### 9.4 mdsmith (Go LSP for Markdown)

**Pattern:** Session overlay with explicit buffer/disk separation.

| Function | Behavior |
|----------|----------|
| `syncBuffer(path, content)` | Push editor buffer into session overlay, drop stale caches |
| `dropPath(path)` | Remove overlay entry, next read falls through to disk |
| `openDocPaths()` | Returns set of paths currently held as open buffers |

**Key code patterns:**
- `didOpen`: Seeds overlay with `syncBuffer()`, triggers lint
- `didChange`: Updates overlay via `syncBuffer()`, triggers lint
- `didSave`: Calls `dropPath()` — on-disk file now matches buffer
- `didClose`: Calls `dropPath()` — reverts to disk content
- `didChangeWatchedFiles`: Skips files in `openPaths` — their content is the overlay

**Insight:** The `openDocPaths()` check prevents external file watcher events from overwriting editor buffers. Files currently open in the editor have their content managed exclusively by didOpen/didChange.

### 9.5 Ibex LSP (Ruby)

**Pattern:** Overlay map with secure include semantics.

| Component | Role |
|-----------|------|
| `LSP::DocumentStore` | Owns open versions, overlay source, parsed snapshots, include closures |
| `Frontend::SourceLoader` | Canonical path and source reads |
| Optional overlay map | Takes precedence for open files, may represent new files |

**Key design:**
- Versions increase monotonically within one open epoch; reopening after close starts a new epoch
- Close publishes empty diagnostics, replaces buffer with disk snapshot
- Shared fragments with multiple roots are conservatively ambiguous
- Rename edits use canonical nonoverlapping spans, retain exact URI spelling

**Insight:** Ibex treats unsaved files as first-class citizens that can participate in the same secure include semantics as disk files. The overlay can represent files that don't yet exist on disk.

### 9.6 Neovim LSP Client (Lua)

**Pattern:** Change tracking with debounce and incremental sync support.

| Sync Kind | Behavior |
|-----------|----------|
| `None` | Server reads files itself |
| `Full` | Client sends full buffer on every change |
| `Incremental` | Client sends only changed ranges (computed via diff) |

**Key implementation details:**
- Double-buffering of line tables reduces GC pressure
- Debounce timer per buffer, smallest interval wins
- Incremental changes computed immediately (can't be debounced)
- Full sync sends `vim.lsp._buf_get_full_text(bufnr)` on flush

**Insight:** Neovim's change tracking is client-side — the LSP client (not server) manages buffer state and decides when to send changes. This is the standard pattern for LSP clients.

---

## Source Digests Referenced

- `lsp_rvben-rumdl-digest.txt` — Rust markdown linter with LSP
- `lsp_artempyanykh-marksman-digest.txt` — F#/Markdig LSP for PKM
- `lsp_feel-ix-343-markdown-oxide-digest.txt` — Rust LSP similar to our use case
- `lsp_microsoft-vscode-markdown-languageservice-digest.txt` — TypeScript reference implementation
- Web research: Biome LSP, TypeScript Go LSP, Zenzic, mdsmith, Ibex, Neovim LSP

---

## Key Takeaways

1. **All markdown PKM LSPs use replacement, not overlay** — editor content replaces disk content while open
2. **RwLock<HashMap> suffices** — DashMap is unnecessary with sequential handler dispatch (C1)
3. **Compute parsed notes on-demand** — don't store them in BufferState (C2)
4. **didClose must revert immediately** — LSP spec requires it
5. **Debounce at 150ms within handler flow** — sequential dispatch prevents staleness
6. **pulldown-cmark full re-parse is viable** — ~0.02ms per 10KB, no incremental parsing needed
7. **No LRU needed** for typical PKM vaults — memory is negligible
8. **Per-request inlink adjustment needs API fixes** — FileIndex must expose &[FileBase] (C3)
9. **Cache LinkResolver between requests** — O(files) build cost dominates (C4)
10. **didSave triggers refresh directly** — not via didChangeWatchedFiles (C5)
11. **FileBase needs new_buffer constructor** — for unsaved/new files (C6)
12. **Overlay layer needs its own LinkResolver** — for cross-overlay link resolution (C7)
13. **Overlay can represent non-existent files** (Ibex pattern) — unsaved files participate in include resolution
