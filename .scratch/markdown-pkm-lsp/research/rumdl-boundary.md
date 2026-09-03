# Research: rumdl capability boundary and coexistence hooks

Resolves ticket [03-research-rumdl-boundary](../issues/03-research-rumdl-boundary.md).

Sources: `docs/digests/lsp_rvben-rumdl-docs-digest.txt` (`docs/lsp.md` lines 1885-2158, rule-comparison tables), `docs/digests/lsp_rvben-rumdl-src-digest.txt`.

## Overview

rumdl is a fast, highly-configurable Markdown linter/formatter with a built-in LSP server (`rumdl server`) whose link/navigation/outline features are **designed from the start to be granularly disabled** so it can run alongside a dedicated PKM/navigation LSP. It explicitly, permanently ignores wikilinks for linting purposes — there is no overlap to resolve there, only for standard Markdown links and document outline/symbols.

## Findings

**Linting rule catalogue scope** (`lsp_rvben-rumdl-docs-digest.txt:15789`)
- 83 built-in rules (some opt-in via config); markdownlint-compatible/superset. Owns: heading-level rules (`MD001`, `MD041`), line length (`MD013`), list spacing/markers (`MD030`), trailing whitespace (`MD009`), and similar structural rules — **a PKM LSP must not duplicate these** (would produce redundant diagnostics on the same line).

**Full LSP settings surface** (`lsp_rvben-rumdl-docs-digest.txt:2118-2158`)
| Setting | Default | Effect |
|---|---|---|
| `enableLinting` | `true` | Real-time diagnostics |
| `enableAutoFix` | `false` | Apply auto-fixes on save |
| `enableLinkCompletions` | `true` | File-path/heading-anchor completion inside **standard** link targets |
| `enableLinkNavigation` | `true` | Hover/definition/references/rename for **standard** links |
| **`enableSymbols`** | `true` | Document outline (`documentSymbol`) + workspace heading search (`workspace/symbol`) — **"set to `false` to avoid duplicate headings when another LSP provides the outline"** |
| `linkCompletionContentRoots` | `[]` | Roots for absolute-style link completion |
| `configPath` | auto | Explicit `.rumdl.toml` path |
| `disableRules`/`enableRules` | from config | Rule overrides |
| `settings` | from config | Per-rule option overrides |

Documented Neovim precedent for full delegation (`lsp_rvben-rumdl-docs-digest.txt:2142-2158`):
```lua
vim.lsp.config("rumdl", {
  cmd = { "rumdl", "server" },
  filetypes = { "markdown" },
  root_markers = { ".git", ".rumdl.toml" },
  init_options = {
    enableLinkCompletions = false,
    enableLinkNavigation = false,
    enableSymbols = false,
  },
})
```
This is the **exact three-setting combination** a Traces+rumdl joint setup should recommend disabling — not just the two (`enableLinkCompletions`/`enableLinkNavigation`) found in the earlier pass.

**Wikilinks: intentionally, permanently out of rumdl's link-checking scope** (`lsp_rvben-rumdl-docs-digest.txt:9979-9981`)
> "Wikilinks and wiki embeds (`[[page]]`, `![[image.png]]`) are not checked. They name a vault entry rather than a path relative to the file that holds them, so the tool that renders them resolves the name itself."

This means the `enableLinkCompletions`/`enableLinkNavigation` toggles are really about rumdl's **standard-Markdown-link** completion/navigation only — wikilinks were never rumdl's territory in the first place, no toggle needed there. Confirmed: rumdl does understand wikilink *syntax* when `flavor = "obsidian"` is set (for formatting/linting purposes, e.g. `MD039` trimming), but never resolves/navigates/completes them.

**Link diagnostics are NOT gated by the LSP toggles** — separate concern
- `enableLinkCompletions`/`enableLinkNavigation` only affect completion/navigation *capabilities*; rumdl's own link-validity lint rules (`MD057` relative-link validation, `MD051` anchor validation) keep running regardless — "Linting, formatting, and code actions are unaffected." To silence rumdl's own broken-link diagnostics (if Traces claims that territory too, e.g. for standard Markdown links), a **separate** `disableRules`/`.rumdl.toml` rule toggle is needed, not the LSP settings.

**Formatting/fix overlap risk**
- `MD039` (link-spacing trim) explicitly **exempts wikilinks** ("a wikilink has no destination to rewrite the text against") — zero formatting overlap risk for wikilinks. Standard-link formatting (space-trimming, etc.) is rumdl's alone; Traces should register **zero** `documentFormattingProvider`/`documentRangeFormattingProvider` capability (confirms the product goal's clean split).

**Workspace-index model**
- The LSP server (not just the CLI) maintains its own `WorkspaceIndex` (paths + headings) for its own link/heading-anchor completion, updated via `workspace/didChangeWatchedFiles` and `textDocument/didChange` — i.e. rumdl runs a **second**, independent file index in parallel to Traces' own `FileIndex` when both are active; no shared-index opportunity exists (different processes, different languages/runtimes).

**Config interop**: none documented. rumdl resolves `.rumdl.toml`/`rumdl.toml`/`.config/rumdl.toml`/`pyproject.toml`/`.markdownlint.*` independently of `.traces/config.toml`; a joint project needs both config files maintained separately (or `configPath` passed explicitly via LSP `init_options`).

**Registration/installation precedent**: `rumdl server` (stdio), `--config`, `--verbose`, `--port` (TCP debug mode) — direct precedent for ticket 34's binary/subcommand shape decision.

## Key takeaway for the map

The three-way `enableLinkCompletions`/`enableLinkNavigation`/`enableSymbols` toggle set (not just two) is rumdl's complete, intentional coexistence contract — ticket 32 should recommend disabling all three, and ticket 27 (structural intelligence) now has a concrete decision point: rumdl's `documentSymbol`/`workspace/symbol` already provide a heading outline, explicitly designed to be ceded to "another LSP" via `enableSymbols`, so Traces claiming document/workspace symbols is not competing with an accidental capability — it's filling a gap rumdl leaves open by design. Link-*diagnostics* (MD057/MD051) are a separate, NOT-toggle-gated concern ticket 25 must account for explicitly (rumdl's standard-link-broken diagnostics keep firing even with navigation/completion disabled, unless separately turned off via lint-rule config) — Traces should probably NOT also diagnose standard (non-wiki) broken links, ceding that fully to rumdl's `MD057`, and own only wikilink/PKM-reference diagnostics, which rumdl structurally cannot see.
