# Research: zk capabilities & architecture (daily notes, tags, notebook model)

Resolves ticket [04-research-zk](../issues/04-research-zk.md).

Sources: `docs/digests/zk-docs-digest.txt` (`docs/config/config-lsp.md`, `docs/notes/note-id.md`, `docs/notes/note-frontmatter.md`, `docs/notes/tags.md`, `docs/notes/note-filtering.md`, `docs/tips/daily-journal.md`), `docs/digests/zk-src-digest.txt` (indexing/diff logic).

## Overview

zk is a Go CLI tool with a built-in LSP, built around a single-root "Notebook" and an embedded SQLite database for near-instant full-text search, link resolution, and tag indexing. It has **no hardcoded daily-note feature** — daily journals are assembled from generic building blocks (note groups, path-scoped templates, date-format placeholders, CLI command aliases), which is a notably different, more compositional approach than Markdown Oxide's dedicated natural-language date-command feature.

## Findings

**LSP capabilities & configuration** (`docs/config/config-lsp.md`)
- Completion for Markdown links (`[[`), hashtags, colon-separated tags. Note creation using current selection as title. Diagnostics for: dead links, missing backlinks, self-links, wiki-title mismatches — severities (`hint`/`warning`/`error`) each independently configurable per diagnostic kind under `[lsp.diagnostics]`. Completion item formatting configurable under `[lsp.completion]`. Custom `workspace/executeCommand` verbs: `zk.index`, `zk.new`, `zk.link`, `zk.list`, `zk.tag.list`.

**Daily notes & date conventions** (`docs/tips/daily-journal.md`, `docs/notes/note-id.md`, `docs/notes/note-filtering.md`)
- No dedicated daily-note concept. Composed from a `[group]` config mapping a directory (e.g. `journal/daily`) to a template with a date-placeholder filename (`{{format-date now '%Y-%m-%d'}}`), typically wired to a CLI alias (`daily = 'zk new --no-input .../journal/daily'`).
- Note IDs: random 4-char alphanumeric (default) or `YYYYMMDDHHMM` timestamp; **no sequential IDs**. Link resolution matches by path-prefix or ID fragment — `[[200911172034]]` resolves to `200911172034-my-note.md` without needing the full filename.

**Frontmatter conventions** (`docs/notes/note-frontmatter.md`)
- Recognized keys: `title` (overrides first heading), `date`, `modified`, `tags`, `keywords` (tag alias), `aliases` (alternate titles usable for link-mention discovery). Date-key names configurable (`[format.markdown.frontmatter]`, e.g. `creation-date-key = "created"`). All keys normalized lowercase; exposed to templating as `{{metadata.<key>}}`.

**Tag model** (`docs/notes/tags.md`, `docs/notes/note-filtering.md`)
- **Flat**, not truly hierarchical — hierarchy is simulated via `/`-separated segments matched with glob patterns (`--tag "year/201*"`), not a first-class hierarchical type. Supports multiple syntaxes simultaneously: `#hashtags`, `:colon:separated:tags:`, Bear-style `#multi-word tags#`, plus YAML frontmatter arrays.

**Notebook/workspace model**
- Strictly **single-root** ("Notebook", marked by a `.zk` directory); notebooks cannot be nested. Subdirectories freely used within one notebook; links/IDs resolve relative to the notebook root. No multi-root concept at all — closer to Traces' single-Project-Root model than to LSP's native multi-root workspaces.

**Note-filtering/search architecture** (`docs/notes/note-filtering.md`)
- CLI-flag-driven structural filters (`--tag`, `--linked-by`, `--created-after`) plus a **Google-style query DSL** for text content (`--match`): boolean `OR`/`NOT`/`AND`, exact-phrase quoting, field scopes (`title:`, `body:`). Backed by three swappable match strategies: SQLite FTS5 (default), exact, regex.

**Incremental indexing/caching** (`zk-src-digest.txt`)
- Embedded SQLite caches note metadata/links/FTS index. `Walk` produces live filesystem metadata, diffed against `IndexedPaths()` from SQLite (`paths.Diff` → add/modify/remove `DiffChange` events) — only changed files get reparsed, directly analogous to Traces' `IndexDelta` merge-join. LSP registers `TextDocumentSyncKindIncremental` to keep SQLite synced with unsaved buffer edits (unlike Markdown Oxide, zk's incrementality reaches into the persisted index, not just an in-memory overlay).
- Concurrency: Go channel/worker-pool pattern (`errgroup.Group` capped at `GOMAXPROCS`) for parallel parsing during the walk — not directly transferable to Rust idiom, but confirms parallelizing the initial-scan parse step is a proven approach other PKM tools use at zk's target scale.

## Key takeaway for the map

zk's single-root Notebook model (no multi-root, no nesting) is the closest precedent to Traces' existing single-Project-Root design and supports treating "one workspace folder = one independent analysis host, no cross-root resolution" (ticket 30's leading hypothesis) as a proven, sufficient product shape rather than an under-ambitious simplification. Its "daily notes are just a template group + date placeholder, no LSP-level special-casing" approach is a strong argument for ticket 18 to resolve toward "no dedicated daily-note semantics in the LSP at all, purely a Template/Config convention" — matching Traces' existing `TemplateVariable` `date` and canonical-frontmatter-roles design rather than adding new LSP-only date-shorthand machinery like Markdown Oxide's.
