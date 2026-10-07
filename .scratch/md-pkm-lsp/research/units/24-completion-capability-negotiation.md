# Ticket 24 part-work: completion capability negotiation & degradation (unit 8 → ticket 29)

Unit: **`completion-capability-negotiation`** (kind: `general-LSP`) — per
[24-part-decomposition](24-part-decomposition.md) §8 (lines 85-91). This file is an **input artifact for
ticket [29](../../issues/29-client-capability-negotiation.md)**; §9 is the ready-to-paste block.

Method: primary LSP 3.18 spec fetched live (2026-10-06) because the local corpus cannot serve as
citation for completion (see §0); crate facts verified against the cached `ls-types 0.0.6` source on
disk; editor/server capability profiles read from each project's own source at `master`/`main`
(2026-10-06). Ecosystem claims carry URLs; spec claims carry spec URLs. Anything not directly
verified is marked **QUESTION**.

---

## 0. Corpus caveat (why spec citations are URLs, not `docs/refs/lsp_spec.md`)

`docs/refs/lsp_spec.md` is a Jekyll include skeleton: the completion section body is literally
`{% include_relative language/completion.md %}` at `docs/refs/lsp_spec.md:659`, and the lifecycle /
`initialize` / `registerCapability` bodies are `{% include general/initialize.md %}` and
`{% include messages/3.18/registerCapability.md %}` (`docs/refs/lsp_spec.md:486-488` region). The local
file therefore carries only the **changelog**, which is still useful for dating features:

| Changelog line | Fact |
| :--- | :--- |
| `docs/refs/lsp_spec.md:749` | 3.18 added `completionList.applyKind` combining `itemDefaults` values |
| `:755` / `:757` | 3.17 added completion item label details; label details + insert text mode |
| `:758` | 3.17 added "shared values on CompletionItemList" (= `itemDefaults`) |
| `:778` | 3.17 added client capability for **resolving text edits** on completion items |
| `:780` | 3.16 added insert & replace ranges (`InsertReplaceEdit`) |
| `:811` | 3.15 added `CompletionItem#tag` |
| `:843` | `preselect` property |
| `:867` | `CompletionTriggerKind.TriggerForIncompleteCompletions: 3` |

**Primary sources used instead** (all fetched 2026-10-06):

- **[S1]** Completion feature spec — <https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.18/language/completion.md>
- **[S2]** `initialize` / `ClientCapabilities` / `ServerCapabilities` — <https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.18/general/initialize.md>
- **[S3]** `client/registerCapability` — <https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_includes/messages/3.18/registerCapability.md>

Grounding from this map: [24-part-crates](24-part-crates.md) (crate facts, cited as `crates:<line>`),
[24-part-codebase](24-part-codebase.md), [23-ticket resolve fallback](../../issues/23-task-and-pkm-semantics.md),
[ticket 12](../../issues/12-concurrency-and-cancellation-model.md), [ticket 11](../../issues/11-source-span-and-position-model.md).

---

## 1. The master rule, and every completion-related client capability

**Spec master rule [S2, `general/initialize.md:357`]**:

> "A missing property should be interpreted as an absence of the capability. If a missing property
> normally defines sub properties, all missing sub properties should be interpreted as an absence of
> the corresponding capability."

Same file also: *"Servers receiving a `ClientCapabilities` object literal with unknown properties
should ignore these properties"* and *"For future compatibility a `ClientCapabilities` object literal
can have more properties set than currently defined."* — i.e. **absence is the norm, not an error**,
and 3.18-only client flags may appear from the future in either direction.

### 1.1 Inventory table — `textDocument.completion.*` client capabilities

Capability × spec behaviour-when-absent × Traces' recommended gate × degraded mode.
"Traces use" refers to claims already locked in tickets 15/16/17/19/20/21/22/23.

| # | Capability (path) | Spec behaviour when absent / off | Traces uses it? | Gate decision | Degraded mode |
 |---|---|---|---|---|---|
