# Links & references: wikilinks, aliases, heading/block refs, embeds, backlinks, images

Type: grilling
Status: resolved

Blocked by: 01, 02, 11

## Question

Decide the full reference model, informed by Markdown Oxide (ticket 01) and Marksman (ticket 02) research and the span model (ticket 11).

Grounding facts already gathered:
- `Link` (`src/note/links.rs:26`): `target: String` (raw), `text: String` (alias/display), `kind: LinkType` (Markdown|Wikilink), `embedded: bool` (covers `![[embed]]` already at the model level). No resolved-path field — `target_parts()` lazily splits into `LinkTarget::{Path, PathWithAnchor, AnchorOnly}`.
- Inlinks are derived via `derive_inlinks` (`src/index/inlinks.rs`): O(N) stem index for Obsidian-style wikilink resolution-by-name, then O(L log N) exact-path matching, producing `HashMap<PathBuf, Vec<PathBuf>>`.
- Headings are currently *not* retained in the Note AST at all.

Decide:
- Aliases: frontmatter-declared note aliases (does Traces have this today? if not, is it in scope here or a Schema/Config concern) and how alias-based wikilink targets resolve.
- Heading references (`[[note#heading]]`, `[[#heading]]`) — requires headings to become first-class AST nodes (coordinate with ticket 11); duplicate-heading ambiguity resolution policy (Marksman diagnoses this explicitly — compare).
- Block references (`^block-id`) — does Traces support these at all today (check `src/note/` for any existing block-id concept); if net-new, define syntax recognition and resolution scope.
- Embeds (`![[target]]`) — rendering-time behavior is out of LSP scope (Traces isn't a renderer) but definition/hover/reference semantics for the embedded target are in scope.
- Images and non-Markdown linked resources (`.png`, `.pdf`, etc.) — do these get definition/hover (e.g. image preview on hover, per LSP spec's hover content-kind options) or just existence-validation diagnostics.
- Ambiguity and unresolved-target policy end to end: what diagnostic (if any) fires for an unresolved wikilink target, and how ambiguous matches (multiple files with the same stem) are surfaced (pick-first? diagnostic? all treated as candidates for references/completion?).
- Whether reference-style Markdown links (`[text][ref]` + `[ref]: url`) get the same treatment as inline links.

Blocks: 24(completion), 25(diagnostics), 26(definition/refs/hover/rename), 27(structural/symbols, for document-link ranges).

## Answer

### Scope

This ticket decides the **full reference model** for how Traces handles links, references, embeds, and images in Markdown/PKM files. It covers resolution algorithms, diagnostic policy, ambiguity handling, and the rumdl coexistence boundary for link intelligence.

Out of scope: block references (`^block-id`) — net-new functionality, not a refinement of the existing model. The `Link` struct has no block-id field, the parser has no block-id extraction, and no other LSP except Markdown Oxide supports them. They should be ticketed separately when the link model is stable.

### Research basis

Three parallel research subagents investigated:

1. **Crate evaluation** (rust-docs-mcp): `camino` for UTF-8 path resolution, `fuzzy-matcher` for completion scoring, `petgraph` for link graph (deferred — `HashMap` suffices initially), `globset` for glob-pattern targets. All lightweight, zero or minimal new transitive deps.

2. **LSP link-handling patterns**: Read all LSP digests (Marksman, Markdown Oxide, VS Code markdown-language-service, rumdl, zk) and existing research files (tickets 01, 02, 03). Extracted resolution algorithms, diagnostic models, ambiguity policies, and completion patterns from each.

3. **Performance best practices**: Read existing performance research (tickets 33, 38) and codebase analysis. Confirmed inverted index design from ticket 33 is sufficient; identified 6 LSP-specific benchmarks to add.

### Resolution model

#### Decision 1: Wikilink resolution — configurable, default stem-first

**Options considered**:
- (a) Stem-first, no configuration — match filename stem first, title as display fallback
- (b) Configurable (title-slug vs file-stem), matching Marksman — user chooses whether wikilinks bind to titles or filenames
- (c) Title-first with stem fallback — always prefer title match, no configuration

**Decision**: **(b) Configurable, default stem-first**.

**Rationale**: Stem-first is the Obsidian-compatible default that most PKM users expect. `[[my-note]]` resolves to `my-note.md` by default, requiring no configuration. But Traces already has the data for title-based resolution — `FrontmatterConfig` provides `title` and `aliases` fields extracted at parse time (from the query-index-redesign work), so title-based resolution is trivially available as an opt-in. Making it configurable follows Marksman's proven pattern (`core.title_from_heading` + `completion.wiki.style`) and lets users who organize by titles rather than filenames opt in.

**Resolution algorithm** (extending the existing `LinkResolver` in `src/index/inlinks.rs:311-335`):

1. **Exact path match** — `[[projects/foo]]` matches `projects/foo.md` exactly
2. **Add `.md` extension** — `[[foo]]` matches `foo.md` if no extension present
3. **Stem match** (default) — `[[foo]]` matches any `foo.md` in the vault; equal-distance ties stay unresolved
4. **Title match** (opt-in via config) — `[[My Note Title]]` matches the file whose frontmatter `title` equals "My Note Title"; case-insensitive
5. **Alias match** (always active) — `[[alias]]` matches any file whose frontmatter `aliases` contains "alias"

**Alias resolution** — frontmatter-declared aliases are already extracted by the query-index-redesign work (`src/index/store.rs`, `FrontmatterConfig::aliases_name()`). They are always active for wikilink resolution regardless of the title-vs-stem configuration. A note with `aliases: ["My Alias", "MA"]` will resolve `[[My Alias]]` and `[[MA]]` to that file. Aliases are display text, not lowercased.

**New crates needed**: `camino` (UTF-8 paths, zero runtime deps) for path operations; `fuzzy-matcher` (zero deps, provides highlight indices for completion) for wikilink completion scoring. `petgraph` deferred — `HashMap`-based forward/backlink maps suffice until graph algorithms (cycle detection, toposort) are needed.

#### Decision 2: Heading references — GitHub-style slugger, reactive ambiguity

**Options considered**:
- (a) GitHub-style slugger — slugify headings (lowercase, spaces→hyphens), deduplicate with `-1`, `-2` suffixes
- (b) Marksman-style reactive ambiguity — slugify, diagnostic only when something references a duplicate heading
- (c) Exact match, no slugification

**Decision**: **GitHub-style slugger + reactive ambiguity for duplicate headings**.

**Rationale**: rumdl validates `#anchor` in standard Markdown links only (MD051) — it permanently ignores wikilinks. This means Traces **must** validate `[[note#heading]]` fragments itself; rumdl can't do it. rumdl's default slug algorithm is GitHub-style, and it supports duplicate heading suffixes (`#faq`, `#faq-1`, `#faq-2`). Using GitHub-style slugger in Traces matches rumdl's default and is the most common convention across the ecosystem (GitHub, VS Code, Obsidian, most static site generators).

For duplicate headings, the reactive approach from Marksman is adopted: when `[[#heading]]` is written and multiple headings match, the `AmbiguousLink` diagnostic fires only on the referencing link, not on the headings themselves. Unreferenced duplicate headings produce no diagnostic. This matches Obsidian's behavior and avoids noise in vaults where intentional duplicates exist.

**Slug algorithm** (GitHub-style):
1. Lowercase all text
2. Remove punctuation and symbols (keep alphanumeric, spaces, hyphens)
3. Replace spaces with hyphens
4. Deduplicate: first match gets `#heading`, second gets `#heading-1`, third `#heading-2`

**Heading AST node**: ticket 11 confirmed flat `Heading { level: u8 }` nodes will be added. The slug is computed at extraction time and stored on the node. The `Link` struct's `LinkTarget::PathWithAnchor(path, anchor)` already splits at `#` — the anchor portion is slugified and matched against stored heading slugs.

#### Decision 3: Block references — out of scope

**Rationale**: Block references (`^block-id`) are net-new functionality. The `Link` struct has no block-id field. The parser has no block-id extraction. Only Markdown Oxide supports them among the researched LSPs. They should be ticketed separately once the core link model is stable and implemented. The `LinkTarget` enum can be extended with a `BlockRef` variant later without breaking changes.

#### Decision 4: Embeds — definition navigates, hover shows preview (where supported)

**Options considered**:
- (a) Definition navigates to target file — same as `[[target]]` for definition
- (b) Definition navigates, hover shows preview — for markdown embeds, hover shows a snippet; for image embeds, hover shows metadata
- (c) Treat embeds identically to links for all operations

**Decision**: **(b) Definition navigates, hover shows preview (where the editor supports it)**.

**Rationale**: Embeds (`![[target]]`) have a different semantic intent than links (`[[target]]`) — they imply "include this content here." The `Link` struct's `embedded: bool` field already captures this distinction. For definition, embeds navigate to the target file (same as links). For hover, the behavior diverges by content type:

- **Markdown embeds** (`![[other-note.md]]`): hover shows a snippet of the embedded note's content (first paragraph or first heading). This is high-value for confirming what will be included.
- **Image embeds** (`![[diagram.png]]`): hover shows file metadata (dimensions, size, format) without base64 encoding. Full image preview is deferred — see Decision 5.

The `embedded` flag on `Link` drives this distinction. No new data structures needed.

#### Decision 5: Image and non-Markdown resources — phased approach

**Options considered**:
- (a) Existence validation only — diagnostics for broken image paths
- (b) Definition navigates to file — go-to-definition opens the image
- (c) Full image intelligence — definition navigates, hover shows preview

**Decision**: **Phased — ship (a)+(b) first, defer (c)**.

**Rationale**: VS Code is the only editor that renders image previews in hover (via `MarkdownString.baseUri` + base64 encoding). Neovim — the primary PKM demographic — doesn't render images in hover without extra plugins. The implementation cost of (c) is front-loaded (base64 encoding, scaling, thumbnail generation) and the value is editor-specific.

**Phase 1** (this ticket): Go-to-definition navigates to the image file. Existence validation diagnostics for broken image paths. Hover shows file metadata (path, size, format) as text.

**Phase 2** (future): Hover with text metadata — `**image.png** — 480×320, 125 KB, PNG`. No base64, just `MarkupContent` with plain text.

**Phase 3** (future, optional): Base64 thumbnail preview in hover. Configurable, with size limits and caching. Defer to VS Code extension if needed.

This gives high-value features immediately while leaving the door open for rich previews when editor support matures.

#### Decision 6: Ambiguity and unresolved-target policy — hybrid approach

**Options considered**:
- (a) Marksman-style — Error for both broken and ambiguous links
- (b) Markdown Oxide-style — Information for unresolved, code actions
- (c) Diagnostic-free for ambiguity, error only for broken

**Decision**: **Hybrid (b+)** — Markdown Oxide's gentleness with Marksman's structure.

**Rationale**: The research found a clear winner across all criteria:

**Ambiguity** (multiple files match): **No diagnostic**. `goto_definition` returns all candidates as `Vec<Location>`. Completion shows a picker. This matches Obsidian, Markdown Oxide, and user expectations. Many PKM users intentionally have same-named files in different folders — reporting these as errors is incorrect; the link works, just non-deterministically.

**Zero matches**: Two sub-cases, distinguished by context:
- **Forward reference** (likely intentional): `Information` severity. The user may be writing `[[new-note]]` before creating the file. Information signals "FYI, this doesn't resolve yet" without noise.
- **Truly broken** (likely a typo): `Warning` severity. An explicit path link to a non-existent file is probably a mistake.

**Code action on zero matches**: **"Create note"** via `CreateFile` workspace edit. Configurable target directory. This is the killer feature Markdown Oxide has that Marksman lacks. For wikilinks, the code action creates a new note. For standard Markdown links, it creates a file at the explicit path. For heading references, it creates a note with the heading as initial content.

**Diagnostic severity table**:

| Scenario | Severity | Rationale |
|----------|----------|-----------|
| Multiple files match (ambiguous wikilink) | No diagnostic | Ambiguity is normal in PKM, not a bug |
| Zero matches, wikilink, file doesn't exist | Information | Forward reference — user may create it later |
| Zero matches, standard link to missing file | Warning | Explicit path, likely a typo |
| Zero matches, heading reference to missing heading | Information | The note may not exist yet |
| Embed to non-existent target | Warning | Embed implies "include this," broken is a problem |

**Configuration model** — per-link-type severity override:

```rust
enum UnresolvedLinkPolicy {
    Silent,      // no diagnostic (default for ambiguity)
    Information, // FYI dot (default for zero matches)
    Warning,     // yellow squiggle (strict mode)
    Error,       // red squiggle (Marksman-style)
}
```

Configurable per link type (wikilinks, markdown links, embeds), defaulting to the gentle end. Users who want Marksman-level strictness can escalate. This follows the VS Code Markdown LS `DiagnosticLevel` pattern.

**RelatedInformation** — borrowed from Marksman's pattern, used sparingly:
- For ambiguous links (if user configures diagnostics on ambiguity): show each candidate location with "Duplicate definition of X in Y"
- For broken links: no RelatedInformation needed (nothing to point to)
- For forward references: could suggest "Create file at `path/`" as a RelatedInformation message

#### Decision 7: Reference-style Markdown links — full parity

**Decision**: **(a) Full parity**. Reference-style links (`[text][ref]` + `[ref]: url`) get identical treatment to inline links.

**Rationale**: Both Marksman and VS Code markdown-language-service handle all four Markdown link variants identically: inline (`[text](url)`), reference full (`[text][ref]`), reference collapsed (`[ref][]`), reference shortcut (`[ref]`). pulldown-cmark already parses all four forms. The `Link` struct captures the resolved target and display text regardless of syntax form.

The link definition site (`[ref]: url "Title"`) is tracked as a definition location. Go-to-definition on `[ref]` goes to the definition site. Hover on `[text][ref]` shows the URL. Find-references on `[ref]` finds all uses. This is what Marksman does with its `MdLink` enum (`IL`, `RF`, `RC`, `RS` variants).

#### Decision 8: rumdl coexistence — Traces owns wikilinks, rumdl owns standard links

**Decision**: **Clear division by design, not coordination**.

**Rationale**: rumdl permanently ignores wikilinks for link checking — "they name a vault entry rather than a path... the tool that renders them resolves the name itself." This is by design, not a gap. rumdl's MD057 (relative link validation) and MD051 (anchor validation) validate standard Markdown links only, and these keep running even when `enableLinkCompletions`/`enableLinkNavigation` are set to `false` (they're lint rules, not LSP capabilities).

**Division of ownership**:

| Domain | Owner | Rationale |
|--------|-------|-----------|
| Wikilink diagnostics (broken, ambiguous) | Traces | rumdl structurally can't see wikilinks |
| Wikilink heading/block reference validation | Traces | rumdl's MD051 doesn't cover wikilink `#anchor` |
| Wikilink completion, definition, references | Traces | Traces' inverted index provides O(1) resolution |
| Standard Markdown link validation (MD057/MD051) | rumdl | Already implemented, well-tested |
| Heading-level lint rules (MD001/MD041) | rumdl | Already implemented |
| Document/workspace symbols (headings) | Traces | rumdl's `enableSymbols` is explicitly designed to be ceded to "another LSP" |

**If user doesn't use rumdl**: Traces can optionally provide standard-link diagnostics via a configuration flag (`diagnostics.standard_links = true`). Default is off — assume rumdl is present. This keeps the default simple while supporting the standalone use case.

**Recommended joint setup** (for documentation/config guide):
```toml
# .rumdl.toml
[rumdl.lsp]
enableLinkCompletions = false  # Traces handles wikilink completion
enableLinkNavigation = false   # Traces handles wikilink navigation
enableSymbols = false          # Traces handles document/workspace symbols
```

Link *diagnostics* (MD057/MD051) are separate — to silence rumdl's standard-link diagnostics, add to `.rumdl.toml`:
```toml
[rumdl.rules]
disableRules = ["MD057", "MD051"]  # only if Traces provides these instead
```

### Summary of decisions

| Decision | Resolution | Key reasoning |
|----------|------------|---------------|
| Wikilink resolution | Configurable, default stem-first | Stem is Obsidian-compatible default; title available via existing FrontmatterConfig |
| Heading references | GitHub-style slugger + reactive ambiguity | Matches rumdl's default; Traces must validate wikilink headings (rumdl can't) |
| Block references | Out of scope for ticket 15 | Net-new functionality; ticket separately when link model is stable |
| Embeds | Definition navigates, hover shows preview | `embedded: bool` on Link drives distinction; editor-dependent preview |
| Images | Phased: (a)+(b) first, defer (c) | Image preview only works in VS Code; Neovim is primary PKM demographic |
| Ambiguity policy | Hybrid: no diagnostic for ambiguity, Information for zero matches | Matches Obsidian/Obsidian; configurable severity escalation available |
| Reference-style links | Full parity with inline links | Both Marksman and VS Code handle all four forms identically |
| rumdl coexistence | Traces owns wikilinks, rumdl owns standard links | rumdl permanently ignores wikilinks by design; zero overlap |

### New crates to add

| Crate | Purpose | Dep weight | Already in Cargo.toml |
|-------|---------|------------|----------------------|
| `camino` | UTF-8 path operations for wikilink-to-path resolution | 0 runtime deps | No |
| `fuzzy-matcher` | Fuzzy completion scoring with highlight indices | 0 runtime deps | No |

`petgraph` deferred — `HashMap`-based link maps suffice until graph algorithms are needed. `globset` deferred — no current need for glob-pattern wikilinks.

### Downstream implications

- **Ticket 24 (completion)**: wikilink completion uses `fuzzy-matcher` for scoring; heading completion uses slug matching; "create note" code action from ticket 15's unresolved-link policy
- **Ticket 25 (diagnostics)**: diagnostic severity table from Decision 6; rumdl coexistence from Decision 8; no duplication of MD057/MD051
- **Ticket 26 (definition/references/hover/rename)**: resolution algorithm from Decision 1; heading resolution from Decision 2; embed semantics from Decision 4; image phased approach from Decision 5; reference-style link parity from Decision 7
- **Ticket 27 (structural/symbols)**: heading slugs from Decision 2 needed for document symbols and CodeLens reference counts
