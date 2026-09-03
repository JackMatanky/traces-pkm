# Query-language intelligence

Type: grilling
Blocked by: 08, 10

## Question

Grounding: the Query DSL is parsed by a hand-rolled recursive-descent parser over a `logos` lexer (`SourceExpr::parse` at `src/query/grammar/source.rs:108`, `FilterExpr::parse` at `src/query/grammar/filter.rs:45`, shared boolean-expr logic at `src/query/grammar/expr.rs:275`). **`miette` is already fully integrated for span-aware diagnostics** (`QuerySyntaxError` derives `miette::Diagnostic` with an exact `SourceSpan`, `src/query/error.rs:134`) — this is the strongest existing precedent in the codebase for exactly the kind of span-precise error a language server needs, and should very likely be reused/forwarded almost directly into LSP diagnostics rather than reimplemented. No AST caching exists (every call re-parses); resolution is a full linear scan of the `FileIndex` (no inverted index).

This DSL is authored where? — first determine (if not already obvious from Template research, ticket 08) whether query strings appear only as Rust/CLI string arguments (`--from`, `--filter`) or also embedded inside template files (`query.*` MiniJinja helper calls, `src/template/engine/query.rs`) — the latter is the case that actually needs an *editor* (LSP) to provide intelligence, since CLI arguments aren't edited in a text buffer with LSP support. Decide:

- Where a Query DSL fragment can appear inside a `.md` template/note that the LSP would offer intelligence for (e.g. inside a `{{ query.pages(...) }}` call's string-literal arguments) — this requires the LSP to detect "cursor is inside a query-DSL string literal within a MiniJinja template" as a distinct completion/hover context, layered on top of template-language intelligence (ticket 22).
- Completion: field-name completion in filter expressions (informed by what fields are known workspace-wide, or schema-scoped if a `@Class` source is already specified — ties to ticket 20), source-expression completion (tag names, folder paths, File Class names).
- Diagnostics: forward `QuerySyntaxError`'s existing `SourceSpan` directly as an LSP diagnostic range for malformed query strings, mapped from the enclosing template's coordinate space.
- Hover: showing the resolved row count or field type for a filter expression (requires actually running the query against the live/overlay index — decide whether this is genuinely useful/affordable at hover time, given the standing performance requirement, or whether hover stays purely syntactic).
