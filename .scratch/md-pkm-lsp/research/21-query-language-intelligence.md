# Research: Query-Language Intelligence

> Consolidated research for [ticket 21](../issues/21-query-language-intelligence.md).
>
> **Sources:** Four parallel research subagents (Rust crate ecosystem, LSP mechanism survey, performance best practices, PKM plugin precedents) plus existing research from tickets 20, 34, 38, 39.

---

## Quick Reference

| Area | Key Finding |
|------|-------------|
| **DSL contexts** | Two contexts: CLI `--from`/`--filter` args (outside LSP scope) and MiniJinja template strings (primary LSP target). Query parser is hand-rolled recursive-descent over `logos` lexer, already has `miette::SourceSpan` diagnostics. |
| **Completion** | Need context-aware completion for: source expressions (tags, paths, file classes), filter expressions (field names, operators, values), and boolean combinators. Schema-aware narrowing from `SchemaService` already exists. |
| **Diagnostics** | `QuerySyntaxError` already carries `miette::SourceSpan` with byte offset + length. Conversion to LSP `Range` is mechanical via `ropey` or widened `ByteTracker`. |
| **Hover** | Feasible for: field type info, resolved row counts (post-execution), operator docs. Not feasible for: pre-execution result previews. |
| **Strongest PKM precedent** | zk — the only PKM tool with a real LSP that handles tag/link completions with span-aware diagnostics. All others use Obsidian's `EditorSuggest` API, not LSP. |
| **Performance** | <20ms completion achievable — no type inference, hash-map lookups only, small schema. Marksman achieves 1.5us goto-def in PKM context. |
| **Novelty** | No existing PKM LSP provides query-language intelligence. Traces would be the first. |

---

## Part I: Where the Query DSL Appears

### The Two Contexts

| Context | Example | LSP scope |
|---------|---------|-----------|
| CLI arguments | `traces query --from "#book and rating > 5"` | Out of scope |
| MiniJinja template strings | `{{ query.pages("#book and rating > 5") }}` | **Primary LSP target** |

### Query Grammar Structure

Two dialects sharing a boolean-expression core:

- `SourceExpr` — tags (`#tag`), paths (`folder/`), File Classes (`@Book*`), boolean combinators
- `FilterExpr` — field comparisons (`rating > 5`), function calls (`contains(tags, "#book")`), boolean logic
- `Expr` — shared boolean-expression logic

**Key parser files:**
- `src/query/grammar/source.rs:108` — `SourceExpr::parse`
- `src/query/grammar/filter.rs:45` — `FilterExpr::parse`
- `src/query/grammar/field.rs` — `FieldPath` parsing, accessor namespaces
- `src/query/grammar/expr.rs:275` — shared boolean-expression logic
- `src/query/error.rs:106` — `QuerySyntaxError` with `miette::Diagnostic` + `SourceSpan`

### Template Integration

Query strings appear inside `{{ query.pages("...") }}` calls, registered via `src/template/engine/query.rs`. The LSP must detect "cursor is inside a query string argument" to provide completions.

### What Completion Should Cover

| Cursor context | Completion items | Source |
|---------------|-----------------|--------|
| Start of source expression | Tags, paths, file classes, boolean keywords | Vault index + SchemaService |
| After `#` | Tag names from vault index | `IndexStore` tags |
| After `@` | File class names | `SchemaService::schemas()` |
| After path prefix | Directory/file paths | FileIndex paths |
| Start of filter expression | Field names from schemas | `Schema::fields()` |
| After field name | Operators | Hardcoded operator set |
| After operator | Values matching field type | `SchemaFieldDef::select_values()` |
| Inside boolean expression | `and`, `or`, `not` keywords | Hardcoded |

---

## Part II: Completion Mechanisms

### Architecture Pattern

Every studied LSP follows the same four-step completion pipeline:

1. **Detect context**: "cursor is inside a query string in a MiniJinja template"
2. **Narrow schema**: "what kind of completion is valid here?" (source vs filter, position within expression)
3. **Match candidates**: fuzzy-match partial input against narrowed schema subset
4. **Present**: format as `lsp_types::CompletionItem` with appropriate kind/detail/documentation

### Context Detection Approaches

