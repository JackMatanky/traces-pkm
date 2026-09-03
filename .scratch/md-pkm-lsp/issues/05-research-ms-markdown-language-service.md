# Research: Microsoft markdown-language-service/server generic capability baseline

Type: research
Status: resolved

## Question

No local digest exists for this one — research directly from primary sources: https://github.com/microsoft/vscode-markdown-languageservice and https://github.com/microsoft/vscode-markdown-languageserver. Use `read` on specific files/READMEs in those repos (not a browser) and `web_search` only to locate the right files/releases.

This is the reference for **generic** (non-PKM) Markdown language-service capability — the product goal requires Traces to "cover at least the valuable generic Markdown language-service capabilities" of the ecosystem, and this project is the closest thing to a canonical implementation (it's what VS Code's built-in Markdown support uses). Establish and cite for each:

- Full capability list: folding ranges, selection ranges, document symbols/outline, workspace symbols, document links, hover, definition, references, rename, completion (path completion, reference-link completion, heading-anchor completion), diagnostics (broken links, duplicate headings, etc.), code actions, semantic tokens (does it define any?), CodeLens (reference-count lens?).
- Which of these are in `vscode-markdown-languageservice` (the reusable library) vs only wired up in `vscode-markdown-languageserver` (the actual LSP server) — this split is itself a relevant architectural precedent (protocol-independent language service vs LSP adapter).
- How it computes folding/selection ranges for Markdown structure (heading nesting, list nesting, code fences).
- Its diagnostic severity/configuration model (e.g. configurable diagnostic levels for broken links).
- Its link/path completion and validation logic for both relative file links and header anchors.
- Any wikilink or PKM-specific support (expect none/minimal — confirm and note the boundary precisely).
- Source-range/position representation used internally (UTF-16 vs UTF-8 offsets — relevant since LSP wire protocol is UTF-16 by default).

Write findings to `.scratch/md-pkm-lsp/research/ms-markdown-language-service.md`, citing each claim's source file/URL.

## Answer

Canonical generic-Markdown baseline (what VS Code uses): folding/selection-ranges/symbols/document-links/hover/definition/references/rename/completion/diagnostics, but explicitly NO semantic tokens and NO CodeLens even in Microsoft's own implementation. Clean protocol-agnostic-library vs LSP-adapter split (IMdLanguageService vs server.ts) is a real, shipping precedent for that architecture. Zero wikilink/PKM support — pure generic baseline. UTF-16 positions, no positionEncoding negotiation.

Full findings: [`research/ms-markdown-language-service.md`](../research/ms-markdown-language-service.md)
