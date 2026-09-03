# Research: Microsoft markdown-language-service/server generic capability baseline

Resolves ticket [05-research-ms-markdown-language-service](../issues/05-research-ms-markdown-language-service.md).

Sources: `https://github.com/microsoft/vscode-markdown-languageservice` (`src/index.ts`, `src/languageFeatures/folding.ts`, `src/languageFeatures/smartSelect.ts`, `src/languageFeatures/diagnostics.ts`, `src/languageFeatures/pathCompletions.ts`, `src/types/textDocument.ts`), `https://github.com/microsoft/vscode-markdown-languageserver` (`src/server.ts`).

## Overview

This is the canonical generic-Markdown language-service implementation (what VS Code's built-in Markdown support runs on), split cleanly into a protocol-agnostic library (`vscode-markdown-languageservice`) and a thin LSP adapter (`vscode-markdown-languageserver`) — a real, shipping instance of exactly the "protocol-independent language service vs LSP adapter" architectural split the map's standing constraints call for ("LSP protocol DTOs and client-specific concerns should remain at the protocol boundary").

## Findings

**Full capability list** (`IMdLanguageService`, `src/index.ts:47-230`)
- Document Links, Document Symbols, Workspace Symbols, Folding Ranges, Selection Ranges, Completions (relative-file paths, reference-link labels, header anchors), References (symbol + file), Definitions, Rename (symbol + file), Code Actions (link-definition organization/extraction), Document Highlights, Hover, Diagnostics.
- **Notable, deliberate omissions**: **no Semantic Tokens, no CodeLens** at all — directly informs ticket 27's semantic-tokens/CodeLens scope question: the closest canonical generic-Markdown precedent doesn't bother with either.

**Library/adapter split** — confirms the architecture precedent
- `vscode-markdown-languageservice` (`IMdLanguageService`): pure computation over parsed document state + config, zero connection/protocol awareness.
- `vscode-markdown-languageserver` (`server.ts`): owns `connection.onInitialize`, config sync, `documents.get()` document sync, and routes each LSP RPC method straight into the library's corresponding method (e.g. `connection.onFoldingRanges` → `getFoldingRanges`).

**Folding ranges** (`languageFeatures/folding.ts:25-37`)
- Token-based (markdown-it-style tokens): scans for open/close token pairs (`fence`, `list_item_open`, `table_open`, `blockquote_open`) plus HTML region markers (`<!-- #region -->`), and separately folds heading sections via a `MdTableOfContentsProvider`.

**Selection ranges** (`languageFeatures/smartSelect.ts:41-55`)
- Three-tier nested hierarchy, narrowest-first: `inlineRange` (e.g. link boundaries) → `blockRange` (containing list item/quote) → `headerRange` (containing ToC section); returns `inlineRange ?? blockRange ?? headerRange`.

**Diagnostic model** (`languageFeatures/diagnostics.ts:1-36`)
- `DiagnosticOptions` maps each of six independently-configurable checks to a `DiagnosticLevel` (`ignore`/`hint`/`warning`/`error`): `validateReferences`, `validateFragmentLinks` (same-file anchors), `validateFileLinks`, `validateMarkdownFileLinkFragments` (anchors in other files), `validateUnusedLinkDefinitions`, `validateDuplicateLinkDefinitions`.

**Link/path completion & validation** (`languageFeatures/pathCompletions.ts:150-205`, `diagnostics.ts`)
- Completion kinds: `CompletionItemKind.Reference` (reference-link labels/anchors), `Value` (HTML element IDs), `File`/`Folder` (relative paths); selecting a folder injects `editor.action.triggerSuggest` to immediately re-trigger completion for its contents.
- Validation: file links checked against an internal stat cache; fragment/anchor links checked against `MdTableOfContentsProvider` output for the target document.

**Wikilink/PKM support**: **none** — strictly bracket+paren or reference-definition syntax, no `[[...]]` handling anywhere. Confirms this is a pure generic-Markdown baseline, not a PKM precedent at all — Traces must add 100% of PKM semantics on top.

**Source-range representation** (`types/textDocument.ts`)
- Standard `vscode-languageserver-textdocument`; **UTF-16 code units**, matching plain LSP defaults. No `positionEncoding` negotiation (no UTF-8 opt-in) — this is the "client that doesn't negotiate" baseline case ticket 11/29's UTF-8/UTF-16 decision should be tested against.

## Key takeaway for the map

This is the concrete "at minimum, match this" bar for ticket 27 (structural/generic intelligence): folding, selection ranges, document/workspace symbols, document links, hover, definition/references/rename, and a configurable multi-level diagnostic model for link/anchor validity — with semantic tokens and CodeLens explicitly out of scope even in Microsoft's own canonical implementation, which should carry real weight against adding them speculatively in ticket 27. Its diagnostic-severity-per-check config model (`DiagnosticOptions`) is a clean, directly-adoptable shape for ticket 25's diagnostics-configuration design.