| A | `completion.dynamicRegistration` | [S3]: *"Not all clients need to support dynamic capability registration. A client opts in via the `dynamicRegistration` property… A client can even provide dynamic registration for capability A but not for capability B."* Absent ⇒ server must not dynamically register completion. | only via registration strategy | **GATE** | none (static registration works on every client) |
| B | `completion.contextSupport` | [S1] on `CompletionParams`: *"This is only available if the client specifies to send this using the client capability `completion.contextSupport === true`."* Absent ⇒ `CompletionContext` (and hence `triggerKind` 1/2/3, `triggerCharacter`) may never arrive. | intended as a hint only | **IGNORE as a requirement**; opportunistically consume when present | none — dispatcher must classify from buffer/AST (see §1.3) |
| C | `completion.completionItem.snippetSupport` | [S1]: *"Client supports snippets as insert text."* Absent ⇒ must not send `insertTextFormat: Snippet`; omitted format defaults to `PlainText`. | yes — ticket 24's own example (`[[target|…]]`), 19's `key: value`, 23's `- [ ] `, 21/22 scaffolds | **GATE** (unit 5) | plain-text insert; cursor lands at end of insertion; no tabstops |
| D | `completion.completionItem.resolveSupport.properties[]` | [S1]: *"By default, the request can only delay the computation of the `detail` and `documentation` properties. Since 3.16.0, the client can signal that it can resolve more properties lazily… All other properties (usually `sortText`, `filterText`, `insertText` and `textEdit`) must be provided in the `textDocument/completion` response and must not be changed during resolve."* | yes — 19's schema-def docs, 16's usage stats (unit 4) | **GATE per-property**, not per-boolean | resolve limited to `detail`+`documentation`; everything else eager |
| E | `completion.completionItem.documentationFormat[]` | [S1]: *"Client supports the following content formats for the documentation property. The order describes the preferred format."* Absent ⇒ do not assume `MarkupContent{kind:markdown}` is rendered; use plain `string`. | yes — 19 wants markdown schema docs (`issues/19:68`) | **GATE** | `documentation` sent as plain text |
| F | `completion.completionItem.labelDetailsSupport` | [S1]: *"The client has support for completion item label details."* Absent ⇒ `CompletionItem.labelDetails` not rendered. | yes — 16's `label_details.detail = "{N} references"` (`issues/16:49`) | **GATE** (+ server must echo `completionOptions.completionItem.labelDetailsSupport`) | usage badges vanish; optionally fold into `detail` |
| G | `completion.completionItem.preselectSupport` | [S1] on `preselect`: *"the tool / client decides which item that is. The rule is that the *first* item of those that match best is selected."* Absent ⇒ don't rely on `preselect`. | yes — 23's "`preselect` top" (`issues/23:24`) | **GATE** | none in practice: 23 already puts config-order first via `sortText` |
| H | `completion.completionItem.insertReplaceSupport` | [S1] on `textEdit`: *"Clients need to signal support for `InsertReplaceEdit`s via the `textDocument.completion.completionItem.insertReplaceSupport` client capability property."* Absent ⇒ return plain `TextEdit` only. | not claimed by any ticket | **GATE (recommend: never emit `InsertReplaceEdit` in v1)** | n/a — plain `TextEdit` is universally accepted |
| I | `completion.completionItem.commitCharactersSupport` (+ server `allCommitCharacters`) | [S1]: per-item `commitCharacters` are what the client honours; `allCommitCharacters` is the server-side fallback *"if clients don't support individual commit characters per completion item."* | no ticket claims commit characters | **IGNORE / do not use** | n/a (deliberate: committing on `(`, `[`, `#`, `:` would break Markdown/PKM typing) |
| J | `completion.completionItem.tagSupport` / `deprecatedSupport` | [S1]: `tagSupport.valueSet` = tags the client renders; *"Clients supporting tags have to handle unknown tags gracefully."* `deprecated` doc: *"@deprecated Use `tags` instead if supported."* | no deprecation concept in Traces | **IGNORE (no tags in v1)** | n/a |
| K | `completion.completionItem.insertTextModeSupport` + `completion.insertTextMode` | [S1] on item `insertTextMode`: *"If not provided, the client's default value depends on the `textDocument.completion.insertTextMode` client capability."* | only if multiline snippets are used (22/21 scaffolds) | **GATE-light (unit 5)** | client's own indentation heuristic applies to multiline inserts |
| L | `completion.completionItemKind.valueSet[]` | [S1]: *"When this property exists the client also guarantees that it will handle values outside its set gracefully… **If this property is not present the client only supports the completion items kinds from `Text` to `Reference`** as defined in the initial version of the protocol."* | yes — research/20 proposes `CompletionItemKind::ENUM_MEMBER` (20) and `File` (17) ([20-designs](../20-designs.md):365-366,644); 16 uses `KEYWORD` (14) | **GATE** | **clamp kinds to 1–18** when `valueSet` absent (see §1.2) |
| M | `completion.completionList.itemDefaults[]` (3.17) | [S1] on `CompletionList.itemDefaults`: *"Servers are only allowed to return default values if the client signals support for this via the `completionList.itemDefaults` capability."* Absent ⇒ no list-level defaults. | would be useful (shared `editRange`/`insertTextFormat`/`data`) | **UNAVAILABLE — crate-blocked** (§8) | n/a; always emit per-item fields |
| N | `completion.completionList.applyKindSupport` (3.18) | [S1]: *"Servers are only allowed to return `applyKind` if the client signals support for this via the `completionList.applyKindSupport` capability."* | no | **UNAVAILABLE — crate-blocked twice** (§8) | n/a |
| O | `general.positionEncodings` (3.17) | [S2, `initialize.md:633-643`]: *"if the value 'utf-16' is missing from the array of position encodings servers can assume that the client supports UTF-16. UTF-16 is therefore a mandatory encoding. If omitted it defaults to `['utf-16']`."* Server side [S2, `:762-772`]: *"If the client didn't provide any position encodings the only valid value that a server can return is 'utf-16'. If omitted it defaults to 'utf-16'."* | yes — ticket 11's whole conversion boundary | **GATE (ticket 11/29)** | UTF-16 conversion path must exist unconditionally (§1.4) |
| P | `window.workDoneProgress` (via `CompletionOptions extends WorkDoneProgressOptions`) | Absent ⇒ no work-done progress for completion. | no | **IGNORE (v1)** | none |

Not a capability but protocol behaviour worth recording:

| Q | `CompletionList.isIncomplete` | [S1]: `CompletionItem[]` *"is interpreted to be complete, so it is the same as `{ isIncomplete: false, items }`"*; `isIncomplete: true` ⇒ *"Further typing should result in recomputing this list. Recomputed lists have all their items replaced (not appended) in the incomplete completion sessions."* | yes — 20 already fixes `is_incomplete: true` (`decomposition:15`) | **No gating; design decision** (§4) | n/a |

**Phantom capability to correct**: `24-part-decomposition.md:88` lists `documentationMarkdown`.
That name appears **nowhere** in the 3.18 completion spec nor in `ls-types` (verified: `[S1]` full
fetched text; `ls-types 0.0.6 source/src/completion.rs`). The real names are
`completion.completionItem.documentationFormat` (per-item formats) and `general.markdown`
(general `MarkupContent` markdown rendering). Closes `[q5]` in
[24-part-crates](24-part-crates.md):795-797 — **do not cite `documentationMarkdown`.**

### 1.2 `completionItemKind.valueSet` — the one gate with a hard numeric cliff

The spec's fallback is precise and unusual: **absent ⇒ only kinds 1–18 are supported**
(`Text`=1 … `Reference`=18). Traces' claimed kinds and where they land:

| Kind | Value | Ticket | In 1–18 floor? |
| :--- | :--- | :--- | :--- |
| `KEYWORD` | 14 | 16 (`issues/16:48`) | yes |
| `FILE` | 17 | 16's contrast case, 20 file results | yes |
| `FIELD` | 5 | research/20:365, research/21:96 | yes |
| `REFERENCE` | 15-ish → `Reference`=18 | 15 wikilink/heading targets | yes |
| `FOLDER` | **19** | rumdl uses it (`digest lsp_rvben…:29920`); Traces 20 may | **no** |
| `ENUM_MEMBER` | **20** | research/20:366 (select-option values) | **no** |

