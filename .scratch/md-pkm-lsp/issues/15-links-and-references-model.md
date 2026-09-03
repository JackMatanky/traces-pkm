# Links & references: wikilinks, aliases, heading/block refs, embeds, backlinks, images

Type: grilling
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
