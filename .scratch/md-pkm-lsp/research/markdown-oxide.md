# Research: Markdown Oxide capabilities & architecture

Resolves ticket [01-research-markdown-oxide](../issues/01-research-markdown-oxide.md).

Sources: `docs/digests/lsp_feel-ix-343-markdown-oxide-digest.txt` (full repo, incl. `src/main.rs`, `src/vault/mod.rs`, `src/gotodef.rs`, `src/diagnostics.rs`, `src/commands.rs`, `src/daily.rs`, `src/completion/*.rs`, `src/rename.rs`, `Cargo.toml`), `docs/digests/lsp_feel-ix-343-markdown-oxide-docs-digest.txt` (`Features Index.md`).

## Overview

Markdown Oxide is a PKM language server written in Rust on `tower-lsp` (a custom fork, `{ git = "https://github.com/Feel-ix-343/tower-lsp" }`) over `tokio`. It takes an eager, **regex-based approach with no formal AST** — everything is extracted via `once_cell::sync::Lazy` regexes over raw text, not a markdown parser like pulldown-cmark. Indexing is parallelized with `rayon`. The workspace model is purely in-memory (no persistence). Ambiguity resolution is explicitly deferred to the LSP client rather than resolved server-side.

**Project-health caveat (added 2026-09-03, web-verified)**: as of September 2025 the creator (Feel-ix-343) publicly requested a maintainer handoff, citing limited personal time for further development. Confirmed the `tower-lsp` (custom-fork) dependency independently via web search, not just the local digest — still accurate, not stale. Weigh this: Markdown Oxide is the most PKM-specific of the researched precedents, but it is also a smaller (~1.6k GitHub stars vs. Marksman's ~3.3k), single-maintainer-dependent project currently between active maintainers — its specific design choices (regex-only parsing, in-memory-only, ambiguity-deferred-to-client) are useful product-capability signal, not evidence of a battle-tested, actively-refined architecture the way Marksman's more broadly-adopted, more conventionally-parsed approach is.

## Findings

**LSP capabilities & triggers** (`src/main.rs`, `impl LanguageServer for Backend`)
- Full text sync, completion (triggers `[`, ` `, `(`, `#`, `>`), inlay hints, definition, references, rename, hover, document symbols, workspace symbols, code actions, semantic tokens (decorators/comments/declarations/deprecated), code lenses.
- File operations (`did_create`/`did_rename`/`did_delete`) registered for `**/*.md`.
- Custom `workspace/executeCommand` commands: `apply_edits`, `jump`, `moxide.findReferences`, and natural-language relative-date shorthands (`tomorrow`, `today`, `yesterday`, `last friday`, `monday`, ...).

**PKM semantics** (`src/vault/mod.rs`, `src/gotodef.rs`, `src/diagnostics.rs`)
- Parses standard Markdown links, wikilinks, block references (`^1j239`), headings, and embeds (`!`) via regex into typed structs (`MDFile`, `MDHeading`, `MDIndexedBlock`, ...).
- Ambiguity handling: `goto_definition` returns `Vec<Location>` for every match when a link resolves to multiple identical headings/aliases — disambiguation is left to the client's peek/picker UI, not resolved server-side.
- Unresolved targets: don't block indexing; gathered on `did_open`/`did_change`, emitted as `INFORMATION`-severity diagnostics (gated by an `unresolved_diagnostics` setting). Code actions offer "Create file" / "Append heading" for unresolved targets.

**Tags & hierarchical tags** (`src/vault/mod.rs`, `src/rename.rs`, `src/completion/tag_completer.rs`)
- **Split semantics, deliberately asymmetric**: rename and find-references use `Referenceable::matches_reference`, a hierarchical prefix split on `/` (renaming `#a/b` → `newtag` cascades `#a/b/c` → `#newtag/c` via `replacen`). Go-to-definition uses `Reference::references`, an **exact string match** only (`#a/b` → only exact `#a/b` occurrences, not the whole subtree).

**Daily notes & date shorthands** (`src/commands.rs`, `src/daily.rs`)
- Natural-language commands (`today`, `tomorrow`, `last tuesday`) dispatch via `workspace/executeCommand` → `commands::jump`. Uses `chrono` + `fuzzydate`; `parse_relative_directive` handles explicit directives (`prev`, `next`, `+7`, `-3`). Resolves against `settings.dailynote` format string; creates/opens the target file if absent.

**Workspace/indexing model** (`src/vault/mod.rs`, `construct_vault`)
- Eager `WalkDir` at startup (skips `.` and `logseq` dirs by default), parsed in parallel via `rayon::par_iter`, kept **entirely in-memory** (`Vault` struct: `MDFile` + `ropey::Rope` in hash maps). **No on-disk cache/persistence.** Hard cap `MAX_INDEXED_LINES = 10_000` per file.

**Incremental updates & buffer precedence** (`src/main.rs`)
- `did_change` → `update_vault` partially mutates just that file's in-memory index. An external filesystem change (`did_change_watched_files`) → `reconstruct_vault`, which **drops the entire vault and rebuilds from disk** via `std::fs::read_to_string` on all watched files — i.e. filesystem changes fully override in-memory unsaved-buffer state on that trigger. **Documented vs. actual discrepancy**: user docs describe the resulting stale-block-completion symptom ("type `:wall`... it should be resolved") but don't name the underlying cause (a full vault rebuild wiping other buffers' pending index state).

**Concurrency/cancellation**
- `tokio` for network I/O, `rayon` for CPU-heavy indexing/symbol work. **No explicit cancellation-token usage** in handlers — relies entirely on `tower-lsp`'s default "drop the future" behavior.

**Parsing architecture** (`src/vault/parsing.rs`)
- No formal AST/parser (not pulldown-cmark). Regex-only extraction; `ropey::Rope` converts byte offsets from regex matches into line/character LSP `Range`s (wrapped as `MyRange`).

**Completion-context detection** (`src/completion/mod.rs`)
- Chain-of-responsibility: `get_completions` tries a sequence of completers (`UnindexedBlockCompleter`, `WikiLinkCompleter`, `TagCompleter`, ...) via `.or_else()`; each does a regex test against `line_to_cursor` text for its own trigger pattern (e.g., "ends in `[[`").

**Documented deliberate scope exclusions** (`Features Index.md`)
- No subheading-chain completion (`[[file#heading#subheading]]`, only one level deep). No native Metadata/Dataview/metadata-tag completions. No semantic-search/vector-DB unindexed-block linking. No advanced refactors (move heading/selection to new file). No lists/indented-lists in Workspace Symbols.

**Crates**: `tower-lsp` (custom fork), `tokio` 1.34 (full features), `rayon`.

## Key takeaway for the map

Markdown Oxide's regex-only, no-AST, no-persistence, in-memory-only approach is the opposite end of the spectrum from Traces (which already has a real pulldown-cmark-based parser and a persistent redb-backed index) — it is **not** an architectural precedent to imitate, only a **product-capability** precedent (which features exist, exact trigger characters, the ambiguity-deferred-to-client pattern, the exact-match-vs-hierarchical-match split between goto-definition and rename/references for tags). Its buffer-vs-filesystem precedence bug (full-rebuild wipes unsaved state) is a concrete failure mode ticket 14 (live buffer vs filesystem overlay) should explicitly design around, not repeat.