**Recommendation**: read `completionItemKind.valueSet` at `initialize`; if absent, map
`FOLDER→FILE(17)` and `ENUM_MEMBER→VALUE(12)` (or `FIELD(5)`) for that client only. Cost: one small
match. Alternative (acceptable): accept the generic icon — the spec only guarantees 1–18 *support*,
it does not forbid higher values, and VS Code/Neovim/Zed all advertise full `valueSet` anyway. Either
way, **record the clamp-or-accept choice as an explicit line for ticket 29**, because Helix (§6) is a
mainstream editor that does *not* advertise `valueSet`.

### 1.3 `contextSupport` — confirm and expand ("the dispatcher must work without it")

Confirmed, and the picture is worse than "absent ⇒ no context": the signal is unreliable **in both
directions**.

1. **Spec**: `CompletionContext` is only guaranteed when `contextSupport === true` [S1].
2. **Crates**: `CompletionParams.context: Option<CompletionContext>`; `CompletionTriggerKind` =
   `INVOKED=1`, `TRIGGER_CHARACTER=2`, `TRIGGER_FOR_INCOMPLETE_COMPLETIONS=3`
   ([24-part-crates](24-part-crates.md):253-290, quoted from `[S1]`).
3. **A mainstream client that never advertises it but sends it anyway**: Helix sets
   `context_support: None, // additional context information Some(true)` at
   `helix-lsp/src/client.rs:657` yet constructs `context: Some(context)` unconditionally in
   `Client::completion` at `helix-lsp/src/client.rs:1180` (raw fetch of
   <https://raw.githubusercontent.com/helix-editor/helix/master/helix-lsp/src/client.rs>, 2026-10-06).
   → A server that *read* `contextSupport` to decide whether to trust `context` would be wrong about Helix.
4. **A mature server that assumes the opposite**: Microsoft's `vscode-markdown-languageserver`
   spreads `...(params.context || {})` into its completion options (`src/server.ts:348-361`,
   <https://github.com/microsoft/vscode-markdown-languageserver/blob/main/src/server.ts>) — i.e. it is
   written to survive `context: undefined` even though it never inspects `contextSupport`.

**Consequence for ticket 24 (this is the seam with unit 1)**: `trigger_kind` / `trigger_character`
are *cache hints only*. Two concrete rules:

- **Dispatch** must be decided by the outward-walking AST/span scan over the buffer (ticket 11 spans)
  plus line-local predicates — identical code path for `Invoked`, `TriggerCharacter`, no-`context`, and
  an unknown client. This matches [24-part-crates](24-part-crates.md):693-698 (implication 1).
- **`TriggerForIncompleteCompletions` (3) must not be required.** When `isIncomplete: true` was
  returned, the *server itself* knows the next request for that document is a re-trigger; track it
  per-document rather than reading `triggerKind`. Otherwise 20's `is_incomplete: true` silently
  degrades to "`Invoked`" for every client that omits context.

### 1.4 Position encoding — completion-specific interplay only

- Every `textEdit` / `additionalTextEdits` / `CompletionList.itemDefaults.editRange` `Position.character`
  is in the negotiated encoding ([S2]; `ls-types` echoes the spec on `Position::character`, quoted
  `crates:338-340`).
- Completion adds **two hard range rules** independent of encoding [S1]: the edit range *"must be a
  single line range and it must contain the position at which completion has been requested"* (the
  *edit* may still write multiple lines), and for `InsertReplaceEdit` the insert range *"must be a
  prefix of the edit's replace range"*.
- **UTF-16 is unavoidable**: it is mandatory to support [S2:633-643] and is the only legal answer when
  the client sends no `positionEncodings` [S2:762-772]. So ticket 11's `LineIndex::byte_to_utf16_cu`
  (planned, `codebase:88-91`) is the *required* path; `utf-8` is a second path that may only be added
  as an optimization (and see §6: **VS Code throws** if a server answers non-`utf-16`).
- For completion specifically, utf-8 negotiation would save one conversion on a <20 ms hot path
  (ticket 33) — that is the only completion-specific argument for it.

---

## 2. Server-side capability & registration fields (what Traces must send)

| Field | Type in `ls-types 0.0.6` | Notes |
| :--- | :--- | :--- |
| `ServerCapabilities.completion_provider` | `Option<CompletionOptions>` (`lib.rs:1779-1781`, `crates:360`) | Absent ⇒ client sends no completion requests at all. Helix short-circuits on it: `capabilities.completion_provider.as_ref()?` (`helix-lsp/src/client.rs:1173`). |
| `CompletionOptions.resolve_provider` | `Option<bool>` (`completion.rs:287`) | Advertise `true` iff Traces implements `completionItem/resolve` (unit 4). |
| `CompletionOptions.trigger_characters` | `Option<Vec<String>>` (`completion.rs:292`) | Flat `Vec` for the *whole* server — there is no per-context registration ([24-part-crates](24-part-crates.md):725-729). Owns the rumdl collision question (unit 2). |
| `CompletionOptions.all_commit_characters` | `Option<Vec<String>>` (`completion.rs:306`) | Recommend `None` (decision I). |
| `CompletionOptions.completion_item` | `Option<CompletionOptionsCompletionItem { label_details_support }>` (`completion.rs:329-337`) | **Server-side echo of F.** VS Code stores `options.completionItem?.labelDetailsSupport` per registration and passes it to `code2ProtocolConverter.asCompletionItem(item, …)` on resolve (`client/src/common/completion.ts:141,181`) — i.e. it *drops* `labelDetails` from the item sent to `completionItem/resolve` unless the server advertised this. Set it to mirror the client. |
| `CompletionRegistrationOptions` | `TextDocumentRegistrationOptions` (documentSelector) + `CompletionOptions` (`completion.rs:340-347`) | Used only for dynamic registration. |
| `ServerCapabilities.position_encoding` | `Option<PositionEncodingKind>` (`lib.rs:1758`, `crates:426-431`) | See §1.4. |

Registration mechanics available in `tower-lsp-server` (crates:120-139): static via
`InitializeResult.capabilities`; dynamic via `Client::register_capability` /
`unregister_capability` (`src/service/client.rs:103-135`), with `Registration { id, method,
register_options }`, executed at the `initialized` notification.

### 2.1 Spec rule on doing both [S3] — quote this in ticket 29

> "The server must not register the same capability both statically through the initialize result and
> dynamically for the same document selector. If a server wants to support both static and dynamic
> registration, it needs to check the client capability in the initialize request and only register
> the capability statically if the client doesn't support dynamic registration for that capability."

Read carefully: the *must not* is about doing **both for one selector**; the conditional sentence
governs servers that want **both modes**. A server that registers **statically only** and never
dynamically does not violate the first sentence either way. That is the cheap, spec-clean option.

### 2.2 Recommendation: **static registration at `initialize`, config-derived trigger set**

Rationale, in order of strength:

1. **Helix — a mainstream floor client — advertises no `completion.dynamicRegistration` at all**
   (`helix-lsp/src/client.rs:637-659` … `..Default::default()`), and Neovim advertises
   `completion.dynamicRegistration = false` (`runtime/lua/vim/lsp/protocol.lua:488`). Any strategy
   that *requires* dynamic registration breaks both.
2. Traces' trigger set is knowable at `initialize` from config (`[tasks]`, `enableLinkCompletions`
   toggles, rumdl coexistence per ticket 03/32) — no runtime requirement has been filed
   ([24-part-crates](24-part-crates.md):710-714).
