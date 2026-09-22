# Research: Links & references — LSP patterns and best practices

Resolves research for ticket [15-links-and-references-model](../issues/15-links-and-references-model.md).

Sources: All LSP digests (Marksman, Markdown Oxide, VS Code markdown-language-service, rumdl, zk), existing research files (tickets 01, 02, 03), three parallel research subagents (crate evaluation, LSP link-handling patterns, performance best practices).

## LSP link-handling comparison

| Feature | Marksman | Markdown Oxide | VS Code Markdown LS | rumdl | Traces (current) |
|---------|----------|----------------|---------------------|-------|------------------|
| Wikilink resolution | Configurable (title-slug vs file-stem) | Regex exact-match only | N/A (no wikilinks) | N/A (ignores wikilinks) | Stem-based (O(N) scan) |
| Heading resolution | Slugified, reactive ambiguity | Regex extraction | GitHub-style slugger | MD051 for standard links only | No heading nodes in AST |
| Block references | Not supported | Fully supported | Not supported | N/A | Not supported |
| Embed handling | Planned, not implemented | Parsed via regex | Document links | N/A | `embedded: bool` on Link |
| Image support | Planned, not implemented | No special handling | Hover preview (VS Code only) | N/A | Not implemented |
| Ambiguity policy | AmbiguousLink error + RelatedInformation | Client-side disambiguation | Configurable severity | N/A | Unresolved (return None) |
| Unresolved policy | BrokenLink error | Information + "Create file" code action | Configurable severity | N/A | No diagnostic |
| Reference-style links | Full parity (IL/RF/RC/RS) | Regex extraction | Full parity | N/A | pulldown-cmark parses all |

## Wikilink resolution algorithms

### Marksman
1. `SuffixTree<CanonDocPath, Doc>` for path-based matching
2. `docsBySlug: Map<Slug, Set<Doc>>` for title-based matching
3. Configurable via `core.title_from_heading` + `completion.wiki.style`
4. Completion candidates filtered by `FileLink.filterFuzzyMatchingDocs`
5. Heading candidates use `Slug.isSubSequence` for fuzzy matching

### Markdown Oxide
1. Regex extraction of link targets from raw text
2. Exact string match against vault file paths
3. No fuzzy matching, no slugification
4. Ambiguity → return all matches, client disambiguates

### VS Code Markdown LS
1. Document links resolve file paths relative to the document
2. Heading anchors validated via GitHub-style slugger
3. No wikilink support

## Ambiguity and unresolved-target patterns

### Marksman
- `BrokenLink` diagnostic: 0 matches → Error for wikilinks, Warning for standard links
- `AmbiguousLink` diagnostic: >1 matches → Error for wikilinks, Warning for standard links
- `RelatedInformation` shows all candidate locations
- Severity varies by link type (wikilinks strictest, YAML least)

### Markdown Oxide
- Unresolved targets → `INFORMATION` severity (gated by `unresolved_diagnostics` setting)
- Ambiguity → no diagnostic; `goto_definition` returns `Vec<Location>`
- Code actions: "Create file" via `CreateFile` workspace edit, "Append heading" for missing headings
- Ambiguity deferred to client entirely

### Recommended hybrid
- Ambiguity → no diagnostic (matches Obsidian, Markdown Oxide)
- Zero matches → Information (default) or Warning (strict mode, configurable)
- Code action: "Create note" via `CreateFile`
- Optional severity escalation per link type

## Heading anchor handling

### rumdl (MD051)
- Validates `#anchor` in standard Markdown links only
- Permanently ignores wikilinks
- Supports multiple slug styles (GitHub default, Kramdown, etc.)
- Handles duplicate headings with auto-suffixing (`#faq`, `#faq-1`, `#faq-2`)

### Marksman
- Slug via character-class logic (similar to GitHub but not identical)
- Duplicate headings get `-{n}` suffix during parsing
- Reactive ambiguity: diagnostic only when something references a duplicate

### GitHub-style slugger (recommended for Traces)
1. Lowercase all text
2. Remove punctuation and symbols
3. Replace spaces with hyphens
4. Deduplicate: first → `#heading`, second → `#heading-1`, third → `#heading-2`

## rumdl coexistence

