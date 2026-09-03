# Research: rumdl capability boundary and coexistence hooks

Type: research
Status: resolved

## Question

Investigate rumdl's actual capability boundary against the local corpus (`docs/digests/lsp_rvben-rumdl-docs-digest.txt`, `-src-digest.txt`, and the full `-digest.txt` only if source/docs are insufficient) and, where stale, https://github.com/rvben/rumdl.

A first pass already read `docs/digests/lsp_rvben-rumdl-docs-digest.txt:1885-2053` (the `docs/lsp.md` file) and found: rumdl ships its own built-in LSP server (`rumdl server`) providing diagnostics, code actions (quick fixes), document/range formatting, fenced-code-block language completion, and **file-path + heading-anchor link completion plus hover/go-to-definition/find-references/rename for Markdown links** — with an explicit `enableLinkCompletions` / `enableLinkNavigation` LSP setting pair documented specifically for disabling rumdl's own link intelligence in favor of "another language server (for example a PKM/notes LSP)". This is the concrete coexistence hook Traces should design against. Confirm/deepen this and answer:

- Full linting rule catalogue scope (the MD001–MDxxx rule set) — is it markdownlint-compatible/superset? What structural issues does it flag that a PKM LSP must NOT duplicate (e.g. heading structure, line length)?
- Exact LSP settings surface (`docs/lsp.md` "LSP settings" section — read the section following what was already captured) —ll settings names, defaults, and what each toggles.
- Whether rumdl's link model understands wikilinks at all, or only standard Markdown links (this determines whether `enableLinkNavigation`/`enableLinkCompletions` fully vacates the wikilink space or only the plain-link space).
- Whether rumdl's diagnostics for links (if any — e.g. broken-link lint rules) are also gated by these settings, or are a separate lint-rule toggle (config file vs LSP setting).
- rumdl's own workspace-index model for its link/heading-anchor completion (how it discovers files/headings) — does it watch the filesystem, and at what cost, since Traces will run its own index in parallel.
- Formatting/fix ownership: does rumdl's `fmt`/format-on-save touch anything a PKM LSP might also want to rewrite (e.g. link text normalization)?
- rumdl's config file (`.rumdl.toml`) surface vs Traces' `.traces/config.toml` — any advertised interop or explicit non-interop.
- Installation/registration model (`rumdl server`) so the map can describe a concrete "both servers registered for `markdown` filetype" client configuration.

Write findings to `.scratch/md-pkm-lsp/research/rumdl-boundary.md`, citing each claim's source.

## Answer

rumdl ships enableLinkCompletions/enableLinkNavigation/enableSymbols — three settings, not two — as its explicit coexistence contract for standard-Markdown-link completion/navigation and document/workspace symbols. Wikilinks are permanently, unconditionally out of rumdl's link-checking scope by design (no toggle needed). Link *diagnostics* (MD057/MD051) are NOT gated by those LSP settings — separate lint-rule toggle needed if Traces wants that territory too. Zero formatting overlap (wikilinks exempted from MD039).

Full findings: [`research/rumdl-boundary.md`](../research/rumdl-boundary.md)
