# Research: Footnotes & Callouts — combined findings

Four parallel research subagents investigated pulldown-cmark support, LSP mechanisms, best practices, and PKM handling.

---

## 1. pulldown-cmark support

**Version**: 0.13.4 (current in project: `"0.13"`)

### Footnotes — native, opt-in

```rust
opts.insert(Options::ENABLE_FOOTNOTES); // GitHub-compatible syntax
```

Events emitted:
- `Event::FootnoteReference(CowStr<'a>)` — inline `[^ref]` references
- `Event::Start(Tag::FootnoteDefinition(CowStr<'a>))` … `Event::End(TagEnd::FootnoteDefinition)` — block `[^label]: definition`

Definitions and references may appear in any order; undefined references still emit `FootnoteReference`. The parser does NOT build a cross-reference table — it emits them as independent events.

### Callouts — native, opt-in (partial)

```rust
opts.insert(Options::ENABLE_GFM);
```

When enabled, `Tag::BlockQuote` changes from `BlockQuote(None)` to `BlockQuote(Some(BlockQuoteKind))`:

```rust
pub enum BlockQuoteKind { Note, Tip, Important, Warning, Caution }
```

**Limitation**: Only 5 GFM alert types. No custom callout types (Obsidian supports 12+ built-in types plus user-defined). No title extraction. See pulldown-cmark issue #919.

### Current Traces parser

- `Event::FootnoteReference(_)` is matched at `src/note/parser.rs:225` — but **only to reject task-list markers**, not to extract footnote content.
- No `FootnoteDefinition` handling anywhere.
- Neither `ENABLE_FOOTNOTES` nor `ENABLE_GFM` is set in the current `Options`.

---

## 2. LSP feature comparison

### Footnotes

| LSP | Completion | Go-to-Definition | Find References | Rename | Diagnostics | Hover |
|---|---|---|---|---|---|---|
| **Markdown Oxide** | ✅ Complete `[^id]` from file footnotes | ✅ Reference → definition | ✅ All uses | ✅ Rename updates all refs | ❌ None | ❌ None |
| **Marksman** | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **rumdl** | ❌ | ❌ | ❌ | ❌ | ✅ MD066/MD067/MD068 (lint rules) | ❌ |
| **VS Code markdown-ls** | ❌ | ❌ | ❌ | ❌ | ❌ (footnotes excluded; known issue) | ❌ |
| **md-lsp** (matkrin) | ✅ Complete definitions | ✅ ref → def | ✅ def → all refs | ✅ Bidirectional | ✅ Code 5: ref without def | ✅ Preview |

### Callouts

| LSP | Completion | Diagnostics | Folding | Notes |
|---|---|---|---|---|
| **Markdown Oxide** | ✅ Full Obsidian callout type completion | ❌ | ❌ | Complete `> [!type]` completion |
| **Marksman** | ❌ | ❌ | ❌ | No callout awareness |
| **rumdl** | ❌ | ⚠️ Recognizes `[!NOTE]` as valid blockquote | ❌ | No semantic callout features |
| **VS Code markdown-ls** | ❌ | ⚠️ Suppresses false-positive diagnostics for `[!NOTE]` | ❌ | v0.5.0-alpha.8 |
| **Noted** (Zed) | ❌ | ❌ | ❌ | Semantic tokens + code actions for callouts |

### Key finding

Markdown Oxide is the only major Rust LSP with both footnote and callout intelligence. md-lsp (less widely adopted) has the most complete footnote support. No LSP provides callout diagnostics beyond suppressing false-positive warnings.

---

## 3. Performant LSP best practices

### Parser strategy

None of rust-analyzer, ruff, or Biome use regex post-processing for extended syntax. They extend the grammar itself. For Traces:
- **Footnotes**: Use `Options::ENABLE_FOOTNOTES` (grammar extension already built into pulldown-cmark).
- **Callouts**: Since callouts are a blockquote metadata pattern (not a grammar extension), the correct approach is event-stream processing — detect `[!type]` when handling `BlockQuote` start events.

