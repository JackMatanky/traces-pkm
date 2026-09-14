# Research: Obsidian Dataview / Templater / Metadata Menu semantics for feature parity+extension

Resolves ticket [08-research-dataview-templater-metadatamenu-parity](../issues/08-research-dataview-templater-metadatamenu-parity.md).

Sources: `docs/digests/obsidian_blacksmithgu-obsidian-dataview-src-digest.txt`, `obsidian_silentvoid13-templater-src-digest.txt`, `obsidian_mdelobelle-metadatamenu-src-digest.txt`, `obsidian_obsidian-tasks-src-digest.txt`.

## Overview

**None of these four plugins provide true LSP-shaped intelligence** (no inline diagnostics, no hover docs, no go-to-definition) — they're all built on Obsidian's `EditorSuggest` API for context-triggered completion popups and CodeMirror extensions for visual rendering, running inside a single proprietary editor rather than over a general protocol. This is itself the key finding: **there is no existing LSP/editor-assist precedent to adapt for any of the four** — Traces would be inventing the LSP-shaped version of each capability from first principles, informed by (not ported from) these plugins' completion-trigger and validation semantics.

## Findings

**1. Obsidian Dataview**
- DQL grammar: `SOURCE`/`WHERE`/`SORT`/`GROUP BY`/`FLATTEN`, SQL-like — structurally comparable to Traces' existing Query grammar (source selection + filter expressions), though Traces' runs natively (CLI/system-level), not embedded in a proprietary editor runtime.
- Editor assistance: registers a CodeMirror mode (`registerDataviewjsCodeHighlighting`) for `dataviewjs`-block syntax highlighting, plus a toggle for "pretty" inline-field rendering in Live Preview (`prettyRenderInlineFieldsInLivePreview`) — purely visual, not semantic.
- **No DQL autocomplete, no inline error diagnostics for malformed `WHERE` clauses** anywhere in the source — errors are execution-time only. Traces' Query DSL already has `miette`-based span-aware syntax errors (confirmed by QueryArch scout findings) that go further than Dataview does today.

**2. Templater**
- Interactive prompts: `PromptModal`/`SuggesterModal` invoked via `tp.system.prompt`/`tp.system.suggester`, blocking execution for user input — the same interactive-blocking shape as Traces' `DialogProvider`-backed `ui.*` namespace.
- Completion: an `Autocomplete` class extends `EditorSuggest<TpSuggestDocumentation>`, triggered by a `tp\.(?<module>[a-z]*)?(\.(?<fn>[a-zA-Z_.]*)?)?$` regex — i.e. member-completion after typing `tp.` or `tp.module.`. Directly analogous to what ticket 22 (template-language intelligence) needs to build for Traces' `ui.`/`file.`/`date.`/`query.`/`tasks.`/`schema.` namespaces.
- Namespace mapping: Templater's `tp.file`/`tp.date`/`tp.system`/`tp.web`/`tp.frontmatter` line up closely with Traces' existing namespace set — a near-1:1 precedent for what member names/functions ticket 22's completion metadata table needs to cover, though Traces has no `tp.web`-equivalent (network fetch) and Templater has no `query`/`schema`-equivalent.
- **No LSP precedent** for hover-on-function or pre-execution "undefined variable" diagnostics — Templater parses TSDoc comments only to power its own completion popup, not to statically validate a template before running it.

**3. Metadata Menu**
- `ValueSuggest` (extends `EditorSuggest<IValueCompletion>`) triggers inside frontmatter or inline Dataview-style fields, offering field-value completion scoped by the field's type as declared on the note's `fileClass`.
- `fileClass` inheritance closely mirrors Traces' Schema module (confirmed against `src/schema/graph/` Kahn's-topo-sort `extends` resolution) — this is the strongest direct structural precedent of the four for ticket 20 (schema/file-class-aware intelligence): field-value completion scoped by resolved-schema type is exactly Metadata Menu's core feature, just needing an LSP-shaped (not Obsidian-modal-shaped) delivery.
- **No LSP precedent** for inline validation squiggles — Metadata Menu validates fields via **modals and settings-menu UI**, never live in-editor diagnostics. Confirms ticket 20's runtime-validator decision would be genuinely new territory, not a port of an existing pattern.

**4. obsidian-tasks**
- `EditorSuggestorPopup` (extends `EditorSuggest`) offers attribute-completion while typing a recognized task line; supports **both** emoji-shorthand (`📅`, `🔁`) and Dataview-bracket (`[due:: ...]`) metadata formats via pluggable `TaskSerializer` implementations (`DefaultTaskSerializer`/`DataviewTaskSerializer`), each with its own `buildSuggestions` function.
- Directly relevant to ticket 23 (task/PKM semantics) and ticket 18 (date shorthands): confirms Traces' existing emoji-marker date-shorthand support (per `src/note/CONTEXT.md`) is following an established, dual-format convention, and that a **pluggable serializer-per-format** shape (rather than one hardcoded parser) is the proven way to support it.
- **No LSP precedent** for hover-on-task-attribute or real-time malformed-task-query diagnostics — same pattern as the other three, execution/render-time only.

## Key takeaway for the map

Across all four plugins, the consistent finding is: **rich completion-trigger-detection precedent, zero diagnostics/hover/definition precedent.** This should calibrate ticket 20/21/22/24's ambition honestly — Traces isn't playing catch-up to some existing Obsidian-ecosystem LSP-equivalent feature (none exists), it would be the **first** system to bring hover/diagnostics/definition-grade intelligence to Dataview-, Templater-, and Metadata-Menu-shaped languages at all. That's consistent with the destination's explicit goal of going *beyond* the Obsidian-plugin ecosystem, not just matching it, but it also means there's more genuine design work (not just porting) required in tickets 20-22 than in the wikilink/tag tickets (15/16), where Markdown Oxide and Marksman already provide real LSP-shaped precedent to adapt.
