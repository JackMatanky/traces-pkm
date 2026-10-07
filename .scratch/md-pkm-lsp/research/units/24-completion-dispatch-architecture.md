# Ticket 24 — `completion-dispatch-architecture`: cursor→context classification & dispatch

Consolidated 2026-10-06. Unit 1 of ticket 24's decomposition
([`24-part-decomposition.md` §1](24-part-decomposition.md)). Planning artifact only — no production
code, no repo-root files. Answers: **which architecture** classifies every Traces completion context,
**precedence rules** for the four named nestings, **static vs dynamic registration**, and **when
classification runs**.

**Method notes.** **V** = verified against the cited primary source this pass; **R** = carried from
search excerpts/secondary source (re-verify before load-bearing use); **I** = inference from verified
facts; **QUESTION** = open (§9). "Source demands" = Traces' own codebase/tickets/spec constraints
(cited to `issues/`/`research/`/`src/`); "ecosystem" = what other systems do (cited to digest lines,
raw URLs, `file:line`). Today is 2026-10-06; ecosystem claims are current-as-of that date.

---

## 0. Scope — what this unit does and does not decide

Decides (decomposition §1 "Outputs expected"): options table (a/b/c), context inventory + precedence
matrix, registration strategy, classification timing. **Does not decide** (owned by sibling units):
trigger-character *enumeration* + rumdl collision (unit 2), `CompletionItem` shape/ranking (unit 3),
initial-vs-resolve field split (unit 4), snippet inventory (unit 5), budget accounting (unit 6). The
parent ticket's bullets 2–4 ([issues/24:12-14](../../issues/24-completion-architecture.md)) are therefore
deferred here to units 2/4/5 respectively; this unit supplies the seam each hangs off.

---

## 1. What the protocol actually allows (spec + source demands)

