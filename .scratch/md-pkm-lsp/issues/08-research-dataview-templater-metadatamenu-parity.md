# Research: Obsidian Dataview / Templater / Metadata Menu semantics for feature parity+extension

Type: research
Status: resolved

## Question

The product goal requires Traces to go *beyond* Markdown Oxide's PKM capability by extending toward Obsidian Dataview, Templater, and Metadata Menu semantics — and Traces' own Query/Template/Schema modules already draw on these (Query DSL ≡ Dataview-like source/filter grammar; Template module already implements MiniJinja versions of Templater-style interactive helpers; Schema module already implements File-Class/field concepts ≡ Metadata Menu). This ticket is about what *language-service* intelligence (completion/hover/diagnostics/navigation) those plugins provide for their respective languages — not about re-litigating whether Traces should have these features (it already does, at the CLI level).

Local corpus available under `docs/digests/`: `obsidian_blacksmithgu-obsidian-dataview-{digest,src-digest,docs-digest}.txt`, `obsidian_silentvoid13-templater-{digest,src-digest,docs-digest}.txt`, `obsidian_mdelobelle-metadatamenu-{src-digest,docs-digest,digest}.txt`, and `obsidian_obsidian-tasks-{digest,src-digest,docs-digest}.txt` (bonus — task query language, directly relevant to Traces' own Task/Query-Tasks-mode semantics). Read directory structures first, then targeted files.

Establish and cite for each plugin:

- **Dataview**: DQL grammar surface (SOURCE/WHERE/SORT/GROUP BY/FLATTEN) vs Traces' existing Query grammar (`src/query/grammar/`) — what editor-assist features (if any — Dataview itself has no LSP, it's an Obsidian plugin with CodeMirror integration) exist for authoring DQL blocks: syntax highlighting, autocomplete for field names/source expressions, inline error display. Note precisely what Obsidian's CodeMirror integration provides that a *language-server* equivalent would need to replicate structurally (since there's no LSP to copy from directly).
- **Templater**: what interactive-prompt / completion assistance (if any) Obsidian's CodeMirror integration provides while authoring a Templater template (function-name completion, argument hints) — compare against Traces' existing `ui`/`file`/`date`/`query`/`tasks`/`schema` MiniJinja namespaces (`src/template/engine/*.rs`) to identify where Traces' helper surface already has, lacks, or differs from Templater's function catalogue.
- **Metadata Menu**: how it provides frontmatter/inline-field editing assistance — field-value autocomplete constrained by field type (select options, file-class filters), inline validation/diagnostics for field values against a File Class definition, and how it handles field inheritance display. Compare directly against Traces' Schema module facts already gathered (Kahn's-topo-sort inheritance, `$ref`, Field Definition types in `src/schema/fields/`) — Metadata Menu is the closest existing precedent for the "Schema/File-Class-aware language intelligence" decision.
- **obsidian-tasks** (bonus): its task query language and task metadata syntax (due dates, recurrence, priority emoji shorthand) — compare against Traces' existing Task model (`TaskStatus`, date-shorthand emoji markers per `src/note/CONTEXT.md`) to see what's already covered vs what's a live gap.

Write findings to `.scratch/md-pkm-lsp/research/dataview-templater-metadatamenu-parity.md`, citing each claim's source (digest file + line range). Where a plugin has no direct LSP/editor-assist precedent to draw from (true for all four — they're Obsidian-plugin-side, not LSP-side), say so explicitly rather than inventing one.

## Answer

None of Dataview/Templater/Metadata Menu/obsidian-tasks provide real LSP-shaped intelligence (no diagnostics/hover/definition anywhere) — all rely on Obsidian's EditorSuggest API for completion popups only, execution-time errors otherwise. Templater's tp.* namespace completion (regex-triggered member completion) is a direct precedent for ticket 22. Metadata Menu's fileClass-scoped field-value completion is the strongest structural precedent for ticket 20 (schema-aware intelligence). obsidian-tasks' pluggable per-format TaskSerializer (emoji vs Dataview-bracket) confirms a serializer-per-format shape for ticket 23/18. Traces would be first to bring hover/diagnostics/definition to any of these four languages — not catch-up, genuine new ground.

Full findings: [`research/dataview-templater-metadatamenu-parity.md`](../research/dataview-templater-metadatamenu-parity.md)
