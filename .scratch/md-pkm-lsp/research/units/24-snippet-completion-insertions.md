# Ticket 24 part-work: snippet completion insertions (unit 5 → ticket 24)

Unit: **`snippet-completion-insertions`** (kind: `general-LSP`) — per
[24-part-decomposition](24-part-decomposition.md):61-67. Input artifact for ticket
[24](../../issues/24-completion-architecture.md); §6 is the ready-to-paste block.

**What this unit decides** (decomposition:64,67): (a) the snippet inventory — which Traces contexts
get tabstops/placeholders vs plain text, with the exact grammar per item *and* the plain-text
fallback for each; (b) how snippet placeholders interact with the `textEdit` vehicle and with
`itemDefaults.insertTextFormat`; (c) the capability-gating mechanics and the degradation rule;
(d) multi-cursor safety — server-side rules plus what each editor actually does; (e) prior art
(do Markdown/PKM/general servers use snippets at all).

**What it does not decide**: the edit vehicle (`item-shape`:615 — "one `textEdit` whose `newText` is
a snippet" is given to this unit as a *contract*, along with "no `commitCharacters`", "one `textEdit`,
`insertText` omitted"); the trigger set (unit 2); the resolve split (unit 4); whether `snippetSupport`
is gated at all (unit 8 already locked that — `capability-negotiation`:68,534; this file implements
it); the `filterText` grammar (unit 3 — `item-shape`:75-77, untouched by this unit).

---

## 0. Sources, corpus caveat, shorthand

`docs/refs/lsp_spec.md` cannot serve as citation for completion — its body is literally
`{% include_relative language/completion.md %}` (`docs/refs/lsp_spec.md:659`); reasoning in
[capability-negotiation](24-completion-capability-negotiation.md) §0. Spec facts are cited to the
live 3.18 document:

- **[S1]** completion feature spec —
  <https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.18/language/completion.md>
  (1083 lines; fetched 2026-10-07 — the same file sibling units cite as fetched 2026-10-06;
  line numbers cross-checked against sibling units' 2026-10-06 fetch at :845-854 and :981-1041; sibling citations of the `textEdit` range note as [S1:864-866]/[S1:869-871] point at the same block, whose exact lines here are 866-872).
- **[S1-init]** initialize feature spec (same repo, `language/initialize.md`) — used only to prove a
  negative (§1.4).

Ecosystem claims carry `file:line` of the project's own source, fetched 2026-10-06/07 from GitHub
raw. Digest shorthand (line numbers = digest lines, as in sibling files):

| Shorthand | File |
| :--- | :--- |
| `mdoxide-src:` | `docs/digests/lsp_feel-ix-343-markdown-oxide-src-digest.txt` |
| `marksman:` | `docs/digests/lsp_artempyanykh-marksman-digest.txt` |
| `zk-src:` / `msmd:` / `rumdl-src:` | `zk-src-digest.txt` / `lsp_microsoft-vscode-markdown-languageservice-digest.txt` / `lsp_rvben-rumdl-src-digest.txt` |
| `templater:` | `docs/digests/obsidian_silentvoid13-templater-src-digest.txt` |
| `crates:` / `item-shape:` / `capability-negotiation:` / `dispatch:` | sibling research files in this dir |

Upstream shorthand (all fetched 2026-10-06/07):

| Shorthand | Upstream file |
| :--- | :--- |
| `gopls-settings.go` | `golang/tools` `gopls/internal/settings/settings.go` |
| `gopls-server-completion.go` | `gopls/internal/server/completion.go` |
| `gopls-default.go` / `gopls-golang-completion.go` | `gopls/internal/settings/default.go` / `gopls/internal/golang/completion/completion.go` |
| `ra-capabilities.rs` / `ra-config.rs` | `rust-lang/rust-analyzer` `crates/rust-analyzer/src/lsp/capabilities.rs` / `crates/ide-completion/src/config.rs` |
| `ra-snippet-completions.rs` / `ra-function.rs` / `ra-to-proto.rs` | `crates/ide-completion/src/completions/snippet.rs` / `crates/ide-completion/src/render/function.rs` / `crates/rust-analyzer/src/lsp/to_proto.rs` |
| `tsls-lsp-server.ts` / `tsls-ts-protocol.ts` / `tsls-completion.ts` | `typescript-language-server/typescript-language-server` `src/lsp-server.ts` / `src/ts-protocol.ts` / `src/completion.ts` |
| `pylsp-jedi.py` / `pylsp-resolvers.py` | `python-lsp/python-lsp-server` `pylsp/plugins/jedi_completion.py` / `pylsp/_resolvers.py` |
| `nvim-completion.lua` / `nvim-snippet.lua` | `neovim/neovim` `runtime/lua/vim/lsp/completion.lua` / `runtime/lua/vim/snippet.lua` |
| `vsc-completion.ts` / `protocolConverter.ts` / `suggestController.ts` / `snippetSession.ts` | `microsoft/vscode-languageserver-node` `client/src/common/completion.ts` / `client/src/common/protocolConverter.ts`; `microsoft/vscode` `src/vs/editor/contrib/suggest/browser/suggestController.ts` / `src/vs/editor/contrib/snippet/browser/snippetSession.ts` |
| `helix-client.rs` / `helix-editor.rs` / `helix-completion.rs` / `helix-lsp-lib.rs` | `helix-editor/helix` `helix-lsp/src/client.rs` / `helix-view/src/editor.rs` / `helix-term/src/ui/completion.rs` / `helix-lsp/src/lib.rs` |
| `zed-completions.rs` / `zed-editor.rs` | `zed-industries/zed` `crates/editor/src/completions.rs` / `crates/editor/src/editor.rs` |
| `eglot.el` | `emacs-lsp/eglot` |

---

## 1. What the spec demands (**spec source**, not opinion)

### 1.1 `insertTextFormat` scope — the vehicle coupling

> "The format of the insert text. The format applies to both the `insertText` property and the
> `newText` property of a provided `textEdit`. If omitted, defaults to `InsertTextFormat.PlainText`.
> Please note that the insertTextFormat doesn't apply to `additionalTextEdits`." [S1:845-854]

Consequences for Traces (already locked on unit 3's side):

1. The snippet string lives in the **same** `textEdit.newText` that unit 3 chose as the universal
   vehicle (`item-shape`:615). No second field, no `insertText`, no extra round-trip. The format is
   a per-item integer (`PlainText = 1` [S1:598], `Snippet = 2` [S1:608], union type [S1:611]).
2. **Never** a snippet in `additionalTextEdits` — `insertTextFormat` does not apply [S1:851-854],
   so a tabstop there would be inserted raw. Traces populates that slot never in v1 anyway
   (`item-shape`:619).
3. Degraded form of an item is therefore *literally* the same `textEdit` with a differently-rendered
   `newText` and the format field omitted — a single renderer with two modes, not two item builders.

### 1.2 Range rules — what a snippet is allowed to do

> "*Note:* The range of the edit must be a single line range and it must contain the position at
> which completion has been requested. Despite this limitation, your edit can write multiple lines."
> [S1:870-872]

- Single-line **range**, multi-line **newText** — a multi-line snippet body (callout scaffold, §5.2)
  is spec-legal [S1:872] and is orthogonal to the one-`textEdit` invariant.
- The range must contain the *request* position [S1:870-871] — the only position the server ever
  sees. With N cursors the server computes one range for one position; remapping to the other
  cursors is entirely client work (§3).
- `InsertReplaceEdit` (insert vs replace halves) is *deferred* by unit 3 (`item-shape`:623-630) —
  relevant here because "replace" vs "insert" changes which side of the cursor the snippet's
  trailing `]]` lands on (§5.1, rows C).

### 1.3 Snippet grammar — what we may emit

Format section: "Completion items support snippets (see `InsertTextFormat.Snippet`)" [S1:981],
`#snippet_syntax` [S1:983].

| Construct | Syntax | Spec line | Traces v1 |
| :--- | :--- | :--- | :--- |
| Tab stop | `$1`, `$2` … (visit order); "Multiple tab stops are linked and updated in sync" | [S1:987-989] | yes |
| Placeholder | `${1:foo}`; "placeholder text will be inserted and selected"; **nestable** | [S1:991-993] | yes (C1) |
| Final tab stop | `$0` — declared in the `snippetSupport` capability doc: "`$0` defines the final tab stop, it defaults to the end of the snippet. Placeholders with equal identifiers are linked, that is typing in one will update others too." | [S1:70-76] | yes (exactly one) |
| Choice | `${1\|one,two,three\|}` — "choices will prompt the user to pick" | [S1:995-997] | **no** (§5.4) |
| Variable | `$name` / `${name:default}`; unset ⇒ default/empty; **unknown ⇒ name inserted and "transformed into a placeholder"** | [S1:999-1004] | **no** (§5.4) |
| Transform | `${name/regex/format/options}` | [S1:1015-1037] | **no** |
| EBNF + escaping | "With `\` (backslash), you can escape `$`, `}` and `\`. Within choice elements, the backslash also escapes comma and pipe characters." | [S1:1039-1041] | **yes — mandatory** (§3.3 R6) |

Two hard rules follow from [S1:999-1004] + [S1:1039-1041] and are easy to get wrong:

- **Literal `$`/`}`/`\` in inserted text must be escaped when format = Snippet**, else a filename,
  tag or YAML key containing `$` is re-parsed as a tabstop/variable — and an *unknown variable name
  is rendered as a placeholder* [S1:1001-1004], i.e. visibly wrong text, not a parse error.
- In PlainText mode **no escaping at all** (raw bytes inserted). The two gate states are genuinely
  different string pipelines ⇒ the escaping helper belongs inside the snippet renderer (§4.4).

### 1.4 The capability, and the spec's verified silence about ignoring it

`snippetSupport?: boolean` is declared once, in `ClientCompletionItemOptions`
[S1:66-78], documented as *"Client supports snippets as insert text. A snippet can define tab stops
and placeholders with `$1`, `$2` and `${3:foo}`. `$0` defines the final tab stop…"* [S1:70-76].

**Verified negative**: the string `snippetSupport` occurs **exactly once** in [S1] (line 77) and
**zero times** in [S1-init]. The spec never states what a client must do if it receives
`insertTextFormat: Snippet` without having advertised the capability — there is no "SHOULD ignore",
no fallback semantics, nothing. ⇒ Degradation behaviour is 100% ecosystem-defined, which is why §3
is source evidence rather than spec citation. Unit 8's conservative reading (*"Absent ⇒ must not
send `insertTextFormat: Snippet`"*, `capability-negotiation`:68) remains the adopted rule; this file
does not re-litigate it, it specifies what gets sent instead (§4).

### 1.5 `itemDefaults.insertTextFormat` (decomposition (b))

- Spec side: `CompletionItemDefaults.insertTextFormat?: InsertTextFormat` [S1:494-499] (3.17+),
  usable only when the client lists `"insertTextFormat"` in `completionList.itemDefaults`
  [S1:200-214] — "If omitted no properties are supported" [S1:210-212]. VS Code honours it end-to-end:
  it reads `list.itemDefaults?.insertTextFormat` and threads it as the default into per-item
  resolution (`protocolConverter.ts:523,589,685-686`).
- Traces side: **crate-blocked** — `ls-types` has no `CompletionItemDefaults` type and
  `CompletionList` is only `{ is_incomplete, items }` (`crates`:450). Spec-legal, unavailable.
- Even if it became available: it would be a *byte-saving* only. Our responses are homogeneous
  (every item in a response is snippet or every item is plain — §4.4 is a single `bool`), so
  `itemDefaults.insertTextFormat` buys nothing semantically and the same `snippetSupport` flag would
  gate it. **Recommendation: do not adopt in v1; record as a non-goal.** Also note `itemDefaults`
  remains blocked for unit 4's other uses (`crates`:450) — this is not a snippet-specific gap.

### 1.6 `insertTextMode` (decomposition (b), adjacent)

`insertTextMode` on the item [S1:856-864] with `asIs = 1` / `adjustIndentation = 2`
[S1:676,687-690], client side `insertTextModeSupport` [S1:124-134]. Only relevant if a multi-line
snippet body is adopted (§5.2). Editor reality: VS Code and Zed advertise `asIs, adjustIndentation`;
**Neovim and Helix advertise neither** (`capability-negotiation`:411) ⇒ their own indentation
heuristic applies, unsteerable. ⇒ if Traces ever emits a multi-line body it must **bake indentation
into the literal text** (Markdown Oxide does: `prefix = "> ".repeat(completer.nested_level)`
`mdoxide-src:3633`) and never rely on `insertTextMode`. Unit 8 already flags this as
"GATE-light (unit 5)" (`capability-negotiation`:76) — decision: **never send `insertTextMode`**.

### 1.7 `filterText` interplay (declaration only — unit 3 owns the grammar)

- Unit 3's contract stands unchanged: `filterText` must match the text between `range.start` and the
  cursor (`item-shape`:75-77), and a mismatching `filterText` makes clients drop the item outright
  (`item-shape`:86-87). **Snippets do not alter this** — `filterText` describes *typed* text, never
  `newText`.
- Two client details worth knowing because they explain why we must always set `filterText`
  ourselves: Neovim, for `format == Snippet` items, parses the snippet body to derive the match word
  when `filterText` is absent/empty (`nvim-completion.lua:208-231`); VS Code scores `filterText`
  and ignores `label` for matching when present (`item-shape`:209-212).
- ⇒ Never put `$`/tabstop syntax in `filterText` (it is matched as literal text), and never put the
  snippet body in `label` (it is displayed).

---

## 2. Prior art (decomposition (e))

### 2.1 Markdown / PKM language servers

| Server | Uses `insertTextFormat: Snippet`? | Gates on `snippetSupport`? | What it emits |
| :--- | :-: | :-: | :--- |
| **Markdown Oxide** | **yes** | **no** | callout body `"{prefix}[!{name}] ${1:Title}\n{prefix}${2:Description}"` (`mdoxide-src`:3631-3644) with `kind = SNIPPET` (`:3645`) and `textEdit` line-start→cursor (`:3646-3657`); markdown-link display `"${1:…}"` (`:4596-4607`); wikilink display `"${1:typed}"` **only in the `Alias` variant**, `None` otherwise (`:4615-4621`); wikilink `new_text = "{}{}]]${{2:}}"` i.e. `refname|display]]${2:}` (`:4212-4216`); unindexed-block variants (`:5660-5697`) |
| **Marksman** | no | — | `CompletionItem` + `TextEdit` only; `insertTextFormat`/`snippet` has **0 hits** in the digest; `marksman`:4751-4793 |
| **zk** | no | — | **0** `InsertTextFormat` hits (its `Snippets` hits are note-content extraction, `zk-src`:2803,6258) |
| **MS markdown-ls** | no | — | **0** hits |
| **rumdl** | no | — | **0** `insert_text_format` hits (its "snippet" hits are TOML config, `rumdl-src`:2478,2804) |

**Markdown Oxide's gate is the load-bearing finding**: its capability reader
`Settings::new(root_dir, &capabilities)` reads exactly one capability — `semantic_tokens`
(`mdoxide-src`:1204-1255) — and `snippet_support` appears **nowhere** in the digest. It emits
snippets unconditionally to every client. *(Q4 below: confirm against live source, digest-based.)*
That is pattern (d) in §4.2, and it is *why* eglot's fallback matters.

### 2.2 General-purpose servers

| Server | Pattern | Evidence |
| :--- | :--- | :--- |
| **gopls** | **two-track**: builds both strings, picks per capability | upgrade `o.InsertTextFormat = SnippetTextFormat` iff `c.CompletionItem.SnippetSupport` (`gopls-settings.go`:1070-1072); `insertText := candidate.InsertText; if options.InsertTextFormat == Snippet { insertText = candidate.Snippet() }` (`gopls-server-completion.go`:139-141); `Snippet()` *falls back to* `InsertText` when no snippet exists (`gopls-golang-completion.go`:133-138); explicit `PlainTextTextFormat` default (`gopls-default.go`:33); **skips items that only exist as snippets**: *"This can happen if the client has snippets disabled but the candidate only supports snippet insertion"* → `continue` (`gopls-server-completion.go`:146-149) |
| **rust-analyzer** | **typestate gate** + degrade | `completion_snippet()` reads `…completion_item.snippet_support` (`ra-capabilities.rs`:488-497, `unwrap_or_default` per `capability-negotiation`:230-231); `pub snippet_cap: Option<SnippetCap>` with `SnippetCap` constructible only in config (`ra-config.rs`:3-9,31); snippet-only completions `match ctx.config.snippet_cap { Some(it) => it, None => return }` (`ra-snippet-completions.rs`:24-28); callable parens/params degrade to the bare name — `complete_call_parens = cap.filter(…)` (`ra-function.rs`:90-107); format set from `item.is_snippet` (`ra-to-proto.rs`:356,214) |
| **typescript-language-server** | **drop unsupported**, build snippet at *resolve* | `this.features.completionSnippets = snippetSupport` (`tsls-lsp-server.ts`:155-158, flag `tsls-ts-protocol.ts`:361); `if (isSnippet && !features.completionSnippets) return null` (**item disappears**) (`tsls-completion.ts`:131-133); `if (features.completionSnippets && …) item.insertTextFormat = Snippet` (`:134-137`); function-call snippet assembled in `resolve` and written into **both** `insertText` and `textEdit.newText` (`:373,415-431`) |
| **python-lsp-server** | **per-item gate passed down** | `snippet_support = item_capabilities.get("snippetSupport")` (`pylsp-jedi.py`:56), threaded to every candidate (`:71-91,243-273`); snippet strings built only when true (`pylsp-resolvers.py`:101-124, e.g. `name(${1:param},…)$0`) |

Nobody in this set emits a snippet without checking the capability. Markdown Oxide is the outlier,
and it is the smallest, most nvim/VS-Code-shaped audience.

---

## 3. Multi-cursor semantics — per-editor, from source (decomposition (d))

### 3.1 The asymmetry that shapes everything

The LSP request carries **one** position (`textDocumentPosition`), and the spec pins the one range
to it [S1:870-871]. So with N cursors the server is structurally incapable of expressing N ranges or
N bodies. Every editor therefore does one of two things: *remap the single body* to its other
cursors, or *ignore them*. All four mainstream editors remap (Neovim core does not, by
construction).

### 3.2 What each editor actually does

| Editor | Advertises | How accept expands | Multi-cursor? | Per-cursor range rule | Failure mode |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **VS Code** | always `true` (`vsc-completion.ts`:77) | LSP `format==Snippet` → `new code.SnippetString(newText)` (`protocolConverter.ts`:685-700); **every** accept routes through the snippet controller — non-snippet text is `SnippetParser.escape`d first (`suggestController.ts`:437-446) | **all selections** | one parse of the *same* template per selection (`snippetSession.ts`:505,532-554); a secondary cursor's replace-range is grown only if its surrounding text equals the primary's (`:526-535`); whitespace re-adjusted per cursor (`:557-563`); variables resolved **per cursor index** (`:565-577`) | secondary cursors whose local text differs silently get a narrower replacement (no growth) |
| **Neovim core** | `true` (`capability-negotiation`:411) | keyed on the **item**, not the advertisement: `expand_snippet = insertTextFormat == Snippet and (textEdit or insertText)` → `vim.snippet.expand(item.textEdit.newText or insertText)` (`nvim-completion.lua`:179-183,981-983,1034-1036) | **single cursor** — `vim.snippet.expand` inserts at `nvim_win_get_cursor(0)` (`nvim-snippet.lua`:503,610-612) | n/a | `error('Snippet has multiple placeholders for tabstop $N')` when one id carries *different* text (`:517-519`) — and it throws **after** the typed word was already deleted (`nvim-completion.lua`:1027-1036 runs `clear_word()` → `apply_snippet`) ⇒ **text loss**; `assert(#tabstop_data[0] == 1, 'multiple $0 tabstops')` (`nvim-snippet.lua`:601-604) |
| **nvim-cmp** | override `true` (`capability-negotiation`:411,438) | cmp path (not audited here) | **undefined**: `hrsh7th/nvim-cmp#644` "not work with vim-visual-multi" closed as `bug, can't reproduce` (2023) | — | treat multi-cursor + cmp as out of scope |
| **Zed** | `true` (`capability-negotiation`:411) | builds ranges for **all** selections then `insert_snippet(&offset_ranges, …)` (`zed-completions.rs`:924-960,983-990) | **all selections** | newest selection uses the server range verbatim; others get prefix/suffix reconciliation so the typed prefix isn't duplicated (`:932-954`) | — |
| **Helix** | config-gated, default **on** (`helix-client.rs`:639; `helix-editor.rs`:642-659,1840) | `if kind == SNIPPET \|\| insertTextFormat == SNIPPET { Snippet::parse(new_text) }` (`helix-completion.rs`:627-646) | **all cursors** — *"The transaction applies the edit to all cursors"* (`helix-lsp-lib.rs`:371-372,393-409) | each cursor keeps the server's range **only if** the text it replaces equals the primary's replaced text, else recomputed via `find_completion_range` (`helix-lsp-lib.rs`:396-401) | **parse failure ⇒ empty `Transaction` = silent no-op insert** with an error log (`helix-completion.rs`:634-637) |
| **eglot** | only when a snippet expansion fn exists (yasnippet) (`eglot.el`:1108-1116,2244-2251) | `(funcall (or snippet-fn #'insert) text)` (`eglot.el`:4111-4119) | single point | — | **no expansion fn + `format==2` ⇒ raw `insert` of `${1:…}` text** — the literal-degradation failure mode (`eglot.el`:4111-4119) |
| **Obsidian plugins** (precedent) | n/a | `EditorSuggest` → `editor.replaceRange(…)` + `editor.setCursor(…)` (`templater`:4101,4114) | single cursor | — | no tabstops at all — the PKM-plugin baseline Traces is departing from |

### 3.3 Server-side multi-cursor rules (R1–R9)

- **R1 — one position, one range, one body.** Never assume the server can see the other cursors.
  Emit exactly one `textEdit` with a single-line range containing the request position
  [S1:870-871]; everything else is client remapping (§3.2).
- **R2 — offset-local body.** The snippet must be fully self-contained inside the replaced span plus
  its own trailing structure: VS Code mirrors the primary's *offsets* onto secondary cursors
  (`snippetSession.ts`:526-535), Helix re-derives them per cursor (`helix-lsp-lib.rs`:396-401),
  Zed reconciles prefix/suffix (`zed-completions.rs`:932-954). A body that implies "keep/see text
  left of the range" is wrong at some cursor.
- **R3 — no variables** (`TM_*`, `$SELECTION`, …): VS Code resolves them **per cursor index**
  (`snippetSession.ts`:565-577) so different cursors would legitimately receive *different* text,
  and Neovim/Helix/VS Code disagree about which exist; unknown names degrade into visible
  placeholders [S1:999-1004].
- **R4 — no transforms, no choices in v1** (§5.4). Choices are an editor popup (VS Code/Zed show
  them: `zed-editor.rs`:5083-5087) and ticket 23 already locked *list-picking/preselect* over
  cycling (`research/23`:364).
- **R5 — exactly one `$0`, and never two placeholders with the same id and different text.**
  Neovim throws on the latter *after* clearing the typed word (§3.2) ⇒ guaranteed text loss; it
  asserts on multiple `$0` (`nvim-snippet.lua`:601-604). Equal-text linked placeholders are fine
  [S1:74-75].
- **R6 — escape `$`, `}`, `\` in every literal** when format = Snippet [S1:1041] (filenames, tags
  and keys can contain them); **do not escape** in PlainText mode. Different pipelines per gate
  state — see §4.4.
- **R7 — v1 snippets are single-line.** Multi-line is spec-legal [S1:872] but `insertTextMode`
  can't be relied on (§1.6); if a multi-line body is adopted (§5.2), bake the indentation literally
  (mdoxide `mdoxide-src`:3633).
- **R8 — ≤2 tab stops.** Tab-through must terminate quickly; all three all-cursor editors advance
  *every* cursor's snippet together (VS Code `_move` loops `for (const snippet of this._snippets)`,
  `snippetSession.ts`:783-790; Zed's `SnippetState` holds all ranges, `zed-editor.rs`:5095-5100;
  Helix keeps one active snippet per doc).
- **R9 — the snippet must never be load-bearing for document validity.** A client can fail to
  expand at any moment (Helix parse failure = insert nothing, `helix-completion.rs`:634-637; eglot
  without yasnippet = raw text, `eglot.el`:4111-4119). The plain rendering must therefore be a
  *valid* Markdown document on its own (§5.1 P1), not a half-open fragment.

---

## 4. Degradation strategy (decomposition (c))

### 4.1 The rule (implementing unit 8's lock, not re-deciding it)

> **GATE**: read `capabilities.textDocument.completion.completionItem.snippetSupport` **once** at
> `initialize` (`Option<bool>` in `ls-types`, `crates`:406); store `snippets_enabled = (==
> Some(true))`. Absent/`null`/`false` ⇒ off (rust-analyzer's `unwrap_or_default` pattern,
> `ra-capabilities.rs`:488-497; unit 8: `capability-negotiation`:230-231, C-N3 at `:534-536`).

Granularity: **one global bool, applied per item** — never per-document, never "assume modern"
(unit 8's floor, `capability-negotiation`:534). There is no context in which a *client* that lacks
snippet support could still be given a snippet "just for this context": §1.4 shows the spec gives
clients no contract for it, and §3.2 shows the outcomes are undefined-to-broken (eglot raw text).

### 4.2 The four ecosystem patterns, and which Traces adopts

| # | Pattern | Who | Result when unsupported | Traces |
| :--- | :--- | :--- | :--- | :--- |
| (a) | **two-track strings** — build plain *and* snippet, select per capability | gopls (`gopls-server-completion.go`:139-149; `gopls-golang-completion.go`:133-138) | coherent plain text, item still offered | ✅ **adopted** — §5's fallback column *is* the second track |
| (b) | **drop snippet-only items** | rust-analyzer (`ra-snippet-completions.rs`:24-28), typescript-language-server (`tsls-completion.ts`:131-132) | item absent | ❌ rejected: every snippet-eligible Traces item (C1) is *still* a useful completion as plain text; dropping it would be a bigger loss than the placement |
| (c) | **fill placeholders with their defaults as plain text** | pylsp (`pylsp-resolvers.py`:101-124 — plain `name()` sibling path) | degraded but complete | ⚠️ mechanical rule only (§4.3) — our placeholders are empty, so "fill with default" = "delete" |
| (d) | **emit anyway** | Markdown Oxide (`mdoxide-src`:1204-1255, no gate) | fine in VS Code/Neovim/Zed/Helix; **breaks in eglot** (raw `${1:…}`), undefined elsewhere | ❌ rejected — this is precisely what unit 8's C-N3 forbids |

### 4.3 "Which placeholder wins" — the mechanical rule

Decomposition (c) asks what the plain fallback is when the client can't expand. Rule:

> Render the snippet string with: every `$N`/`${N:…}` **placeholder replaced by its default text**
> (empty default ⇒ removed), every literal kept verbatim, `$0` removed, no escaping (raw mode),
> format field omitted. Then, per context, **drop any structure that exists only to host a cursor**
> (a `|` whose slot the cursor can no longer reach is residue the user must clean; a link without
> `|` is a valid link the user extends by typing).

Applied to C1 (§5.1): `${1:}` disappears, `|` is dropped by the second clause, `]]` stays ⇒
**P1 = `[[projects/active]]`**.

This deliberately supersedes `crates`:753-755's parenthetical fallback ("`[[target|]]`-style text and
let the user arrow back"): with the cursor at the end of the insertion the `|` is unreachable without
leaving the link, i.e. residue — and "arrow back" is precisely the placement plain text cannot provide
(the sentence's own conclusion, "effectively no reliable no-snippet cursor placement"). Final call on
the residue question is Q6.

### 4.4 Gating mechanics — ready-to-paste contract

- `snippets_enabled: bool` on the server state, set in `initialize`; read in exactly one place: the
  snippet renderer (`render_new_text(ctx) -> (String, Option<InsertTextFormat>)`).
- Off ⇒ `insert_text_format: None` (spec default PlainText [S1:849]). Sending explicit `1` is also
  legal and gopls does it (`gopls-default.go`:33) — pick omit (fewer bytes, `ls-types` field is
  `Option`); cosmetic, note as Q9.
- **`kind = SNIPPET`(15) must be gated by the same bool.** Helix branches on `kind == SNIPPET` **or** `format == SNIPPET` (`helix-completion.rs`:627-633): a plain-text item with `kind = 15`
  would be pushed into Helix's parser and, on failure, inserted as **nothing**
  (`:634-637`). Relevant only if §5.2/C13 adopts the `SNIPPET` kind (item-shape Q4, `item-shape`:846-847).
- Off ⇒ the renderer must not even *build* a snippet string (no escaping, no tabstop assembly): the
  two modes are separate `match` arms, so a gating regression can't leak `$1` into a response.
- Cost: one `bool` test per snippet-eligible item; escaping is O(len) and only runs in on-mode.
  Nothing per-keystroke beyond what unit 6 already budgets.
- `itemDefaults.insertTextFormat`: not used (§1.5); would follow the same bool if ever unlocked.

---

## 5. Snippet inventory (decomposition (a)) — the deliverable

**Master rule (the test for every context):**

> A snippet is warranted **iff the cursor must end up somewhere the insertion does not naturally
> leave it** — i.e. *before* the end of the inserted text, or *behind* structure the insertion
> creates. Plain text always leaves the cursor at the end of the insertion; that is the entire
> capability gap (`capability-negotiation`:68 degradation column: *"cursor lands at end of
> insertion; no tabstops"*).

Corollary: **for any context whose insertion ends at the cursor, plain text is not a degradation —
it is the same UX**, and the correct answer is "never a snippet".

### 5.1 C1 — wikilink target (the ticket's own example; the only v1-required snippet)

Notation: `T = projects/active` (resolved target, ticket 15), `p` = typed prefix. Vehicle per unit
3: one `textEdit` (`item-shape`:615), single-line range containing the request position [S1:870-871].
Two range options are in play and they change the degradation cost — this is a **joint decision with
unit 3's paired-region row** (`item-shape`:616):

| Row | Document state | Range covers | `format=Snippet` `newText` | Cursor after accept | `format=PlainText` `newText` (same range) | Cursor | Delta |
| :-: | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **A1** | `[[proj` (no closing `]]` yet) | `[[proj` → cursor | `[[T\|${1:}]]$0` | **in the empty alias slot** (selected); Tab ⇒ after `]]` | `[[T\|]]` | after `]]` | plain loses the slot; leaves an unreachable `\|` (residue) |
| **A2** | " | " | `[[T]]${1:}` | after `]]` | `[[T]]` | after `]]` | **none** — snippet buys only a tab stop ⇒ don't use a snippet |
| **B1** | `[[proj]]`, cursor before `]]` | `[[proj` → **through `]]`** (unit 3's paired row) | `[[T\|${1:}]]$0` | in the empty alias slot | `[[T\|]]` | after `]]` | as A1 |
| **B2** | " | " | `[[T]]$0` | after `]]` | `[[T]]` | after `]]` | **none** ⇒ plain |
| **C1** | `[[proj]]`, cursor before `]]` | `[[proj` → cursor (**`]]` NOT in range**) | `[[T\|${1:}` (doc's `]]` stays) | between `\|` and `]]` — **inside** | `[[T\|` | **between `\|` and `]]` — inside** | **none: plain already achieves the ticket's placement** |
| **D1** (Q3) | `[[proj\|di]]`, cursor in the display | through `]]` | `[[T\|${1:}di]]$0` | on the typed display (editable), Tab ⇒ after `]]` | `[[T\|di]]` | after `]]` | plain preserves the text, loses preselection |

Reading of the matrix:

1. **The ticket's stated goal ("cursor left inside `[[\|…]]`") is achievable with plain text whenever
   the closing `]]` already exists and the range stops at the cursor (row C1).** The decomposition's
   odd phrase *"insert `[[target|]]` and position via trailing text?"* (`part-decomposition`:64)
   resolves to exactly this: let the pre-existing `]]` sit after the insertion, end `newText` with
   `|`, and the cursor is already in the slot.
2. **What a snippet uniquely buys in C1 = create the closing `]]` *and* land the cursor before it**
   (rows A1/B1). Plain text structurally cannot do both: its cursor is at the end of the insertion.
   That is the whole capability delta for Traces v1.
3. Rows A2/B2 show the "safe but pointless" shape — a snippet whose only stop is at the end. Do not
   emit it (it invites R5/R6 bugs for zero UX).

**Recommendation for C1:**

- **Range** (joint unit-3 call, recommended): cover `[[` … cursor **plus an existing trailing `]]`**
  (row B) — i.e. unit 3's paired-region row. Rationale: one `newText` string works in every state
  (`A1`/`B1` identical), the emitted link is always terminated (R9), and the `filterText` contract
  (`item-shape`:75-77) sees a stable prefix.
- **Snippet on**: `[[T|${1:}]]$0` (row A1/B1). Post-accept: alias slot selected, Tab ⇒ after `]]`.
- **Snippet off (P1)**: `[[T]]` — clean, terminated link, cursor after `]]`.
  Loss (documented): the alias slot is not pre-opened; the user types `|` + display manually.
  **Do not** emit `[[T|]]` as the fallback — with the cursor after `]]` that `|` is unreachable
  residue (§4.3 second clause), and empty-alias render semantics are unverified (Q6).
- **Optimization (unit-3-callable)**: if unit 3 instead chooses the *cursor-stopping* range (row C1),
  both modes collapse to the same UX (`[[T|`), Traces needs **no snippet at all for C1**, and this
  unit's required inventory reduces to C13-or-nothing. Worth putting to unit 3 explicitly — it is the
  single highest-leverage decision in this file.
- **D1** only matters if the dispatcher offers C1 completion with the cursor in the display portion
  (markdown-oxide does: `mdoxide-src`:4615-4621 `Alias` variant) — currently `dispatch`:132 scopes C1
  to "cursor in target", so D1 is conditional on Q3.

### 5.2 C13 — callout type (optional, scope-expanding; **not** recommended for v1)

Markdown Oxide's shape, verbatim pattern (`mdoxide-src`:3631-3644):

```
{prefix}[!{name}] ${1:Title}\n{prefix}${2:Description}      prefix = "> ".repeat(nested_level)
```

with `textEdit` from line-start → cursor (`:3646-3657`), `kind = SNIPPET` (`:3645`), multi-line
`newText` inside a single-line range ([S1:872]).

Traces v1 per unit 3: insert the literal type token, `kind = Keyword` (`item-shape`:654) — i.e.
plain, cursor at end, **no snippet needed** by the master rule. Adopting the body would require
three coupled changes: `item-shape` Q4 flips kind `Keyword`→`Snippet` (15), §4.4's kind/format
gating applies, and R7 forces baked `"> "` prefixes (never `insertTextMode`, §1.6).
**Recommendation: defer to a later ticket; record as the only other snippet candidate.**

### 5.3 Every other context — "never a snippet", with the reason

Rule applied: each row's insertion ends at the request cursor, so plain text *is* the target UX.

| Ctx | Context (dispatch:132-148) | Insert shape (ticket source) | Format | Why not a snippet |
| :-: | :--- | :--- | :-: | :--- |
| C2 | wikilink heading `[[n#h` | replace through cursor with `#Heading` (15) | plain | ends at cursor; no structure to host a cursor |
| C3 | markdown-link target/anchor | replace through cursor (15, ownership Q) | plain | as C2 |
| C4 | body tag `#…` | "Replaces from the initial `#` through the cursor with `#path/to/tag`" (`issues/16`:43) | plain | ends at cursor |
| C5 | frontmatter tag list | "Replaces the bare word with `path/to/tag` without a `#` prefix" (`issues/16`:45-47) | plain | ends at cursor |
| C6 | frontmatter key | key token + `: ` (19/20) | plain | ends at cursor; a tabstop on the *value* can't re-trigger the `:` value list anyway — that's `command`-re-trigger territory (unit 3 §8, `item-shape`:621) |
| C7 | frontmatter / enum value | value token (20) | plain | ends at cursor |
| C8 | inline-field key `[k:: ` | key token (19) | plain | ends at cursor |
| C9 | task checkbox `- [` | "text_edit replaces **inner span only** … `x] `" (`research/23`:124) | plain | ends at cursor (23 deliberately preselects items instead of cycling, `research/23`:364) |
| C10 | task date value `📅 ` / `[due:: ` | spanning slot → ISO date (`research/23`:307,530) | plain | slot replace ends at cursor |
| C11 | daily-note date in link target | date token (18) | plain | ends at cursor |
| C12 | footnote ref `[^…` | label (+ closing `]` if in range) (17) | plain | ends at cursor |
| C13 | callout type `> [!…` | literal token (`item-shape`:654) | plain (v1) | see §5.2 |
| C14 | query DSL tokens | field/operator/value fragments (21) | plain | token fragments; **no query "scaffold" is ticketed for v1** (21 Phase 1 = tokens only, `issues/21`:26) |
| C15 | template helper member `ui.` | member name (22, D5: never emit degraded items) | plain | ends at cursor; paren/arg scaffolds (`ui.f($0)`) are *not* in 22's scope — same opinionated choice gopls/rust-analyzer gate behind their snippet caps |
| C16 | template include/extends name | file path (22) | plain | ends at cursor |
| C17 | file-field / fileClass value | path / enum / string (20) | plain | ends at cursor |

Also explicitly **out of scope for v1** because no ticket asks for them: "frontmatter `key: value`
skeletons" with a value tabstop, "query/template scaffolds" as whole-block insertions
(`part-decomposition`:64 lists them as *candidates*, not requirements). They would be C6/C14
add-ons, and each fails the master rule today (C6's insertion ends at the cursor; C14 has no ticketed
scaffold).

### 5.4 Grammar we deliberately never use (v1)

| Construct | Verdict | Reason |
| :--- | :-: | :--- |
| Choices `${1\|a,b\|}` | **no** | editor-popup-only UX (zed-editor.rs:5083-5087); ticket 23 locked list-picking/preselect over cycling (`research/23`:364); adds a client-behaviour fork for a status picker Traces already renders as items |
| Variables `TM_*` / `$SELECTION` | **no** | R3 — per-cursor resolution differs (vscode `snippetSession.ts`:565-577); unknown names become visible placeholders [S1:1001-1004] |
| Transforms | **no** | no use case; strictly more parser-compat risk (R9) |
| Linked placeholders (same id twice) | **not yet** | legal [S1:74-75] and Neovim tolerates *equal* text (`nvim-snippet.lua`:517-519) but nothing needs it |
| `itemDefaults.insertTextFormat` | **no** | crate-blocked (`crates`:450); see §1.5 |
| `insertTextMode` | **no** | §1.6 — nvim/Helix don't advertise it (`capability-negotiation`:411) |

---

## 6. Ready-to-paste statements for ticket 24

> **S-13 Snippets are opt-in per client, one flag.** `snippets_enabled ==
> (InitializeParams.capabilities…completionItem.snippetSupport == Some(true))`, read once;
> absent ⇒ off. Never assumed, never per-document. [spec: `snippetSupport` is declared once at
> [S1:66-78] and the spec says nothing else about it — verified single occurrence; unit 8 C-N3,
> `capability-negotiation`:68,534]
>
> **S-14 Two renderers, one item builder.** Every snippet-eligible item is rendered by
> `render_new_text(snippets_enabled)`; off ⇒ plain text with `insertTextFormat` omitted (spec
> default PlainText, [S1:845-854]) and **no snippet string is ever constructed** (no tabstops, no
> escaping). The fallback column of §5 *is* the second track (gopls pattern,
> `gopls-server-completion.go`:139-149). Snippet-only items are never dropped (rust-analyzer/tsls
> pattern rejected — §4.2b).
>
> **S-15 Snippet inventory = C1 (+ C13 optional, deferred).** C1 when `snippets_enabled`:
> `[[T|${1:}]]$0` inside the one `textEdit.newText`; fallback `[[T]]`. Every other context C2–C17 is
> permanently plain because its insertion ends at the request cursor — no gate can change that
> (§5.3). If unit 3 picks the cursor-stopping range for C1 (row C1), C1 degrades to plain too and
> Traces ships zero snippets in v1.
>
> **S-16 Degradation cost, stated honestly.** Off-state loss = the alias slot in C1 is not pre-opened
> (user types `|`), and C13's body (if ever adopted) collapses to a type token. Nothing else in
> Traces v1 depends on snippets. Cursor placement *inside* an insertion has **no** non-snippet
> substitute in Traces — `command`-based placement needs a client command handler Traces doesn't
> have (`crates`:750-756).
>
> **S-17 Multi-cursor = client-remapped, server-agnostic.** One single-line range containing the
> request position [S1:870-871]; body offset-local (R2), no variables (R3), exactly one `$0`, no
> duplicate placeholder ids with differing text (R5 — Neovim throws *after* deleting the typed
> word), `$`/`}`/`\` escaped in snippet mode and unescaped in plain mode (R6, [S1:1039-1041]),
> ≤2 tab stops (R8), and the plain rendering must always be valid Markdown on its own (R9 — Helix
> silently inserts nothing on parse failure; eglot inserts raw `${1:…}`).
>
> **S-18 `kind = SNIPPET` is gated by the same bool as the format** (Helix parses on kind *or*
> format, `helix-completion.rs`:627-637) — moot in v1 because no item uses kind 15.
>
> **S-19 Non-goals:** `itemDefaults.insertTextFormat` (crate-blocked, `crates`:450 — and
> semantically redundant given homogeneous responses), `insertTextMode`, choices, variables,
> transforms, and any query/template scaffold insertion (no ticket demands one).

---

## 7. Open questions

- **Q1 — C1 range, the joint unit-3 call.** Cover the existing `]]` (rows A1/B1: snippet needed for
  inner-cursor placement, plain fallback = P1) or stop at the cursor (row C1: **plain text achieves
  the ticket's placement**, no snippet needed)? Highest-leverage decision in this file; unit 3's
  paired-region row (`item-shape`:616) currently assumes the former.
- **Q2 — product call on the alias slot.** Does v1 *always* open `[[T|…]]` on accept (the ticket's
  example) — accepting that the selected empty slot swallows the user's next keystrokes if they
  meant to keep writing prose — or Obsidian-style `[[T]]` with the user typing `|` when they want a
  display? If the former: Q1's plain fallback must still be P1 (clean link), so gated-off clients get
  the Obsidian flow by accident.
- **Q3 — does the dispatcher offer C1 when the cursor is in the *display* (after `|`)?** `dispatch`:132
  says "cursor in target"; Markdown Oxide completes there too (`mdoxide-src`:4615-4621). Decides
  whether row D1 exists at all.
- **Q4 — verify Markdown Oxide's absent gate against live source.** Digest-based: `Settings::new`
  reads only `semantic_tokens` (`mdoxide-src`:1204-1255) and `snippet_support` has zero hits. Mark
  unverified until the crate source is checked.
- **Q5 — C13 callout body** (cross-unit with `item-shape` Q4, `item-shape`:846-847): adopt
  Markdown Oxide's multi-line body in v1 (flips kind `Keyword`→`Snippet`, activates §4.4's kind
  gate, forces R7 baked indentation) or keep the literal token?
- **Q6 — empty-alias terminal state.** Is `[[T|]]` a valid, acceptable rendering in Traces if the
  user never fills the slot? (Affects whether the *fallback* could ever keep the `|` — §4.3 says it
  shouldn't, pending this.)
- **Q7 — empirical multi-cursor pass.** §3.2 is source-verified, but three behaviours are
  observation-only: (i) VS Code secondary cursors with *mismatched* local text (`snippetSession.ts`:526-535
  declines the range growth — what actually lands there?); (ii) Neovim core accept with multiple
  cursors (`vim.snippet.expand` uses the window cursor — is the request even sent per cursor?);
  (iii) nvim-cmp + vim-visual-multi (#644 closed "can't reproduce"). Recommend a 5-minute manual
  matrix before ticket 29 documents client support.
- **Q8 — Helix parse-failure tolerance.** Our grammar is a strict subset (tabstop, empty/named
  placeholder, one `$0`), but R9 says never depend on it. Add the exact emitted strings to a
  cross-parser test (Helix `helix-core/src/snippets/parser.rs`, Neovim `vim.snippet`, VS Code
  `SnippetParser`) as an implementation acceptance criterion.
- **Q9 — omitted vs explicit `insertTextFormat: 1` when gated off.** Both legal [S1:849]; gopls
  sends explicit `1` (`gopls-default.go`:33), Traces's field is `Option`. Cosmetic; recommend omit.
- **Q10 — is "plain ends at the cursor" universally true for our clients?** The claim underpinning
  §5.3 holds for VS Code/Neovim/Zed/Helix/Obsidian-plugin accept paths (all insert-then-place-cursor
  at the edit end; §3.2). No mainstream client places the cursor *before* inserted text on a plain
  completion — if one is found, that context's row must be revisited.