| Approach | Used by | How it works | Trade-off |
|----------|---------|-------------|-----------|
| Region boundary check | Metadata Menu | Detect delimiters, check cursor between | Simple, reliable for fenced blocks |
| Regex trigger | Templater | Match `tp.\w*(\.\w*)?$` against line prefix | Good for structured syntax with predictable prefixes |
| CST range walk | Marksman | Walk concrete syntax tree, check cursor inside node range | Most accurate, needs incremental parser |
| LookBehind + position | zk | N chars before cursor, check trigger chars | Simple, works for char-triggered contexts |
| Line-prefix regex + fence detection | rumdl | Regex patterns for link targets, fence stack | Heuristic, fast |

**Recommended for Traces:** Combine regex trigger (Templater pattern) for MiniJinja detection + position-based token classification from the query parser's token stream.

### Completion Item Construction

From zk's `completion.go` and `lsp-types`:

```rust
CompletionItem {
    label: "rating".into(),
    kind: Some(CompletionItemKind::FIELD),
    detail: Some("Number".into()),
    documentation: Some(lsp_types::Documentation::String(
        "Schema field: Number (min: 0, max: 10)".into()
    )),
    text_edit: Some(Either::Left(TextEdit {
        range: lsp_range,
        new_text: "rating".into(),
    })),
}
```

### Schema Narrowing Strategy

| Cursor position in query | Schema subset | Size |
|--------------------------|---------------|------|
| Inside source expression | Tags + paths + file classes | 50-200 |
| After `@` in source | File class names only | 10-50 |
| Inside filter LHS | Field names from all schemas | 20-100 |
| Inside filter RHS (after field name) | Valid values for that field's type | 5-20 |
| At expression start | Keywords + operators | 10-20 |

Pre-build these subsets at schema load. Completion never iterates full schema.

### Fuzzy Matching

| Crate | Speed | Recommendation |
|-------|-------|----------------|
| `fuzzy-matcher` (skim) | Baseline | Keep for now (already in codebase) |
| `nucleo-matcher` | 6x faster | Consider for Phase 2 if >100 candidates |

---

## Part III: Diagnostic Forwarding

### Existing Pattern: QuerySyntaxError

`src/query/error.rs` already has the correct shape:

```rust
#[derive(Diagnostic, Error)]
#[error("invalid {dialect} expression")]
pub struct QuerySyntaxError {
    pub(crate) dialect: QueryDialect,
    #[source_code]
    pub(crate) input: String,
    #[label("{lex_error}")]
    pub(crate) span: SourceSpan,
    #[source]
    pub(crate) lex_error: Box<LexError>,
}
```

### Conversion Pipeline

```
QuerySyntaxError
  -> span.offset() + span.len()
  -> ropey: byte_to_line(), byte_to_char() -> char_to_utf16_cu()
  -> lsp_types::Range { start: Position, end: Position }
  -> lsp_types::Diagnostic { range, severity, message, source, code }
```

### Diagnostic Types for Query DSL

| Diagnostic | Severity | Trigger | Message pattern |
|-----------|:--------:|---------|-----------------|
| Syntax error | Error | Parser returns `QuerySyntaxError` | "Invalid {dialect} expression: {lex_error}" |
| Unknown tag | Warning | Source references non-existent tag | "Tag '{tag}' not found in vault" |
| Unknown file class | Warning | `@ClassName` references non-existent schema | "File class '{class}' not found" |
| Unknown field | Information | Filter references field not in any schema | "Field '{field}' not defined in any schema" |
| Type mismatch | Error | Operator applied to wrong field type | "Cannot compare '{field}' ({type}) with '{value}'" |
| Dead path | Information | Source path doesn't match any files | "No files match path '{path}'" |

### Debounce Strategy

- **300ms debounce** with `AtomicU64` generation counter
- Only the last edit in a burst triggers diagnostics
- Push model (`publishDiagnostics`) for Phase 1; pull diagnostics in Phase 2

---

## Part IV: Hover Feasibility

### What Hover Can Show

| Hover target | Content | Feasibility |
|-------------|---------|:-----------:|
| Source expression (tag) | "Tag used in N notes" + recent notes | **Yes** |
| Source expression (path) | "Matches N files" + folder structure | **Yes** |
| Source expression (file class) | Schema definition summary | **Yes** |
| Filter field name | Field type, constraints, source schema | **Yes** |
| Filter operator | Operator documentation, valid field types | **Yes** |
| Filter value | Resolved value info (e.g., select option) | **Yes** |
| Entire query expression | Execution result summary (row count) | **Partial** — requires `QueryService::execute()` |
| Boolean keywords | Syntax help | **Yes** |

### Execution-Dependent Hover

