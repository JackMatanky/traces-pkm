# Ticket 24 — Completion architecture: consolidated research

Date: 2026-10-07. Status: research consolidation for the **grilling session** on
[issues/24-completion-architecture.md](../issues/24-completion-architecture.md) (`Status: claimed`,
not resolved). This file integrates every ticket-24 research input into one place; the per-unit
files remain the detailed record and are cited inline — they live in
[`units/`](units/) (moved there 2026-10-07; citation tags resolve via this table, so the
`tag:line` citations are path-independent).

**Citation shorthand** (file : line):

| Tag | File |
| :-- | :--- |
| `dispatch` | [24-completion-dispatch-architecture.md](units/24-completion-dispatch-architecture.md) (unit 1) |
| `trigger` | [24-completion-trigger-characters-rumdl.md](units/24-completion-trigger-characters-rumdl.md) (unit 2) |
| `item-shape` | [24-completion-item-shape-and-ranking.md](units/24-completion-item-shape-and-ranking.md) (unit 3) |
| `resolve` | [24-completion-resolve-lazy-detail.md](units/24-completion-resolve-lazy-detail.md) (unit 4) |
| `snippet` | [24-snippet-completion-insertions.md](units/24-snippet-completion-insertions.md) (unit 5) |
| `latency` | [24-completion-latency-architecture.md](units/24-completion-latency-architecture.md) (unit 6) |
| `novel` | [24-novel-embedded-context-precedent.md](units/24-novel-embedded-context-precedent.md) (unit 7) |
| `capability` | [24-completion-capability-negotiation.md](units/24-completion-capability-negotiation.md) (unit 8) |
| `zed` | [24-zed-client-reality.md](units/24-zed-client-reality.md) (Zed primary-target verification pass, 2026-10-07) |
| `crates` | [24-part-crates.md](units/24-part-crates.md) |
| `codebase` | [24-part-codebase.md](units/24-part-codebase.md) |
| `decomp` | [24-part-decomposition.md](units/24-part-decomposition.md) |

**Corpus caveat** (all units): `docs/refs/lsp_spec.md`'s completion body is an unmaterialized Jekyll
include (`{% include_relative language/completion.md %}`, `lsp_spec.md:659`) — the local file has TOC +
changelog only. Every spec quote below comes from the live LSP 3.18 primary source
`microsoft/language-server-protocol/.../language/completion.md`, fetched 2026-10-06 (tagged **[S]** in
unit files). Evidence markers: **V** = verified against source this pass, **R** = carried from a
digest-citing research file, **[question]** = unverified.

**Ground fact** (user, 2026-10-07): **Zed is a primary target editor.** The original eight units swept
VS Code / Neovim / nvim-cmp / Helix for client behavior and left Zed as open questions; the `zed` pass
closed those (fan-out, sessions, ordering, resolve timing, capabilities) and is folded into every
section below. Zed line numbers are against `zed-industries/zed` @ main fetched 2026-10-07.

---

## 0. The four decision bullets at a glance

Ticket 24's four bullets, answered (details in §1–§4; the seams and conflicts in §7–§9 are what the
grilling should pressure-test):

| # | Bullet | Recommendation | Owning unit(s) |
| :-: | :--- | :--- | :--- |
| 1 | Context-detection strategy | **Variant (c) layered hybrid**: ONE classifier over layered inputs — L0 protocol hints (never classification) → L1 region→language (frontmatter / `{{…}}` depth / code / body) → L2 line-local predicate ladder → L3 first-match-wins dispatch to exactly one completer; `None` ⇒ `[]`. Per-context independent registration (b) is **not expressible** in LSP (flat `triggerCharacters` array) and fails when `context` is absent. | unit 1 + 7 |
| 2 | Trigger-character set + rumdl | **Static, config-derived flat set at `initialize`**: `[`, `#`, `:`, `@`, `.`, `^`, `>` (`"`/`(` deferred to 21-Phase-2/17). Not registered: `/`, `-`, `` ` ``, space, `|`, `]`, word chars. rumdl claim **verified** (registers `` ` ``(,`#`,`/`,`.` `-` iff `enableLinkCompletions`); with ticket 32's recommended flags the two sets are **disjoint by construction**; without them overlap is benign for *correctness* (every client merges into one popup — VS Code per-provider gating, Zed union-gate fan-out `lsp_store.rs:7910-7941`) but real for *request volume* in Zed (every gated char reaches rumdl too) → rumdl must `null`-decline non-owned positions, and **both servers must keep `isIncomplete: false` by default** (Zed OR-couples it across servers). | unit 2 + 8 + zed |
| 3 | Cheap initial list vs lazy resolve | **`resolveProvider: Some(true)`; defer `documentation` only.** Initial = label/kind/sortText/filterText/textEdit/detail (+`data`); resolve = documentation via a self-contained stateless `data` key. Never defer immutable fields, never defer `detail` (live fallback slot), no `itemDefaults` (crate-blocked). Resolve gets its own proposed budget p95 ≤ 5 ms. | unit 4 (shape from unit 3) |
| 4 | Snippet support | **Gate on `snippetSupport`; one v1 snippet: C1 wikilink** `[[T\|${1:}]]$0` inside the single `textEdit.newText`. Every other context is permanently plain because its insertion ends at the cursor (no capability gap). Degradation is honest: off ⇒ user types `\|` themselves; `command`-based cursor placement is not available (no client command handler). **Open:** the C1 range choice (§9.1) can reduce this to *zero snippets in v1*. | unit 5 (joint with unit 3) |

Cross-cutting architecture the four bullets sit on (units 3/6/8): one item shape everywhere
(single `CompletionTextEdit`, `sortText`/`filterText` always set, no `commitCharacters`), `COMPLETION_CAP = 100`
with `isIncomplete = produced > cap`, static registration only, an ~11 ms typical stage budget against
ticket 33's <20 ms p95 under `concurrency_level(1)`, and a 14-capability gating matrix handed to ticket 29.

---

## 1. Decision 1 — Context detection & dispatch

### 1.1 Why not variant (b) or plain (a)

- **(b) per-context trigger registration is unrepresentable**: `triggerCharacters` is one flat
  `Vec<String>` on the single `completionProvider`; `CompletionRegistrationOptions` adds only a document
  selector (`crates:725-729`, **V**; **[S]** registration options). "Register `#` only outside code
  blocks" would require repeated re-registration of `textDocument/completion` — client-dependent
  (`trigger:162`), races keystrokes, and buys nothing a server-side guard doesn't already do.
- **`CompletionContext` is optional and unreliable in both directions**: the flag only gates
  *advertised* support — Helix never advertises `contextSupport` yet **always sends** `context`, and
  MS markdown LS is written to survive `context: undefined` (**V**, `dispatch:41-44`,
  `capability:124-129`; **[S]**; `crates:287-290`) → classification must not depend on it, and
  reading the flag to decide whether to trust `context` "would be wrong about Helix"
  (`capability:127-129`). rumdl's own handler degrades to "full detect runs" when `context` is
  missing (`trigger:65`, **V**) — the ecosystem pattern.
- **(a) pure AST walk is the right engine but the wrong packaging**: it must still handle line-local
  fall-throughs (task markers, footnote `[^`, `#` guards) and pre-span code. Hence the layered hybrid.

### 1.2 The architecture (unit 1 §4–§7, **V** against this repo)

