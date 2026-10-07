# Ticket 24 part-work: completion trigger characters, registration lifecycle & rumdl coordination (unit 2 → tickets 24/32)

Unit: **`completion-trigger-characters-rumdl`** — per [24-part-decomposition §2](24-part-decomposition.md) (lines 37-43). Input artifact for ticket [24](../../issues/24-completion-architecture.md) decision bullet 2 and for the documented boundary owed to ticket [32](../../issues/32-rumdl-coexistence-boundary.md). Planning artifact only; no production code.

**Method.** Claims are marked **V** (verified this pass against the cited primary source), **R** (carried from an earlier research file/digest — re-verify before load-bearing use), **I** (inference from verified facts). Live sources fetched **2026-10-06**: rumdl `main`, VS Code `main`, Helix `master`, Neovim `master`, nvim-cmp `main`, `cmp-nvim-lsp` `main`, typescript-language-server `master`, microsoft/language-server-protocol `gh-pages` 3.18. Digest line numbers are the exact lines read. "Source demands" = facts about Traces' own tickets/research (cited to issues/research files); "ecosystem practice" = what other systems do (cited to sources). Open questions are §10 — nothing there is load-bearing.

---

## 0. Headline answers

1. **The map's claim is TRUE.** rumdl registers `` ` ``, `(`, `#`, `/`, `.`, `-` when `enableLinkCompletions` is on, and only `` ` `` when it is off — verified in `src/lsp/server.rs` (§1.1), and stated verbatim in rumdl's own docs (`lsp_rvben-rumdl-docs-digest.txt:1986-1991`). Wave-1's `research/tool-rumdl-boundary.md` has zero trigger hits only because it read the *docs* file for the settings contract, not the capability construction — the docs in fact state the same thing.
2. **The three rumdl flags gate different capabilities** (§1.2): `enableLinkCompletions` → completion `trigger_characters` *and* the link branch of the completion handler; `enableLinkNavigation` → definition/references/hover/rename (+ prepareRename), double-gated (capability + per-request check); `enableSymbols` → `documentSymbol` + `workspaceSymbol`. Fenced-code completion (` `` ` ``) is never gated.
3. **Registration is static at `initialize`**; the only dynamic registration rumdl ever makes is `workspace/didChangeWatchedFiles`. Config changes via `didChangeConfiguration` re-read per request but do **not** re-negotiate capabilities → asymmetry documented in §1.4 (matters for ticket 32's setup docs).
4. **A same-character collision between two servers is benign in every mainstream client** (§3): VS Code fans out only to providers that registered that char and *merges* into one widget; Neovim core fans out to matched clients only; nvim-cmp keeps one source per client and merges; Helix drops empty responses and merges. The real cost is **duplicate requests + redundant items**, never double popups.
5. **Recommended Traces set** (§4): `[`, `#`, `:`, `@`, `.`, `^`, `>` (+ `"`/`(` deferred with 21 Phase 2); **deliberately not registered** (§5): `/`, `-`, `` ` ``, space, `|`, `]`, all identifier/word characters. Registration: **one static set, config-derived at `initialize`** (§7); every guard runs server-side because registration cannot express negatives (§2).

---

## 1. rumdl — registered set, gating, guards, lifecycle (verified)

### 1.1 The registration itself

`https://raw.githubusercontent.com/rvben/rumdl/main/src/lsp/server.rs` — `initialize()` builds `InitializeResult { capabilities: … }` directly (static registration), reading three booleans from `self.config` **after** `initialization_options` are parsed and `.rumdl.toml` auto-discovery runs (`server.rs:414-417`, `:466-475`):