The most valuable hover (execution result summary) requires running the query. **Recommendation:** Defer to Phase 2. Phase 1 provides schema-level hover (field types, tag counts, class definitions) which requires no query execution.

---

## Part V: Cursor-in-Query Detection

### Detection Algorithm

1. Find all `{{ ... }}` regions in the document (regex or pulldown-cmark event scan)
2. Check if cursor falls within any region (binary search on region ranges)
3. If yes, parse the template expression to find function calls
4. Check if the function is `query.pages` / `query.lists` / `query.tasks`
5. Find the string argument's byte range
6. Check if cursor falls within the string argument
7. Compute cursor offset relative to string start

### Implementation Approach

Regex trigger (Templater pattern) adapted for MiniJinja:

```rust
lazy_static::lazy_static! {
    static ref TEMPLATE_REGION: Regex = Regex::new(r"\{\{.*?\}\}").unwrap();
}

fn find_template_at_cursor(text: &str, cursor_byte: usize) -> Option<TemplateRegion> {
    TEMPLATE_REGION.find(text).filter(|m| {
        m.start() <= cursor_byte && cursor_byte <= m.end()
    }).map(|m| TemplateRegion { range: m.range(), text: m.as_str() })
}

fn is_query_call(template_text: &str) -> bool {
    template_text.contains("query.pages(")
        || template_text.contains("query.lists(")
        || template_text.contains("query.tasks(")
}
```

### Alternative: logos Extras for State Tracking

`logos::Extras` provides mutable state during lexing. A template-aware lexer could track `Normal -> InTemplate -> InQueryString` state transitions. More efficient for full-document scan but requires integrating with `pulldown-cmark`. For Phase 1, regex is simpler and sufficient.

---

## Part VI: Crate Ecosystem

### Crates Already Integrated

| Crate | Role for Query Intelligence |
|-------|----------------------------|
| `logos` | Lexer for query tokens — already produces byte spans |
| `miette` 7.5.0 | Diagnostic spans — already used in `QuerySyntaxError` |
| `fuzzy-matcher` 0.3.7 | Fuzzy matching for completions |
| `pulldown-cmark` | Markdown parsing — identifies template regions |
| `lsp-types` 0.97.0 | LSP wire types |
| `tower-lsp-server` | LSP server framework |

### Crates to Add

| Crate | Role | Justification |
|-------|------|---------------|
| `ropey` 1.6.1 | Text buffer + byte/char/UTF-16 conversion | De facto standard; used by Markdown Oxide for same purpose |
| `nucleo-matcher` 0.3.5 | High-performance fuzzy matching (Phase 2) | 6x faster than fuzzy-matcher; only needed if >100 candidates |

### Crates NOT Needed

| Crate | Why not |
|-------|---------|
| `tree-sitter` | Zero PKM-LSP precedent; full re-parse per edit is the industry norm |
| `nom` / `pest` | Hand-rolled logos parser already working |
| `serde_json` for schema | Schema is TOML-based via SchemaService |

---

## Part VII: PKM Precedents

### System Comparison

| System | Query Language | LSP | Completion | Diagnostics | Hover | Key Source Files |
|--------|:---:|:---:|:---:|:---:|:---:|---|
| Dataview | DQL | No | No | No | No | `src/expression/parse.ts:7587` (expr grammar), `src/query/parse.ts:8701` (query grammar) |
| Tasks | Emoji syntax | No | Yes (EditorSuggest) | No | No | `docs/Editing/Auto-Suggest.md:2182`, `src/Query/` |
| Templater | Template functions | No | Yes (EditorSuggest) | No | No | `src/editor-suggest/Autocomplete.ts:3954` |
| Metadata Menu | Frontmatter fields | No | Yes (EditorSuggest) | No | No | `src/suggester/metadataSuggester.ts:22712` |
| **zk** | Zettelkasten filters | **Yes** | **Yes** | **Yes** | **Yes** | `internal/adapter/lsp/server.go:4150`, `completion.go:3080`, `document.go:3152` |

### Key Findings

1. **Only zk has a real LSP** — all others use Obsidian's `EditorSuggest` API. zk is the only system with true LSP-shaped intelligence.

2. **Dataview has the best parser architecture** (Parsimmon combinators, typed AST) but zero editor intelligence. Parser invoked only at render time, not edit time.

3. **No PKM system provides query-language intelligence** — completion is for links/tags/file paths, not query DSL syntax. Traces would be the first.