1. **There is no per-context registration in LSP.** `triggerCharacters` is one flat `Vec<String>` on
   `CompletionOptions` for the whole server (`24-completion-capability-negotiation.md:170`,
   `24-part-crates.md:725-729`). Dynamic registration exists, but its scope is a `documentSelector`
   — a file-type scope, never a syntactic-context scope (spec `Registration` /
   `*RegistrationOptions` **R**; gating quoted at `24-part-crates.md:131-134`). ⇒ **Option (b) "per-context
   independent trigger registration" is not
   representable in LSP at all** (**V** spec/crates). The only system where per-context trigger lists
   exist is VS Code's *client* provider API (`registerCompletionItemProvider(selector, …,
   triggerCharacters)`), where an LSP server is exactly **one** provider
   (`24-novel-embedded-context-precedent.md:42` **V**) — that is a client-side capability, not
   something Traces can adopt.
2. **`CompletionContext` is optional and unreliable in both directions.** Guaranteed only when
   `contextSupport === true` [S1]; Helix never advertises it yet always sends `context`
   (`capability-negotiation:120-129` **V**); MS markdown LS is written to survive `context: undefined`
   (`capability-negotiation:130-133` **V**). Consequence already recorded by unit 6: `trigger_kind` /
   `trigger_character` are **cache hints only**, dispatch must be the identical code path for
   `Invoked`, `TriggerCharacter`, no-context, and unknown clients
   (`capability-negotiation:135-143` **V**) — and `TriggerForIncompleteCompletions` must not be
   *required* (`capability-negotiation:140-143`).
3. **Latency + dispatch model.** `concurrency_level(1)` sequential handlers, no cancellation
   (`24-completion-latency-architecture.md:19`); context detection is budgeted **≤1 ms per request,
   line-local / O(log n)** (`latency:187` S2 row); the 20 ms budget survives only if classification
   stays line-local and every O(document)/O(vault) scan is precomputed or memoized per version
   (`latency:302-307`).
4. **Span substrate is incomplete today.** Parsed `Note` nodes carry no byte spans — parser byte
   spans are received and discarded (`24-part-codebase.md:79-84`, `src/note/parser.rs:108-113,241-258`);
   ticket 11's planned `Range<BytePos>` on `Link`/`Tag`/inline fields and frontmatter ranges are
   decided but unimplemented (`part-codebase.md:93-103`). Until then a classifier can only lexically
   re-scan the buffer (`part-codebase.md:105-110`). ⇒ **the architecture must be written so the
   lexical scan and the future span-containment test are swappable behind one interface** (**I**).

---

## 2. How established systems classify the cursor (ecosystem evidence)

| System | Entry point → classification | Uses LSP `CompletionContext`? | Variant |
|---|---|---|---|
| **rust-analyzer** | Two-phase: (1) `CompletionContext::new` collects context — dummy-identifier reparse ("IntelliJ Trick") + classification routines (<https://rust-analyzer.github.io/book/contributing/guide.html> **V**); (2) `completions()` matches **one** `CompletionAnalysis` enum → `complete_name` / `complete_name_ref` / `complete_lifetime` / string-attr routines (`crates/ide-completion/src/lib.rs`, raw fetch **V**); `trigger_character` is only an extra *guard* (`if trigger_character == Some('(') … return`) | yes, as guard only | **(a)** |
| **gopls** | Outer switch on **file kind** `Go/Mod/Work/Tmpl` → per-language completer (`gopls/internal/server/completion.go:46-63` raw fetch **V**); inside Go, a chain of routines (`lexical`, `selector`, `structLiteralFieldName`, …) over the AST (`golang/completion/completion.go:1496,1347,1812` **V**); `params.Context` narrowed into `completionContext{triggerCharacter, triggerKind}` (`:364-367,623-625`) used only to *enable* specific routines (`== "."` → selector path `:1121`; comment completion `:1134`) | yes, as routine-enabling hint | **(c)** |
| **tsserver** | Single `getCompletionsAtPosition()` entry classifies position itself (wiki: Codebase Services Completions **R**); the LSP layer (`typescript-language-server/src/completion.ts`) builds its own `CompletionContext { isMemberCompletion, isNewIdentifierLocation, dotAccessorContext }` from tsserver's answer (**R**); static registration `resolveProvider: true`, `triggerCharacters: ['.','"','\'','/','@','<']` (`capability-negotiation:283` **V**) | effectively no — context re-derived | **(a)** |
| **Pyright** | `findNodeByOffset(parseTree, offset)` + `getTokenOverlapping(tokens, offset)` (`completionProvider.ts:1323,1352` raw fetch **V**); `this.options.triggerCharacter` only gates narrow paths (`:1532` list `[`, `:1891`, `:2846` quote) **V** | yes, as gate | **(a)** |
| **Marksman** | `findCompletableAtPos` — one classifier with an **explicit priority**: *"The priority is generally link > partialElement > tag. However … `[[#f` will be completed as a wiki link, rather than a tag"* (`digest:5162-5165` **V**); then `Prompt.ofCompletable` → `findCandidatesForCompl` (`digest:5149-5176`) | not consulted in that path | **(a)** |
| **Markdown Oxide** | `get_completions` = `.or_else()` chain of 7 completers, **first-Some-wins**, each testing line-local text (`digest:4921-4976` **V**); registers flat `["["," ","(","#",">"]`, `resolve_provider: false` (`digest:2028-2037` **V**) | not consulted | **(a)** chain form |
| **zk** | Branches on `params.Context.TriggerKind == Invoked` → `buildInvokedCompletionList` else `buildTriggerCompletionList` (`digest:4354-4359` **V**) — and *both* paths re-derive from the buffer: *"We don't use the context because clients might not send it. Instead, we'll look for trigger patterns in the document"* → `LookBehind` switches on `]](`/`[[`/`#`/`:` (`digest:4829-4849` **V**) | only to pick a code path, never as truth | **(c)** |
| **MS vscode-markdown-languageservice** | `provideCompletionItems` → `#getPathCompletionContext(document, position)` (line-local regexes on line prefix/suffix, `digest:5549,5649-5685` **V**) → enum `CompletionContextKind` → `switch (context.kind)` (`digest:5563-5569` **V**); LSP `context` only spread through `params.context \|\| {}` (`capability-negotiation:285-288` **V**). Code regions are excluded **candidate-side** (tests "Should not consider links in code fenced…" `digest:9500`, ids-in-code `digest:13323` **V**); **no cursor-side code-region gate found in the digest** (**QUESTION** §9-Q2) | passed through, not classifier input | **(a)** |
| **rumdl** | Region predicates in `completion.rs`: fence language after ```` ``` ````, link target inside `](` (`digest:29374-29400` **V** per unit 7 matrix #9) | n/a | **(a)** lexical |

**Reading** (**I**): nobody implements (b). Every LSP registers one flat trigger set and classifies the
cursor **in-process, per request, from the buffer**. Trigger context, where consulted at all, only
*enables/narrows* paths (gopls, Pyright, rust-analyzer) or *picks a branch that then re-derives*
(zk). Two independent Markdown-LS precedent shapes exist for the classifier itself: **explicit
priority ladder with a documented nesting carve-out** (Marksman) and **line-local predicate chain →
enum → switch** (MS md LS, Markdown Oxide).

---

## 3. Options table (a/b/c)

| Variant | Correctness under the four nestings | Latency | Code complexity | Degraded without `contextSupport` |
|---|---|---|---|---|
| **(a) single AST-walking dispatcher** (one classifier, one context, one completer) | **Good** — one place owns precedence; Marksman's `[[#f` carve-out is the proof it scales to nestings (`digest:5162`) **V**. Weakness: needs region truth (frontmatter/code), which is unavailable until ticket 11 spans → lexical scan must emulate it (`part-codebase.md:105-110`) | **Best**: O(line) predicates + one memoized O(document) region scan (**I**, budget `latency:187`) | **Low-moderate**: one `classify` fn + ladder; each added context is a new arm, no registration wiring | **Full** — never reads `context` (**V** by construction) |
| **(b) per-context trigger registration** | **Unrepresentable** in LSP (§1.1) — flat `Vec`, selector granularity only. Even if emulated via multiple dynamic registrations, trigger chars cannot distinguish `[[#` from `#tag` from `# Heading`: `#` is one character | Poor: fires a request per keystroke for *all* contexts, each then re-classifying anyway | High: registration lifecycle, rumdl re-registration churn (`capability-negotiation:192-206`) | **Fails**: without `context`/`triggerKind` the registered context is unknowable (`capability-negotiation:135-143`) |
| **(c) layered hybrid** (hints layer + region layer + line-local layer + dispatch) | **Best** — outer region test resolves frontmatter / `{{ }}` / body exclusivity, inner ladder resolves `#`/`[`/`:` collisions, hints only shape *whether* a request happens. Matches gopls' file-kind-then-language structure and MS's classifier→switch | **Good**: region scan memoized per `(uri, version)`, per-request work O(line) (**I**) | Moderate: two layers + an ordered ladder; one seam (`classify(&Buffer, pos) → Option<ContextKind>`) hides lexical-vs-span swap | **Full** — hints layer degrades to "no hints" (identical behavior), as unit 6 requires |

**(a) and (c) are the same shape at different zoom** — (c) is (a) with an explicit region stage in
front. Recommendation below is therefore "single dispatcher, layered inputs", i.e. **(c)**.

---

## 4. Recommendation: single dispatcher over layered inputs (variant c)

Four layers, evaluated per `textDocument/completion` request, first-match-wins, one list out
(unit 7 §5 "one region classifier → one completer → one list; decline = `null`" `:185-189` **V**):

- **L0 — protocol hints (not classification).** Static flat trigger set at `initialize` (unit 2 owns
  the set; §6 here owns the mechanism). `trigger_kind`/`trigger_character` may skip *nothing* in the
  classifier; identical code path for all trigger kinds (`capability-negotiation:135-143`).
- **L1 — region → language.** Mutually exclusive host regions: **frontmatter** → YAML world;
  **template expression at `{{`/`}}` depth > 0** (21's counter, `issues/21:24`) → MiniJinja/query
  world; **code** (fenced/indented/inline) → veto; else **Markdown body**. This reproduces the
  parser's own `enum BlockContext { None, MetadataBlock, CodeBlock, Text }` — *"mutually exclusive
  parsing states"* (`src/note/parser.rs:166-174`, `start_metadata_block :395`, `start_code_block
  :444`) — as a **cursor-facing** classifier (**I**: same state machine, two consumers).
- **L2 — line-local predicate ladder** inside the selected language (§5), each O(line), built from
  the primitives tickets already specify: `LineIndex::line_at` O(log n) (`src/position.rs:56`),
  `scan_marker_prefix` (`src/note/parser/marker.rs:86`, tri-state `:70-79`), `char_before` look-behind
  (`src/note/parser/lexer.rs:59-69,272-279`), 19's `:` regex (`issues/19:82`), 21's depth counter.
- **L3 — dispatch.** `Option<ContextKind>` → exactly one completer → one list; `None` ⇒ `[]`.
  Never union of completers' items (unit 7 §7 reject-table `:212-221` **V**).

**Interface seam (for ticket 34):** one function/classifier object whose implementation today is a
lexical scan and after ticket 11 is a span-containment test (`part-codebase.md:105-110`). Callers
(completions, later hover/diagnostics needing regions) depend only on the enum (**I**).

---

## 5. Context inventory + precedence matrix

**Inventory** (decomposition `:32` list; owner = who supplies predicate vs candidates):

| # | Context | Detection (owner) | Evidence |
|---|---|---|---|
| C1 | Wikilink target `[[t…` | line-local `[[` + cursor in target (15) | `part-codebase.md:273` |
| C2 | Wikilink heading `[[n#h…` / `[[#h…` | C1 region + `#` position (15) | `issues/15:19,85`; Marksman carve-out `digest:5162` |
| C3 | Markdown-link anchor/target `[t](f.md#…` | line-local `](` region (15 / **ownership Q** unit 2) | `issues/16:37` guard 4; `part-decomposition:40` |
| C4 | Body tag `#…` | `#` + 5 guards (16) | `issues/16:33-38` |
| C5 | Frontmatter tag list | frontmatter region + `tags|tag|keywords` key (16) | `issues/16:45-47` |
| C6 | Frontmatter key | frontmatter region + YAML-key shape (19) | `issues/19:81-83` |
| C7 | Frontmatter value | frontmatter region + after `key: ` (20) | `issues/19:82` (**Q** §9-Q3) |
| C8 | Inline-field key `[k:: ` / `Key:: ` | bracket/line shape (19 predicate) | `issues/19:81` |
| C9 | Task checkbox `- [` | indent+bullet+ws+`[`, cursor between brackets, `Incomplete` (23) | `issues/23:24`; `research/23:128`; `marker.rs:70-79,86-113` |
| C10 | Task date value `📅 ` / `[due:: ` | slot predicate (19) + candidates (23) | `issues/23:27` ("19 = position predicate, 23 = candidates, 24 = dispatch") |
| C11 | Daily-note date in link target | C1 region + date-shaped target (18/15) | `part-decomposition:32` |
| C12 | Footnote ref `[^…` | `[^` line-local (17) | `part-decomposition:40` (`[` serves 17-`[^`) |
| C13 | Callout type `> [!…` | line-start `>` + `[!` (17) | `part-decomposition:40` (`>` candidate) |
| C14 | Query DSL tag/class/LHS/op/RHS | depth>0 ∧ token-before-cursor classification (21) | `issues/21:24` + `issues/21:26-46` (logos-prefix last-token classification) |
| C15 | Template helper namespace `ui.…` | depth>0 ∧ pre-dot token ∈ globals (22) | unit 7 `:111-116` |
| C16 | Template `include`/`extends` name | depth>0 ∧ keyword context (22) | `issues/22:16` |
| C17 | File-field / fileClass value (20) | frontmatter region ∧ key ∈ schema fields | `issues/19:20`, `part-codebase.md:275` |

**Fall-through chains already locked by tickets** (must be honored, not re-decided):

- 23: `- [` state `Incomplete` ⇒ checkbox; `Rejected` ⇒ **fall through to 19's contexts**;
  `Complete` ⇒ field/value layer (`issues/23:24`; `research/23:528`).
- 19: **`CodeRegion` exclusion runs FIRST**, before any trigger handling (`issues/19:83`).
- 22: nested/template completions obey D5 — *completion-offered ⇒ `find`-resolvable*; unknown root ⇒
  decline, never degraded items (`issues/22:35`, unit 7 `:74-75`).

### Precedence rules for the four named nestings

**P0 — Code veto (first, unconditionally).** Cursor in fenced/indented code or an inline code span ⇒
`[]`. Source demands: `issues/19:83` (CodeRegion-first), `issues/16:34` guard 1; parser already
tracks `BlockContext::CodeBlock` (`src/note/parser.rs:168,444`).

**P1 — Region decides language (mutually exclusive, first match):** frontmatter ⇒ P1a YAML rules;
template depth > 0 ⇒ P1b MiniJinja/query rules; else ⇒ P2 Markdown ladder. Frontmatter is located by
a delimiter scan (`---` at doc head); depth by 21's counter (`issues/21:24`, O(document) ⇒ memoize,
`latency:187` flag). *This alone resolves the `:` nesting*: `title: My Note` (YAML value) vs `[key::`
(body inline field) vs `[due:: ` (task slot) vs value-key colon never compete — they live in
different regions (**I**).

**P2 — Nested-language isolation (inside depth > 0).** Only C14–C16 may fire, and only if their token
gate passes; otherwise **decline — never fall back to body completers** (unit 7 §2.1 `:74-77`,
§7 reject "union of completers" `:220`). *This resolves the `#`/`@` in `{{ query.pages("#x") }}` vs
body-tag nesting*: `#`/`@` inside a `query.*` string argument ⇒ 21's completer; same characters in
body ⇒ P2 never applies.

**P3 — Markdown ladder, first match wins** (each step O(line); `None` ⇒ next):

1. **Wikilink `[[…`** ⇒ C1/C2 (target vs heading). *Resolves `#` inside `[[link#heading]]`: heading
   wins over tag* — the ticket-16 guard-4 rule (`issues/16:37`) with Marksman's documented precedent
   *"`[[#f` will be completed as a wiki link, rather than a tag"* (`digest:5162-5165` **V**).
2. **Footnote `[^…`** ⇒ C12. Must precede step 3: `scan_marker_prefix("[^…")` returns `Incomplete`
   (no `]`/ws yet), so `- [^` would otherwise be misclaimed as a checkbox (**I** from
   `marker.rs:86-113`).
3. **Task marker** (indent + bullet + ws + `[`, cursor between brackets, state `Incomplete`) ⇒ C9;
   `Rejected` ⇒ continue (23's own fall-through, `issues/23:24`); `Complete` ⇒ field/value layer.
   Step 1/2 first also fix the `- [[` false positive: `scan_marker_prefix("[[")` is `Incomplete`
   (`marker.rs:91-98`), so an un-ordered ladder would offer a checkbox mid-wikilink
   (**I**).
4. **Inline field** `[k::`/`(k::` ⇒ C8; `[due:: ` ⇒ C10 (19 predicate + 23 candidates,
   `issues/23:27`).
5. **Markdown link target `](`** ⇒ recognized, then decline-or-C3 per unit 2/15 ownership
   (**QUESTION** §9-Q1) — recognition itself is mandatory so steps 1–4 never leak into it.
6. **Line shape:** ATX heading `#{1,6} ` at line start ⇒ heading line, *not* tag (`issues/16:38`
   guard 5); `> ` + `[!` ⇒ callout (C13).
7. **`#` guards:** URL/autolink `http://…#frag` ⇒ decline (`issues/16:35` guard 2); word-internal
   `foo#bar` ⇒ decline (`issues/16:36` guard 3; same guard exists in the indexer — `char_before`
   rejects alphanumeric/`_` before `#`, `src/note/parser/lexer.rs:272-279` **V**).
8. **Default body:** `#` ⇒ C4 tag; word-boundary `:` per 19's regex ⇒ C8-adjacent (`issues/19:82`).

**P4 — YAML ladder (frontmatter):** 19's `:` regex (valid bare-word key before, not followed by
another `:`, in region) ⇒ key/value contexts (`issues/19:82`); `#` inside a tag-list key's value
⇒ C5 (`issues/16:45-47`); otherwise decline (never body completers — P1 exclusivity).

**Nesting audit** (all four from `part-decomposition:32`):

| Nesting | Resolved by | Result |
|---|---|---|
| `#` in `[[l#h]]` vs tag vs ATX vs URL vs code | P0 → P3.1 → P3.6 → P3.7 | heading > tag; ATX/URL/code decline |
| `:` frontmatter vs inline field vs `[due::` vs value-key | P1 (region) → P3.4 / P4 | region-exclusive, no competition |
| `[` at `- [` vs `[[` vs `[^` vs `[text](` vs inline field | P3.1→2→3→4→5 | wikilink > footnote > task > inline-field > link-target |
| `#`/`@` in `{{ query("…") }}` vs body | P1 → P2 | query completer inside, body outside, no cross-fallback |

---

## 6. Registration strategy — **static at `initialize`**

- **Recommendation: static, config-derived trigger set, one `CompletionOptions` at `initialize`.**
  Owned by unit 2 for the *set*; this unit owns the *mechanism*. Evidence: no per-context primitive
  exists (§1.1); Helix advertises no `dynamicRegistration`, Neovim advertises `false`, the set is
  config-derivable (`[tasks]`, wiki style, `enableLinkCompletions`), and one selector must not be
  registered twice (`capability-negotiation:192-206,469-470,520-523` **V**).
- **Dynamic registration is not a context mechanism**; at most it is a *config-change* mechanism
  (re-register the whole flat set when a toggle flips mid-session), and only when
  `completion.dynamicRegistration == Some(true)` (`capability-negotiation:520-523`).
- Precedent (**V**): Marksman static `[`,`#`,`(` + `ResolveProvider = None`
  (`capability-negotiation:296-300`, `digest:11150-11154`); Markdown Oxide static
  `["["," ","(","#",">"]` (`digest:2028-2037`); typescript-language-server static
  `resolveProvider: true` + 6 triggers (`capability-negotiation:283`); MS md LS never reads client
  capabilities at all (`capability-negotiation:285-288`).
- rumdl coordination (which characters ceded/kept, duplicate-request cost) is **unit 2 / ticket 32**;
  this unit only fixes the mechanism those decisions will be expressed through.

## 7. Classification timing & trigger/re-request interaction

- **When: per request, inside the handler, against a per-version memoized region table.** Never a
  cached "context envelope" that survives `didChange` (buffer version changes invalidate it), and no
  eager classification at `didChange` (wasted work when no request follows) (**I**, from `latency:187`
  S1/S2 memoization rules + `concurrency_level(1)` `latency:19`).
- **Split:** L1 region table (frontmatter range, code ranges, template depths) = O(document) **once
  per `(uri, version)`**, lazy, memoized — shared by hover/diagnostics later; L2 ladder = O(line)
  **per request**, ≤1 ms (`latency:187` S2, `latency:302-307`). Today L1 is a lexical scan
  (`part-codebase.md:105-110`); when ticket 11 spans land, L1 becomes span lookup behind the same
  interface.
- **`Invoked` / `TriggerForIncompleteCompletions` / no-context:** identical classification path
  (`capability-negotiation:135-143`). Re-triggers are recognized from the server's own state —
  *"when `isIncomplete: true` was returned, the server itself knows the next request … is a
  re-trigger; track it per-document rather than reading `triggerKind`"* (`capability-negotiation:140-143`).
  23 explicitly requires Invoked fallback + incomplete re-requests (`issues/23:24`), which is only
  satisfiable if classification ignores trigger kind (**I**).
- **Degraded modes:** no `contextSupport` ⇒ unchanged (layer L0 has nothing to read); no ticket-11
  spans ⇒ lexical L1 (correctness preserved, cost memoized); no template `unstable_machinery` ⇒
  22's stored-`Err` degrade flag is threaded to 24 (`issues/22:55` Q15(c)) — classifier declines C15/C16
  rather than emitting wrong-region items (**I**, D5 soundness).

## 8. Inputs to sibling units

1. **Unit 2:** trigger *set* is yours; mechanism = single static flat array (§6); every trigger char
   must be justified as "makes the client ask" — none may be load-bearing for classification (L0).
2. **Unit 3/4:** dispatcher hands one `ContextKind` + prefix range; item shape and resolve-split hang
   off that enum, so the enum (§5) is your input contract.
3. **Unit 5 (snippets):** L3's single-completer discipline means snippet insert shapes are decided
   per context, one list — cite unit 7 §5 against multi-provider union.
4. **Unit 6 (latency):** budget line = "L1 memoized per version + L2 O(line) ≤1 ms" — replace the
   provisional S2 wording (`latency:187`) with this split.
5. **Ticket 34 (packaging):** `scan_marker_prefix` is module-private (`research/23:528`) — the ladder
   needs it widened/colocated; `MarkerPrefix` tri-state is the model for `ContextKind`.
6. **Ticket 29 (capabilities):** nothing in §4–§7 requires a capability; `contextSupport` is read-only
   ornament (already C-N2 in `capability-negotiation:528-530`).

## 9. Open questions (not load-bearing above)

- **Q1 — `[text](target)` ownership.** Does Traces complete standard-Markdown link targets/anchors,
  or deliberately decline and leave them to rumdl? Unit 2 (`part-decomposition:40` bullet 2) +
  ticket 15's ownership table decide; the ladder only requires *recognition* (P3.5).
- **Q2 — MS md LS cursor-side code gate.** Digest shows candidate-side code exclusion
  (`digest:9500,13323` **V**) but no cursor-side gate in `#getPathCompletionContext`
  (`digest:5649-5685`) — absence-of-evidence; re-fetch `pathCompletions.ts` if load-bearing.
- **Q3 — frontmatter `:` completes keys or values?** 19's three-layer completion is keys
  (`issues/19:40-50`) while `: after a valid YAML key` suggests a value context (`issues/19:82`);
  C6 vs C7 boundary belongs to 19/20. Classifier emits "after-key" position only.
- **Q4 — `- [` inside blockquote/task nesting** (`> - [`, deeply indented): line-shape predicate must
  strip `>`/indent prefixes the way the parser's item scan does (`src/note/parser/list.rs` marker
  buffering) — implementation detail, verify with fixtures.
- **Q5 — L1 memo key.** Is `(uri, version)` sufficient (vs `(uri, version, line)` micro-caching)?
  Decide with unit 6's S1 instrumentation (Q1 there).

## 10. Ready-to-use statements

1. **Architecture**: a single cursor→context classifier over layered inputs (L0 hints / L1 region /
   L2 line-local ladder / L3 one-completer dispatch), first-match-wins, `None` ⇒ empty list. Variants
   (a) and (c) are this shape; (b) is unrepresentable in LSP and fails without `context`.
2. **Precedence**: P0 code veto → P1 region (frontmatter / `{{ }}` / body) → P2 nested isolation →
   P3 Markdown ladder (wikilink > footnote > task > inline-field > link-target > heading/ATX > guards
   > default) → P4 YAML ladder; fall-throughs exactly as 19/22/23 state them.
3. **Registration**: static `CompletionOptions.trigger_characters` at `initialize`, config-derived,
   never per-context; dynamic re-registration only for whole-set config changes, gated on
   `completion.dynamicRegistration`.
4. **Timing**: classify per request against a region table memoized per `(uri, version)`; identical
   path for `Invoked`/`TriggerCharacter`/incomplete/no-context; re-triggers tracked server-side.