```rust
// Completion always stays available for fenced code-block language
// labels (backtick trigger). The link-target triggers (`(` `#` `/`
// `.` `-`) are only registered when link completions are enabled, so
// a client with its own link-completion source (e.g. a PKM-focused
// LSP) is not invoked on those characters when the feature is off.
completion_provider: Some(CompletionOptions {
    trigger_characters: Some(if enable_link_completions {
        vec!["`".to_string(), "(".to_string(), "#".to_string(),
             "/".to_string(), ".".to_string(), "-".to_string()]
    } else {
        vec!["`".to_string()]
    }),
    resolve_provider: Some(false),
    …
}),
```
(`server.rs:508-526`, **V**) — this comment is rumdl explicitly designing for a PKM LSP co-tenant.

Docs corroborate (`docs/digests/lsp_rvben-rumdl-docs-digest.txt:1986-1991`, **V**): "If you use another language server for link completion (for example a PKM/notes LSP) … disable it with `enableLinkCompletions` … When disabled, rumdl returns no link suggestions and **does not register the link-target trigger characters (`(`, `#`, `/`, `.`, `-`)**, so it is not invoked on them; fenced code-block language completion still works."

### 1.2 Gating matrix (what each flag actually buys)

| rumdl flag | Default | Gates at `initialize` (capability absent) | Gates at request time (returns `null`/`[]`) |
| :--- | :--- | :--- | :--- |
| `enableLinkCompletions` | `true` | `trigger_characters` = 6 vs 1 (`server.rs:514-522`) | completion handler link branch (`server.rs:667`) |
| `enableLinkNavigation` | `true` | `definition_provider`, `references_provider`, `hover_provider`, `rename_provider` (+`prepare_provider`) (`server.rs:531-537`) | `definition`/`references`/`hover`/`rename`/`prepare_rename` (`server.rs:1389,1401,1413,1425,1437`) |
| `enableSymbols` | `true` | `document_symbol_provider`, `workspace_symbol_provider` (`server.rs:500-501`) | `document_symbol`/`workspace_symbol` (`server.rs:1490,1512`) |

All **V**. Note `hover` is a **link preview only** (`src/lsp/navigation.rs:545-589`, **V** — no rule-doc hover in the LSP), so `enableLinkNavigation=false` costs Traces nothing it must replace except standard-link navigation itself.

### 1.3 Server-side guards rumdl applies after a trigger fires (precedent for Traces' miss path)

| Guard | Where | Effect |
| :--- | :--- | :--- |
| Fence branch runs **first**, ungated | `server.rs:650-664` **V** | `` ` `` inside ```` ```lang ```` completes languages even with link completions off |
| `.`/`-` fast path: only if the *current line* contains `](` | `server.rs:666-677` **V** — comment: "For trigger characters that fire on many non-link contexts (`.`, `-`), skip the full parse when there is no `](` on the current line … avoids needless work on list items and contractions" | turns a high-frequency char into an O(line) miss |
| Link target requires `](` before cursor | `completion.rs` `detect_link_target_position` (`lsp_rvben-rumdl-src-digest.txt:29706-29710`) **V** | rumdl **structurally never completes wikilinks** |
| Closed `](…)` → `null` | digest `:29713-29715` **V** | no re-completion after the link is finished |
| Odd backtick count before `](` → `null` | digest `:29717-29721` **V** | code-span heuristic |
| Missing `context` (client without `contextSupport`) | `server.rs:671-677` **V** | `trigger_character = None` ⇒ fast path disabled, full detect runs — i.e. **trigger context is a hint, never the classifier** |

This is the pattern `24-part-crates.md` §"Trigger-character registration" #1-#2 asks for: registration decides *when a request arrives*; the dispatcher decides *what it means*.

### 1.4 Lifecycle & the mid-session-flip asymmetry (for ticket 32's docs)

- Capabilities are computed once from `initialization_options` + discovered config (`server.rs:414-417, 466-475, 478+`). rumdl's only `register_capability` call is for `workspace/didChangeWatchedFiles` (`server.rs:630-636`, **V**); `did_change_configuration` rewrites `self.config` in place with **no** capability re-registration (`server.rs:753-790`, **V**).
- rumdl's docs state the contract: "Capability flags like `enableSymbols` are negotiated when the server starts, so changes take effect after the server (re)starts" (`…docs-digest.txt:2160-2163`, **V**).
- **Consequence I:** flipping `enableLinkCompletions` *true→false* mid-session stops rumdl returning link items but leaves `(`/`#`/`/`/`.`/`-` registered until restart (residual, harmless request fan-out). Flipping *false→true* mid-session makes link completion **unreachable except by manual `Ctrl+Space`** until restart, because the triggers were never registered. Setup documentation must say "set these before the server starts (or restart after)".

---

## 2. What the spec actually demands (primary: LSP 3.18 `language/completion.md`, fetched 2026-10-06)

| Fact | Citation | Consequence for Traces |
| :--- | :--- | :--- |
| `triggerCharacters` exists so completions fire on characters "**not being valid inside an identifier** (for example, `.` in JavaScript)"; "**Characters that make up identifiers don't need to be listed here**" | `completion.md:236-256` **V** | Registering letters is unnecessary *and* non-portable — word-char popups come from the editor's own quick-suggest rules |
| `triggerCharacter` in `CompletionContext` is "a single character", defined only when `triggerKind === TriggerCharacter` | `completion.md:330-376` **V** | Register **single characters only**. (`[[`, `[^`, `- `, `📅 ` are not registrable as units — VS Code inspects only the last typed char, §3.) |
| Trigger kinds: `Invoked: 1`, `TriggerCharacter: 2`, `TriggerForIncompleteCompletions: 3` | `completion.md:336-355` **V** | 23's `Invoked` fallback + `TriggerForIncompleteCompletions` re-requests are spec-sanctioned paths |
| `contextSupport` (client) governs whether `context` is sent | `completion.md:45, 324` **V** | classification must not *require* `context` (§1.3 last row) |
| `triggerCharacters` is a **flat `Vec<String>` on the single `completionProvider`**; `CompletionRegistrationOptions extends TextDocumentRegistrationOptions, CompletionOptions` adds only a document selector | `completion.md:300-310` **V**; `24-part-crates.md:725-729` | **Per-context registration is not expressible** — no way to say "register `#` only outside code blocks". All negatives (code spans, URLs, word-internal, `[[#…]]`) are server-side guards |
| The spec says **nothing about multiple servers** registering overlapping characters — that is client behavior | §3 below | coordination is a documentation problem (ticket 32), not a protocol problem |

---

## 3. Client fan-out: what actually happens when two servers register the same character

| Client | Mechanism (verified) | Result of a Traces+rumdl `#`/`.`/`(` collision |
| :--- | :--- | :--- |
| **VS Code** | `suggestModel.ts:276-296` **V**: `supportsByTriggerCharacter` maps char → the providers that declared it; `onDidType` → `trigger({triggerKind: TriggerCharacter, triggerCharacter: lastChar, providerFilter: supports, providerItemsToReuse})`, where providers **not** in `supports` have their existing items reused | One merged widget; both servers get exactly one request each; **no double popup** (VS Code #80889, cited in `24-novel-embedded-context-precedent.md:176`) |
| **VS Code, subsequent typing** | `suggestModel.ts:767-785` **V**: cursor-right on a non-empty word + an incomplete model ⇒ re-trigger with `TriggerForIncompleteCompletions`, `providerFilter` = only providers whose items are `incomplete`, others reused. A **non-word, unregistered** char does *not* re-query (falls to the "update UI" branch) | word-char continuation is sustained by `isIncomplete: true` ⇒ no need to register letters; a non-registered non-word char mid-session does not refresh the list |
| **Neovim core** (`vim/lsp/completion.lua`) | `:~1225-1345` **V**: `handle.triggers[char]` = list of clients; only `matched_clients` are queried; `TriggerCharacter` context built from `client.server_capabilities.completionProvider.triggerCharacters`; `autotrigger` defaults **false**; `:1202-1206` re-requests with `TriggerForIncompleteCompletions` while `Context.isIncomplete` | only servers that registered the char are bothered; others never see the request |
| **nvim-cmp** (+ `cmp-nvim-lsp`) | `lua/cmp/source.lua:213-228, 296-313` **V**: per-source trigger list (config override → source's `get_trigger_characters` → LSP `triggerCharacters`, returned verbatim by `cmp-nvim-lsp/…/source.lua:74-75`); `:307` re-requests `TriggerForIncompleteCompletions` when `self.incomplete` and the keyword length is satisfied | one source per client, merged into one menu; two servers = two sources, merged |
| **Helix** | `handlers/completion.rs:118-180` **V**: fires if **any** completion-capable server's `trigger_characters` match (`text.ends_with(trigger)`), plus path `/` and word-char auto-trigger via `completion-trigger-len`; `completion.rs:41` **V** drops responses that are empty *and* not incomplete (`continue`), so one server's `null` doesn't cancel the other's; `completion.rs:173-187` **V** clears the popup on any non-word char then `trigger_auto_completion`, and word chars with an incomplete list go to `request_incomplete_completion_list` (`request.rs:341-363`, `TriggerForIncompleteCompletions`) | merged, tolerant of empty responses; **any non-registered non-word char kills the open popup** |

**I (design consequence).** Two facts combine: (a) clients merge, so duplicate items are a *quality* problem, not a UX-structure problem; (b) an unregistered non-word character either refreshes nothing (VS Code) or clears the popup (Helix). Therefore the trigger set should be built from **session-opening characters** — every non-word character at which a Traces context can *begin* or *must refresh* — and nothing else. Sustaining a session across letters is `isIncomplete: true`'s job; opening it inside a `[`-or-`#`-started session is the trigger's job.

---

## 4. Traces' trigger inventory — char × contexts × guards × mechanism × fallback

Registration mechanism for every row: **static `Vec` at `initialize`, config-derived** (§7). Fallback column = what happens for contexts where the char is *not* the session opener.

| Char | Contexts it serves (sessions it opens) | Guards that must run **before** offering anything (registration cannot express them) | Ticket / source | Register? |
| :--- | :--- | :--- | :--- | :--- |
| `[` | wikilink `[[…]]`; footnote `[^…]`; inline field `[key:: …]`; task checkbox `- [`; also the bracket that precedes `](…)` | `CodeRegion` exclusion runs **first**; then classify: `[[`→wikilink, `[^`→footnote, `- [`→23's `scan_marker_prefix` predicate (`Incomplete` offers / `Rejected` falls to 19's field layer / `Complete`→value layer; the `- [t` ambiguity is owed to 24), `[…::`→inline field, else **return null** (never complete inside `[text](` link *text*) | 19 (`:80-84`), 23 (`:24`), 17 (`:39`), 15 | **YES** |
| `#` | body tags (`#book`); wikilink heading anchors `[[#…]]`; query tags inside `{{ }}` | 16's five guards verbatim: fenced/indented code + inline code spans; URLs/autolinks; word-internal (`peek_char(-1).is_alphanumeric()`); `[[#heading]]`/`[link](file.md#heading)` reserved for heading completion; `# ` ATX at line start — plus 21's rule that query-tag completion applies only inside `{{ }}` | 16 (`:32-38`), 21 (`:49`) | **YES** |
| `:` | frontmatter YAML key/value; inline-field key `[key:`; task field `[due:: ` | `CodeRegion` first; key must match `^[a-zA-Z_-][a-zA-Z0-9_-]*\s*$`; must **not** be followed by another `:` (that's `::`, the value separator); must be in frontmatter for YAML contexts | 19 (`:80-84`) | **YES** |
| `@` | query file-class tokens `@class` | only inside `{{ }}` query context (21's O(N) depth counter over `{{`/`}}`); null everywhere else | 21 (`:49`) | **YES** |
| `.` | template helper-namespace members (`ui.`, `date.`, `query.` …); (21 Phase 2: filter/RHS) | must be exactly a helper-namespace member position (regex on line-to-cursor: `{{\s*(\w+\.)` family), **not** prose/URLs/decimals/ellipsis — this is the highest-frequency char in the set, so the miss path must be near-zero (same reasoning as rumdl's `.`/`-` fast path, §1.3) | 22 (owed input, `24-part-decomposition.md:17`) | **YES** |
| `^` | footnote reference labels when typing `[^…` | only when immediately preceded by `[`; `CodeRegion`/URL guards as for `#` | 17 (`:39`) | **YES** (recommended — see §5 on why `[^` itself can't be registered) |
| `>` | callout type after `> [!…` | only when the line prefix matches the blockquote-callout pattern `^\s{0,3}>\s*\[!`; `CodeRegion` first | 17 (`:17`, marked *provisional*) | **YES (provisional)** — matches Markdown Oxide's registered `>` (§6) |
| `"` | query-DSL string delimiters; template include/extends names | — | 21 Phase 2 (`:67`) | **NO in phase 1** — defer with 21; re-file when 21 Phase 2 lands |
| `(` | standard link target `](…)`; query/template call arguments | — | 19 explicitly "dropped as standalone trigger" (`:84`); 21 Phase 2; 15 cedes `](…)` | **NO** — see §8/Q3 for the one scenario where this becomes a gap |
| space | task emoji stage 2 (`📅 `), heading-adjacent suggestions | — | 23 **rejects** it explicitly, prefers `triggerSuggest` + `isIncomplete` (`:24,30`); Markdown Oxide registers `" "` (§6) | **NO** — fires in every prose gap; 23's decision is explicit |

### 5. Deliberately **not** registered (with rationale)

| Char | Why not |
| :--- | :--- |
| `/` | Only meaningful inside a standard-link target `](path` — territory 15 cedes to rumdl (§8). Wikilinks take note *names*, not paths. Session for any wikilink path is already open from the earlier `[`. |
| `-` | rumdl registers it solely to catch `](…)` paths. In Traces, `- [` completion is opened by `[`; word-internal hyphens (`state-of-the-art`) would fire constantly. |
| `` ` `` | rumdl **always** owns fenced-code language completion (ungated, §1.1) and no Traces ticket claims fence completion. Registering it would double every fence request for no product gain. |
| space | 23's explicit decision (use `triggerSuggest`/`isIncomplete` instead); would fire on every keystroke boundary in prose. |
| `\|` | Wikilink *alias* (`[[target\|alias]]`). Open question Q4: VS Code will not re-query on an unregistered non-word char (§3), so alias completion *at the `|`* needs either registration or an `isIncomplete` list already containing aliases. Cost of registering: every table row fires a request. |
| `]` | Nothing Traces offers begins after `]`. |
| letters / word characters | Spec: "Characters that make up identifiers don't need to be listed" (`completion.md:248-251`, §2). Continuation of `#tag`, `[[note`, `[key` is sustained by `isIncomplete: true`; word-char popups are the editor's quick-suggest / keyword-length rules (VS Code `editor.quickSuggestions`, Helix `completion-trigger-len`, nvim-cmp keyword length) — **editor-dependent and therefore not a gate Traces may rely on** (§10 Q1). |
| `[[`, `[^`, `📅 `, `- ` | Not single characters — see §2. Their first character is the registrable trigger; the second character is classified server-side. |

---

## 6. Collision matrix — Traces × rumdl (both toggle states) × prior art

`V` = verified this pass; `R` = carried from a research file that cites a digest line.

| Char | Traces (§4) | rumdl, `enableLinkCompletions=true` (default) | rumdl, `=false` (ticket 32's recommendation) | Overlap with prior art | Duplicate-item risk in the overlapping context? |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `[` | yes | no | no | Marksman `[` **V** (`lsp_artempyanykh-marksman-digest.txt:11152`), Markdown Oxide `[` **V** (`…oxide…digest.txt:2031-2037`), zk `[` **V** (`zk-src-digest.txt:4261,4272`) | n/a (rumdl absent) |
| `#` | yes | **YES** | no | Marksman `#`, Markdown Oxide `#`, zk `#`, MS `['.','/','#']` **V** (`src/server.ts:368`) | **No** — rumdl's `#` path requires `](` earlier on the line (§1.3) and Traces' `#` guard excludes `[link](file.md#anchor)` (16 guard 4). Disjoint by construction. |
| `:` | yes | no | no | zk `:` **V** (`zk-src-digest.txt:4261`) | n/a |
| `@` | yes | no | no | typescript-language-server `['.','"',"'",'/','@','<']` **V** (`src/lsp-server.ts:262`) — non-Markdown, illustrative | n/a |
| `.` | yes (22) | **YES** | no | MS `.`, TS `.`, gopls `["."]` **R** (`gopls/internal/server/general.go:204-230`), rust-analyzer `.` **R** (`crates/rust-analyzer/src/lsp/capabilities.rs:50-67`) | **No** — rumdl returns null unless the line contains `](` (fast path); Traces returns null unless in a helper-namespace position |
| `(` | **no** | **YES** | no | Marksman `(`, Markdown Oxide `(`, zk `(`, rust-analyzer `(` | n/a — only rumdl offers it |
| `>` | yes (provisional) | no | no | Markdown Oxide `>` **V** | n/a |
| `^` | yes | no | no | — | n/a |
| `` ` `` | no | **YES (always)** | **YES (always)** | — | n/a — Traces never competes here |
| `/` | no | **YES** | no | MS `/`, TS `/` | n/a |
| `-` | no | **YES** | no | — | n/a |
| space | no | no | no | Markdown Oxide `" "` **V** | n/a |

**Reading.** Under the default rumdl config the *shared* characters are `#`, `.` and (if Traces ever registers it) `(`, `/`, `-`. On all of them the two servers' contexts are provably disjoint, so the failure mode is **two requests + redundant-but-not-duplicated items**, which §3 shows every mainstream client merges. Under ticket 32's recommended `enableLinkCompletions = false`, **the overlap set is empty by construction** — rumdl registers only `` ` ``, which Traces does not register.

---

## 7. Registration mechanism recommendation

1. **Static `completionProvider.trigger_characters` in the `initialize` result, one flat `Vec`, derived from config at startup.** Sources: rumdl does exactly this (§1.1); `24-part-crates.md` #4; sibling `24-completion-capability-negotiation.md:200-207` (set is knowable at initialize; dynamic re-registration for toggle flips is a client-dependent cost nobody has shown a need for).
2. **Do not pursue per-context dynamic registration.** `triggerCharacters` is a flat array on one provider (§2); the only way to "register `#` only outside code blocks" is repeated `client/registerCapability` of `textDocument/completion` with different option sets — client-dependent, races with the user's keystrokes, and buys nothing that a server-side guard doesn't already do. rumdl (static) and MS (`['.','/','#']` registered once at startup via `registerDynamicClientFeature`, `src/server.ts:364-371` **V**) are the ecosystem's two patterns; neither varies the set per document or context.
3. **Config-derived contents**: the rumdl-coexistence toggle (§8) and the phase flags (21 Phase 2 `(`/`"`, 17 callouts) should be read once at startup, exactly as rumdl reads `enable_link_completions`.
4. **Guard placement**: `CodeRegion`/`{{`-depth/`[link](`-anchor exclusions run **first**, before any trigger-character handling (19's ordering, `:83`), because the dispatcher — not the trigger — decides what a request means (`24-part-crates.md` #1-#2).

---

## 8. Coordination statement recommended for ticket 32 (ready to adapt)

> **Trigger characters.** Traces registers `[`, `#`, `:`, `@`, `.`, `^`, `>` (plus `"`/`(` when tickets 21-Phase-2 and 17 land) statically at `initialize`. rumdl registers `` ` `` plus — only while `enableLinkCompletions = true` — `(`, `#`, `/`, `.`, `-` (`src/lsp/server.rs:514-522`; docs digest `:1986-1991`).
>
> **Recommended joint setup (set before the server starts, or restart after changing):**
> ```toml
> # .rumdl.toml
> [rumdl.lsp]
> enableLinkCompletions = false   # stops rumdl registering ( # / . -
> enableLinkNavigation   = false   # stops rumdl definition/references/hover/rename
> enableSymbols          = false   # stops rumdl documentSymbol/workspaceSymbol
> ```
> With `enableLinkCompletions = false` the two trigger sets are **disjoint by construction** (rumdl keeps only the fence backtick, which Traces does not register). If a user leaves it `true`, the overlap (`#`, `.`, and `(`-adjacent contexts) is *benign*: every mainstream client fans out only to the servers that declared the character and merges the results into one widget — the cost is one extra request per keystroke, never a second popup. Disjointness is enforced by guards, not by configuration: rumdl only completes where `](` precedes the cursor, Traces never completes inside `](…)`.
>
> **What disabling actually withdraws.** `enableLinkCompletions = false` does **not** hand Traces the wikilink space — rumdl never touches wikilinks (its detector requires `](`), so that territory was never contested. It withdraws **standard-Markdown-link path and anchor completion** (`](file.md#`), which under [ticket 15's ownership table](../../issues/15-links-and-references-model.md) nobody then provides. Document it as an accepted gap or keep the flag on (see Q3).
>
> **Flags are negotiated at startup.** rumdl re-reads config per request but does not re-register capabilities (`server.rs:753-790`); flipping a flag mid-session either leaves stale triggers registered (true→false) or leaves completion reachable only via manual invocation (false→true) until restart (`docs-digest.txt:2160-2163`).
>
> **No runtime detection is possible or needed.** LSP servers cannot see each other — there is no protocol path by which Traces could discover rumdl's flags — so this is **user-facing setup documentation**, exactly as ticket 32 already frames it (`issues/32:13`).

**Two corrections to carry back into ticket 15's wording (Q3/Q4 below):**
- Its joint-setup comment `enableLinkCompletions = false  # Traces handles wikilink completion` is imprecise: rumdl never completed wikilinks, so the flag is about *standard-link* completion and request hygiene, not about vacating wikilink territory.
- Its ownership table assigns standard-link *validation* (MD057/MD051) and heading lints to rumdl, and all wikilink completion/navigation to Traces, but has **no row for standard-link completion or standard-link navigation** — the two things the recommended flags switch off.

---

## 9. Spec/source demands vs ecosystem practice (kept separate)

| Axis | Spec / Traces source demand | Ecosystem practice (cited) |
| :--- | :--- | :--- |
| What must be registered | Only non-identifier characters; single characters (`completion.md:236-256, 372-376`) | rumdl 6-or-1; Marksman 3 (`[ # (`); Markdown Oxide 5 (`[ ␣ ( # >`); zk 4 (`( [ # :`); MS 3 (`.` `/` `#`, dynamic); TS 6; gopls 1; rust-analyzer 4 — i.e. **small sets + server-side guards are the norm** |
| Negative contexts (code, URLs, word-internal) | Not expressible — flat `Vec`, no predicate form | rumdl: line-scan + closed-link + code-span heuristics (§1.3); MS: deliberately returns *no* completions inside code blocks (`…vscode-markdown-languageservice-digest.txt:13323`, **R**); Markdown Oxide: per-completer regex chain (`tool-markdown-oxide.md:43-44`, **R**); 16 already adopts Marksman's word-internal check |
| Same-char, two servers | Silent — no protocol rule | VS Code merged widget + provider filter (§3); Helix drops empty responses (§3); **no client produces double popups** |
| Sustaining a session | `TriggerForIncompleteCompletions: 3` exists for this | VS Code re-queries incomplete providers on word chars; Neovim core while `Context.isIncomplete`; nvim-cmp on cursor move; Helix `request_incomplete_completion_list` |
| Registration lifecycle | Static `ServerCapabilities` or `CompletionRegistrationOptions` + `dynamicRegistration` (`completion.md:300-310`) | rumdl static; MS dynamic-once-at-startup; **nobody varies the set per context or per keystroke** |
| Symbols / navigation cession | n/a | rumdl's three flags are the documented "cede to a PKM LSP" contract (`docs-digest.txt:1986-1994, 2160-2163`) |

---

## 10. Open questions (none load-bearing for §4/§6/§7)

- **Q1 — word-char contexts are editor-dependent.** Sessions opened by `#`/`[`/`:` are sustained by `isIncomplete: true` in all four clients checked (§3), but *initial* word-char popups (editor quick-suggest / keyword length) are per-editor configuration. Is any Traces context reachable **only** through a word-char first keystroke? Candidates: continuing a tag with no preceding `#` in scope, and the `- [t` ambiguity owed to 24 (`24-part-decomposition.md:18`). Needs a decision on `Invoked`-only support versus a documented editor setting.
- **Q2 — `|` alias completion (15).** If aliases are completed at the `|`, register `|` (cost: every table row fires a request) or return an `isIncomplete` list containing aliases at `[[target` time (cost: larger initial list). VS Code will not re-query on an unregistered non-word char (§3).
- **Q3 — who completes `](file.md#anchor`?** Three coherent options: (a) keep `enableLinkCompletions = true` and let rumdl own it (overlap is benign, §6) — best UX, slightly noisier logs; (b) set it `false` and accept the gap (15's current recommended config + ownership table); (c) set it `false` and file a later ticket for Traces to register `(` and complete inside `](…)` itself — contradicts 15's "rumdl owns standard links" division. **Ticket 32 must pick one**; (a) or (b) are the only ones consistent with text already written.
- **Q4 — standard-link *navigation*.** Same shape as Q3: `enableLinkNavigation = false` switches off rumdl's definition/references/hover/rename for standard links (§1.2), and 15's table has no row assigning standard-link navigation to anyone. Is Traces meant to cover it (26), is rumdl meant to keep it (i.e. recommend `enableLinkNavigation = true`), or is the gap accepted?
- **Q5 — `>` and `^` are provisional.** 17 marks callout/footnote completion as in-scope-but-limited (`issues/17:17,39`); if 17 is descoped these two chars drop out of the set, shrinking the collision surface with Markdown Oxide's `>`.
- **Q6 — does 22's `.` need to be phase-gated?** It is the highest-frequency character in the set; if template helper completion ships later than the rest, registering `.` early costs a miss-path request per `.` keystroke for no benefit. Suggest config- or phase-derived contents (§7.3) rather than a fixed array.
- **Q7 — Zed / other clients.** §3 covers VS Code, Neovim core, nvim-cmp, Helix. Zed's completion fan-out was not verified this pass; the design does not depend on it (registration is static and guards are server-side), but a doc line for Zed users would need it.

---

## 11. Sources

**rumdl (primary, `rvben/rumdl` @ `main`, fetched 2026-10-06)**
- `src/lsp/server.rs` — <https://raw.githubusercontent.com/rvben/rumdl/main/src/lsp/server.rs> (`initialize` 410-545, completion handler 641-700, `did_change_configuration` 753-790, navigation gates 1389-1437, symbol gates 1490/1512, dynamic file-watcher registration 630-636)
- `src/lsp/navigation.rs` — <https://raw.githubusercontent.com/rvben/rumdl/main/src/lsp/navigation.rs> (`handle_hover` 545-589)
- Local: `docs/digests/lsp_rvben-rumdl-src-digest.txt:29374-29400` (completion module doc), `:29695-29760` (`detect_link_target_position`); `docs/digests/lsp_rvben-rumdl-docs-digest.txt:1980-1994, 2118-2163`. **Note for future passes**: `lsp_rvben-rumdl-src-digest.txt` omits `src/lsp/server.rs` and `src/lsp/navigation.rs` — that absence is why wave-1 reported no trigger evidence.

**LSP spec (3.18, `microsoft/language-server-protocol` @ `gh-pages`, fetched 2026-10-06)**
- `…/_specifications/lsp/3.18/language/completion.md` — client `dynamicRegistration`/`contextSupport` (20-46), `CompletionOptions.triggerCharacters` (236-256), `CompletionRegistrationOptions` (300-310), `CompletionTriggerKind` (330-355), `CompletionContext.triggerCharacter` (372-376)

**Clients (fetched 2026-10-06)**
- VS Code `src/vs/editor/contrib/suggest/browser/suggestModel.ts` — <https://raw.githubusercontent.com/microsoft/vscode/main/src/vs/editor/contrib/suggest/browser/suggestModel.ts> (231-247, 276-296, 745-800)
- Neovim `runtime/lua/vim/lsp/completion.lua` (79-96, 1145-1206, 1225-1345)
- nvim-cmp `lua/cmp/source.lua` (59, 98, 146, 213-228, 290-313, 356); `cmp-nvim-lsp` `lua/cmp_nvim_lsp/source.lua:74-75`
- Helix `helix-term/src/handlers/completion.rs` (41-73, 118-187) and `…/completion/request.rs` (188, 316-363)

**Prior-art servers**
- Markdown Oxide `src/main.rs` — `docs/digests/lsp_feel-ix-343-markdown-oxide-src-digest.txt:2020-2045` (`[`, `" "`, `(`, `#`, `>`)
- Marksman — `docs/digests/lsp_artempyanykh-marksman-digest.txt:11145-11158` (`[`, `#`, `(`)
- zk — `docs/digests/zk-src-digest.txt:4255-4278` (`(`, `[`, `#`, `:`)
- MS vscode-markdown-languageserver `src/server.ts:333-371` (`['.', '/', '#']`, dynamic, gated on `markdown.suggest.paths.enabled`)
- typescript-language-server `src/lsp-server.ts:258-306` (`['.', '"', "'", '/', '@', '<']`; signature help `['(', ',', '<']` + retrigger `[')']`)
- rust-analyzer `crates/rust-analyzer/src/lsp/capabilities.rs:50-67` and gopls `gopls/internal/server/general.go:204-230` — **R** (carried from `06-research-rust-analyzer-precedent.md` / sibling research; not re-fetched this pass)

**Traces-side inputs**
- [24-part-decomposition §2](24-part-decomposition.md) (37-43) — this unit's brief; its "Outputs expected" = trigger table + collision matrix + coordination statement + not-registered list (all present above)
- [24-part-crates](24-part-crates.md) (dispatcher/registration findings, `crates:725-729`)
- [24-completion-capability-negotiation](24-completion-capability-negotiation.md) (sibling: `contextSupport`, static-vs-dynamic; its open item ":590-591 rumdl's completion registration unverified" is **resolved by §1**)
- [24-novel-embedded-context-precedent](24-novel-embedded-context-precedent.md) (sibling: mdoxide trigger partition, MS code-block suppression, VS Code merge)
- tickets [03](../../issues/03-research-rumdl-boundary.md), [15](../../issues/15-links-and-references-model.md), [16](../../issues/16-tags-and-hierarchical-tags.md), [17](../../issues/17-footnotes-and-callouts.md), [19](../../issues/19-frontmatter-and-inline-field-intelligence.md), [21](../../issues/21-query-language-intelligence.md), [22](../../issues/22-template-language-intelligence.md), [23](../../issues/23-task-and-pkm-semantics.md), [24](../../issues/24-completion-architecture.md), [32](../../issues/32-rumdl-coexistence-boundary.md)