4. **Cursor-in-query detection** patterns: region boundaries (Metadata Menu), regex triggers (Templater), AST-guided position (Dataview), LookBehind + position (zk).

5. **Most actionable pattern for Traces:** Metadata Menu's boundary detection + Templater's regex trigger + completions from vault index (similar to zk).

6. **Rich completion-trigger-detection precedent across all four plugins, but zero diagnostics/hover/definition precedent.** Traces isn't playing catch-up — it would be the first system to bring hover/diagnostics/definition-grade intelligence to these languages at all.

### zk's LSP Architecture (Closest Precedent)

- **Completion flow**: `LookBehind` for trigger chars -> `IsTagPosition` check -> build completion list from vault index
  ```
  handler.TextDocumentCompletion
    -> buildInvokedCompletionList (manual/typing trigger)
         -> check LookBehind for [[ or ((  -> buildLinkCompletionList
         -> check IsTagPosition            -> buildTagCompletionList
    -> buildTriggerCompletionList (trigger char)
         -> LookBehind for #  -> buildTagCompletionList
         -> LookBehind for :  -> buildTagCompletionList
  ```
- **Completion item construction**: `completionTemplates` with configurable Label/FilterText/Detail via Handlebars templates; `newTextEditForLink()` builds TextEdit with optional `AdditionalTextEdits`
- **Diagnostics**: `DidOpen`/`DidChange` with 1-second debounce; iterates document links, resolves each via `noteForLink()`, emits `publishDiagnostics`
- **Hover**: Shows full note content for link targets
- **Cursor detection**: `LookBehind(pos, length)` + `WordAt(pos)` + `IsTagPosition`

### Implementation Patterns (from Sub-Document Analysis)

**Dataview** — Combinator-based parser (`P.createLanguage<ExpressionLanguage>()`), expression language separate from query language, error handling via `Result<Query, string>`. No editor intelligence at all — completion is third-party, unmaintained (`dnlbauer/obsidian-dataview-autocompletion`).

**Tasks** — `EditorSuggestorPopup` extends `EditorSuggest`; smart context gating (only on task lines matching `- [ ]`/`* [ ]`/`+ [ ]`); progressive suggestions (after priority, next menu shows remaining options); format-aware (detects Dataview bracket `[]` format); pluggable `TaskSerializer` implementations (`DefaultTaskSerializer`/`DataviewTaskSerializer`).

**Templater** — `Autocomplete` extends `EditorSuggest`; two-level completion (module names then functions); regex trigger `tp\.(?<module>[a-z]*)?(\.(?<fn>[a-zA-Z_.]*)?)?$`; documentation-as-data (loaded from structured source, not derived at runtime). Namespace mapping: `tp.file`/`tp.date`/`tp.system` → Traces `file.*`/`date.*`/`ui.*`.

**Metadata Menu** — `ValueSuggest` extends `EditorSuggest<IValueCompletion>`; field-type-aware (Cycle/Multi/Select → option lists, File/MultiFile → vault files, tags → vault tag list); `onTrigger()` checks `isInFrontmatter()` via `---` delimiter scan or `getLineFields()` parser; `fileClass` inheritance mirrors Traces' Schema module. No live diagnostics — validates via modals.

### Diagnostics Patterns (from zk)

| Pattern | Implementation |
|---------|---------------|
| Dead references | Flag links/paths that don't resolve to existing notes |
| Type mismatches | Flag expressions where operators don't match operand types |
| Syntax errors | Parse errors with position information (parsers produce these natively) |
| Unused clauses | Warn about redundant operations |

Debounce diagnostics (zk uses 1 second) to avoid lag during typing.

---

## Part VIII: Performance Best Practices

### Performance Targets

| Operation | Target | Stretch | Rationale |
|-----------|:------:|:-------:|-----------|
| Completion | <20ms | <10ms | In-memory index, hash-map lookup only |
| Hover | <10ms | <5ms | Schema lookup, no computation |
| Go-to-definition | <5ms | <2ms | O(1) hash-map (Marksman: 1.5us) |
| Find-references | <20ms | <5ms | Linear scan of in-memory index (~1.4us/doc) |
| Diagnostics (per-file) | <15ms | <5ms | Re-parse + lint single file |
| Document symbols | <10ms | <5ms | Headings from in-memory index |
| Cold start (1000 files) | <3s | <1s | Parallel rayon scan + redb load |
| Warm start | <500ms | <200ms | Load redb + filesystem diff |

### Verified Industry Benchmarks (p95)

