# Research: Markdown Oxide capabilities & architecture

Type: research
Status: resolved

## Question

Investigate Markdown Oxide against the local corpus (`docs/digests/lsp_feel-ix-343-markdown-oxide-digest.txt` full repo, `-src-digest.txt`, `-docs-digest.txt`) and, where the local corpus is stale/incomplete, https://github.com/Feel-ix-343/markdown-oxide and https://oxide.md/.

Establish and cite (file/section) for each:

- Full LSP capability list actually implemented (not just advertised) and their trigger contexts.
- PKM-specific semantics: wikilink/alias/heading-ref/block-ref/embed/backlink binding and resolution rules, including ambiguity handling (duplicate headings/aliases) and unresolved-target behavior.
- Tag and hierarchical-tag semantics (completion, rename, references).
- Daily-note / date-shorthand handling, if any.
- Workspace/indexing model: eager vs lazy, in-memory structure, on-disk persistence/caching (if any).
- Incremental update strategy: how edits to open buffers vs filesystem changes are reconciled.
- Open-buffer vs filesystem-truth precedence.
- Concurrency/cancellation approach (async runtime used, request cancellation support).
- Parsing architecture (which markdown parser, whether source ranges/spans are tracked per semantic entity, how).
- Completion-context detection technique (how it decides "inside a wikilink target" vs "inside body text").
- Diagnostic architecture (what gets flagged, e.g. broken links, duplicate headings).
- Deliberate scope exclusions the maintainers documented (things they decided NOT to do, and why, if stated).
- Rust crates used for the LSP framework/JSON-RPC transport (crate name + version if visible in Cargo.toml within the digest).

Write findings to `.scratch/md-pkm-lsp/research/markdown-oxide.md`, citing each claim's source (digest file + line range, or upstream URL). Where documentation and source code disagree, record the discrepancy explicitly.

## Answer

Regex-only (no AST), tower-lsp/tokio, rayon-parallel eager in-memory index, no persistence. Ambiguity deferred to client (Vec<Location>). Tag rename/references use hierarchical prefix match; go-to-definition uses exact match only — a deliberate split. Filesystem-watch rebuilds fully replace in-memory state, overriding unsaved buffers (a precedent to avoid, not follow).

Full findings: [`research/markdown-oxide.md`](../research/markdown-oxide.md)