- rumdl permanently ignores wikilinks by design
- MD057/MD051 validate standard Markdown links only
- Link diagnostics keep running even when `enableLinkCompletions`/`enableLinkNavigation` are `false`
- Three-setting disable: `enableLinkCompletions=false`, `enableLinkNavigation=false`, `enableSymbols=false`
- Traces owns wikilink/PKM diagnostics; rumdl owns standard link validation

## Crate recommendations

| Category | Recommended | Skip | Rationale |
|----------|-------------|------|-----------|
| Path resolution | `camino` + `path-slash` | `url`, `relative-path` | UTF-8 paths, zero deps |
| Fuzzy matching | `fuzzy-matcher` | `nucleo`, `subsequence` | Right-sized for LSP, zero deps, highlight indices |
| Graph | `petgraph` (deferred) | — | HashMap suffices initially |
| Glob | `globset` (deferred) | `glob` | No current need for glob-pattern wikilinks |

## Performance patterns

### Inverted index design (from ticket 33)

```
wiki_link_index: HashMap<String, Vec<PathBuf>>   // file stem/title → paths
tag_index: HashMap<String, Vec<PathBuf>>          // tag → files
file_class_index: HashMap<String, Vec<PathBuf>>   // file class → files
outgoing_link_index: HashMap<PathBuf, Vec<Link>>  // file → its outgoing links
```

At 20K files, these indexes total ~7.3MB — well within the always-resident metadata budget (~2MB estimated in ticket 33).

### Link resolution performance

**Current Traces approach:**
- `InlinkMap` (HashMap<PathBuf, Box<[PathBuf]>>): O(1) point lookup for inlinks
- `LinkResolver::new`: O(N) stem index construction
- `LinkResolver::resolve`: O(1) per link (stem lookup) + O(L log N) for path matching
- `InlinkMap::without_sources` + `with_edges`: edge patching for incremental updates

**Marksman approach:**
- `SuffixTree<CanonDocPath, Doc>`: O(M) lookup where M = query length
- `docsBySlug: Map<Slug, Set<Doc>>`: O(log N) title lookup
- Full re-parse per edit, incremental only at document-index level

**Performance comparison:**
- Traces: O(1) for stem-based resolution (HashMap), O(log N) for path-based (binary search in sorted Vec)
- Marksman: O(M) for suffix tree, O(log N) for slug map
- Both achieve sub-millisecond resolution at 20K files

### Incremental update patterns

**What Traces already does correctly:**
- `InlinkMap::without_sources`: removes all edges from changed files
- `InlinkMap::with_edges`: inserts new edges from re-parsed files
- `IndexDelta::compute`: O(N) path-sorted merge-join for change detection
- `RefreshPlan::collect`: full filesystem scan + store read (for warm start)

**What ticket 13 added:**
- `RefreshPlan` single-file constructor: unified sync for editor `didChange` + future filesystem watcher
- Bypasses `RefreshPlan::collect` entirely for single-file updates
- 100ms debounce for `didChange` events

**What ticket 33 established:**
- Cold start <2s at 20K via rayon parallel scan
- Warm start <500ms
- Single-file reparse + index update <10ms
- Structural diff (links/headings/tags) <1ms
- Backlink recomputation <5ms

### Memory at 20K files

- Always-resident metadata: ~100 bytes/file × 20K = ~2MB (paths, tags, links, headings, file-classes, frontmatter field names, schema definitions)
- LRU AST cache: configurable max (e.g. 256 entries), evictable under memory pressure
- Inverted indexes: ~7.3MB total (wiki_link_index, tag_index, file_class_index, outgoing_link_index)
- Total idle: <100MB (ticket 33 budget)

### Benchmark plan for link/reference operations

**Extend existing (index_lifecycle.rs):**
- `WorkspaceIndex::refresh/single-file-didchange`: reparse one file + update index + recompute backlinks
- `WorkspaceIndex::refresh/parallel-cold-start`: rayon-parallelized vs single-threaded

**New file (lsp_latency.rs):**
- Interactive latency: simulated gotodef/completion/find-refs at 1K/5K/10K/20K files
- Memory footprint: measure WorkspaceIndex memory at each workspace size
- Debounce effectiveness: simulate 100ms keystroke bursts
- Schema intelligence: frontmatter completion/validation latency