| Source | Completion | Hover | Go-to-def | Diagnostics | Notes |
|--------|:----------:|:-----:|:---------:|:-----------:|-------|
| Solidity LS (p95) | 59.9ms | 7.3ms | 8.3ms | 2.8ms | Production benchmarks |
| Marksman (M1 Pro) | — | — | 1.5–2.9us | — | Hash-map lookup, 10–250 docs |
| clangd (cold) | 80ms | — | 120ms | — | 4-core i7, LLVM source tree |
| clangd (incremental) | — | — | — | 8ms | Incremental update only |
| rust-analyzer (goal) | <100ms | — | — | — | Stated goal #17491 |
| gopls (budget) | <100ms | — | — | — | Dynamic scope reduction under budget |

### Why <20ms Completion Is Achievable

- No type inference — field names/tags/classes are hash-map lookups
- No cross-file semantic analysis — query strings are self-contained
- Small schema — hundreds of items, not millions of types
- Marksman achieves 1.5us goto-def in PKM context
- Bottleneck is context detection, not schema lookup

### Caching Strategies

| Cache Target | Key | Invalidation | LRU Capacity | Rationale |
|-------------|-----|-------------|:------------:|-----------|
| Parsed AST | path + content hash | On didChange (full re-parse) | 128 | Matches rust-analyzer's Parse LRU |
| Completion items | (path, position, trigger) | On document edit | 64 | Small, fast to recompute |
| Diagnostics | content hash + version | Debounced on didChange | 128 | One per open file |
| Schema lookups | field/tag name | Schema reload only | Unlimited | Rarely change |
| Structural data (headings, links) | content hash of parse output | Only when structural diff non-empty | 256 | Larger than file count to cover recently closed |

Content-hash keys (blake3), not mtime. Two-tier cache: raw parsed output + derived data separately. Derived data (links, tags, completions) is invalidated transitively only when the raw parse changes structurally.

### Durability-Based Invalidation (Salsa Pattern)

Divide inputs by change frequency:
- **Durable**: Schema definitions — rarely change
- **Normal**: File headings, link targets — change on edit
- **Volatile**: Query string content, cursor — every keystroke

A link edit should not trigger re-evaluation of schema-based completions. Salsa's global version vector: maintain a version vector keyed by durability level. Incrementing a volatile input only bumps the volatile counter; durable queries skip re-validation entirely. This reduced rust-analyzer's per-edit overhead by ~300ms.

### Same-Thread Revision Caching

From rust-analyzer PR #20527: cache results within the same revision (same set of inputs) on the same thread. Avoids synchronization overhead while still providing cache hits for batch requests (highlighting + inlay hints + completions on the same file).

### Diagnostic Publishing

- Push + 300ms debounce (AtomicU64 generation counter) for Phase 1
- Pull diagnostics (LSP 3.17) with resultId caching for Phase 2
- Hash-based deduplication (FNV-1a) — skip notification if unchanged

---

## Part IX: Architectural Recommendations

### 1. Query Intelligence Module

Create `src/query/intelligence/` with:
- `cursor.rs` — cursor-in-query detection (regex trigger + position classification)
- `completion.rs` — completion item generation from schema narrowing
- `hover.rs` — hover content generation from schema lookup
- `diagnostics.rs` — diagnostic forwarding from QuerySyntaxError + semantic checks

### 2. Phase 1 Scope

- **Completion**: Source expressions (tags, paths, file classes) + filter expressions (field names, operators)
- **Diagnostics**: Forward `QuerySyntaxError` spans + unknown tag/class/field warnings
- **Hover**: Schema-level info (field types, tag counts, class definitions)
- **Cursor detection**: Regex-based MiniJinja region detection

### 3. Phase 2 Scope

- Execution-dependent hover (row count preview)
- Value-level completion (select options, date format placeholders)
- Type-mismatch diagnostics
- Pull diagnostics (LSP 3.17)
- nucleo-matcher for faster fuzzy matching

### 4. Integration Points

- `ropey::Rope` for live-buffer text representation and UTF-16 conversion
- `SchemaService` for field/class completions and hover
- `IndexStore` for tag completions and tag-count hover
- `QueryService` for execution-dependent features (Phase 2)
- `pulldown-cmark` for template region identification

---

## Part X: Open Questions

1. **Execution-dependent hover**: Should Phase 1 include a lightweight "estimated row count" hover using index metadata (without full query execution), or defer all execution-dependent features to Phase 2?