3. No per-context dynamic registration exists as a useful primitive: `triggerCharacters` is one flat
   array on one method (crates:725-729); per-context selectivity must come from server-side dispatch.
4. Dynamic `register/unregister` remains *available* for one real future case: withdrawing or adding
   trigger characters when a config toggle flips mid-session (e.g. rumdl's
   `enableLinkCompletions` changed without restart). Gate that path on
   `completion.dynamicRegistration == Some(true)`; when false, log that a restart is required.

Contrast: Microsoft's own `vscode-markdown-languageserver` registers completion **dynamically only**
— its `capabilities` block has **no** `completionProvider` (`src/server.ts:100-126`), and completion is
wired via `connection.client.register(CompletionRequest.type, registrationOptions)` inside
`registerDynamicClientFeature` (`src/server.ts:333-373`). It also does **not** check
`dynamicRegistration` before registering (the helper only reacts to settings changes,
`src/server.ts:311-331`). **QUESTION**: does `vscode-languageserver` no-op/fail that request for a
client that didn't opt in, and would such a client then get *zero* completion? (Verify before citing
MS's markdown LS as a model for registration.)

---

## 3. Ecosystem practice: how mature servers branch on these flags

Separate from §1 (spec). All URLs fetched 2026-10-06.

### 3.1 rust-analyzer — the most granular gating model (recommended template)