Four layers evaluated per `textDocument/completion`, first-match-wins, **one list out** (unit 7:
"one region classifier → one completer → one list; decline = `null`", `novel:185-189`):

- **L0 — protocol hints, not classification.** Static flat trigger set at `initialize` (§2 owns the
  set; §1 owns the mechanism). `trigger_kind`/`trigger_character` may *skip nothing* in the classifier;
  identical code path for every trigger kind and for absent context.
- **L1 — region → language.** Mutually exclusive host regions: frontmatter → YAML world;
  `{{`/`}}` depth > 0 → MiniJinja/query world; code (fenced/indented/inline) → veto; else Markdown
  body. This mirrors the parser's own `enum BlockContext {None, MetadataBlock, CodeBlock, Text}`
  (`src/note/parser.rs:166-174`) as a **cursor-facing** classifier.
- **L2 — line-local predicate ladder** inside the selected language, each O(line), from primitives
  tickets already specify: `LineIndex::line_at` O(log n) (`src/position.rs:56`),
  `scan_marker_prefix` tri-state (`src/note/parser/marker.rs:70-79,86`), `char_before` look-behind
  (`src/note/parser/lexer.rs:59-69,272-279`), 19's `:` regex, 21's depth counter.
- **L3 — dispatch.** `Option<ContextKind>` → exactly one completer → one list; `None` ⇒ `[]`. **Never
  a union of completers' items** (unit 7 reject-table `novel:212-221`).

**Interface seam (ticket 34)**: one classifier object whose implementation is a lexical scan today and
a span-containment test after ticket 11 (`codebase:105-110`); callers depend only on the enum.
`scan_marker_prefix` is module-private and must be widened/colocated (`dispatch:266-267`).

### 1.3 Precedence for the four known nestings (unit 1 §5)

