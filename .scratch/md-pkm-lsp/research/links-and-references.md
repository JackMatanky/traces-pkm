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