2. **Diagnostic severity calibration**: Unknown tag = Warning? Information? Depends on whether tags are "expected to exist" or "optional metadata."

3. **Query string nesting**: Can query strings contain nested quotes (e.g., `contains(tags, "#book")`)? Affects regex detection accuracy.

4. **Multi-argument queries**: Do `query.pages()` calls have additional arguments beyond the query string? Affects cursor detection scope.

5. **Performance at vault scale**: At 50k+ notes, does tag-class completion (which needs to scan the tag index) stay under 20ms? May need pre-computed tag frequency sorting.

---

## Appendix: Sources

### Research Subagents

1. **Rust crate ecosystem** — ropey, nucleo-matcher, fuzzy-matcher, miette LSP mapping, logos context-sensitive lexing, pulldown-cmark, tree-sitter, schema-aware completion patterns
2. **LSP mechanism survey** — VS Code Markdown LS, Markdown Oxide, Marksman, rumdl completion/diagnostics/hover/embedded-detection patterns
3. **Performance best practices** — Industry latency benchmarks, caching strategies, completion reduction techniques, diagnostic publishing, schema-aware architecture
4. **PKM plugin precedents** — Dataview, Tasks, Templater, Metadata Menu, zk query-language intelligence patterns

### Existing Research Files

- `research/20-schema-fileclass-intelligence.md` — Schema system, file-class binding, type mapping, completion architecture
- `research/39-source-span-position-model.md` — Source span model, ByteTracker, ropey adoption

### Sub-Documents (Merged — Retain for Reference Only)

The following sub-documents' content has been consolidated into Parts VII and VIII above. They may be deleted from the research directory.

- `research/21-pkm-precedents-detail.md` — **Merged into Part VII.** Original content: Dataview/Templater/Metadata Menu/zk detailed precedents with source-line references, cursor detection approaches, completion item types, diagnostics patterns. All key content now in Part VII's "Implementation Patterns" and "Diagnostics Patterns" subsections.
- `research/34-query-dsl-lsp-performance-patterns.md` — **Merged into Part VIII.** Original content: industry benchmark latency numbers (p95), LRU capacity rationale table, same-thread revision caching pattern. All key content now in Part VIII's "Verified Industry Benchmarks," "LRU Capacity Rationale," and "Same-Thread Revision Caching" subsections.
- `research/08-dataview-templater-metadatamenu-parity.md` — **Merged into Part VII.** Original content: per-plugin implementation findings (what each plugin provides and lacks, EditorSuggest patterns, namespace mapping). All key content now in Part VII's "Key Findings" item 6 and "Implementation Patterns" subsection.

### Digests

- `docs/digests/lsp_rvben-rumdl-digest.txt` — rumdl LSP
- `docs/digests/lsp_feel-ix-343-markdown-oxide-digest.txt` — Markdown Oxide LSP
- `docs/digests/zk-src-digest.txt` — zk source
- `docs/digests/obsidian_blacksmithgu-obsidian-dataview-*.txt` — Dataview
- `docs/digests/obsidian_silentvoid13-templater-*.txt` — Templater
- `docs/digests/obsidian_mdelobelle-metadatamenu-*.txt` — Metadata Menu
- `docs/digests/obsidian_obsidian-tasks-*.txt` — obsidian-tasks

---

## Part XI: Stress Test Results — Round 1

Four parallel stress tests challenged the initial decisions against the codebase, previous map decisions, performance claims, and LSP conventions.

### Decision Revisions from Round 1