- **P0 Code veto, first, unconditionally** (19's `CodeRegion`-first rule `issues/19:83`; 16 guard 1).
- **P1 Region decides language** (frontmatter / template depth>0 / body — mutually exclusive). This
  alone resolves every `:` nesting: YAML value vs `[key::` vs `[due:: ` vs value-key colon live in
  different regions and never compete.
- **P2 Nested-language isolation**: inside depth>0 only query/helper/include contexts may fire; else
  **decline — never fall back to body completers** (unit 7 `novel:74-77`). Resolves `#`/`@` inside
  `{{ query.pages("#x") }}` vs body tags.
- **P3 Markdown ladder, first match**: `[[` wikilink → `[^` footnote (must precede the task marker:
  `scan_marker_prefix("[^")` is `Incomplete`, else `- [` misclaims it) → task marker (23's `Incomplete`
  offers / `Rejected` falls to 19 / `Complete` → field layer) → inline field `[k::` → `](` link target
  (recognize, then decline-or-complete per §9.1 of unit 1) → ATX `#{1,6} ` at line start (heading, not
  tag) / `> [`! callout → `#` guards (URL, word-internal) → default body (`#` tag, word-boundary `:`).
- **P4 YAML ladder** (frontmatter): 19's `:` regex → key/value; tag-list value → C5; else decline.

Nesting audit table (`dispatch:207-212`): `#` in `[[l#h]]` → heading > tag (Marksman precedent
*"`[[#f` will be completed as a wiki link, rather than a tag"* **V**); `- [[` → wikilink wins, never a
checkbox; `](` link text never gets body completions.

**Context inventory**: 17 contexts C1–C17 (`dispatch:130-148`) with detection owner per ticket —
the dispatcher supplies *position predicates* only; candidates and comparators stay with tickets
15/16/17/18/19/20/21/22/23. Ticket 23's seam wording is the model: "19 = position predicate,
23 = candidates, 24 = dispatch". **Ticket 18's deferral lands here**: daily-note date completion in a
link target was "dispatch explicitly deferred to 24" (`decomp:13`) — it is inside the `](` rung of
the P3 ladder and counted in C1–C17; nothing else is owed to 18.

### 1.4 Timing, re-triggers, degraded modes (unit 1 §7)

- Classify **per request** against a region table memoized per `(uri, version)`: L1 = O(document) once
  per version, lazy, shared later by hover/diagnostics; L2 = O(line) per request, ≤1 ms. No cached
  "context envelope" across `didChange`, no eager classification at `didChange` (wasted work).
- `Invoked` / `TriggerForIncompleteCompletions` / no-context → identical classification. Re-triggers
  are recognized from **server state** ("my last list for this document was `isIncomplete=true`"),
  never by reading `triggerKind` (`capability:140-143`). 23 requires Invoked fallback + incomplete
  re-requests (`issues/23:24`).
- Degraded: no `contextSupport` ⇒ unchanged; no ticket-11 spans ⇒ lexical L1 (correct, memoized);
  22's template construction-failure degrade flag declines C15/C16 rather than emitting wrong-region
  items (`dispatch:251-254`).

### 1.5 Registration (units 1 §6, 2 §7, 8 §7 — unanimous)

**Static `CompletionOptions.trigger_characters` at `initialize`, config-derived, one flat array.**
Evidence: no per-context primitive exists; Helix advertises no `dynamicRegistration`, Neovim advertises
`false`; one selector must not be registered both statically and dynamically (**[S3]** rule,
`capability:524-526`); Marksman/Markdown Oxide/typescript-language-server all static. Dynamic
re-registration at most handles a mid-session config flip, gated on
`completion.dynamicRegistration == Some(true)` — otherwise document "restart required" (same
asymmetry rumdl has: `trigger:73`).

### 1.6 Novel nested contexts (unit 7)

Precedent matrix over 18 systems (`novel:20-46`): Templater's lexical `tp.\w*(\.\w*)?$` regex is the
closest match for helper-namespace completion; JSON-schema-in-YAML for schema-layered frontmatter;
strongest named precedents for the other rows — IntelliJ language injection, graphql
marker-activation, gopls template AST, Liquid LS (`novel:35-40,49-56`); **no direct match** for
triple-nesting (query DSL inside a template string inside Markdown) — first-principles, bounded by
22's D5 ("completion-offered ⇒ find-resolvable") and 21's token classification. Unit 7's rejection
table (`novel:212-221`): union-of-completers lists; helm-style child-server delegation (2 IPC hops
vs <20 ms, `novel:216`); client-side `embeddedLanguages` injection (not LSP-portable, `:217`);
Templater's region-blind regex as sole detector (`:219`). Note what is **not** a rejection: VS Code
#80889 is *merge* evidence — VS Code merges host/child results, so "no client-side double-popup
mitigation is required" (`novel:42,175-184`); "eager classification at `didChange`" is unit 1's
rejection (`dispatch` §7), not unit 7's. Novelty disclosures for the design doc: triple-nesting,
engine-derived members, three-layer provenance (`novel:253-254`). Unit 7 also registers `(` and `"`
in its classifier row (a) (`novel:85-86`) — see §9.12 for the interaction with §2.2's deferral.
Owed comparator inputs from 22: **local-before-global ordering** and **filter/test/namespace
completion only outside query-string contexts** (`decomp:17`) — carried into §3.1's comparator
contract for 22.

---

## 2. Decision 2 — Trigger characters & rumdl coordination

### 2.1 rumdl facts (unit 2 §1, all **V**)

`rumdl/src/lsp/server.rs:508-526`: registers `` ` ``, `(`, `#`, `/`, `.`, `-` when
`enableLinkCompletions = true`, only `` ` `` when false — with a source comment explicitly designing
for a PKM-LSP co-tenant; docs corroborate (`lsp_rvben-rumdl-docs-digest.txt:1986-1991`). Flag gates:
`enableLinkCompletions` → completion triggers + link branch; `enableLinkNavigation` → definition/
references/hover/rename; `enableSymbols` → document/workspace symbols; fence completion never gated.
Registration is static; `didChangeConfiguration` rewrites config **without** re-negotiating
capabilities (`server.rs:753-790`) → mid-session flips are asymmetric until restart (`trigger:73`).
rumdl's guards after a trigger fires (`server.rs:666-677`): `.`/`-` fast path only if the line contains
`](`; link target requires `](` before cursor → **rumdl structurally never completes wikilinks**.
rumdl's hover is **link-preview-only** — no rule-doc hover exists (`trigger:54`), so
`enableLinkNavigation = false` costs Traces only standard-link *navigation* itself (definition/
references/hover/rename), nothing else. This *resolves* capability unit's open item "rumdl
registration unverified" (`capability:590-591`) and the codebase part's QUESTION
(`codebase:295-301`); **rumdl's own `resolveProvider` remains unverified** (unit 4 Q2,
`resolve:524-527` — matters only if rumdl advertises resolve while co-tenant).

### 2.2 Recommended Traces set (unit 2 §4–§5)

| Register | Chars | Why |
| :--- | :--- | :--- |
| **YES** | `[` | opens wikilink / footnote / inline field / task checkbox sessions (one char, four contexts — the ladder disambiguates) |
| **YES** | `#` | body tags, `[[#heading]]`, query tags in `{{ }}` — 16's five guards run server-side |
| **YES** | `:` | frontmatter key/value, `[key:`, `[due:: ` — 19's regex + region gates |
| **YES** | `@` | query file-class tokens, only inside `{{ }}` |
| **YES** | `.` | helper-namespace members — highest-frequency char; miss path must be ~zero (phase-gate per §9.3) |
| **YES** | `^`, `>` | footnote ref / callout type (provisional — drop if 17 descopes) |
| **NO** | `` ` `` | rumdl always owns fence-language completion |
| **NO** | `/`, `-` | rumdl's `](path)` territory; `-` would fire on every hyphenated word |
| **NO** | space | 23 explicitly rejected it (use `triggerSuggest` + `isIncomplete`) |
| **NO** | `(`, `"` | deferred to 21-Phase-2 / 19 dropped `(` as standalone (`issues/19:84`) |
| **NO** | `|`, `]`, word chars | `|` alias: closed for Zed — typing `|` sends **no request** and closes the menu (`zed` Q3); VS Code likewise won't re-query on an unregistered non-word char → answer must come from the `[[target` response (§9.5). Word chars: spec says identifier chars need not register; sessions sustained by `isIncomplete:true`. |

Design rule distilled from client evidence (`trigger:100`): registers **session-opening characters**
only — every non-word char at which a Traces context can *begin* or *must refresh*. Sustaining a
session across letters is `isIncomplete`'s job. **Registration is single-character only**: clients
inspect only the last typed char (VS Code `suggestModel.ts`), so `[[`, `[^`, `📅 `, `- ` are not
registrable units — this is *why* `^` stands in for `[^` (`trigger:82,132`). **Zed caveat**
(`zed` Impact 1): the session rule must be enforced by *our responses* — Zed gates the union of
trigger chars once, then fans out to **every**
capable server (`lsp_store.rs:7910-7941`), so a Traces-only char such as `[` also reaches rumdl;
expect nothing from client-side selective routing. Also: Zed never queries on multi-char input
(paste, `completions.rs:1527-1529`) — bulk insertion is `Invoked`/absent-context territory (§1.4
already requires identical classification).

### 2.3 Collision matrix & client fan-out (unit 2 §3, §6)

**One popup, always** — same-char collisions merge into a single widget on every verified client;
cost = one extra request + possibly redundant items. The *gating mechanism* differs per client and
the consolidation must not blur it: **per-provider char filtering is verified only for VS Code**
(`suggestModel.ts` providerFilter, `trigger:94`) **and Neovim core** (`trigger:96`); **Helix** fires
if *any* completion-capable server declared the char and drops empty responses (`trigger:14,98`);
**nvim-cmp** merges its sources into one window (`trigger:96`); **Zed** has no per-char filter at
all (below). Under ticket 32's recommended
`enableLinkCompletions = false`, overlap is **empty by construction** (rumdl keeps only `` ` ``).
Under default config the shared chars are `#` and `.` and contexts are provably disjoint
(rumdl needs `](` on the line; Traces' `#` guard excludes `[link](file.md#anchor)`). Registration
cannot express negatives → every guard runs server-side (§1.3 ladder).

**Zed divergence** (`zed` Q1–Q2, verified): Zed has **no per-character provider filter**. The union
trigger gate (per-buffer `BTreeSet`, `buffer.rs:132`/`:3492-3514`) decides only *whether any request
happens*; the request then fans out to every capability+scope-passing server
(`lsp_store.rs:7910-7941`). Consequences folded into ticket 32: (a) one popup always — results merge
into a single `CompletionsMenu`, double popup structurally impossible (`completions.rs:787-788`);
(b) **no LSP-vs-LSP dedup** — items from both servers render side by side if both produce them
(disjoint contexts remain the guard); (c) **`isIncomplete` is OR-ed across servers** with an explicit
upstream TODO (`completions.rs:640-651`) — one `true` forces full re-query of *both* servers every
keystroke → adopt `isIncomplete = produced > cap` discipline on both servers, and rumdl must decline
non-owned positions with `null`/empty, never error (Zed drops per-server errors, `lsp_store.rs:7997-8002`);
(d) Zed joins all server tasks before building the menu with `lsp_fetch_timeout_ms` default `0` =
wait indefinitely (`default.json:2384`) — a slow rumdl delays the whole Traces popup (config knob
exists; note in 32).

A ready-to-adapt coordination statement for ticket 32 (recommended TOML, restart semantics, what
disabling actually withdraws, "no runtime detection is possible") is at `trigger:168-186`.

### 2.4 Latency dimension (unit 6 Q8)

Every registered char fires the full S1–S8 pipeline on keystrokes that previously early-exited; audit
the final set against unit 6's scope-gating design (§5.3) before locking it. The degenerate
"not a context" path must be O(line) ≤0.3 ms and never touch candidate/scoring/serialization stages.

---

## 3. Decision 3 — Item shape, ranking, and the resolve split

### 3.1 One item shape (unit 3 §5–§8; ready-to-paste S-1..S-15 at `item-shape:745-827`)

- Exactly **one `CompletionTextEdit`** with explicit range (single-line, containing the request
  position, insert-range a prefix of replace-range per **[S]**); `insertText` omitted;
  `sortText` + `filterText` on 100% of items; **never `commitCharacters`** (zero corpus precedent);
  no `insertTextMode`, no `tags`/deprecated; no `itemDefaults`/`textEditText`/`applyKind` (crate gaps,
  §6). `additionalTextEdits` reserved but unpopulated in v1; `InsertReplaceEdit` deferred.
- **Dispatcher contract addition**: classification must return the **replace range** alongside
  `ContextKind` — `range.start` is part of the item contract (`item-shape:820-822`).
- **Per-request pipeline order** (`item-shape:439-454`): classify → generate → **dedup by
  context key** → comparator → assign `sortText` → truncate (cap) → build → `isIncomplete`; steps
  1–2 precomputed/cheap, 4–8 inside the stage budget (§5). Per-context dedup keys: 19 = `FieldKey`,
  16 = tag path, 15 = link target. Rank is assigned **after** the comparator and **before**
  truncation. Owed ordering input from 22: local-before-global + filter/test/namespace completion
  only outside query-string contexts (`decomp:17`) — lands in 22's comparator behind the same
  grammar.
- **`command` / stage-2 delivery** (`item-shape:675-681,848-850`): `command` is permitted **only**
  for "re-open suggest after accepting a prefix item"; the *portable* route for 23's stage-2 emoji
  flow is an `isIncomplete` session, because Neovim's core client does not execute completion-item
  commands — confirm at 23's grilling (handoff in §10).
- **`strsim::closest_match` role split** (`crates:674-678`): diagnostic zero-match escalation only —
  never a completion ranker; ranking stays in `rank_candidates` (§9.10 matcher wrapper).
- **`sortText` grammar**: `format!("{group}{rank:04}{}", group_char, rank, label.to_lowercase())` —
  `group` = the context's semantic group (19's layer `0|1|2`, 20's relevance class, task config…),
  `rank` = position under the owning ticket's comparator, assigned after ranking and **before**
  truncation; array emitted already sorted ascending. Four locked per-ticket schemes become four
  comparators behind one grammar (unit 3 §3–§4.2 reconciles all of 15/16/19/20/23's orders).
- **`filterText` co-designed with `range.start`** (R-SHARED-FILTER-WORD): wrong filterText *deletes*
  the item (VS Code `completionModel.ts:198-205`). C4 keeps `"#parent/child …"`; C5 YAML tag **drops**
  the leading `#` form (range excludes `#`); keys = raw `FieldKey`; task status = name only. Always set.
- **label/labelDetails**: `label` = unqualified name; counts → `labelDetails.detail`, provenance badge
  → `labelDetails.description`; when `labelDetailsSupport` absent, counts fall back to `detail`, badge
  appends to `label`. (Amends 19's badge placement; corrects 16's zk citation — zk uses top-level
  `detail`.)
- **`isIncomplete = (candidates_after_dedup > cap)`, nothing else** — amends 20's unconditional `true`
  (unconditional forces a fresh round trip per keystroke under `concurrency_level(1)` for an identical
  response). Degraded/declining contexts return `true` so they don't look final (`dispatch:251-254`).
- **Cap**: `COMPLETION_CAP = 100` (20's number), rank-then-truncate by sortText ascending, one
  config constant. **128 is rust-analyzer's `workspace/symbol` default, not a completion precedent**
  — corrects `issues/23:52`; ticket 16 states no cap at all.
- **Kind**: emit only `CompletionItemKind` 1..18 (avoids the valueSet clamp fork; FOLDER→FILE,
  REFERENCE for footnotes).
- **preselect**: at most one, only on a positively identified best candidate (23's "top" = config
  order + preselect on item 0).
- **Filtering ownership is settled, and the fork is worth stating explicitly**: the alternative to
  server-side filter+rank was (a) client-filter with server order ignored, or (b) server-filter with
  `isIncomplete: true` every keystroke (`crates:767-778`, `capability:387-394` C-N13). We take
  server-side rank-then-truncate with cap-conditional `isIncomplete` — (b)'s request volume is
  unacceptable under `concurrency_level(1)`, and (a) throws away comparator semantics. The
  `sortText` grammar above is what survives client re-sorting (client reality bullet below).
- Client reality check (S-4): VS Code orders by *its* fuzzy score first, `sortText` only ties/empty-
  word → server scores are a selection + intra-group rank signal, not a promised display order.
  **Zed check (`zed` Q4)**: S-2/S-4 verified exactly (tier rungs: score 3, `sortText` 6, kind 7,
  label 8; empty query ⇒ score 0 for all ⇒ `sortText`→kind→label fully orders the menu — our
  100%-`sortText` contract is the right posture, and a *missing* `sortText` sorts *above* provided
  ones, `code_context_menus.rs:1554-1559`). Three Zed-specific clauses to carry into the item
  contract: (a) Zed's filter query is the **cursor's surrounding-word segment**, not
  `range.start..cursor` (`completions.rs:833-845`) → suffix-style `filterText`s (already our design);
  (b) an `OtherMatch` demotion voids `sortText` authority entirely when no split word of `filterText`
  starts with the query's first char (`:1567-1579`) → every `filterText` must keep a split word
  aligned with the typed prefix (true for C1–C8; re-check on any future shape); (c) server order is
  discarded even for empty queries — grouping lives in `sortText`, never in array order.

### 3.2 Resolve split (unit 4 D1–D7, `resolve:341-484`)

- **D1**: advertise `resolveProvider: Some(true)`; defer **`documentation` only** (schema field docs,
  stats prose, previews). Never defer `detail` (it is a live fallback slot for counts), never any
  immutable field (spec: `sortText`/`filterText`/`insertText`/`textEdit` must be present initially and
  must not change on resolve — **[S]**, `crates:114-118`), never `additionalTextEdits`/`command`
  (v1 emits neither). `true` beats the Markdown Oxide/gopls `false` because `false` forces either
  shipping prose every keystroke (blows S5/S8) or dropping 19's headline requirement.
- **D2**: `data` = self-contained, stateless, per-item key ≤ ~120 B:
  `{"c":"fm-key","k":"review_status","v":1}` plus minimal context state (C14 expression position,
  C2 `note#heading`). Key = "what to look up", not "how to recompute" — resolve is a lookup into
  refresh-built artifacts, so neither TS's `cacheId` nor rust-analyzer's position+hash is needed;
  statelessness makes every failure trivially fail-open. **The `c` enum is a wire-stable contract**:
  renaming a context kind is a breaking change for in-flight `data` keys — pin it wherever the
  dispatcher pins `ContextKind` (`resolve:506-509`; contract 12 in §7).
- **D3**: `resolveProvider` unconditional; `resolveSupport.properties` gates **nothing in v1**
  (`documentation` deferrable by default since 3.16) — if later deferring more, gate **per property**
  (rust-analyzer model), never one boolean. Shared sparse-client rule with ticket 23:
  *absent `resolveSupport` ⇒ still defer `documentation` (legal by spec default), defer nothing else,
  never drop* (`resolve:406-412,498-499`) — shipping docs eagerly for sparse clients is exactly the
  S5/S8 blow-up D1 rejects. Escape hatch = `initializationOptions` eager-docs switch, not
  client-name sniffing.
- **D4**: every failure path fail-open (missing/unparsable/stale `data` → return item unchanged; doc
  changed mid-flight → answer anyway, stale-discard handles it; resolve for an item we never deferred →
  no-op). Fail-open rate must be telemetry'd (§D7) — non-zero after refresh means the `data.v`
  invalidation seam is wrong.
- **D5**: **proposed resolve budget p95 ≤ 5 ms (stretch ≤ 2 ms)** — ticket 33 has no resolve row; record
  it there. Handler shape: sync (no `.await`, ticket 12), validate-`data`-first, lookup only — *a
  resolve-time vault scan is a bug*. Shares ticket 12's single gate, so storms (per selection change)
  steal keystroke time — hence the tight number. **Zed sharpens this** (`zed` Q5): resolve fires in
  ~20-item serial batches at menu open, at **every keystroke-refilter**, and per selection change —
  detached, uncancellable across batches, deduped only by a per-item `resolved` flag, and *not*
  reduced by docs-off (`code_context_menus.rs:662-745`, `lsp_store.rs:8164-8166`). So 20×5 ms = 100 ms
  of serialized server work per menu event sits on the **typing path** — the budget is more
   load-bearing than unit 4 modeled. Accept-blocking is **client-dependent**: in Zed it never blocks
   (`zed` Q5, main edit lands before resolve), but Helix `block_on`s and nvim-cmp bounds it at 80 ms
   (`resolve:163-168,359-362`) — VS Code's accept-resolve exists only for `additionalTextEdits`,
   which we never defer. So the budget must serve **both** the display/typing path and the accept
   path; deferring `documentation` remains safe everywhere. Zed never sends trigger-kind 3
   (`completions.rs:531-536` is the sole
   context site) → unit 1's "ignore triggerKind" stance is vindicated; a follow-up `Invoked` at the
   same position may be an incomplete-re-query.
- **D6**: `itemDefaults` **not adopted** (crate-blocked §6; Helix advertises no `completionList`
  capabilities; savings ~20 B/item). Revisit only if `lsp_latency.rs` shows S8 serialization
  dominating at the cap.
- Per-context initial-vs-resolved matrix: `resolve:300-324` (docs deferred on contexts where prose
  exists — 19/16/20-previews; nothing to defer on statuses/callouts/dates).

---

## 4. Decision 4 — Snippets (unit 5)

- **Gate**: `snippets_enabled == (…completionItem.snippetSupport == Some(true))`, read once at
  initialize; absent ⇒ off. Two renderers, one item builder: `render_new_text(snippets_enabled)`; off
  ⇒ plain text, `insertTextFormat` omitted (spec default PlainText), **no snippet string ever
  constructed** (gopls pattern; snippet-only items are never dropped — rust-analyzer/tsls pattern
  rejected). **The gate is not optional**: rejected pattern (d) — Markdown Oxide emits snippets to
  every client ungated, which breaks in eglot (raw `${1:…}` inserted verbatim,
  `snippet:190-194,291`). **S-18 companion rule**: `kind = SNIPPET` (15) must be gated by the *same*
  bool as the format — Helix branches on `kind == SNIPPET` **or** `format == SNIPPET`
  (`snippet:319-321,484-485`), so a plain-text item labeled kind 15 gets parsed and can insert
  *nothing*; never emit kind 15 while `snippets_enabled` is false (ties item-shape Q4 / C13).
- **Master rule**: a snippet is warranted **iff the cursor must end up somewhere the insertion does not
  naturally leave it**. Corollary: for any context whose insertion ends at the cursor, plain text is
  not a degradation — it is the same UX → "never a snippet".
- **Inventory**: **C1 wikilink is the only v1-required snippet** — `[[T|${1:}]]$0` inside the one
  `textEdit.newText`, fallback `[[T]]`. C13 callout body optional/deferred. C2–C17 permanently plain,
  each with its reason (`snippet:408-436`). No grammar beyond tabstops/empty-or-named placeholders/
  one `$0`; no choices, variables, transforms, `insertTextMode`. Scope caveat: "plain ends at the
  cursor" — the corollary's premise — is verified for VS Code/Neovim/Zed/Helix/Obsidian-plugin only
  (Q10, `snippet:528-531`).
- **Multi-cursor**: client-remapped, server-agnostic — rules R1–R9 (`snippet:232-269`): one
  single-line range containing the request position, offset-local body, exactly one `$0`, no duplicate
  placeholder ids, `$`/`}`/`\` escaped in snippet mode, ≤2 stops, and **the plain rendering must
  always be valid Markdown on its own** (Helix silently inserts nothing on parse failure).
- **Honest degradation cost**: off-state loss = C1's alias slot isn't pre-opened (user types `|`);
  cursor placement inside an insertion has **no** non-snippet substitute in Traces (`command` needs a
  client command handler we don't have, `crates:750-756`). Everything else in v1 is unaffected.
- Vehicle contract (locked by unit 3, restated): `insertTextFormat` applies to `textEdit.newText`,
  **not** to `additionalTextEdits` (**[S]** `item-shape:811-814`).

---

## 5. Latency architecture (unit 6)

Four substrate constraints: ticket 33 completion p95 **<20 ms** (stretch <10); ticket 12
`concurrency_level(1)` sequential dispatch with stale-result-discard only (no mid-execution
cancellation; a slow completion head-of-line-blocks `didChange`/hover/diagnostics — and resolve shares
the same gate); ticket 11/14 full re-parse per edit; refresh/query never overlap handlers.

**Stage budget** (`latency:183-194`), typical sum ≈ **11 ms** with ~9 ms slack:

| Stage | Target | Note |
| :--- | :-: | :--- |
| S0 queue wait | (measured) | gate depth under `concurrency_level(1)` — reported, not budgeted (`latency:185`) |
| S1 content access | ≤1 ms | **must memoize per `(uri, version)`** — worst case 18.6 ms for a 51.2 KB long line |
| S2 context detection | ≤1 ms (miss ≤0.3) | L1 memoized per version + L2 O(line); 21's O(document) depth counter is a named tail risk |
| S3 candidate-universe handoff | ≤0.5 ms | precomputed at refresh/initialize, O(1); never vault scans |
| S4 filter + fuzzy score | ≤3 ms | skim V2 ≈1.7 ms @10⁴; empty pattern ⇒ ordering rides `sortText` |
| S5 item build (top-K) | ≤1.5 ms | ≤200 items; docs pushed to resolve |
| S6 byte→UTF-16 range mapping | ≤0.5 ms | `byte_to_utf16_cu` doesn't exist yet (ticket 11); compose byte→char→utf16 |
| S7 sort + cap | ≤0.5 ms | |
| S8 serialize | ≤3 ms | may dominate scoring — unmeasured; cap is the valve |
| S9 stale-discard | (free) | discarded results are the cost of no-cancellation (`latency:194`) |
| degenerate path | ≤0.3 ms | cursor not in context → S2 early exit; **the common case** |

The **<10 ms stretch goal is reachable only with the S7/S8 serialization caps (≤ ~200 emitted
items)** — ticket 33's stretch target is coupled to the cap decision (`latency:180-181`). Queue
math for 33: p95 20 ms ⇒ ≥50 req/s drain vs a 5–10 keystrokes/s typist (`latency:250-253`).

Precompute inventory (`latency:204-219`): `WorkspaceIndex`, unique tags/classes, `VaultFieldIndex`,
first-line previews, schema registry, template/helper metadata, sorted note-path universe, per-doc
`LineIndex` — all built at refresh/initialize with named invalidation points (refresh swap / version
bump / `clear_`), because `WorkspaceIndex` is immutable and swapped between handlers (ticket 12) — no
locks. Rule: anything O(vault) is never built inside the handler; LRU-tiered memos may hold **only
recomputable derivations, never the sole copy of a universe** (`latency:219`). **Prerequisite
inventory**: several entries are *owed by other tickets, not yet present* — `unique_tags()`/
`unique_file_classes()` (21), `SchemaService::list_all`/`suggest_class()` (19), `TaskStatusMap::iter()`
(23), `src/template/analysis.rs` + helper metadata (22), `VaultFieldIndex` (20/33), `fuzzy-matcher`
dep (15) (`codebase:157-161,274-278`) — §5 assumes they exist; §10 lists the owners.

HOL mitigations (leverage order): per-stage wall-clock deadline with graceful degrade (gopls pattern —
return partial top-K, don't fail; **not** gopls's sleep-to-budget padding); build during idle;
scope-gate by intent; cap evaluated (≤10⁴) and emitted (≤100–200) sets; measure before adding
in-handler rayon (YAGNI); stale-waste accounting; **send `ContentModified` when the document changes
mid-handler** so a stale S1 snapshot never ships as valid (`latency:258-259`; unit 4 D4 reserves it
too, `resolve:425`). Named tail risks: S1 long-line re-parse, S2 O(N)
counter, S8 unbounded serialization.

Measurement owed: per-stage `Instant` spans into ticket 33's `lsp_latency.rs` (S8-vs-S4 split, queue
depth/discarded-request rate, universe sizes at 20 K files, resolve-duration histogram per D7).

---

## 6. Capability gates — input to ticket 29 (unit 8)

Full ready-to-paste block C-N1..C-N14 at `capability:514-580`. Summary:

- **Registration**: static at `initialize`, config-derived; never register the same capability both
  statically and dynamically (**[S3]** verbatim rule to quote in 29).
- **Ignore**: `contextSupport` (hint only, dispatcher works without it); `commitCharactersSupport`,
  `tagSupport`, `deprecatedSupport`, `insertTextModeSupport`.
- **Gate**: `snippetSupport` (off ⇒ PlainText, documented cost = no in-insertion cursor placement);
  `resolveSupport` per-property, absent ⇒ *still defer `documentation` (spec default), defer nothing
  else, never drop* (`resolve:406-412` — D3's rule, restated for 29); `documentationFormat` (absent ⇒ plain
  string); `labelDetailsSupport` **and mirror it** in our `ServerCapabilities` (vscode-languageclient
  strips labelDetails on resolve otherwise); `preselectSupport`; `insertReplaceSupport` (never emit
  InsertReplaceEdit in v1); `completionItemKind.valueSet` — hard floor kinds 1..18: we emit only
  1..18 today (FOLDER→FILE, footnote→REFERENCE per §3.1), so the clamp fork disappears **as long as
  19/20's claimed FOLDER/ENUM_MEMBER kinds stay mapped down** — if a future context wants them
  literally, ticket 29 makes the clamp-or-accept call (C-N9, `capability:554-556`); `positionEncoding`
  — UTF-16 default; VS Code *throws*
  on non-utf-16, so utf-8 must be strict opt-in.
- **Unavailable (crate gaps, `ls-types` 0.0.2–0.0.6, verified)**: `CompletionList.itemDefaults`,
  `textEditText`, `applyKind`, and even *reading* `applyKindSupport`. Plus the **phantom**
  `documentationMarkdown` from `decomp:88` — does not exist in spec or crate; use
  `documentationFormat`. `tower-lsp-server`'s `proposed` cargo feature is currently unbuildable
  against ls-types ≥0.0.5 (dangling feature) — CI note for `--all-features`, no completion type is
  gated by it. **Dependency hygiene** (`crates:51-63,196,224-227`): take completion types via
  `tower_lsp_server::ls_types` re-export; **never add `lsp-types`** (structurally identical, will not
  type-check against ls-types types); prefer ls-types 0.0.6, don't pin `=0.0.2` (drags
  `fluent-uri 0.3`).
- **Sparse-client floor (MVC-C)**: every completion capability absent + utf-16 only ⇒ all contexts
  still work (list + sortText + filterText + single-line TextEdit); only presentation-only losses
  (snippets, badges, preselect, markdown docs, insert/replace, kinds >18). Nothing silently breaks.
- **Zed column re-verified** (`zed` Q7, row-for-row): cite drift `resolveSupport` → `lsp.rs:979-989`;
  three advertised-but-unconsumed fields found — `labelDetails` advertised but never read,
  `itemDefaults` honored only for `editRange`+`data`, `commitCharacters` never read anywhere — plus
  `preselect`/`commitCharactersSupport` absent and never read. All of these **widen** the floor: we
  can serve a smaller surface than Zed advertises with no behavior loss (e.g. never emit
  `labelDetails` in Zed-only deployments without loss — but keep it for VS Code, C-N9).

---

## 7. Cross-unit contracts (must survive consolidation)

Ordered, with the producer → consumer edge:

1. **Dispatcher returns `ContextKind` + replace range + filter word**, not just a kind
   (`item-shape:820-822` → units 3/4/5).
2. **One list from one completer; decline = `null`/`[]`; never union** (unit 7 → 1, 3).
3. **L0 triggers are hints; none is load-bearing for classification** (unit 1 → unit 2).
4. **Classification identical for `Invoked`/`TriggerCharacter`/re-trigger/no-context; re-triggers
   tracked server-side** (units 1/8 → 3's `isIncomplete` rule, 23's fallback).
5. **Snippet vehicle = single `textEdit.newText`; `insertTextFormat` does not apply to
   `additionalTextEdits`; no `commitCharacters`; `insertText` omitted** (units 3+5 interlock).
6. **`snippetSupport` gating locked by unit 8 (C-N3)** — unit 5 implements, never re-decides
   (`snippet:272`).
7. **Resolve payload = `documentation` + `data`; `detail` is a live fallback, NOT resolvable**
   (unit 4 D1 ↔ unit 3 S-15/§4.4).
8. **`isIncomplete` = produced > cap** (unit 3 S-8) with re-trigger routing owned by unit 1
   (`dispatch:245-250`); degrade contexts return `true`.
9. **Budget line for classification**: "L1 memoized per version + L2 O(line) ≤1 ms" replaces unit 6's
   provisional S2 wording (`dispatch:264-265`).
10. **Capability gates declared by unit 8, wired by ticket 29** (C-N1..C-N14); unit 2's rumdl
    verification closes capability's open item Q2 (`capability:590-591`).
11. **Workspace-symbol caps and refreshed universes belong to tickets 27/23/33 — not to 24.**
12. **The resolve `data.c` enum is wire-stable** — renaming a context kind breaks in-flight keys;
    pin it beside `ContextKind` (unit 4 `resolve:506-509` → dispatcher/34).
13. **Ticket 24 owns the shared completion-generation function** (owed to 21: generate candidates
    given a resolved context, `decomp:16,50`) — dispatcher and per-ticket comparators call it;
    21's Phase-2 work plugs into it rather than duplicating it.
14. **Config access follows ticket 31's untrusted-roots decision** — completion never reads config
    from untrusted roots (`codebase:185-188` → ticket 31 boundary).

---

## 8. Corrections & amendments to sibling tickets (carry back)

1. **20** (`issues/20:37`): `is_incomplete: true` → **cap-conditional** (§3.1).
2. **23** (`issues/23:52`): "cap 128 (16 precedent)" is a mis-attribution — 128 is RA's
   `workspace/symbol` default; ticket 16 states no cap.
3. **16** (`issues/16:49`): zk citation wrong (zk uses top-level `detail`, no `labelDetails`); the
   decision stands on Markdown Oxide's precedent. (`issues/16:48` "KEYWORD in mdoxide and zk" — zk sets
   no kind on tags.)
4. **19** (`issues/19:44`): badges move to `labelDetails.description` when supported, else append to
   `label` (spec wants unqualified labels). **Also** (`issues/19:48`, `item-shape:734-736`): 19's
   layer `sortText` prefixes are a **tiebreak only** — the layer guarantee actually rests on
   `FieldKey` dedup, so ordering *between different keys* within a layer yields to fuzzy quality.
5. **20** (FOLDER/ENUM_MEMBER): unit 8 records 20 as claiming kinds 19/20 (`capability:105-113`),
   but unit 3's settled shape maps FOLDER→FILE and emits 1..18 only — if 20 wants them literal,
   record the clamp-or-accept decision for 29 (C-N9).
6. **15**: joint-setup comment "`enableLinkCompletions = false  # Traces handles wikilink completion`"
   is imprecise — rumdl never completed wikilinks; the flag is about standard-link completion and
   request hygiene. Also 15's ownership table has **no row** for standard-link completion/navigation,
   the two things the recommended flags switch off (`trigger:188-190`).
7. **`decomp:88`**: `documentationMarkdown` is a phantom capability — never cite it.
8. **`research/21-query-language-intelligence.md:257`**: cites `nucleo-matcher` 0.3.5 — no such
   version exists (max 0.3.1); re-check before reuse (`crates:620-622`).
9. **Open item closed**: capability §10 Q2 (rumdl registration unverified) — resolved by unit 2 §1.
10. **21** (`issues/21:63`): the Phase-2 deferral note now has a designed mechanism —
    position-bearing `data` per D2; Phase 2 only flips it on (`resolve:510-511`).

---

## 9. Conflicts, tensions, and joint decisions for the grilling

These are the places where units disagree, defer to each other, or rest on unmeasured numbers — the
load-bearing agenda:

1. **C1 wikilink range — THE joint decision for bullet 4** (`snippet:495-498` vs `item-shape:616`).
   - Range covers `[[`…cursor **plus an existing trailing `]]`** (unit 3's "paired-region" row): one
     `newText` in every state, link always terminated, **snippet genuinely needed** (plain text can't
     both create `]]` and land the cursor before it), fallback **`[[T]]`** — unit 5 explicitly
     forbids `[[T|]]` as a fallback: with the cursor after `]]` the `|` is unreachable residue
     (`snippet:303-304,379-382,466`), which also matches §0's "off ⇒ user types `|`".
   - Range stops at cursor: the document's own `]]` stays put, plain text already lands the cursor in
     the alias slot (**row C1**) → **Traces ships zero snippets in v1**.
   - Unit 3's paired row currently assumes the former; unit 5 says this is "the highest-leverage
     decision in this file". Product sub-question: does v1 *always* open `[[T|…]]` on accept, or
     Obsidian-style `[[T]]` (user types `|`)? (`snippet:499-503`)
   - Coupled: **Q6 — is `[[T|]]` a valid terminal rendering?** unresolved, and it gates whether the
     *plain* fallback may ever keep the `|` (`snippet:513-515,306-310`, supersedes `crates:753-755`);
     plus a cross-parser acceptance test — emit exact strings against Helix/neovim/VS Code snippet
     parsers before shipping (Q8, `snippet:522-525`). C13's three coupled changes (kind flip to
     SNIPPET, §4's S-18 kind gate, R7 baked `"> "` prefixes) travel with any snippet decision
     (`snippet:402-406`).
2. **Standard-link completion ownership** (`trigger:211-212`, unit 1 Q1): does Traces complete
   `](file.md#anchor` at all? (a) keep rumdl's flag on (overlap benign, best UX); (b) flag off and
   accept the gap (15's current text); (c) flag off and later register `(` ourselves (contradicts 15).
   **Ticket 32 must pick** — (a) or (b) are the only options consistent with existing text. Same shape
   for standard-link *navigation* (`enableLinkNavigation`, ticket 26/15 gap) — note rumdl's hover is
   link-preview-only (`trigger:54`), so turning navigation off costs only definition/references/
   hover/rename for standard links, nothing else.
3. **`.` trigger phase-gating** (`trigger:214`): highest-frequency char; if template helper completion
   (22) ships later, registering `.` early costs a miss-path request per keystroke for nothing →
   config/phase-derived contents rather than a fixed array. Ties to unit 6 Q8 (trigger-set latency
   audit).
4. **Word-char-only contexts** (`trigger:209`): is any Traces context reachable *only* through an
   editor-dependent word-char first keystroke (continuing a tag with no `#` in scope; the `- [t`
   ambiguity)? Decision: `Invoked`-only support vs documented editor setting. **Zed data point**
   (`zed` Q3): word chars do re-query (gated by `trigger_in_words`, no min-length for LSP items —
   `words_min_length` gates only buffer words, Markdown buffer words off by default), always as
   `Invoked`, so an `Invoked`-only context is reachable in Zed after the first letter.
5. **`|` alias completion** (`trigger:210`): **closed for Zed** — typing `|` sends no request and
   closes the menu (`zed` Q3, `completions.rs:399-401`); VS Code likewise won't re-query on an
   unregistered non-word char. Remaining question is only the delivery vehicle: register `|`
   (every table row fires a Traces-guarded miss path) vs return an `isIncomplete` alias list at
   `[[target` time (works everywhere, alias offered before `|` is typed). Neovim/Helix unverified
   for `|` but both support registration, so either vehicle is expressible.
6. **Server ranking vs client re-sort** (`item-shape:767-771` + Q1): VS Code orders by *its* fuzzy
   score first — our `group` tiers can be outranked by the client's matcher. Zed is the same shape
   with a verified tier chain (score strictly above `sortText`, `zed` Q4) and the `OtherMatch`
   demotion clause (§3.1). Is that acceptable for every context (empty-word input is where `sortText`
   fully controls order in both clients)?
7. **Unmeasured budget numbers**: resolve p95 ≤5 ms (D5, reasoned not measured); S8 serialization vs
   S4 scoring dominance (unmeasured); 21's O(document) depth counter cost; S1 long-line memoization
   need; in-handler rayon (default: don't); **Q9 cross-doc heading-universe residency vs lazy redb
   enumeration with a cap** — the most expensive universe, deferred until Q7 numbers
   (`latency:293-295`). All → `lsp_latency.rs` before ticket 33 adopts rows.
8. **Client-behavior unknowns** (none load-bearing): ~~Zed completion fan-out/resolve timing~~
   **CLOSED** by `zed` (Q1–Q7); remaining: rumdl's own `resolveProvider` (unit 4 Q2,
   `resolve:524-527`); Marksman's registered-but-unadvertised resolve handler
   (source fetch 404'd); whether Neovim built-in honors `preselect`; VS Code resolve with details
   pane closed (read as yes); multi-cursor edge cases (Q7 in `snippet:516-521` — recommend a
   5-minute manual matrix before 29 documents support); Zed's minor leftovers (`zed` closed/open
   table): keep-menu branch under multi-cursor selections, mouse-hover→selection mapping inside the
   menu (affects resolve-trigger completeness only); minor unit open items — MS md LS cursor-side
   code gate is absence-of-evidence with a re-fetch instruction (`dispatch:276-278`), C13
   `Keyword`-vs-`Snippet` kind (I-Q4, `item-shape:846-847`, drags §9.1's C13 row in), per-context
   cap override (I-Q6, `item-shape:851-853`).
9. **Kind clamp product call** (`capability:599-600`): **not moot** — resolved only *conditionally*.
   We emit 1..18 today by mapping FOLDER→FILE (§3.1), which makes the valueSet clamp fork
   disappear; but unit 8 records 19/20 claiming FOLDER/ENUM_MEMBER, so the clamp-or-accept decision
   (C-N9) is owed to 29 the moment a context wants those kinds literally (§8.5).
10. **Fuzzy matcher choice** (`crates:614-636`): `fuzzy-matcher` (settled by ticket 15, unmaintained
    since 2020) is adequate at Traces' volumes; `nucleo-matcher` is the credible upgrade path *if*
    measurement shows scoring at fault — architecture must not hard-code matcher internals (wrap in
    `rank_candidates`).
11. **Frontmatter `:` — keys or values?** (`dispatch:279-281`): 19's three-layer completion targets
    *keys* (`issues/19:40-50`) while "`:` after a valid YAML key" suggests a *value* context; the
    classifier emits "after-key" only and C6-vs-C7 ownership stays with 19/20 — an unresolved
    boundary the grilling should settle (unlike pure impl-detail Q4/Q5).
12. **Query-string trigger entry** (`novel:85-86` vs §2.2): unit 7's classifier row (a) registers
    `(` and `"` to open query-string contexts, but §2.2 defers both to 21-Phase-2/19 — so *how a
    query-string request starts in v1* is unstated: today it can only be `Invoked`/word-char entry
    (or an `isIncomplete` continuation). Decide: accept Invoked-only entry for v1, or pull `(`
    forward into the v1 trigger set (it would also fire rumdl's link path — collision check §2.3).
13. **C14 expression-position staleness** (`resolve:539-541`): does key lookup + `ContentModified`
    suffice for resolving inside a live template expression, or does 21's Phase-2 resolve need a
    per-document version stamp in `data`? Open — carries into 21's handoff (§10).

Open questions per unit for reference: dispatch Q1–Q5 (`dispatch:271-286`), trigger Q1–Q7
(`trigger:207-215`), item-shape Q1.. (`item-shape:831+`), resolve Q1–Q7 (`resolve:518-544`),
snippet Q1–Q10 (`snippet:493-531`), latency Q1–Q9 (`latency:263-295`), novel Q1–Q7
(`novel:225-240`, esp. Q5 `enumerate()` side effects, Q6 Neovim/nvim-cmp two-source merge vs stack),
capability Q1–Q8 (`capability:584-608`), crates q1–q6 (`crates:783-800`). **Closed by `zed`:**
trigger Q1/Q2/Q7, resolve Q3, item-shape S-5 refinement, capability §6 Zed column — status table at
`zed:395-407`.

---

## 10. What each downstream ticket gets from this file

| Ticket | Input |
| :--- | :--- |
| **29** (capability negotiation) | C-N1..C-N14 block (`capability:514-580`); sparse-client floor; crate gap list (§6) |
| **32** (rumdl coexistence) | Coordination statement `trigger:168-186`; flag semantics + restart asymmetry; corrections to 15's wording; **Zed clauses** (`zed` Impact 1–2): union-gate fan-out ⇒ rumdl receives Traces-triggered queries (must `null`-decline non-owned positions, never error), shared `isIncomplete` OR-coupling ⇒ both servers default `false`, and the unbounded `join_all` (slow server delays both popups, `lsp_fetch_timeout_ms` knob) |
| **33** (performance) | Stage budget S0–S9 + degenerate path (§5); precompute inventory; proposed S-R resolve row (≤5 ms) **now justified by Zed's per-keystroke resolve batches** (§3.2 D5); measurement questions Q1–Q9 |
| **15/16/17/19/20/21/22/23** | Corrections in §8 (incl. 19's tiebreak-only layers, 20's FOLDER kind, 21's Phase-2 mechanism); comparator/filterText/label contracts + pipeline order in §3.1; 22's owed ordering inputs; the C1 range decision (§9.1) back to 15; **`command`/stage-2 delivery confirmation to 23** (§3.1); **owed precompute prerequisites** for §5's inventory — `unique_tags`/`unique_file_classes` (21), `SchemaService` list-all/`suggest_class` (19), `TaskStatusMap::iter` (23), template analysis/metadata (22), `VaultFieldIndex` (20/33), `fuzzy-matcher` dep (15) (`codebase:157-161,274-278`) |
| **18** (daily notes) | Its deferral is satisfied — date completion in `](` lives in the P3 ladder/C1–C17 (§1.3); no further input owed |
| **31** (config trust) | Completion config access must route through the untrusted-roots decision (§7.14) |
| **34** (packaging) | Classifier seam + `scan_marker_prefix` visibility widening (`dispatch:266-267`) |
| **09/12/11/14** (substrate) | No changes; all units consumed them as settled constraints (`decomp:5-21`) |

---

## Sources

**This ticket's research (all under `.scratch/md-pkm-lsp/research/`)**: `24-part-decomposition.md`
(8-unit spec + resolved-ticket constraints), `24-part-codebase.md`, `24-part-crates.md`,
`24-completion-dispatch-architecture.md`, `24-completion-trigger-characters-rumdl.md`,
`24-completion-item-shape-and-ranking.md`, `24-completion-resolve-lazy-detail.md`,
`24-snippet-completion-insertions.md`, `24-completion-latency-architecture.md`,
`24-novel-embedded-context-precedent.md`, `24-completion-capability-negotiation.md`,
`24-zed-client-reality.md` (primary-target verification pass, 2026-10-07).

**Primary spec**: LSP 3.18 `language/completion.md` (fetched 2026-10-06) — `triggerCharacters`
236-256, registration 300-310, trigger kinds 330-355, context 372-376, item fields/textEdit rules
845-871, resolve rules 273-300/917-929, snippet grammar ~1039, label details 762-767.

**Code (local)**: `src/note/parser.rs:166-174`, `src/note/parser/marker.rs:70-113`,
`src/note/parser/lexer.rs:59-69,272-279`, `src/position.rs:31-56`, `src/schema/model.rs:71,129`,
`src/strsim.rs:11-21`, `Cargo.toml`/`Cargo.lock` (no LSP crates pinned yet), rumdl upstream
`src/lsp/server.rs:508-526,641-700,753-790`.

**Clients/ecosystem**: VS Code `suggestModel.ts`/`completionModel.ts`/`suggestWidget.ts`, Neovim
`vim/lsp/completion.lua`, nvim-cmp `source.lua`, Helix `handlers/completion.rs`, **Zed** (`crates/editor/src/completions.rs`,
`crates/editor/src/code_context_menus.rs`, `crates/project/src/lsp_store.rs`, `crates/lsp/src/lsp.rs`,
`crates/language/src/buffer.rs`, `crates/editor/src/input.rs`, `crates/editor/src/selection.rs`,
`crates/fuzzy/src/strings.rs` — fetched 2026-10-07, line numbers per `zed` file), plus prior-art
servers (Marksman, Markdown Oxide, zk, MS markdown LS, typescript-language-server, rust-analyzer,
gopls) — per-unit source lists.
