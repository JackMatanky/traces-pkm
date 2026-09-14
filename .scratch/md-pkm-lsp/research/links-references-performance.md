# Research: Links & references — performance patterns

Resolves research for ticket [15-links-and-references-model](../issues/15-links-and-references-model.md).

Sources: Existing performance research (tickets 33, 38), codebase analysis (src/index/inlinks.rs, src/query/service.rs), Marksman source digest.

## Inverted index design (from ticket 33)

```
wiki_link_index: HashMap<String, Vec<PathBuf>>   // file stem/title → paths
tag_index: HashMap<String, Vec<PathBuf>>          // tag → files
file_class_index: HashMap<String, Vec<PathBuf>>   // file class → files
outgoing_link_index: HashMap<PathBuf, Vec<Link>>  // file → its outgoing links
```

At 20K files, these indexes total ~7.3MB — well within the always-resident metadata budget (~2MB estimated in ticket 33).

## Link resolution performance

### Current Traces approach
- `InlinkMap` (HashMap<PathBuf, Box<[PathBuf]>>): O(1) point lookup for inlinks
- `LinkResolver::new`: O(N) stem index construction
- `LinkResolver::resolve`: O(1) per link (stem lookup) + O(L log N) for path matching
- `InlinkMap::without_sources` + `with_edges`: edge patching for incremental updates

### Marksman approach
- `SuffixTree<CanonDocPath, Doc>`: O(M) lookup where M = query length
- `docsBySlug: Map<Slug, Set<Doc>>`: O(log N) title lookup
- Full re-parse per edit, incremental only at document-index level

### Performance comparison
- Traces: O(1) for stem-based resolution (HashMap), O(log N) for path-based (binary search in sorted Vec)
- Marksman: O(M) for suffix tree, O(log N) for slug map
- Both achieve sub-millisecond resolution at 20K files

## Incremental update patterns

### What Traces already does correctly
- `InlinkMap::without_sources`: removes all edges from changed files
- `InlinkMap::with_edges`: inserts new edges from re-parsed files
- `IndexDelta::compute`: O(N) path-sorted merge-join for change detection
- `RefreshPlan::collect`: full filesystem scan + store read (for warm start)

### What ticket 13 added
- `RefreshPlan` single-file constructor: unified sync for editor `didChange` + future filesystem watcher
- Bypasses `RefreshPlan::collect` entirely for single-file updates
- 100ms debounce for `didChange` events

### What ticket 33 established
- Cold start <2s at 20K via rayon parallel scan
- Warm start <500ms
- Single-file reparse + index update <10ms
- Structural diff (links/headings/tags) <1ms
- Backlink recomputation <5ms

## Memory at 20K files

- Always-resident metadata: ~100 bytes/file × 20K = ~2MB (paths, tags, links, headings, file-classes, frontmatter field names, schema definitions)
- LRU AST cache: configurable max (e.g. 256 entries), evictable under memory pressure
- Inverted indexes: ~7.3MB total (wiki_link_index, tag_index, file_class_index, outgoing_link_index)
- Total idle: <100MB (ticket 33 budget)

## Benchmark plan for link/reference operations

### Extend existing (index_lifecycle.rs)
- `FileIndex::refresh/single-file-didchange`: reparse one file + update index + recompute backlinks
- `FileIndex::refresh/parallel-cold-start`: rayon-parallelized vs single-threaded

### New file (lsp_latency.rs)
- Interactive latency: simulated gotodef/completion/find-refs at 1K/5K/10K/20K files
- Memory footprint: measure FileIndex memory at each workspace size
- Debounce effectiveness: simulate 100ms keystroke bursts
- Schema intelligence: frontmatter completion/validation latency
