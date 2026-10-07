# Ticket 24 — `novel-embedded-context-precedent`: Precedent for the three novel nested contexts

Consolidated 2026-10-06. Unit 7 of ticket 24's decomposition
([`24-part-decomposition.md` §7](24-part-decomposition.md)). Planning artifact only;
no production code, no repo-root files. Answers: **what precedent exists** for (a) query-DSL
completion inside `{{ }}`, (b) `ui.`/`date.` helper-namespace member completion, (c) schema-layered
frontmatter completion — and for each, **which detection + interleaving pattern Traces should
adopt**, with explicit no-precedent markings.

**Method notes.** Claims are marked **V** (verified against cited primary source this pass),
**R** (carried from search excerpts / secondary source — re-verify before load-bearing use),
**I** (inference from verified facts). Web claims carry URLs and keep the capability axis (what a
tool does) separate from the adoption axis (what the ecosystem recommends). Digest line numbers
are the exact lines read. "Source demands" = facts about Traces' own codebase/tickets (cited to
issues/research files); "ecosystem practice" = what other systems do (cited to sources). Open
questions are §8 — nothing in §8 is load-bearing.

---

## 1. Precedent matrix

System × nested-region detection × completion handoff × trigger partition.

| # | System | Kind | Region detection | Completion handoff | Trigger partition | Rel. |
|---|--------|------|------------------|--------------------|-------------------|------|
| 1 | **Templater `tp.` autocomplete** | Obsidian client (EditorSuggest) | Purely lexical: line-prefix regex `/tp\.(?<module>[a-z]*)?(?<fn_trigger>\.(?<fn>[a-zA-Z_.]*)?)?$/` against `line[0..cursor]`; **no delimiter/region awareness** (fires in prose, any file type) | In-process, two-stage: module names first (`is_module_name` gate), then functions after `.`; docs as data (`Documentation` table) | Implicit: regex *is* the trigger; returns `null` ⇒ no popup. `onTrigger` gets first look per keystroke | (b) **closest structural precedent** |
| 2 | **Metadata Menu `ValueSuggest`** | Obsidian client (EditorSuggest) | Two-mode: (i) frontmatter = scan lines for opening/closing `---` (`isInFrontmatter`, digest `:22766-22777`), (ii) inline = per-line field parser (`getLineFields`) with `key::`/`[key::]`/`(key::)` regexes; gated on `.md` + autosuggest setting + post-select `didSelect` suppression | In-process; values scoped by fileClass: `plugin.fieldIndex.filesFields.get(file.path)?.find(f => f.name === fieldName)` (`:2412`, `:2877`); type-gated (Select/Cycle/Multi/File/Lookup); tags special-cased via `metadataCache.getTags()` | EditorSuggest per-keystroke; returns `null` when region/type doesn't match | (c) **closest structural precedent** |
| 3 | **Dataview** | Obsidian plugin | — | — | — | **No editor completion at all**: zero `EditorSuggest` matches in `obsidian_blacksmithgu-obsidian-dataview-src-digest.txt` **V** |
| 4 | **Obsidian Tasks** | Obsidian client (EditorSuggestor) | Line-level suggestor for task fields/dates (`obsidian_obsidian-tasks-src-digest.txt:22980-23013`) | In-process value list | Per-keystroke `onTrigger` | (c), weak |
| 5 | **Markdown Oxide** | LSP | Chain-of-responsibility: each completer regex-tests `line_to_cursor` ("ends in `[[`", "starts with `#`"…) via `.or_else()` (`tool-markdown-oxide.md:43-44`) | Single server, single list | `triggerCharacters: ["[", " ", "(", "#", ">"]`, `resolve_provider: false` (**V** `lsp_feel-ix-343-markdown-oxide-src-digest.txt:2025-2041`) | Detection pattern precedent; **"No native Metadata/Dataview/metadata-tag completions"** (`tool-markdown-oxide.md:47`) |
| 6 | **Marksman** | LSP | Link syntax only (inline/ref/wiki) | Single server | triggers `[`, `#`, `(` (`tool-marksman.md:14`) | none for (a)(b)(c) |
| 7 | **zk** | LSP | `[[`, `#`, colon-tags | Single server; completion formatting configurable (`[lsp.completion]`) | per-delimiter (`tool-zk.md:14`) | none for (a)(b)(c) |
| 8 | **MS vscode-markdown-languageservice** | LSP | Path/anchor/reference regions | Single server | n/a (invoked + char-driven) | **Counter-precedent**: deliberately returns *no* HTML-id completions inside code blocks (`lsp_microsoft-vscode-markdown-languageservice-digest.txt:13323` — test *"Should not return html id completions from inside code blocks"*) **V**; no frontmatter/DSL completion (`tool-ms-…:14`) |
| 9 | **rumdl** | LSP | Fence-language after ```` ``` ````; link target inside `](…)` (`lsp_rvben-rumdl-src-digest.txt:29374-29400` doc comment) | Single server | `(`, `#`, backtick | none for (a)(b)(c) |
| 10 | **gopls Go templates** | LSP, single server | **Real parse** of the template (`x/tools/internal/lsp/template`, delimiters `Left`/`Right` = `{{`/`}}`); feature set includes Completion/Definition/Hover over template AST **R** (URL fetched; go.dev page JS-rendered — verify) | Same server answers over parsed template + host Go context | opt-in `experimentalTemplateModules` **R** | **Strongest server-side precedent for (a)+(b)**: one server, embedded-language AST, host-mapped spans |
| 11 | **helm-ls** | Multi-server | tree-sitter template AST; `.Values.`/`.Chart.`/`.Release.` path + `include`/`define` completion inside `{{ }}` **R** | **Delegates host YAML content to an embedded yaml-language-server subprocess** (position-transforming passthrough) **R** (deepwiki secondary) | client-side | Precedent for *choosing* not to re-implement schema completion in the host server |
| 12 | **graphql-language-service-server** (vscode-graphql) | LSP over **host** files | Activation by lexical marker: `gql`/`graphql` tagged templates or `#graphql` comment; positions remapped into the fragment **R** | Position-mapped fragment completion inside host file | marker-based activation + `.`/`$` etc. | **Closest analogue to (a)**: language-within-language via lexical region detection; **known failure mode: completion-item noise leaking into JSX/TSX contexts** (github.com/graphql/vscode-graphql issue #21) **R** |
| 13 | **Volar Hybrid Mode (Vue)** | Multi-server | Partition by feature domain: TS intelligence via `@vue/typescript-plugin`, Vue LS owns HTML/CSS/JSON; intent = avoid duplicated/competing results **R** (gist 62580d04cb86e576e0e8d6bf1cb44e73 JS-rendered, not captured) | Ownership split, not merge | per-domain | Pattern precedent for rumdl coexistence (ticket 32) |
| 14 | **IntelliJ language injection** | IDE | **"Places Patterns"** (PSI patterns, e.g. `functionArgument(...)`) select which string literal spans host the injected language; also `// language=SQL` markers **V** (jetbrains.com/help/idea/language-injection-settings*.html) | Injected fragment gets **full** child-language intelligence (completion/hover/errors mapped back to host) | injection-pattern-driven | **Strongest conceptual precedent for (a)**: cursor-in-host-span ⇒ full child completion on host coordinates |
| 15 | **Shopify Liquid language server** (now `shopify/theme-tools`) | LSP | Owns Liquid grammar; contextual: tags/filters/objects **and object properties after `.` inside `{{ }}`**; plus HTML attrs + theme schema JSON **R** | Single server | context-sensitive within Liquid AST | In-template member-completion precedent for (b) |
| 16 | **yaml-language-server** | LSP | Whole-file YAML; schema association via modeline (`# yaml-language-server: $schema=`), `$schema` key, `yaml.schemas` setting, `json/schemaAssociations` client notification; schema-resolution priority documented; `yaml.hoverSchemaSource` setting exposes provenance **V** (github.com/redhat-developer/yaml-language-server README) | Single server, schema-driven key + value + structure completion | standard YAML triggers | **Closest precedent for (c) mechanics** — but single-layer only |
| 17 | **VS Code client merge** | Client | Completion results from **all** providers are *merged* into one suggest widget — *"results from all providers are merged (e.g. completion, hover, definition, problems, symbols)"* (github.com/microsoft/vscode issue #80889) **V**; providers are **scored by language-selector specificity** and lower-score buckets are consulted only if higher yields nothing — *"to prevent mixing good suggestions with word-based suggestions"* (jrieken on issue #21611) **V** | One widget; layering via `editor.wordBasedSuggestions` / `editor.suggest.showWords` / `snippetSuggestions` settings (code.visualstudio.com/docs/editing/intellisense) **V** | `editor.suggestOnTriggerCharacters` + per-provider `triggerCharacters` | (c) layering UX precedent + §5 interleaving evidence |
| 18 | **LSP spec (3.18)** | Spec | — | Client filters & sorts; server steers via `filterText`, `sortText`, `labelDetails` (3.18), `itemDefaults` **V** (`docs/refs/lsp_spec.md` completion.md) | `completionProvider.triggerCharacters` | (b)/(c): badge in label, **`filterText` = raw key** so badges don't break prefix filtering |

---

## 2. Context (a): query-DSL completion inside `{{ query.pages("…") }}`

**What exists.** Double-nesting precedent exists and is mature: IntelliJ language injection
(matrix #14) maps a host string span to a child language that gets *full* intelligence **V**;
graphql-language-service activates on a lexical marker and remaps positions into the fragment
(#12) **R**; gopls parses Go templates and answers completion over the template AST in one server
(#10) **R**; helm-ls parses templates with tree-sitter and delegates the *host YAML* to a child
server (#11) **R**. **Triple** nesting (Markdown → MiniJinja → query DSL) has **no direct
precedent found** — the added hop is only over a string-literal boundary inside an already-parsed
region, so every precedent mechanism still applies; the region *detection* is the novel part.

**What does not exist (explicit).** No PKM LSP completes inside any embedded language (matrix
#5-#9, verified from source digests — §6.1). No Obsidian plugin completes inside a query context:
Dataview — the query language itself — ships **zero** editor completion (#3) **V**; Templater and
Metadata Menu offer no query completion (#1, #2). MS's markdown LS instead *suppresses* completions
inside embedded regions (#8) **V**.

**Recommended pattern** (synthesis, first-principles where marked):

1. **Detection — two-stage, both server-side.** Stage 1: ticket 21's depth counter over `{{`/`}}`
   (increment/decrement, cursor must be at positive depth) — **source demand**
   ([21](../../issues/21-query-language-intelligence.md) "Cursor detection"). Stage 2, still within
   positive depth: 21's **lexer-token classification** must additionally confirm the cursor is in
   a *string-literal argument of a `query.*` call* (trigger chars `(` and `"` per 21:61).
   Precedent for the staging: Templater's regex returns `null` to decline (#1) **V**; Metadata
   Menu's mode check returns `null` outside its region (#2) **V**; graphql's marker-based
   activation (#12) **R**. Detection failure mode to design against: the graphql-in-TSX *noise*
   issue (#12) — **degrade to `null` (no items), never to degraded/wrong-region items**; this is
   exactly 22's D5 soundness invariant "completion-offered ⇒ find-resolvable" applied to regions.
   *No precedent found for the specific depth-counter algorithm* → first-principles, but bounded
   by 21's O(N) full-reparse budget.
2. **Handoff — in-process, single server, host-coordinate edits.** Parse the query fragment with
   21's query lexer over the document's existing parse, emit `textEdit` ranges already in host
   coordinates (ticket 11's `BytePos` → `LineIndex` UTF-16 ladder). Precedent: gopls (#10) **R**
   and IntelliJ (#14) **V** both do exactly this; **reject** helm-style child-server delegation
   (#11): two IPC hops + position transforms conflict with the `<20ms` budget and span fidelity
   (source demands: single `tower-lsp-server` sequential dispatch, full re-parse per edit —
   [24-completion-architecture](../../issues/24-completion-architecture.md)).
3. **Trigger partition.** `(`, `"` (21:61) registered by Traces; `#`/`@` (21:49) reconcile with
   ticket unit 2's rumdl-facing trigger decision — do not re-decide here.

---

## 3. Context (b): helper-namespace member completion (`ui.`, `date.`, …)

**What exists.** *Partial* precedent — the flow (member list after `.`) is old and well-served;
the *namespace-membership gate* is the part with thin precedent:

- **Templater's two-stage `tp.` regex** (#1) **V** is the structural twin: stage 1 must match a
  known module name (`is_module_name`) *or* the suggestor returns `null` — i.e., Templater **also
  gates on namespace membership before showing members**, and its members come from a hand-authored
  documentation table (`Documentation`), not reflection. Traces' situation per 22: member **names**
  are runtime-derivable (`env.globals()` + every namespace's `Object::enumerate()` →
  `Enumerator::Str(METHODS)`), signatures/docs are hand-authored — same shape as Templater
  (**source demand**, [22](../../issues/22-template-language-intelligence.md):13 + grounding #3).
- **Liquid LS** completes object properties after `.` inside `{{ }}` (#15) **R**; **gopls**
  completes inside template call arguments (#10) **R**; TS server / rust-analyzer member
  completion is the generic LSP machinery for `.`-triggered member lists.
- **Obsidian itself has no `{{ }}` analogue** (Templater's syntax is `<% %>`; its regex never
  checks delimiters) — so *delimiters-gated* member completion in a PKM note is **no precedent
  in Obsidian plugins**.

**Recommended pattern:**

1. **Detection.** Trigger char `.` (coordinate with unit 2). Require *both*: ticket-21 depth
   counter (cursor inside `{{ }}`) **and** token classification showing the token immediately
   before `.` is an identifier that equals a registered global namespace root (`ui`, `file`,
   `date`, `query`, `tasks`, `schema`). Non-matching root ⇒ return `null`. This is Templater's
   `is_module_name` gate (#1) **V** generalized to Traces' four conditions — *first-principles
   composition of two individually-precedented checks*.
2. **Handoff.** Single server; members from a table built by `enumerate()` (safe: registration is
   side-effect-free; **Q5** verifies `enumerate()` itself never invokes dialog-touching code) +
   hand-authored signatures for `label`/`documentation`/`completionItem/resolve`. Trigger member
   completion **only** in region — never emit `ui.` members from prose (reject Templater's
   file-agnostic permissiveness for Traces, because Traces shares the file with prose where a
   bare `.`-trigger would misfire; Templater tolerates this because `tp.` itself is an unlikely
   prose sequence — a bare namespace gate after depth-check removes the risk either way).
3. **Precedent gap marked:** no source derives member lists from a live template engine at
   completion time. Closest: TS server from a type checker (in-memory, no execution) and
   Templater from static docs. Traces' `enumerate()`-at-startup table sits between them —
   recommend building it **once at engine setup**, not per request (source demand: <20ms budget).

---

## 4. Context (c): schema-layered frontmatter completion

**What exists.** *Mechanics* have strong precedent; the *layering* does not.

- **yaml-language-server** (#16) **V** is the canonical match: opaque YAML region, external schema
  association through four documented channels, schema-driven key/value/structure completion, and
  a setting (`yaml.hoverSchemaSource`) that surfaces *which schema* produced a hover — the
  ecosystem does treat provenance as user-visible.
- **VS Code client** (#17) **V** proves a single widget can carry heterogeneous layers —
  word-based (analogous to *inferred*), language-server (analogous to *schema*), snippets — with
  per-layer on/off/ordering settings; and that providers are scored so lower-priority sources are
  consulted only when higher-yield buckets are empty (jrieken, #21611) **V**. Caveat for layering:
  that bucketing is *per provider*, not per item — Traces returns all three layers from **one**
  provider, so intra-list ordering is entirely its own `sortText` responsibility (client sorts per
  spec, but nothing else ranks layers for it).
- **Metadata Menu** (#2) **V** precedent for the *value* half (ticket 20): fileClass-scoped field
  lookup keyed by note path, value-type-gated lists, tags special-cased.
- **Obsidian has no key-completion-with-schema**: no plugin offers schema-layered *key*
  completion (Templater/Metadata Menu/Tasks all complete *values* or DSL members, §6.3).

**Explicit no-precedent finding.** No ecosystem example found of **three provenance layers
(schema > global > inferred) deduped by canonical key with sortText prefixes and origin badges**.
Ticket 19's `Vec<(FieldKey, Layer, Option<SchemaFieldDef>)>` + `FieldKey` case/hyphen-folded dedup
+ `⟨global⟩`/`⟨inferred⟩` badges ([19](../../issues/19-frontmatter-and-inline-field-intelligence.md)
:40-50) is therefore **novel in its layering+provenance design**; its *mechanics* are conventional.
→ Marked: first-principles design accepted, informed by the two precedents above.

**Recommended pattern:**

1. Keep 19's three-layer construction and layer-numbered `sortText` prefixes (client sorts per
   spec **V**). Precedent support: VS Code's layered widget (#17) **V**.
2. **`filterText` = raw `FieldKey`** on every item, badge only in `label`/`labelDetails.detail` —
   otherwise a user typing past `⟨global⟩` characters filters against the badge (**I**, from
   spec's label/filterText split **V**). `labelDetails` is 3.18 → gate on `labelDetailsSupport`
   (input for unit 8 / ticket 29, do not decide gating here).
3. Position-precise `textEdit` replacing the whole key token (yaml-ls behavior #16); values per
   ticket 20 modeled on Metadata Menu's fileClass-scoped, type-gated lookup (#2) **V**.
4. Provenance visibility: badge-in-label (19's decision) is the client-portable equivalent of
  yaml-ls's hover-side schema-source setting (#16) **V**.

---

## 5. Client interleaving & double-popup evidence (cross-cutting)

- **VS Code merges, does not stack**: all providers' completion results land in one widget
  (#80889) **V**. Consequence for rumdl coexistence (ticket 32): in VS Code the failure mode of
  two servers on one `.md` file is *redundant items + duplicate request cost*, **not double
  popups**. Score-bucket fallback (#21611) **V** means a same-selector provider's results can
  *displace* word-based ones — worth remembering for diagnostics later, not load-bearing for
  layering inside one provider.
- **Volar** chose domain-ownership split instead of merge (#13) **R** — the alternative pattern:
  two servers, disjoint spans. Traces+rumdl will instead *merge* in VS Code; unit 2 owns the
  coordination, this unit only records that **no client-side double-popup mitigation is required
  in VS Code**; **Q6** covers Neovim/nvim-cmp behavior (unverified).
- **Server-side single-list discipline**: every precedent server returns one list per request
  (mdoxide's chain-of-responsibility `.or_else()` picks the *first* matching completer rather than
  concatenating — `tool-markdown-oxide.md:43-44` **V**). Recommended for Traces' dispatcher: one
  region classifier → one completer → one list; never union of region-candidate lists (union would
  re-create the graphql "noise" failure (#12) **R**).

---

## 6. Explicit no-precedent statements

1. **No PKM LSP offers embedded-language completion.** Markdown Oxide, Marksman, zk, MS
   markdown-ls, and rumdl all complete only links/tags/paths/anchors/fence-languages — verified
   from source digests and research tool files (matrix #5-#9) **V**. MS goes further and
   *suppresses* completions inside code blocks (#8) **V**. None complete frontmatter keys or
   template/query contexts (mdoxide explicitly: "No native Metadata/Dataview/metadata-tag
   completions", `tool-markdown-oxide.md:47` **V**).
2. **No Obsidian plugin completes inside query context.** Dataview has no EditorSuggest at all
   (#3) **V**; Templater/Metadata Menu/Tasks complete elsewhere (#1, #2, #4).
3. **No precedent for schema/global/inferred layered frontmatter completion with dedup +
   provenance badges** (§4) — ticket 19's layering is novel; mechanics conventional.
4. **No precedent for triple-nested Markdown→template→query completion** (§2); double-nesting
   precedent is strong (IntelliJ, graphql, gopls).
5. **No precedent for deriving template-helper member lists from a live engine at completion
   time** (§3); nearest shapes: Templater static docs table, TS server type-derived members.

---

## 7. Patterns to reject

| Pattern | Precedent | Reject because |
|---|---|---|
| Child-server delegation for host-region completion (helm-style, #11) | exists **R** | Two servers + position transforms vs single sequential server & <20ms budget (source demands, ticket 33/12); span fidelity across two mappers unproven |
| Client-side injection grammar (`embeddedLanguages`) as *the* mechanism | VS Code feature | Not LSP-portable (Obsidian/Neovim clients get nothing); supplies lexemes, not schema-scoped semantics; Traces doesn't own highlighting |
| Dataview-style "no completion in query context" (#3) | exists **V** | Directly violates ticket 21's completion deliverable |
| Templater-style region-blind regex as sole detector for Traces | exists **V** | Traces shares files with prose where `.`-delimiters are ubiquitous; needs depth-counter + token gate (§2.1, §3.1) |
| Union of all candidate completers' items (no region classifier) | contradicted by mdoxide chain (#5) **V** | Recreates graphql noise failure (#12) **R**; violates D5 soundness when items come from the wrong region |
| Re-deciding trigger-char partition here | — | Owned by ticket 24 unit 2 / rumdl coordination (ticket 32) |

---

## 8. Open questions (unverified — none load-bearing above)

- **Q1** Exact wording/mitigation of vscode-graphql completion-noise issue #21 **R** — fetch the
  issue before citing its resolution strategy.
- **Q2** When multiple Obsidian `EditorSuggest`s return non-null for the same keystroke, which
  wins (registration order? first non-null?) — affects parity reasoning only; not in digests.
- **Q3** Volar Hybrid Mode gist wording (JS-rendered page) **R** — confirm from
  github.com/vuejs/language-tools README before citing.
- **Q4** helm-ls delegation mechanism (deepwiki secondary) **R** — confirm from helm-ls source.
- **Q5** Does `Object::enumerate()` on `ui`/`query`/… ever execute side-effectful code paths?
  22 says registration is side-effect-free; verify enumeration is too (rust-docs-mcp / cached
  minijinja 2.24.0).
- **Q6** Neovim/nvim-cmp behavior with two completion sources on one buffer (merged menu vs
  stacked) — determines whether §5's "VS Code merges" generalizes.
- **Q7** gopls `experimentalTemplateModules` current status/docs (**V** for package existence,
  **R** for feature list) — go.dev fetch was JS-rendered.

---

## 9. Ready-to-use inputs for ticket 24

1. **Dispatcher shape**: one region classifier (depth counter → token/regex gate → first match
   wins, mdoxide-style) → one completer per region kind → one list; decline = `null`.
2. **Three classifier rows**: (a) positive depth ∧ string-arg of `query.*` → query completer
   (triggers `(`, `"`, `#`, `@` — reconcile with unit 2); (b) positive depth ∧ pre-dot token ∈
   globals set → namespace member completer (trigger `.`); (c) frontmatter region / inline-field
   region → schema-layered completer (ticket 19 layers, `filterText` = raw key).
3. **Rejections to cite in ticket 24**: §7 table.
4. **Novelty disclosures for the design doc**: triple-nesting (a), engine-derived members (b),
   three-layer provenance (c) — each has adjacent precedent but no direct match (§6).
