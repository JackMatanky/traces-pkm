# Research: Marksman capabilities & architecture

Resolves ticket [02-research-marksman](../issues/02-research-marksman.md).

Sources: `docs/digests/lsp_artempyanykh-marksman-digest.txt` (full repo, incl. `Server.fs`, `Diag.fs`, `Folder.fs`, `Index.fs`, `Doc.fs`, `Parser.fs`, `PatchedLinkInlineParser.cs`), `docs/digests/lsp_artempyanykh-marksman-docs-digest.txt` (`docs/features.md`, `README.md`).

## Overview

Marksman is an F# LSP server built on the Markdig parser (patched for wikilinks), using an Actor-model concurrency approach (`MailboxProcessor`) and an in-memory workspace suffix-tree index for cross-document resolution. It has deep cross-file intelligence but performs a **full re-parse per edited document** — incrementality exists only at the workspace-index level (add/remove one document's entry), never inside a single document's AST.

## Findings

**LSP capabilities** (`Server.fs:11145-11200`)
- `WorkspaceSymbolProvider`, `DocumentSymbolProvider`, `CompletionProvider` (triggers `[`, `#`, `(`), `DefinitionProvider`, `HoverProvider`, `ReferencesProvider`, `CodeActionProvider`, `SemanticTokensProvider` (full + range), `RenameProvider`, `CodeLensProvider`, configurable `TextDocumentSync` (Incremental or Full).

**Link/reference model** (`docs/features.md`, `README.md`)
- Standard inline links, internal anchors, reference-style links (`[ref]: /url "Title"`), wikilinks `[[note]]` / `[[note#heading]]` / `[[#heading]]`.
- **Configurable resolution strategy**: `core.title_from_heading` + `completion.wiki.style` (`title-slug` vs `file-stem`) — a wikilink can resolve against the target's `# Title` heading text *or* its filename, and this choice changes rename-refactor outcomes.

**Heading semantics & diagnostics** (`Diag.fs:6645-6775`)
- Level-1 heading = document title by default.
- `BrokenLink` for unresolved link/wikilink targets; `AmbiguousLink` for multi-destination matches; `NonBreakableWhitespace` for NBSP-corrupted heading syntax.
- **Documentation/source discrepancy**: `docs/features.md` claims a "duplicate/ambiguous headings" diagnostic, but there's **no standalone duplicate-heading check** in `Diag.fs` — duplicates only surface as `RelatedInformation` ("Duplicate definition of...") attached to an `AmbiguousLink` diagnostic when something actually tries to resolve to them, i.e. an unreferenced duplicate heading produces no diagnostic at all.

**Workspace/indexing model** (`Folder.fs:7215-7272`, `Index.fs:8035-8100`)
- `SuffixTree<CanonDocPath, Doc>` (paths) + `docsBySlug: Map<Slug, Set<Doc>>` (titles) per folder. Workspace-level add/remove is incremental (`withDoc`/`withoutDoc`); per-document catalog (headings/wikilinks/mdLinks/tags) is rebuilt whole on each edit.

**Open-buffer vs filesystem** (`Server.fs:11638-11660`, `Folder.fs:7461-7524`)
- Single-file mode: opening a file with no `.git`/`.marksman.toml` root creates an isolated `Folder` for just that buffer.
- Workspace mode: eager scan respecting `.gitignore`/`.ignore`. Open buffers mutate in-memory `State` directly on `didChange`; a rename is `didClose(old)` + `didOpen(new)`.

**Parsing architecture** (`Parser.fs:9072-9120`, `PatchedLinkInlineParser.cs`)
- Markdig (C#) + F# CST mapping + custom patches (`WikiLinkInline`) for wikilink syntax. Spans via Markdig's `SourceSpan`, translated to LSP `Range` via a line map. **Fully re-parses on every edit** (`Doc.fs:6917-6930`, `Doc.withText` feeds the whole buffer to `Parser.parse` then `Index.ofCst` — confirmed no incremental patching despite the elaborate index structures).

**Client capability negotiation** (`Server.fs:11180-11200`)
- Inspects `InitializeParams`' `ClientDescription`: toggles `WorkspaceSymbolProvider`/`DocumentSymbolProvider` based on `not clientDesc.IsVSCode`, and configures `RenameProvider` based on `clientDesc.SupportsPrepareRename`.

**Concurrency** (`Server.fs:11260-11455`)
- F# `MailboxProcessor` actor pattern: a `BackgroundAgent` for diagnostics, a `StatusAgent` for telemetry, a state mailbox sequentially applying reads/mutations — no explicit mutexes.

**Documented scope exclusions** (`docs/features.md:258-263`)
- Images (diagnostics/completion/goto) marked "planned", not implemented. Jupyter notebooks planned. Standalone `check`/`build` CLI commands planned.

## Key takeaway for the map

Marksman's `BrokenLink`/`AmbiguousLink` diagnostic model (ambiguity surfaced only reactively, via `RelatedInformation` on an actual reference attempt, never as a standing diagnostic on the duplicate itself) is a concrete, lower-cost alternative to Markdown Oxide's "always show unresolved diagnostics" approach worth weighing in ticket 15/25. Its title-vs-filename configurable wikilink-resolution strategy is directly relevant to ticket 07/15's binding-rule decision. Its "full re-parse per edit, incremental only at the document-index level" is the same shape Traces already has today (per NoteParsing scout findings) — not a new idea, a confirmation that this is an accepted, workable middle ground even in a mature, widely-used PKM LSP.
