# Footnotes & callouts

Type: grilling
Blocked by: 01, 02, 05

## Question

Two smaller, related extended-Markdown-syntax scope decisions, informed by Markdown Oxide/Marksman (tickets 01, 02) and the generic-Markdown baseline (ticket 05, since footnotes are closer to "generic Markdown extension" than PKM-specific).

Footnotes (`[^1]` / `[^1]: definition`):
- Does Traces' note parser recognize footnote syntax at all today (check `src/note/` — not covered by the earlier scout pass; verify directly).
- If in scope: definition/references/hover linking a footnote reference to its definition, diagnostics for undefined/unused footnotes, rename.

Callouts (`> [!NOTE]`, `> [!WARNING]`, etc., an Obsidian/GFM-alert-style blockquote extension):
- Does the parser recognize callout syntax today.
- If in scope: completion for callout-type keywords, diagnostics for unrecognized callout types (against a fixed or configurable vocabulary), folding-range behavior for callout bodies (feeds into ticket 27).

Decide scope (in/out) and depth for both independently — they don't need to share an implementation approach, just a shared research grounding.