### Footnote linking: inverted index

Build a `HashMap<String, Range<ByteOffset>>` for definitions and `HashMap<String, Vec<Range<ByteOffset>>>` for references during the single parse pass. O(n) build, O(1) per lookup. Linear scan is unnecessary.

```rust
struct FootnoteIndex {
    definitions: HashMap<String, Range<ByteOffset>>,
    references: HashMap<String, Vec<Range<ByteOffset>>>,
}
```

### Callout detection

Scan the first `Event::Text` after `Event::Start(CmarkTag::BlockQuote)` for `>\s*\[!([a-zA-Z]+)\]`. O(C) where C is blockquote count — negligible.

### Diagnostics pipeline

```
Parse (pulldown-cmark) → Note AST with spans (transient)
    → FootnoteIndex (HashMap, built during parse)
    → Diagnostic check (O(1) lookups)
    → LSP publishDiagnostics (ephemeral, not stored)
```

Diagnostics should be computed on demand and served ephemerally, not persisted (lesson from OpenCode issue #36300 research).

### Performance cost

- `ENABLE_FOOTNOTES`: single extra O(n) pass over source to collect definitions before emitting events — negligible.
- Callout detection: byte-scan of first line of each `BlockQuote` — negligible.
- Real cost is serving diagnostics to the client, not computing them.

---

## 4. PKM handling patterns

### Obsidian

**Footnotes**: Full support — `[^ref]` references, `[^ref]: definition` blocks. Backlinks panel shows footnote references. Clicking a footnote reference jumps to definition. Rename updates all refs. Dataview does NOT index footnotes (invisible to metadata system).

**Callouts**: 12 built-in types (note, abstract/info/todo, tip/hint/important, success/check/done, question/help/faq, warning/caution/attention, failure/fail/missing, danger/error, bug, example, quote/cite). Custom types defined via CSS. Folding supported. No diagnostics for unknown types — they just render as generic callouts.

### Zk

No special footnote or callout handling found in the digest.

### Dataview

No native footnote or callout querying. Footnotes are invisible to the metadata system.

### Templater/Metadata Menu/Obsidian Tasks

No footnote or callout-specific intelligence.

### User expectations (from PKM community)

- Footnote reference-to-definition linking is expected as baseline.
- Callout type completion is expected (at least the 12 built-in types).
- Unknown callout types should NOT produce errors — they fall back to generic rendering.
- Folding callout bodies is expected.
- Diagnostics for undefined footnote references are valuable.

---

## 5. Implications for ticket 17

### Footnotes

**Scope**: In scope. pulldown-cmark provides native support; Markdown Oxide and md-lsp prove the LSP feature set is valuable; the implementation is straightforward (enable flag + inverted index).

**Depth**:
- Definition/references linking (`[^ref]` ↔ `[^ref]: definition`): YES
- Diagnostics for undefined references: YES (straightforward with inverted index)
- Diagnostics for unused definitions: YES (optional, lower priority)
- Completion for existing footnote labels when typing `[^`: YES
- Hover showing definition content: YES (cheap, useful)
- Rename: YES — file-local find-replace of the label string (confirmed trivial by Panache, md-lsp, mdsmith; simpler than heading/file rename; no slug recomputation, no cross-file anchors).

### Callouts

**Scope**: In scope, but limited. pulldown-cmark's GFM support covers only 5 types; Obsidian's vocabulary is 12+ types plus custom. The parser should detect callouts via event-stream processing, not rely solely on `ENABLE_GFM`.

**Depth**:
- Completion for callout type keywords after `> [!`: YES (static vocabulary of 12 built-in types + configurable overrides)
- Diagnostics for unrecognized callout types: NO (by design — unknown types fall back to generic rendering, matching Obsidian behavior)
- Folding-range behavior for callout bodies: YES (feeds into ticket 27)
- Hover showing callout type description: NICE-TO-HAVE (low priority)

### Shared

- Both use spans from `into_offset_iter()` (already available, currently discarded).
- Neither requires new redb tables — transient, recomputed per parse.
- Both feed into ticket 27 (structural/editor intelligence) for folding ranges.

---

## 6. Footnote edge cases

Source: stress-test subagent investigating pulldown-cmark, GFM, CommonMark, zk, Templater, Dataview behavior across 20 edge cases.

### Parser-level gaps in pulldown-cmark

1. **No backreference tracking** — emits `FootnoteDefinition` and `FootnoteReference` events but does NOT reorder footnotes or generate backreference IDs. The LSP must build this from the event stream.
2. **Definition ordering** — emits definitions in source order, not reference order. Track both `definition_line` and `first_reference_line` to support "reorder footnotes" code actions.
3. **No validation** — does NOT validate that every reference has a definition or vice versa. LSP diagnostics must compare the two index sets.
4. **Nested footnotes** — definitions can contain references to other footnotes, creating inter-dependencies. The inverted index handles this naturally (each label independent).

### Edge case inventory

| # | Case | Verdict | LSP action |
|---|------|---------|------------|
| 1 | `[^]` empty label | Invalid in all implementations | No diagnostic — parser won't emit events |
| 2 | Self-referencing `[^1]` in its own definition | Valid, semantically meaningless | Optional info diagnostic |
| 3 | Definition inside blockquote | Valid in pulldown-cmark, invalid in Pandoc | Consider "move to top level" code action |
| 4 | Definition inside list item | Valid in pulldown-cmark, invalid in Pandoc | Same as #3 |
| 5 | Duplicate definitions `[^1]:` twice | First wins in all parsers | Warning diagnostic |
| 6 | Multi-paragraph definitions (4-space indent) | Valid in new syntax | Hover truncation (~200 chars) |
| 7 | Inline vs multi-line definitions | Both valid | No special handling |
| 8 | `[^1]` in YAML frontmatter | Not a footnote reference (raw text) | Index must skip frontmatter |
| 9 | `[^1]` inside code blocks | Correctly ignored by parser | No issue |
| 10 | Very long definitions | Parser emits full content | Truncate hover at 200 chars |
| 11 | Undefined references (no matching def) | Valid syntax, semantically broken | Error diagnostic |
| 12 | Orphaned definitions (never referenced) | Valid syntax, wasteful | Warning diagnostic |
| 13 | Footnote reference inside link | Parser may misparse | Consider diagnostic |
| 14 | `[^1]` ambiguity with link-reference defs | Unambiguous with `ENABLE_FOOTNOTES` | Document in notes |
| 15 | Consecutive defs without blank lines | Valid in new syntax | No issue |
| 16 | Footnotes inside tables | Reference works, definition may not | Consider diagnostic for def-in-table |
| 17 | Case sensitivity of labels | Labels are case-sensitive (`[^Foo]` ≠ `[^foo]`) | Index key = raw label string |
| 18 | Spaces in labels | Not allowed | No normalization needed |
| 19 | Definitions before references | Valid, common pattern | Index handles naturally |
| 20 | Definitions at end of document | Valid, standard pattern | No issue |

### Index design

```rust
struct FootnoteIndex {
    definitions: HashMap<String, DefinitionInfo>,  // label → {line, preview}
    references: HashMap<String, Vec<ReferenceInfo>>,  // label → [{line, context}]
}
```

### Diagnostics to implement

1. Undefined reference — error
2. Orphaned definition — warning
3. Duplicate definition — warning (first wins)
4. Self-referencing — info/suggestion (optional)
5. Empty definition — warning

### Code actions to implement

1. Create definition for undefined references
2. Move definition to top level (if inside blockquote/list)
3. Reorder definitions by first-reference order