| Decision | Original | Stress Test Finding | Revision |
|----------|----------|-------------------|----------|
| DSL scope | Template-string only | Fenced code blocks (```query) are Dataview's dominant pattern; ~50 lines to add | **Add fenced code blocks as secondary scope** |
| Cursor detection | Regex on full document | O(n) per request; single-line check is O(1) | **Single-line detection: check current line for {{ delimiters** |
| Severity | Information for all unknown tags | `suggest_field` already does edit-distance matching; typos deserve Warning | **Two-tier: Warning for close matches (distance ≤ 2), Information for distant** |
| Hover | Schema-level only | Index metadata (tag counts, class instances) is cheap and already available | **Enrich with index metadata counts where cheaply available** |
| Phase 1 completion | Source + filter expressions, no values | Value-after-operator is highest-value, lowest-cost (select_values API exists) | **Include Select + Boolean value completion (~25 lines)** |

### New Findings from Round 1

| # | Finding | Severity |
|---|---------|----------|
| 1 | Trigger characters not specified — must register `#`, `@`, `(`, `"` or no auto-completion | Major |
| 2 | Use `textEdit` not `insertText` — insertText causes word-boundary bugs in string contexts | Major |
| 3 | Declare all capabilities in `initialize` — missing = editor sends no requests | Major |
| 4 | 300ms debounce may be too slow for query syntax errors; zk uses 1s but learning-curve feedback needs faster | Major |

---

## Part XII: Stress Test Results — Round 2

Four parallel stress tests challenged the revised Round 2 decisions.

### Decision Revisions from Round 2

| Decision | Revised | Stress Test Finding | Further Revision |
|----------|---------|-------------------|------------------|
| R1: Fenced code blocks | Add ```query / ```traces-query | Collision risk with third-party plugins using ```query; pulldown-cmark discards info string | **Defer to Phase 2; if added, use only `traces-query` (namespaced)** |
| R2: Select + Boolean value completion | Include in Phase 1 | No cursor-position-aware parser exists; AST doesn't carry spans; completing RHS requires partial-parse mode | **Defer value completion to Phase 2; Phase 1 = field names + operators only** |
| R3: Two-tier severity | Warning for close matches | `suggest_field` works on fields (small set), not tags (freeform, 500+); multiple close matches create noise | **Split by category: fields use suggest_field (Warning on single match); tags use Information only; defer tag fuzzy-matching to Phase 2** |
| R4: Trigger chars `#`, `@`, `(`, `"` | Register all four | `"` closes string literal; `(` is context-dependent; `is_query_call` doesn't match actual API shape | **Register only `#` and `@` in Phase 1; defer `(` and `"` to Phase 2** |
| R5: Single-line cursor detection | Check current line only | Multi-line `{{ }}` expressions are common; single-line check misses them entirely | **Use depth counter (increment on `{{`, decrement on `}}`), not single-line regex** |
| R6: Two-tier debounce 150ms+300ms | Syntax faster than semantic | Adds complexity with no latency benefit; cascading timer problem makes semantic diagnostics slower (450ms effective) | **Single 300ms debounce for all diagnostics; configurable via `[lsp.diagnostics] debounce_ms`** |

### Implementation Feasibility Findings

| Component | Exists Today | Needs Building | Effort |
|-----------|-------------|---------------|--------|
| Cursor context classification | Logos lexer with spans | `classify_cursor_position()` + `CursorContext` enum | S |
| Tag fuzzy-matching | `PATHS_BY_TAG` table, `strsim::closest_match` | `IndexStore::unique_tags()` + ranked completion | S |
| Fenced code block language | `CodeBlockKind` discarded at parse time | Extract info string from pulldown-cmark | M |
| Query re-parse cost | Benchmarks exist; parsers are fast | None — existing parse sufficient | — |
| Template function names | Hardcoded in `QueryOps` Object impl | Static registry (~50 LOC) | S |
| Module placement | `src/query/` same-crate access | New `src/query/intelligence/` module | S |

### Cross-Round Consistency Findings

| Check | Verdict | Severity | Action |
|-------|---------|----------|--------|
| Round 1→2 consistency | Consistent | Tension | Value-completion overlap needs ticket 24 ownership |
| Fenced blocks vs ticket 09 | Consistent | Nit | Document as independent feature |
| Value completion vs ticket 20 | Tension | Tension | Ticket 24 must own shared completion-generation function |
| Two-tier severity vs ticket 25 | Misalignment | Misalignment | Severity choices provisional pending T25's cross-cutting policy |
| Trigger chars vs ticket 24 | Tension | Tension | T21 triggers are requirements input, not settled decisions |
| Single-line detection | Tension | Tension | Document as Phase 1 limitation; Phase 2 depth-counter path exists |
| Debounce timing | Consistent | Nit | Formalize three-tier model in T25 |

### LSP Convention Findings

| Convention | Verdict | Severity |
|-----------|---------|----------|
| Trigger chars fire everywhere; server checks context | Follow — register `#`, `@`; check cursor-in-query in handler | HIGH |
| Use textEdit with explicit range, not insertText | Follow strictly — compute range from query parser spans | CRITICAL |
| Declare triggerCharacters, hoverProvider, textDocumentSync FULL in initialize | Follow exactly — without these, features silently don't activate | HIGH |
| completionItem/resolve | Skip for Phase 1; schema lookups are fast enough | Nit |
| Work done progress | Include for initial indexing (~30 lines) | Minor |

---

## Part XIII: Final Decision Record (Locked)

### Settled Decisions

| # | Decision | Status | Notes |
|---|----------|--------|-------|
| D1 | Query intelligence scope: `{{ query.*() }}` template strings | **Locked** | Fenced code blocks deferred to Phase 2 |
| D2 | Cursor detection: depth counter over `{{`/`}}` delimiters | **Locked** | Handles multi-line; O(N) in document length |
| D3 | Phase 1 completion: field names (LHS) + operators + Select/Boolean values (RHS) | **Locked** | Value completion via lexer-based token classification (see D11) |
| D4 | Trigger characters: `#` and `@` only in Phase 1 | **Locked** | `(` and `"` deferred; requirements input for T24 |
| D5 | Diagnostic severity: fields use suggest_field (Warning/Information); tags use Information only | **Locked** | Tag fuzzy-matching deferred to Phase 2 |
| D6 | Diagnostics: single 300ms debounce, configurable | **Locked** | Two-tier complexity not justified |
| D7 | Hover: schema-level + index metadata counts (tags, classes) | **Locked** | Execution-dependent hover deferred to Phase 2 |
| D8 | Use textEdit with explicit range for all completions | **Locked** | CRITICAL convention from LSP spec |
| D9 | Declare capabilities: completionProvider (#, @), hoverProvider, textDocumentSync FULL | **Locked** | Without these, features don't activate |
| D10 | Cursor detection is one context provider for T24's dispatcher | **Locked** | Not a standalone detection path |
| D11 | Value completion mechanism: lexer-based token classification | **Locked** | Run logos lexer on text prefix before cursor, classify last token type. Covers `<field> <op> ` and `contains(tags, ` patterns. ~50 LOC. |
| D12 | Fenced code blocks: deferred to Phase 2 | **Locked** | Documented limitation; collision risk with third-party plugins |
| D13 | Pre-conditions for hover: build `unique_tags()`, `unique_file_classes()`, `suggest_class()` | **Locked** | ~65 LOC total; must exist before hover can show counts |

### Phase 1 Scope (Locked)

- **Completion**: Tags (`#`), file classes (`@`), field names (filter LHS), operators (`and`/`or`/`not`/comparisons), Select options + Boolean values (RHS via lexer token classification)
- **Diagnostics**: Forward `QuerySyntaxError` spans; unknown field (Information); unknown class with close match via suggest_class (Warning); unknown class no match (Information)
- **Hover**: Tag count ("Tag #book — 47 notes"), class instance count ("File class Book — 23 instances"), field type + constraints
- **Cursor detection**: Depth counter over `{{`/`}}` delimiters, checking cursor falls within positive-depth region
- **Debounce**: 300ms for all diagnostics
- **Trigger chars**: `#` and `@` (requirements input for T24)
- **Conventions**: textEdit with explicit range; capabilities declared in initialize; work-done progress for initial indexing

### Phase 2 Scope (Deferred)

- Fenced code blocks (`traces-query`)
- Tag fuzzy-matching for diagnostics (Warning tier)
- Trigger characters `(` and `"`
- Execution-dependent hover (row count preview via `QueryService::execute()`)
- `completionItem/resolve` for expensive documentation
- Pull diagnostics (LSP 3.17)

### Pre-Conditions (must be built before Phase 1 implementation)

| Pre-condition | Owner | Effort | Notes |
|---------------|-------|--------|-------|
| `IndexStore::unique_tags() -> Vec<(Tag, u32)>` | T21 impl | S (~30 LOC) | Scan `PATHS_BY_TAG` multimap keys, count entries per key |
| `IndexStore::unique_file_classes() -> Vec<(ClassName, u32)>` | T21 impl | S (~20 LOC) | Same pattern on `PATHS_BY_FILE_CLASS` |
| `SchemaService::suggest_class(name: &str) -> Option<&str>` | T21 impl | S (~15 LOC) | Levenshtein against schema names via existing `closest_match` |

### Open Items for Other Tickets

| Item | Owner | Notes |
|------|-------|-------|
| Shared completion-generation function | T24 | Both T20 (frontmatter) and T21 (query) use select_values() |
| Trigger character reconciliation | T24 | T21 needs `#`, `@`; T19 needs `[`, `:`; rumdl needs `(`, `#`, `/` |
| Severity calibration policy | T25 | Cross-cutting: when to use Warning vs Information |
| Three-tier debounce model | T25 | 50-100ms reparse, 150ms interactive, 300ms diagnostics |
