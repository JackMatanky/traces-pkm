# Structural/editor intelligence: symbols, folding, selection ranges, document links, CodeLens, code actions, semantic tokens

Type: grilling
Blocked by: 05, 15

## Question

Grounded by the Microsoft markdown-language-service research (ticket 05) and the link model (ticket 15, for document-link ranges). **Caveat on ticket 05's evidentiary weight**: it is the map's *only* researched generic-Markdown-LSP precedent — no second implementation (e.g. a CommonMark-reference LSP, or a generic Markdown mode in a different editor's built-in tooling) was researched to cross-check it. Treat "even Microsoft's own implementation doesn't have semantic tokens/CodeLens" as one data point about one team's product choices, not as a normative ceiling on what Traces' generic-Markdown coverage should be — the destination asks for "the valuable generic Markdown language-service capabilities," decided on Traces' own users' needs, not for parity-with-a-cap against whichever single reference got researched first. Decide, for each, in/out-of-scope and depth:

- Document symbols / outline: heading hierarchy (requires headings as first-class AST nodes — coordinate with ticket 11/15's decision to retain them) plus, optionally, task and File-Class-field structure.
- Folding ranges: heading sections, list nesting, code fences, callouts (if in scope per ticket 17), frontmatter block.
- Selection ranges: nested-selection expansion through list items, headings sections, inline spans.
- Document links (`textDocument/documentLink`): every resolvable link/embed target as a clickable range — decide overlap/non-overlap with rumdl, which already provides file-path completion but check ticket 03 for whether it also registers `documentLink` (if so, this needs the same coexistence-toggle treatment as link completion/navigation).
- CodeLens: reference-count lens above headings/notes ("3 references"), informed by whether Marksman/Markdown Oxide do this (tickets 01/02) — decide value vs cost (requires backlink computation eagerly for every visible heading).
- Code actions beyond quick-fixes already covered elsewhere: e.g. "convert Markdown link to wikilink" or vice versa, "extract selection to new note".
- Semantic tokens: decide whether Traces defines any at all —ground this against whether TextMate/tree-sitter grammar-based client-side highlighting already covers Markdown/wikilink syntax adequately (likely yes, semantic tokens add limited value for prose-heavy Markdown vs code) versus a specific case semantic tokens would uniquely enable (e.g. distinguishing a *resolved* vs *unresolved* wikilink by token type/modifier, which syntax highlighting alone cannot do).
