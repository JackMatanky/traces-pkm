# Query-language intelligence

Type: grilling
Blocked by: 08, 10
Status: resolved

## Question

Grounding: the Query DSL is parsed by a hand-rolled recursive-descent parser over a `logos` lexer (`SourceExpr::parse` at `src/query/grammar/source.rs:108`, `FilterExpr::parse` at `src/query/grammar/filter.rs:45`, shared boolean-expr logic at `src/query/grammar/expr.rs:275`). **`miette` is already fully integrated for span-aware diagnostics** (`QuerySyntaxError` derives `miette::Diagnostic` with an exact `SourceSpan`, `src/query/error.rs:134`) — this is the strongest existing precedent in the codebase for exactly the kind of span-precise error a language server needs, and should very likely be reused/forwarded almost directly into LSP diagnostics rather than reimplemented. No AST caching exists (every call re-parses); resolution is a full linear scan of the `WorkspaceIndex` (no inverted index).

This DSL is authored where? — first determine (if not already obvious from Template research, ticket 08) whether query strings appear only as Rust/CLI string arguments (`--from`, `--filter`) or also embedded inside template files (`query.*` MiniJinja helper calls, `src/template/engine/query.rs`) — the latter is the case that actually needs an *editor* (LSP) to provide intelligence, since CLI arguments aren't edited in a text buffer with LSP support. Decide:

- Where a Query DSL fragment can appear inside a `.md` template/note that the LSP would offer intelligence for (e.g. inside a `{{ query.pages(...) }}` call's string-literal arguments) — this requires the LSP to detect "cursor is inside a query-DSL string literal within a MiniJinja template" as a distinct completion/hover context, layered on top of template-language intelligence (ticket 22).
- Completion: field-name completion in filter expressions (informed by what fields are known workspace-wide, or schema-scoped if a `@Class` source is already specified — ties to ticket 20), source-expression completion (tag names, folder paths, File Class names).
- Diagnostics: forward `QuerySyntaxError`'s existing `SourceSpan` directly as an LSP diagnostic range for malformed query strings, mapped from the enclosing template's coordinate space.
- Hover: showing the resolved row count or field type for a filter expression (requires actually running the query against the live/overlay index — decide whether this is genuinely useful/affordable at hover time, given the standing performance requirement, or whether hover stays purely syntactic).

## Resolution

### Answer

**Where queries appear:** Query strings appear in MiniJinja template calls (`{{ query.pages("...") }}`). CLI args are out of scope. Fenced code blocks (` ```query `) deferred to Phase 2.

**Cursor detection:** Depth counter over `{{`/`}}` delimiters — increment on `{{`, decrement on `}}`, check cursor falls within positive-depth region. O(N) in document length, handles multi-line expressions.

**Completion (Phase 1):**
- Tags after `#` (from `IndexStore` tag index)
- File classes after `@` (from `SchemaService`)
- Field names in filter LHS (from `Schema::fields()`)
- Operators (`and`, `or`, `not`, `>`, `<`, `=`, `contains`, etc.)
- Select options + Boolean `true`/`false` on RHS (via lexer-based token classification: run logos on text prefix before cursor, classify last token type)

**Diagnostics (Phase 1):**
- Forward `QuerySyntaxError` spans directly as LSP diagnostics (mechanical conversion via `ByteTracker`)
- Unknown field: Information severity
- Unknown class with close match (via `suggest_class`): Warning severity
- Unknown class no match: Information severity
- Tags: Information only (no fuzzy-matching at diagnostic time)
- Single 300ms debounce for all diagnostics

**Hover (Phase 1):**
- Tag count ("Tag #book — 47 notes") via `IndexStore::unique_tags()`
- Class instance count ("File class Book — 23 instances") via `IndexStore::unique_file_classes()`
- Field type + constraints from `SchemaFieldDef`
- Execution-dependent hover (row count preview) deferred to Phase 2

**LSP conventions:**
- Use `textEdit` with explicit range (not `insertText`) for all completions
- Register `#` and `@` as trigger characters (requirements input for T24)
- Declare capabilities: `completionProvider`, `hoverProvider`, `textDocumentSync: FULL`
- Work-done progress for initial vault indexing

**Pre-conditions before implementation:**
- `IndexStore::unique_tags() -> Vec<(Tag, u32)>` (~30 LOC)
- `IndexStore::unique_file_classes() -> Vec<(ClassName, u32)>` (~20 LOC)
- `SchemaService::suggest_class(name) -> Option<&str>` (~15 LOC)

**Phase 2 (deferred):**
- Fenced code blocks (`traces-query`)
- Tag fuzzy-matching for diagnostics
- Trigger characters `(` and `"`
- Execution-dependent hover
- `completionItem/resolve`
- Pull diagnostics (LSP 3.17)

**Cross-ticket dependencies:**
- T24 owns: shared completion-generation function, trigger character reconciliation
- T25 owns: severity calibration policy, three-tier debounce model

### Research

Consolidated research: `research/21-query-language-intelligence.md` (603 lines, Parts I-XIII)
- Part I: DSL embedding contexts
- Part II: Completion mechanisms
- Part III: Diagnostic forwarding
- Part IV: Hover feasibility
- Part V: Cursor-in-query detection
- Part VI: Crate ecosystem
- Part VII: PKM precedents (merged from `21-pkm-precedents-detail.md` and `08-dataview-templater-metadatamenu-parity.md`)
- Part VIII: Performance best practices (merged from `34-query-dsl-lsp-performance-patterns.md`)
- Part IX: Architectural recommendations
- Part X: Open questions (resolved)
- Part XI: Stress test results — Round 1
- Part XII: Stress test results — Round 2
- Part XIII: Final decision record (locked)
