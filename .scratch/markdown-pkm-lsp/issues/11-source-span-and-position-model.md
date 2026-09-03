# Source span/position model for the Note AST

Type: grilling

## Question

Grounded fact (from direct investigation of `src/note/`): **most semantic entities in the parsed `Note` AST carry no source position today.** Specifically:
- Links (`src/note/links.rs:26`, `Link { target, text, kind, embedded }`) — no line/byte position.
- Tags (`src/note/model.rs:29`, `Vec<Tag>`) — no location.
- Inline Fields (`src/note/field.rs:30`, `IndexMap<FieldKey, Vec<NoteFieldValue>>`) — no location.
- Frontmatter (`src/note/metadata.rs:19,50`) — parsed via `yaml-serde` into an `IndexMap`; raw text and spans discarded.
- Headings are not retained in the AST at all today (only tags/fields found inside them are collected).
- The one exception: List Items/Tasks track a 1-indexed `SourceLine` (not byte offset) via `ListItemPosition` (`src/note/lists.rs:285`).
- `src/position.rs` already defines the shared `ByteOffset`/`SourceLine` newtype vocabulary the rest of the codebase would extend.
- `src/note/parser/line.rs`'s `ByteTracker` already does O(log n) `ByteOffset`→`SourceLine` conversion via a precomputed line-start table, and `pulldown-cmark` (the underlying event-stream parser, `src/note/parser.rs:108`) natively emits byte-offset spans per event — so the raw data needed to attach spans is available at parse time even though it's currently discarded.

LSP hover/definition/references/rename/completion-context-detection *all* require precise ranges. Decide:

- Whether spans get added to the `Note` AST model itself (making `Link`, `Tag`, inline-field entries, and reconstructed heading nodes span-aware structurally), versus a parallel LSP-only span index built during/alongside parsing without touching the shared `Note` model, versus re-deriving positions on demand by re-scanning text.
- Byte offset vs UTF-16 code-unit offset: the LSP wire protocol's default `Position` encoding is UTF-16 (confirm current behavior/negotiability via `docs/refs/lsp_spec.md`'s position-encoding section) while `src/position.rs::ByteOffset` is UTF-8 bytes — decide the conversion boundary and whether `positionEncoding` capability negotiation (LSP 3.17+) is used to request UTF-8 from clients that support it, avoiding conversion entirely for those clients.
- Whether this is a breaking change to the `Note`/redb-persisted schema (spans stored in the `NOTES` table means an index-format version bump and migration/rebuild-on-load) or spans are computed transiently at LSP-request time from raw source text plus existing line-only positions, never persisted.
- Whether frontmatter needs raw-text-relative span reconstruction (since it's currently fully opaque post-YAML-parse) for frontmatter-field hover/completion/diagnostics to work, and how (re-scan the frontmatter block text, map YAML value paths back to source lines).
- Whether the underlying `Note` parser itself stays a full single-pass re-parse on every edit (today's model, and — per the Markdown Oxide/Marksman/zk research — a genuinely accepted pattern even in mature PKM LSPs at note-collection scale, not just a legacy shortcut) or becomes incrementally re-parseable (e.g. a rope-based text representation like `ropey` plus incremental re-lex/re-parse of only the changed region, or a tree-sitter-style incremental grammar). This question was previously folded silently into ticket 14 as "full re-parse, debounced or not" without ever considering true parser-level incrementality as an option — decide it explicitly here, on performance merits (ticket 33) and implementation cost, not by default inheritance from today's CLI-batch parsing model.

This has no research blocker — it's answerable from the existing codebase alone — but blocks 15 (links/references), 19 (frontmatter/inline-field intel), 26 (definition/references/hover/rename), and 13 (LSP persistence/caching).
