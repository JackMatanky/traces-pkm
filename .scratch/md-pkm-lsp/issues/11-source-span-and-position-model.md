# Source span/position model for the Note AST

Type: grilling
Blocked by: 39
Status: resolved

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

## Answer

### 1. Spans on the AST — extend existing types, don't build a parallel index

Add `Range<ByteOffset>` fields to `Link`, `Tag`, inline-field entries, and a new `Heading` AST node. Capture `range.end` from `pulldown-cmark`'s `into_offset_iter()` (currently discarded at `src/note/parser.rs:106`) and from `logos` token spans in `InlineTokenLexer`. This is additive — existing consumers that don't care about spans keep compiling unchanged.

A parallel LSP-only span index was rejected: it duplicates the position-computation work the parser is already doing in the same pass, and violates the standing constraint "reuse existing services rather than building parallel LSP-only models."

### 2. Heading node — flat list with `level: u8`

Every studied peer (Marksman, Markdown Oxide, Microsoft's generic Markdown service) stores headings as flat lists with a level field, deriving hierarchy on demand. No peer builds a parent-child tree at parse time. A flat list is the smallest API surface, and tree structure can be built from it in O(n) by a stack-based algorithm whenever a consumer actually needs parent pointers (symbols view, heading-anchor links). No such consumer was identified that would materially benefit from a parse-time tree.

### 3. ByteOffset width — `u32` with `From`/`TryFrom`

`ByteOffset` narrows from `usize` to `u32` (4 GiB/file cap). `From<u32>` for infallible construction; `TryFrom<usize>` for narrowing from `usize` with `ByteOffsetError` on overflow (caller uses `.unwrap_or(ByteOffset::MAX)` at pulldown-cmark event boundaries). Consistent with `SourceLine(NonZeroU32)` — same width philosophy, same safety net.

### 4. ByteTracker — move to `src/position.rs`, widen to `pub(crate)`

`ByteTracker` relocates from `src/note/parser/line.rs` to `src/position.rs`. It has zero `note::parser` dependencies (imports only `ByteOffset` and `SourceLine`, both from `position.rs`), and its logic (count newlines, binary search line starts) is domain-general. Widened from `pub(super)` to `pub(crate)`. A `byte_to_utf16_cu(&self, offset: ByteOffset) -> u32` method is added — walks characters from the line start, sums `char::len_utf16()`.

The module doc in `position.rs` updates from "only the vocabulary lives here" to "vocabulary and shared conversion infrastructure."

### 5. Text-buffer conversion — two concrete functions, no trait

The byte→LSP-position conversion happens at the LSP response serialization boundary, not in the core AST. Two concrete call patterns:

- **File buffers** (closed notes, loaded from disk): `ByteTracker::line_at(offset)` + `tracker.byte_to_utf16_cu(offset)` — both now in `src/position.rs`
- **Live buffers** (open editor buffers, backed by `ropey::Rope` per ticket 14): `rope.line_to_utf16_cu(rope.byte_to_line(offset))` — ropey's native API

A `TextBuffer` trait was rejected: zero polymorphic call sites (each handler knows its text type from context), two implementations with no third on the horizon (violates the "Rule of Three" from `anti-over-abstraction`), and the trait would be shallower than the two concrete functions it wraps. The "single interface" preference doesn't apply here because the text type is determined by handler context, not by runtime dispatch.

### 6. Frontmatter spans — via `noyalib` `Spanned<T>` (reconciled 2026-09-23)

Frontmatter byte ranges come from `noyalib`'s `Spanned<T>` single-pass value + position extraction, following the YAML parser swap decided in [Metadata/frontmatter & inline-field intelligence](19-frontmatter-and-inline-field-intelligence.md) — full YAML structure (nesting, block scalars, anchors, aliases), ranges relative to the frontmatter block. The originally-decided hand-rolled Level-2 raw-text scanner is dropped as redundant: its sole motivation (`yaml_serde` has zero span capability) disappears once that swap lands. Frontmatter spans therefore land with ticket 19, not this ticket — no consumer (19, 20) needs them earlier.

### 7. Parser — stay on pulldown-cmark, full re-parse per edit

Zero peer Markdown/PKM LSP (Markdown Oxide, Marksman, Microsoft's service) uses tree-sitter-markdown. Its ecosystem positioning is "editor highlighting/folding," explicitly not recommended where parsing correctness matters. `pulldown-cmark` is confirmed as "the industry standard in Rust" for CommonMark. Full re-parse per edit is the accepted pattern across every studied peer at note-collection scale. `ropey` is adopted for the live-buffer text representation (ticket 14), not for incremental parsing.

### 8. Persistence — spans stay transient, never in redb

Spans are cheap to recompute (re-parsing a file the LSP just read also re-derives every span in the same pass), invalidating on every edit (poor cache-hit shape), and persisting them triggers an avoidable index-format version bump. Only live-editing-session requests (hover, completion, rename) need spans, and those are against a file whose current text the LSP already has to touch.

### 9. Ropey — scoped to live buffers (ticket 14), not universal

`ropey::Rope` becomes the text representation for live open buffers. Closed/persisted notes stay as `&str` (today's model). The Note AST carries `Range<ByteOffset>` (protocol-agnostic); the LSP adapter converts using whichever text representation is in scope. Ropey-everywhere is not foreclosed architecturally but is not adopted now.