`crates/rust-analyzer/src/lsp/capabilities.rs`
(<https://github.com/rust-lang/rust-analyzer/blob/master/crates/rust-analyzer/src/lsp/capabilities.rs>):

- **`completion_snippet()`** → `…text_document.completion.completion_item.snippet_support`
  `.unwrap_or_default()` — **absent ⇒ snippets off** (feature-off-without-capability, no ambiguity).
- **`completions_resolve_provider()`** → builds `CompletionFieldsToResolve::from_client_capabilities(
  &resolve_support.properties)` and returns `fields != CompletionFieldsToResolve::empty()` —
  **`resolveProvider` is advertised only if the client listed at least one *recognised* property**.
- **`completion_label_details_support()`** → `.unwrap_or_default()`, then mirrored back to the client
  through `CompletionOptionsCompletionItem { label_details_support: Some(...) }` — exactly the echo
  required by decision F (§2).
- **Client sniffing as a last resort**: `resolve_provider: if config.client_is_neovim() {
  config.has_completion_item_resolve_additionalTextEdits().then_some(true) } else { Some(…) }` —
  i.e. rust-analyzer special-cases Neovim rather than trusting the advertised flag.
- **Per-property resolve allow-list** (`crates/ide-completion/src/lib.rs:47-68`): `labelDetails`,
  `tags`, `detail`, `documentation`, `filterText`, `textEdit`, `command` — each independently
  deferrable. `filterText`/`textEdit` in that list is exactly what [S1] permits *only* when listed.
- **History worth knowing**: PR #18630 *"Temporarily disable completion resolve support for helix and
  neovim"* (2024-12-06) and PR #18589 *"Advertise completions and inlay hints resolve server
  capabilities based on the client capabilities"* —
  <https://github.com/rust-lang/rust-analyzer/pull/18630>,
  <https://github.com/rust-lang/rust-analyzer/pull/18589>. **Advertised ≠ honoured**: two mainstream
  clients advertised resolve properties they mishandled, and the *server* had to back off. Lesson for
  24: gate on the flag, but keep the resolve payload small and testable, and keep an escape hatch.

### 3.2 gopls — coarse boolean gates + plaintext default

`gopls/internal/settings/settings.go` `ForClientCapabilities` (<https://github.com/golang/tools/blob/master/gopls/internal/settings/settings.go>):

```go
if c := caps.TextDocument.Completion; c.CompletionItem.SnippetSupport { o.InsertTextFormat = protocol.SnippetTextFormat }
o.InsertReplaceSupported = caps.TextDocument.Completion.CompletionItem.InsertReplaceSupport
// tags preferred, else deprecatedSupport:
if caps...TagSupport != nil && ...ValueSet != nil { o.CompletionTags = true } else if ...DeprecatedSupport { o.CompletionDeprecated = true }
```
`DefaultOptions()` sets `InsertTextFormat: protocol.PlainTextTextFormat` — **plaintext is the
built-in default**, snippets are opt-in from the client flag. gopls also runs a *self-imposed latency
budget* (`CompletionBudget`, default 100 ms, "as we use up our budget we dynamically reduce the
search scope") — direct prior art for ticket 33's budget-reduction pattern.
**QUESTION**: `PreferredContentFormat` is derived from `hover.contentFormat`, not from
`completion.completionItem.documentationFormat` — whether gopls's completion documentation honours
the *completion-specific* format list is unverified.

### 3.3 typescript-language-server — feature flags derived per-field

`src/lsp-server.ts:151-161` (<https://github.com/typescript-language-server/typescript-language-server/blob/master/src/lsp-server.ts>):

```ts
const { commitCharactersSupport, insertReplaceSupport, labelDetailsSupport, snippetSupport } = completionItem;
this.features.completionCommitCharactersSupport = commitCharactersSupport;
this.features.completionInsertReplaceSupport = insertReplaceSupport;
this.features.completionSnippets = snippetSupport;
this.features.completionLabelDetails = …useLabelDetailsInCompletionEntries && labelDetailsSupport && typescriptVersion.version?.gte(API.v470);
```
Note the **triple gate** on label details: client flag ∧ user preference ∧ *server-side library
version*. Its server capability is static with `resolveProvider: true` and
`triggerCharacters: ['.', '"', '\'', '/', '@', '<']` (`src/lsp-server.ts:261-263`).

### 3.4 Microsoft markdown-language-service/server — capability-blind, context-tolerant

- `registerCompletionsSupport` never reads client capabilities (§2.2); the completion handler
  degrades on `params.context || {}` (`src/server.ts:348-361`).
- Items are plain: `CompletionItemKind.Reference` / `Value` / `File` / `Folder` and a `command` that
  fires `editor.action.triggerSuggest` after accepting a folder ([tool-ms-markdown-language-service](../tool-ms-markdown-language-service.md):30-32).
  **That use of `Folder`=19 is itself a reminder that `valueSet` matters** — the folder case is
  exactly where a kind > 18 is emitted.
- **No capability gating anywhere in the completion path** → this is the "assume-modern-client"
  pole. It works because the payload is conservative (no snippets, no resolve, no labelDetails).

### 3.5 Marksman / Markdown Oxide — conservative static registrations, plus client sniffing

- **Marksman**: `CompletionProvider = Some { TriggerCharacters = Some [| '['; '#'; '(' |];
  ResolveProvider = None; AllCommitCharacters = None; CompletionItem = None }`
  (`docs/digests/lsp_artempyanykh-marksman-digest.txt:11150-11154`). No resolve, no server-side
  `labelDetailsSupport` echo. It *does* sniff the client for other features — `ClientDescription.IsVSCode`
  flips `WorkspaceSymbolProvider`/`DocumentSymbolProvider` (`digest:11147-11149`, `:12074-12138`),
  and `PreferredTextSyncKind` is client-derived (`:11064,11079`). Precedent for *explicit client
  detection* over blind assumption.
- **Markdown Oxide**: `completion_provider: Some(CompletionOptions { resolve_provider: Some(false),
  trigger_characters: Some(["[", " ", "(", "#", ">"]), all_commit_characters: None, completion_item: None })`
  (`docs/digests/lsp_feel-ix-343-markdown-oxide-src-digest.txt:2028-2037`). It reads
  `ClientCapabilities` only to disable semantic tokens when the client lacks them
  (`digest:1244-1256`) — a clean, minimal example of gating.
- **rumdl**: its `completion.rs` region is in the digest (`digest:29374+`) but the **`ServerCapabilities`
  construction is not** — static-vs-dynamic registration for rumdl remains unverified
  (matches the existing `codebase:295-301` flag). **QUESTION for unit 2.**

### 3.6 pylance / closed-source clients

No public source. **QUESTION** — treat pylance as unmodelled; the vscode-languageclient-derived
profile (§6, VS Code row) is the best proxy since Pylance runs in the VS Code client.

---

## 4. Sparse-client floor statement (for ticket 29's "minimum viable client")

**Definition — the minimum viable completion client (`MVC-C`)**, derived purely from §1 (every
capability absent/`None`):

```
textDocument.completion present, everything under it absent/None
  ⇒ no dynamicRegistration, no contextSupport, no snippets, no resolveSupport,
    no documentationFormat, no labelDetailsSupport, no preselectSupport,
    no insertReplaceSupport, no commitCharactersSupport, no tagSupport,
    no insertTextModeSupport, no completionList.itemDefaults, no applyKindSupport,
    no completionItemKind.valueSet
general.positionEncodings absent  ⇒ utf-16 only
```

**MVC-C must still get** (this is the floor ticket 29 holds Traces to):

1. Full candidate lists with `label`, `kind` (clamped ≤18), `sortText`, `filterText`, and a
   single-line `TextEdit` containing the cursor — i.e. **every context in 15/16/17/19/20/21/22/23
   remains functional**; only presentation degrades.
2. `documentation` as a **plain string**, computed eagerly or on `resolve` (both legal without
   `resolveSupport`), never as markdown `MarkupContent`.
3. `insertTextFormat` omitted (= `PlainText`) everywhere; no tabstops.
4. Static registration only; `CompletionContext` may be `null` — dispatch identical either way.
5. UTF-16 ranges via `LineIndex::byte_to_utf16_cu` / ropey.

**What disappears on MVC-C (feature-off, not broken):**

| Feature | Ticket | Why |
| :--- | :--- | :--- |
| Snippet cursor placement inside `[[target|…]]` etc. | 24/unit 5 | no `snippetSupport`; **no substitute** — `command`-based cursor placement needs a client-side command handler Traces does not have ([24-part-crates](24-part-crates.md):751-756) |
| Usage-stat badges on tags | 16 | no `labelDetailsSupport` |
| `preselect` on task checkbox | 23 | no `preselectSupport` — *cosmetic only*, `sortText` still orders |
| Deferred schema-field docs | 19/unit 4 | `resolveSupport` absent ⇒ only `detail`+`documentation` may be deferred (still enough for 19's docs!) — so **19 degrades to "eager or `detail`-only", not to "gone"** |
| Insert-vs-replace distinction | (none claimed) | plain `TextEdit` only |
| Enum-member / folder icons | 20 | kind clamp |

**Relationship to 23's precedent**: ticket 23's "`workspaceSymbol/resolve` with **full-Location
fallback when `resolveSupport` absent**" (`issues/23:51`, handed to 29 at `issues/23:64`) is the
same shape as decision D: *absent ⇒ put the expensive payload in the initial response instead of
deferring it*. The completion analogue is "eager `documentation`/`detail` in the initial list".
Recommend ticket 29 records **one shared rule**: *"absent resolveSupport ⇒ eager-resolve, never
drop."* — it covers 23's symbols and 24's completion with a single sentence.

**Floor vs ceiling in one line**: floor = MVC-C (works everywhere, loses presentation);
ceiling = VS Code/Neovim/Zed (§6) gets snippets, labelDetails, itemDefaults-equivalent behaviour
(once ls-types allows it), markdown docs, `context`, and preselect.

---

## 5. Filtering/sorting ownership — the capability that isn't a capability

[S1] sets the default model explicitly (quoted `crates:369-385`):

- *"usually the client is responsible for filtering and sorting… However servers can enforce
  different behavior by setting a `filterText` / `sortText`."*
- *"for speed, clients should be able to filter an already received completion list if the user
  continues typing. Servers can opt out of this using a `CompletionList` and mark it as `isIncomplete`."*
- Two insertion modes: with `insertText` the client does **word-boundary guessing**; with `textEdit`
  *"no word guessing takes place and no automatic filtering (like with an `insertText`) should happen…
  If the text edit is a replace edit then the range denotes the word used for filtering."*

Traces' natural shape is **text-edit mode** (precise replacement of a partial `[[tar`, `#pro`,
`field: val` span — 21 already mandates `textEdit` with explicit range for *all* completions,
`decomposition:16`). Consequences that belong to 29 as inputs:

1. **Server may not assume client fuzzy quality.** In text-edit mode the client's word-guessing is
   switched off; what remains is client-side matching against the replace range using `filterText`.
   Options remain as [24-part-crates](24-part-crates.md):769-778 frames them — (a) return candidates
   and let the client filter, or (b) server-filter with `fuzzy-matcher` and return
   `isIncomplete: true` so every keystroke re-queries. **(b) is the only option that guarantees
   identical ranking across MVC-C and VS Code**, but costs a full round-trip per keystroke under
   ticket 12's `concurrency_level(1)` — and with `contextSupport` off the server cannot even see
   `TriggerForIncompleteCompletions`, so it must detect re-triggers itself (§1.3).
2. **`sortText` is the only order guarantee** that survives every client. All four locked ordering
   schemes (19's layer prefixes, 23's config order, 20's relevance sort, 15's fuzzy score) must be
   folded into it.
3. Empty-word contexts (cursor right after `[`, `#`, `:`) mean `fuzzy-matcher` scores everything
   equal → ordering rests entirely on `sortText` (crates:779-781).

---

## 6. Editor reality profile (2026-10-06, per-editor source)

Completion-related capabilities actually advertised. `—` = absent/`None` (⇒ §1 rule applies).

| Capability | **VS Code** (`vscode-languageclient`) | **Neovim** (core defaults) | **Neovim + nvim-cmp** (`cmp-nvim-lsp`) | **Zed** | **Helix** |
|---|---|---|---|---|---|
| `dynamicRegistration` | `true` | `false` | `false` (override default) | `true` | — |
| `contextSupport` | `true` | `true` | `true` | `true` | **— (but still sends `context`)** |
| `snippetSupport` | `true` | `true` | `true` (override) | `true` | `Some(config)` — default on, user-configurable |
| `resolveSupport.properties` | `documentation, detail, additionalTextEdits` | `additionalTextEdits, command, documentation, detail` | `documentation, additionalTextEdits, insertTextFormat, insertTextMode, command` | `additionalTextEdits, command, detail, documentation` (**explicitly excludes `textEdit`** — *"NB: Do not have this resolved, otherwise Zed becomes slow"*) | `documentation, detail, additionalTextEdits` |
| `documentationFormat` | `markdown, plaintext` | `markdown, plaintext` | (not set by helper) | `markdown, plaintext` | — |
| `preselectSupport` | `true` | `true` | `true` | — | — |
| `insertReplaceSupport` | `true` | `true` | `true` | `true` | `true` |
| `commitCharactersSupport` | `true` | `true` | `true` | — | — |
| `tagSupport` | `[Deprecated]` | `[Deprecated]` | `[Deprecated]` | `[Deprecated]` | `[Deprecated]` |
| `labelDetailsSupport` | `true` | `true` | `true` | `true` | — |
| `insertTextModeSupport` | `asIs, adjustIndentation` | (—) | (—) | `asIs, adjustIndentation` | — |
| `completionItemKind.valueSet` | all 1–25 | all 1–25 | (—) | (not set ⇒ 1–18 floor) | **— (⇒ 1–18 floor)** |
| `completionList.itemDefaults` | all five + **`applyKindSupport: true`** | all five + `applyKindSupport: true` | all five | all five | — (no `completionList` at all) |
| `general.positionEncodings` | **`['utf-16']` only** | `['utf-8','utf-16','utf-32']` | same | `['utf-16']` only | `['utf-8','utf-32','utf-16']` |

Sources:
- VS Code: `client/src/common/completion.ts:81-116` (`fillClientCapabilities`) and
  `client/src/common/client.ts:2150` (`generalCapabilities.positionEncodings = ['utf-16']`),
  <https://github.com/microsoft/vscode-languageserver-node>.
- **VS Code hard-fails non-utf-16 servers**: `client/src/common/client.ts:1489-1491` throws
  `Unsupported position encoding … received from server` when `result.capabilities.positionEncoding !== UTF16`.
  → **utf-8 negotiation must be strictly opt-in per-client, never assumed.** (Bears directly on
  tickets 11/29; also the reason MS's own markdown LS is utf-16-only, [tool-ms-markdown-language-service](../tool-ms-markdown-language-service.md):36-37.)
- Neovim core: `runtime/lua/vim/lsp/protocol.lua:487-523` (completion) and `:359-365`
  (`general.positionEncodings = { 'utf-8', 'utf-16', 'utf-32' }`),
  <https://github.com/neovim/neovim>. Neovim's built-in completion **does** expand snippets
  (`runtime/lua/vim/lsp/completion.lua:179-183,208,1034-1035` → `vim.snippet.expand`), so its
  `snippetSupport = true` is trustworthy today.
- nvim-cmp: `hrsh7th/cmp-nvim-lsp` `lua/cmp_nvim_lsp/init.lua` `default_capabilities`,
  <https://github.com/hrsh7th/cmp-nvim-lsp>. (Note a latent quirk: `contextSupport = if_nil(override.snippetSupport, true)`
  keys off the *snippet* override — harmless in practice, worth knowing if Traces tests against it.)
- Zed: `crates/lsp/src/lsp.rs` `InitializeParams`,
  <https://github.com/zed-industries/zed/blob/main/crates/lsp/src/lsp.rs>.
- Helix: `helix-lsp/src/client.rs:637-659` (completion block), `:757-761` (position encodings),
  <https://github.com/helix-editor/helix>.

**Reading of the table**

- **Realistic ceiling** = VS Code / Neovim(+cmp) / Zed: everything except `applyKind`-aware
  `itemDefaults` handling on Zed/Neovim-vs-VS Code nuance. All three give snippets, markdown docs,
  label details, preselect, and `context`.
- **Realistic floor among mainstream editors** = **Helix**: it is a first-class editor that omits
  `contextSupport`, `dynamicRegistration`, `completionItemKind.valueSet`, `documentationFormat`,
  `labelDetailsSupport`, `preselectSupport`, `commitCharactersSupport`, `insertTextModeSupport` and
  the whole `completionList` block. **MVC-C is therefore not a hypothetical** — Helix sits ~1 flag
  away from it, and it exercises the kind clamp (§1.2) and the no-`context` dispatcher (§1.3) today.
- **Position encodings**: only VS Code and Zed are utf-16-only; Helix and Neovim offer utf-8 first.
  Since VS Code *throws* on a non-utf-16 answer, the safe rule for 29 is: **answer utf-16 unless the
  client's list omits `utf-16` or explicitly leads with something else — and even then, prefer utf-16
  until ticket 11's conversion is proven.** The completion-specific stake is only the hot-path
  conversion cost (§1.4).
- **Trust but verify**: rust-analyzer had to disable resolve for Helix and Neovim despite their
  advertised flags (§3.1). Flags are necessary, not sufficient.

---

## 7. Registration decision summary (seam with units 1 & 2)

| Question | Answer for 24/29 | Evidence |
| :--- | :--- | :--- |
| Static or dynamic `textDocument/completion`? | **Static at `initialize`** | Helix has no `dynamicRegistration`; Neovim `false`; trigger set config-derivable; [S3] forbids doing both for one selector |
| Where does the trigger set live? | `CompletionOptions.trigger_characters` (one flat array) | no per-context registration exists (crates:725-729) |
| Per-context selectivity? | Server-side dispatch only | units 1 & 2 |
| Dynamic re-registration on config flip? | Optional, gated on `dynamicRegistration == Some(true)`; otherwise require restart | [S3]; `Client::register_capability` available (`crates:123-139`) |
| When is `context` trusted? | Never required; always a hint | §1.3 |
| `resolveProvider`? | `Some(true)` if unit 4 ships resolve; the *fields* deferred are per §1 D | §3.1 (rust-analyzer turns it off entirely when no recognised property — optional stricter variant) |
| `completionOptions.completionItem.labelDetailsSupport`? | Mirror the client's `labelDetailsSupport` | §2; VS Code drops `labelDetails` on resolve otherwise |

---

## 8. "Spec says X, crate can't send X" — complete completion gap list (`ls-types 0.0.2`–`0.0.6`)

Verified by reading `ls-types 0.0.6 source/src/completion.rs` on disk (0.0.2 is byte-identical for
completion per `crates:229-232`; all gaps below persist in both).

| # | Spec feature (since) | `ls-types` status | Consequence / recommended handling |
|---|---|---|---|
| 1 | `CompletionList.itemDefaults` (3.17: `commitCharacters`, `editRange`, `insertTextFormat`, `insertTextMode`, `data`) | **Absent** — `CompletionList { is_incomplete, items }` only (`completion.rs:414-421`); no `CompletionItemDefaults` type anywhere | Server cannot emit list defaults. Client capability side *is* readable (`CompletionListCapability.item_defaults`, `completion.rs:200-211`) but there is nothing to answer with ⇒ **decision M is forced to "ignore" regardless of client.** Flag to 09/29: upstream PR or hand-serialized response enum if unit 4 ever needs it |
| 2 | `CompletionItem.textEditText` (3.17) | **Absent** (no field on `CompletionItem`, `completion.rs:425-566`) | No shorthand even if #1 were fixed |
| 3 | `CompletionList.applyKind` / `ApplyKind` (3.18) | **Absent** | 3.18 merge semantics unavailable — harmless while #1 is unavailable |
| 4 | `CompletionListCapabilities.applyKindSupport` (3.18) | **Absent** — `CompletionListCapability` has *only* `item_defaults` (`completion.rs:200-211`) | The server cannot even *read* whether the client supports `applyKind`. Distinct from #3: this is a **client-capability** gap, worth stating separately in 29 |
| 5 | `documentationMarkdown` (named in `decomposition:88`) | **Does not exist in the spec or the crate** | Phantom — see §1.1. Use `documentationFormat` + `general.markdown` |

**Everything else in the §1 inventory is expressible.** Verified present in
`ls-types 0.0.6 source/src/completion.rs`:

`CompletionClientCapabilities` (`:215-244`) = `dynamic_registration`, `completion_item`,
`completion_item_kind`, `context_support`, `insert_text_mode`, `completion_list`;
`CompletionItemCapability` (`:63-131`) = `snippet_support`, `commit_characters_support`,
`documentation_format`, `deprecated_support`, `preselect_support`, `tag_support`,
`insert_replace_support`, `resolve_support`, `insert_text_mode_support`, `label_details_support`;
`CompletionItemKindCapability.value_set` (`:185-196`); `CompletionOptions` (`:286-326`) with
`completion_item: CompletionOptionsCompletionItem { label_details_support }` (`:329-337`);
`CompletionRegistrationOptions` (`:340-347`); `CompletionItem` full field set incl. `label_details`,
`preselect`, `tags`, `text_edit: CompletionTextEdit::{Edit, InsertAndReplace}`, `data`
(`:425-566`).

**`tower-lsp-server`-specific** (crates:198-232): the `proposed` cargo feature is unbuildable against
current `ls-types` (probe: `proposed` was removed in `ls-types` 0.0.5); **no completion type is
gated behind `proposed` in any version** — irrelevant to unit 8 except as a CI note if
`--all-features` builds are planned (`crates:798-800`). Do **not** pin `ls-types =0.0.2` to get it
(it drags `fluent-uri 0.3`).

---

## 9. Ready-to-paste input lines for ticket 29

```
INPUT from 24 / completion-capability-negotiation (research/24-completion-capability-negotiation.md)

C-N1  Static vs dynamic: register textDocument/completion STATICALLY in the initialize result,
      config-derived trigger set. Do not dynamically register completion. Rationale: Helix
      advertises no completion.dynamicRegistration; Neovim advertises false; the trigger set is
      knowable from config at initialize. Keep Client::register_capability only for mid-session
      trigger-set changes, gated on completion.dynamicRegistration == Some(true); otherwise require
      a restart. Spec rule to record verbatim (S3): "The server must not register the same
      capability both statically through the initialize result and dynamically for the same document
      selector."

C-N2  contextSupport: IGNORE as a requirement. CompletionContext/triggerKind are optional hints only;
      the dispatcher must classify from buffer + AST identically for Invoked / TriggerCharacter /
      null context. Helix advertises contextSupport absent yet still sends context — read it, never
      depend on it. TriggerForIncompleteCompletions must be reconstructable server-side from
      "my last list for this document was isIncomplete=true".

C-N3  snippetSupport: GATE. Absent => insertTextFormat omitted (PlainText). Documented cost of the
      floor: no cursor placement inside insertions (command-based placement needs a client command
      handler we do not have). Snippets off, never "assume modern".

C-N4  resolveSupport: GATE PER PROPERTY (rust-analyzer model), not as a boolean. Absent => only
      detail + documentation may be deferred (pre-3.16 default); everything else eager. SHARED RULE
      with ticket 23's workspaceSymbol full-Location fallback: "absent resolveSupport => eager-resolve,
      never drop the payload."

C-N5  documentationFormat: GATE. Absent => plain-string documentation, never MarkupContent{markdown}.

C-N6  labelDetailsSupport: GATE, and MIRROR it in ServerCapabilities.completionProvider
      .completionItem.labelDetailsSupport — vscode-languageclient drops labelDetails from the item it
      sends to completionItem/resolve unless the server advertised this.

C-N7  preselectSupport: GATE (23's preselect). Absent => omit preselect; sortText already orders.

C-N8  insertReplaceSupport: GATE; recommend NEVER emitting InsertReplaceEdit in v1 (plain TextEdit
      is universally accepted and already required to be single-line + contain the cursor).

C-N9  completionItemKind.valueSet: GATE with a hard floor. Absent => client only supports kinds
      1..18. Decide: clamp FOLDER(19)/ENUM_MEMBER(20) to FILE(17)/VALUE(12), or accept the generic
      icon. Helix (mainstream) does not advertise valueSet.

C-N10 commitCharactersSupport / allCommitCharacters / tagSupport / deprecatedSupport /
      insertTextModeSupport: IGNORE in v1 (no Markdown/PKM use case filed).

C-N11 completionList.itemDefaults + applyKindSupport: UNAVAILABLE — ls-types cannot send
      itemDefaults, textEditText, applyKind, and cannot even read applyKindSupport (gap #4).
      Record as a crate gap (ticket 09 / upstream), not as a policy choice.

C-N12 positionEncoding: ticket 11/29 decision applies unchanged to completion textEdit ranges.
      Completion-specific notes: UTF-16 is mandatory (spec default; only legal answer when the
      client sends no positionEncodings); VS Code THROWS if the server answers non-utf-16, so
      utf-8 negotiation must be strictly opt-in. The only completion-specific payoff for utf-8 is
      saving one conversion on the <20ms path.

C-N13 Filtering ownership: spec default is CLIENT filters + sorts; textEdit mode switches off client
      word-guessing. All ordering schemes must be expressed in sortText. With contextSupport absent
      and isIncomplete=true, every keystroke is a full request — that interacts with ticket 12's
      concurrency_level(1) and ticket 33's <20ms budget (see unit 6).

C-N14 Sparse-client floor (MVC-C): every completion capability absent + utf-16 only => all completion
      contexts still work (list + sortText + filterText + single-line TextEdit); presentation-only
      losses are: snippets, label-details badges, preselect, markdown docs, insert/replace
      distinction, kinds >18. Nothing silently breaks.
```

---

## 10. Open questions (carried into the grilling)

1. **[Q] Does `vscode-languageserver`'s `connection.client.register` fail or no-op for a client that
   did not advertise `dynamicRegistration`?** Microsoft's markdown LS registers completion
   *dynamically only* and never checks the flag — if the request errors, such a client gets zero
   completion from it. Verify before citing that server as a registration model (§2.2).
2. **[Q] rumdl's completion registration** (static vs dynamic, exact trigger set) is still not in
   `docs/digests/lsp_rvben-rumdl-src-digest.txt` — must be checked against rumdl's source; owned by
   unit 2, but the answer decides whether §7's "one flat static array" claim holds (matches the
   existing `codebase:295-301` flag).
3. **[Q] Does gopls's completion documentation honour `completion.completionItem.documentationFormat`,
   or only `hover.contentFormat`?** (`PreferredContentFormat` is set from hover in
   `settings.go:ForClientCapabilities`.) Affects how much weight decision E carries.
4. **[Q] Pylance and other closed-source clients**: no public capability source; VS Code's
   `vscode-languageclient` profile is the proxy. Unverified either way.
5. **[Q] Is the kind clamp (C-N9) worth it?** Cheaper alternative is accepting the generic icon for
   `FOLDER`/`ENUM_MEMBER` on clients without `valueSet`. Needs a product call, not more research.
6. **[Q] Does Neovim's built-in (non-nvim-cmp) completion honour `preselect`?** It advertises
   `preselectSupport = true`; behaviour unverified. Low stakes — `sortText` covers 23 either way.
7. **[Q] Upstream `ls-types` PR for `CompletionList.itemDefaults`?** Only worth it if unit 4 decides
   `itemDefaults` is load-bearing for the <20ms budget; otherwise record gap #1 and move on.
8. **[Q] Trust-vs-verify policy**: rust-analyzer had to disable resolve for Helix and Neovim despite
   their advertised flags (§3.1). Does Traces want a client-name escape hatch (Marksman's
   `ClientDescription.IsVSCode` pattern) or an initializationOptions override? Recommend: an
   `initializationOptions` kill-switch, not client sniffing — but that is a 09/29 decision.
9. **[Q] `documentationMarkdown` origin** (decomposition:88) — traced to nothing in spec or crate;
   recommend correcting the decomposition so no later ticket cites it.
