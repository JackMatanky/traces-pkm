# Research: zk capabilities & architecture (daily notes, tags, notebook model)

Type: research
Status: resolved

## Question

Investigate zk against the local corpus (`docs/digests/zk-digest.txt` full repo, `-src-digest.txt`, `-docs-digest.txt`, particularly `docs/config/config-lsp.md`, `docs/notes/note-id.md`, `docs/notes/note-frontmatter.md`, `docs/notes/tags.md`, `docs/tips/daily-journal.md`) and, where stale, https://github.com/zk-org/zk and https://zk-org.github.io/zk/.

Establish and cite for each:

- LSP capabilities zk exposes (`config-lsp.md`) and how they're configured.
- Daily-note / date-based note creation and reference conventions (note-id generation, date placeholders, how a "daily note link" is recognized/resolved if at all in the LSP).
- Frontmatter conventions (note-frontmatter.md) and how they inform link/tag/metadata resolution.
- Tag model (tags.md) — flat vs hierarchical, syntax.
- Notebook/workspace model (notebook.md) — is it single-root like Traces' Project Root, or does it support something closer to multi-root?
- Note-filtering/search architecture — is there a query-like DSL comparable to Traces' Query DSL, and if so how does it resolve sources/filters?
- Any incremental indexing or caching strategy documented.
- Architecture notes on concurrency (zk is Go-based — goroutines/channels) only to the extent they inform a *product* decision (not to imitate Go idioms in Rust).

Write findings to `.scratch/md-pkm-lsp/research/zk.md`, citing each claim's source.

## Answer

Single-root "Notebook" model, no multi-root/nesting. No dedicated daily-note feature — composed from generic template groups + date-placeholder filenames + CLI aliases. Flat tag model with glob-matched slash-hierarchy simulation, not first-class hierarchical tags. SQLite-backed incremental indexing (diff-based, syncs with unsaved-buffer edits) is the closest precedent to Traces' own IndexDelta approach.

Full findings: [`research/zk.md`](../research/zk.md)
