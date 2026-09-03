# Research: Marksman capabilities & architecture

Type: research
Status: resolved

## Question

Investigate Marksman against the local corpus (`docs/digests/lsp_artempyanykh-marksman-digest.txt` full repo, `-docs-digest.txt`) and, where stale/incomplete, https://github.com/artempyanykh/marksman and https://github.com/artempyanykh/marksman/blob/main/docs/features.md.

Establish and cite for each:

- Full LSP capability list (completion, definition, references, rename, diagnostics, and anything else).
- Link/reference model: Markdown links, reference-style links, wikilinks (`[[note]]`, `[[note#heading]]`, `[[#heading]]`), and how each resolves.
- Heading semantics and the specific diagnostics Marksman raises for broken references and duplicate/ambiguous headings (this is a documented differentiator vs Markdown Oxide worth precise capture).
- Workspace/indexing model and incremental update strategy.
- Open-buffer vs filesystem behavior.
- Parsing architecture (parser used, span/range tracking).
- Client capability handling / capability negotiation approach.
- Concurrency approach (F# async? threading model?).
- Documented scope exclusions.

Write findings to `.scratch/md-pkm-lsp/research/marksman.md`, citing each claim's source. Note documentation/source discrepancies explicitly.

## Answer

F#/Markdig-based, actor-model (MailboxProcessor) concurrency, full re-parse per edited document with incremental workspace-index updates only. Wikilink resolution strategy (title vs filename) is user-configurable and changes rename outcomes. Duplicate-heading diagnostic is documented but not actually implemented standalone — only surfaces via AmbiguousLink's RelatedInformation.

Full findings: [`research/marksman.md`](../research/marksman.md)
