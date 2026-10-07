# Ticket 24 part-work: `completionItem/resolve` lazy-detail strategy (unit 4 → ticket 24 Q3)

Unit: **`completion-resolve-lazy-detail`** (kind: `general-LSP`) — per
[24-part-decomposition](24-part-decomposition.md) §4 (lines 53-59). Answers ticket
[24](../../issues/24-completion-architecture.md):13 (cheap initial list vs lazily-filled detail) and the
five sub-questions (a)-(e) of the decomposition.

Method: primary LSP 3.18 spec fetched live 2026-10-06 (same files, same line numbers as sibling
units' `[S1]`); editor and server behaviour read from each project's own source at `master`/`main`
today; local ticket/research/digest citations inline. Unverified claims are marked **QUESTION**.

**Decision summary (details in §5):**

1. Ship `resolveProvider: Some(true)`; **defer exactly one field: `documentation`** — `detail`,
   every immutable field, `labelDetails`, `kind` stay eager (D1). This answers (a) for all 17
   contexts via §4's matrix.
2. `data` = small **self-contained per-item key** (`{c, k, …}`), no server-side cacheId (D2).
3. `resolveSupport.properties` gates **nothing in v1** (documentation deferral is legal without it,
   `[S1]:3`); per-property gating (rust-analyzer model) is reserved for any future
   `additionalTextEdits`/`textEdit` deferral (D3).
4. Degraded modes are all **fail-open** — return the item unchanged, never error on a bad `data`
   (matches rust-analyzer, TS, zk) (D4).
5. Resolve gets **its own budget**: sync handler, p95 ≤ 5 ms proposed (target ≤ 2 ms), O(1) lookup,
   no vault scans — because it shares ticket 12's sequential gate with keystrokes (D5). Not part of
   ticket 33's < 20 ms completion row (no such row exists — proposed input to 33).
6. `completionList.itemDefaults` is **not adopted for unit-4 purposes** — `ls-types` cannot express
   it, Helix has no `completionList` support at all, and `data` must be per-item anyway (D6).

---

## 0. Corpus caveat

`docs/refs/lsp_spec.md` is a Jekyll include skeleton — the completion body is literally
`{% include_relative language/completion.md %}` at `docs/refs/lsp_spec.md:659`, so the local corpus
cannot be cited for completion (see [24-completion-capability-negotiation](24-completion-capability-negotiation.md)
§0 for the full statement and the changelog lines that *are* usable). Primary sources, both fetched
2026-10-06 and cached locally during this session:

- **[S1]** Completion feature spec —
  <https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.18/language/completion.md>
  (line numbers identical to sibling units' `[S1]` citations).
- **[S4]** Base protocol / lifecycle —
  <https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.18/specification.md>
  (cancellation, error codes, `ContentModified` guidance).

---

## 1. Normative floor — what the spec actually demands

### 1.1 The request, and when clients send it

- Resolve exists "if computing full completion items is expensive"; the request "is sent **when a
  completion item is selected in the user interface**", and the typical use case is *not* filling
  `documentation` in the initial response (`[S1]:3`).
- Request shape is trivial: method `completionItem/resolve`, params `CompletionItem`, result
  `CompletionItem` (`[S1]:1073-1083`). The whole item round-trips — whatever the server cannot
  re-derive must ride in `data`.

### 1.2 Which properties may be deferred

- **Default**: "the request can only delay the computation of the `detail` and `documentation`
  properties" (`[S1]:3`).
- **Since 3.16**: `completionItem#resolveSupport` client capability "lists all properties that can
  be filled in during a `completionItem/resolve` request" (`[S1]:3-4`); the interface itself says
  "Before version 3.16.0 only the predefined properties `documentation` and `details` could be
  resolved lazily" (`[S1]:119-125`).
- **Everything else** — "usually `sortText`, `filterText`, `insertText` and `textEdit`" — "must be
  provided in the `textDocument/completion` response and must not be changed during resolve"
  (`[S1]:4`; already locked as a seam by [24-completion-item-shape-and-ranking](24-completion-item-shape-and-ranking.md):128,
  §9.1, S-15).

### 1.3 Field types that constrain the split

- `detail?: string` — plain string only, "additional information about this item, like type or
  symbol information" (`[S1]:786-788`); never `MarkupContent`.
- `documentation?: string | MarkupContent` (`[S1]:793-794`) — the only field large enough to be
  worth deferring that can carry markdown.
- `data?: LSPAny` — "A data entry field that is preserved on a completion item between a completion
  and a completion resolve request" (`[S1]:935-938`). The spec says *preserved*, not *scoped*: it
  survives the round trip, which is the entire contract unit 4 may rely on.

### 1.4 Server capability vs client capability

- Server side: `CompletionOptions.resolveProvider?: boolean` — "The server provides support to
  resolve additional information for a completion item" (`[S1]:273-275`).
- Note the asymmetry unit 4 must respect: **clients gate their resolve calls on our
  `resolveProvider`** (verified per client in §2), while `resolveSupport.properties` only widens
  *which fields we are allowed to defer*. A server may advertise `resolveProvider: true` and defer
  only `documentation` even if the client never sent `resolveSupport` — that is the pre-3.16 default
  and remains legal (`[S1]:3,119-121`).

### 1.5 Staleness and cancellation

- A canceled request "still needs to return from the server and send a response back… advised to set
  the error code to `ErrorCodes.RequestCancelled`" (`[S4]:344-361`; `RequestCancelled = -32800` at
  `[S4]:311`, `ContentModified = -32801` at `[S4]:305`).
- Directly on point for resolve: "a client should not send resolve requests for out of date objects…
  If a server receives a resolve request for an out of date object, the server can error these
  requests with `ContentModified`" (`[S4]:712`), and a server whose internal state changed mid-request
  "can error these requests with `ContentModified`" (`[S4]:711`).
- Ticket 12 already chose the house style: handlers finish, stale results are discarded
  (`issues/12:32-36`); `$/cancelRequest` is effectively disabled by `concurrency_level(1)`
  (`issues/12:30`). §5/D4 below states how resolve behaves under that model.

### 1.6 `itemDefaults` and `data` merge semantics (decomposition question (b))

- `CompletionList.itemDefaults` exists only if the client lists it in the
  `completionList.itemDefaults: string[]` capability (`[S1]:205-214,415-433`); it carries
  `editRange`, `insertTextFormat`, `insertTextMode`, `commitCharacters`, `data`
  (`ItemDefaults.data` = "A default data value", `[S1]:505-513`).
- `data` merging is governed by `applyKind` (3.18, `applyKindSupport`): default **Replace**; `Merge`
  merges top-level fields only ("no merging of nested fields"); an empty object opts an item out of
  the shared default (`[S1]:560-583`). So a shared list-level `data` + per-item key *is*
  spec-expressible — but doubly capability-gated (itemDefaults for `data`, applyKind for Merge).
- Related: `textEditText` is "only honor[ed] if they opt into … `completionList.itemDefaults`"
  (`[S1]:895-903`) — irrelevant to unit 4, but it is why item shape (unit 3) called the field
  unavailable ([24-completion-item-shape-and-ranking](24-completion-item-shape-and-ranking.md):124).

### 1.7 The normative box

| Question | Spec answer | Line |
| :--- | :--- | :--- |
| What may be deferred by default? | `detail` + `documentation` only | `[S1]:3` |
| What widens that? | `resolveSupport.properties` (3.16+) | `[S1]:3-4,119-125` |
| What may *never* change at resolve? | `sortText`, `filterText`, `insertText`, `textEdit` (must be present initially) | `[S1]:4` |
| What carries resolve context? | `data` (preserved across the round trip) | `[S1]:935-938` |
| When is resolve sent? | "when a completion item is selected in the user interface" | `[S1]:3` |
| Stale resolve? | server *may* error `ContentModified`; client shouldn't send it | `[S4]:712` |
| Server capability | `CompletionOptions.resolveProvider` | `[S1]:273-275` |

Everything beyond this box — *when* clients actually call, what they do if we never fill the field,
what `data` should contain, and how much time resolve may take — is ecosystem behaviour, i.e. §2-§5.

---

## 2. What clients actually do (behavioural prior art)

### 2.1 When resolve fires, and how each client gates it

| Client | Resolve trigger | Gates on | Cancels/debounces | Blocks insertion on resolve? |
| :--- | :--- | :--- | :--- | :--- |
| **VS Code** | every suggest-list **focus change** (`editor/contrib/suggest/browser/suggestWidget.ts:374-445`: on `item !== _focusedItem` it cancels the previous promise `:399-401` then `await item.resolve(token)` `:407-428`, with a 250 ms details-spinner timeout `:410-414`); at accept: item already resolved/having `additionalTextEdits` ⇒ applied synchronously (`suggestController.ts:348-365`), item **unresolved** ⇒ async resolve *to fetch `additionalTextEdits`* (`:377-429`) | server `resolveCompletionItem` capability (`suggest.ts:125-126` — items with no resolver are pre-marked resolved) | cancels the in-flight resolve on the next focus change | **No** — insertion happens first; the accept-time resolve is async and aborts if the document changes (`suggestController.ts:377-429`) |
| **Neovim core** | `CompleteChanged` → only if `completion_item_needs_resolving = server_supports_resolve and not info_complete` (`runtime/lua/vim/lsp/completion.lua:550-551,618`; "info complete" = doc+detail fields already present, `:405-410,412+`) | server `resolveProvider` (`:900-902`) | `CompletionResolver:request` debounces with `adaptive_debounce` = remaining slack of an RTT-based estimate (initial `doc_rtt_ms = 100`, EMA `exp_avg(10,5)`, `:828-829,149-155,881-930`), cancels pending resolves (`:836-844`) and re-validates before/after | No (details popup only) |
| **nvim-cmp** | at **confirm/insert**: `async.sync(resolve, confirm_resolve_timeout)` with default **80 ms** (`lua/cmp/core.lua:372-374`, `lua/cmp/config/default.lua:24`); a second resolve for `additionalTextEdits` *after* insert (`core.lua:410-440`) | provider resolve support; entry caches `resolved_completion_item` + queues callbacks (`lua/cmp/entry.lua:563-581`) | cached per entry, single-flight | **Bounded yes** — insertion waits up to 80 ms for resolve |
| **Zed** | (timing **QUESTION**) capability claims `resolveSupport.properties = additionalTextEdits, command, detail, documentation` and *explicitly excludes* `textEdit`: *"NB: Do not have this resolved, otherwise Zed becomes slow to complete things"* (`crates/lsp/src/lsp.rs:979-987`) | server capability | — | — |
| **Helix** | details popup for the **selected** item on every render (`helix-term/src/ui/completion.rs:474-478` → `ResolveHandler::ensure_item_resolved`), and **synchronously at accept** if not yet resolved (`:215-230`) | server `resolve_provider: Some(true)` — checked *before* calling (`:384-404`, `helix_lsp::block_on`) | `resolved` flag per entry | **Yes** — `block_on` at accept |

Advertised `resolveSupport.properties` (for contrast with the triggers above): nvim-cmp
`documentation, additionalTextEdits, insertTextFormat, insertTextMode, command`; Neovim
`additionalTextEdits, command, documentation, detail`; VS Code/Zed/Helix
`documentation, detail, additionalTextEdits` (+Zed's explicit `textEdit` exclusion) —
[capability-negotiation](24-completion-capability-negotiation.md):412 (sources:
`client/src/common/completion.ts:81-116`, Zed `lsp.rs:979-987`, …). **Every advertised list contains
`documentation`** — the field unit 4 defers — in all five clients.

### 2.2 Takeaways unit 4 must design against

1. **Resolve is a *per-selection* event, not a per-accept event.** VS Code, Zed (per its comment)
   and Helix fire it as the user arrows through the list; each fires again on the next selection.
   Under ticket 12's stale-discard model every queued resolve still *executes* then gets discarded
   (`issues/12:36`) — so a slow resolve does not corrupt anything, it **steals gate time from the
   next keystroke** (`latency:74`: "a slow resolve delays the next" request).
2. **The display path is non-blocking; the accept path is blocking in two of five clients**
   (Helix `block_on`, nvim-cmp 80 ms bound). VS Code's accept-time resolve exists *only* to fetch
   `additionalTextEdits` (`suggestController.ts:377-429`) — which Traces never defers — so for us it
   is async and harmless, while Helix/nvim-cmp still *wait* on it before inserting (it returns only
   `documentation`). ⇒ resolve must be fast for **both** paths, even though nothing insert-time
   depends on it.
3. **Every client gates on our `resolveProvider`, not on its own `resolveSupport`.** Neovim
   (`:900-902`) and Helix (`:386-397`) refuse to call resolve if we do not advertise it. So the
   switch that matters for "will anyone call" is ours; `resolveSupport` only bounds *what we may
   withhold* (§1.4).
4. **A client that already has doc+detail skips resolve entirely** (Neovim's `not info_complete`,
   `:618`; VS Code's pre-marked-resolved items, `suggest.ts:125-126`). If we ship eager docs, the
   round trip disappears by itself — that is the symmetry D1 exploits in reverse: withhold
   `documentation`, and resolve becomes the *only* path to it.
5. **`textEdit` deferral is a known client-side footgun** (Zed's comment) — and it is also
   spec-forbidden without `resolveSupport` listing it (`[S1]:4`). Traces never defers it (§4).

---

## 3. Server-side resolve designs (prior art for the `data` payload)

Three real payload strategies, plus the no-resolve strategy:

| Strategy | `data` content | Cost on the wire | Failure mode | Used by |
| :--- | :--- | :--- | :--- | :--- |
| **A. Stateless re-derivation** | full resolve context: position, trigger char, content hash of the item, extra inputs | large (hundreds of bytes × every item) | recompute + hash-mismatch ⇒ return item unchanged (stale ⇒ fail-open) | **rust-analyzer** |
| **B. Server-side cache key** | one integer | ~10 bytes | cold/evicted ⇒ return item unchanged | **typescript-language-server** |
| **C. Content-address key** | an address of the content itself (file path) | small; leaks a path | unreadable ⇒ return item unchanged | **zk** |
| **D. No resolve** | — | 0 | docs simply absent from the list (or eagerly computed) | gopls, Markdown Oxide, MS markdown LS, Marksman (§3.5) |

### 3.1 rust-analyzer — strategy A, per-property deferral

- `data = CompletionResolveData { position, trigger_character, hash, for_ref, imports }`; the
  handler re-runs completion at the recorded position
  (`crates/rust-analyzer/src/handlers/request.rs:1219-1247`, cached during this session).
- **Fail-open everywhere**: no `data` ⇒ return item unchanged (`:1219-1221`); position unparseable ⇒
  unchanged (`:1228-1231`); hash mismatch (item replaced/edited) ⇒ unchanged (`:1248-1256`). It never
  errors on a stale payload.
- Deferral is driven by a `CompletionFieldsToResolve` flag set: `to_proto` *omits* a field when its
  flag is set (`resolve_filter_text`, `resolve_text_edit`, `resolve_tags`, `resolve_command`,
  `resolve_detail`, `resolve_documentation`, `resolve_label_details` —
  `crates/rust-analyzer/src/lsp/to_proto.rs:306-318,357-364,374-387,410+`). Resolve recomputes with
  `fields_to_resolve = empty()` so every deferred field comes back (`request.rs:1233-1234,1258-1266`),
  then appends auto-import `additionalTextEdits` (`:1274-1311`).
- Capability: `resolveProvider` advertised **iff the client listed ≥1 recognised
  `resolveSupport.properties`** property, plus a Neovim-specific sniff
  ([capability-negotiation](24-completion-capability-negotiation.md):232-245, quoting
  `crates/rust-analyzer/src/lsp/capabilities.rs`). History: PR #18589 gated on client caps, PR #18630
  *"Temporarily disable completion resolve support for helix and neovim"* — **advertised ≠ honoured**
  (`capability-negotiation:244-250`).

### 3.2 typescript-language-server — strategy B, resolve is load-bearing

- Initial item is deliberately skeletal: `label`, `kind`, `sortText`, `preselect`,
  `commitCharacters`, `data: { cacheId }` — no `detail`, no `documentation`, no edits
  (`src/completion.ts:93-114`; cache is an incrementing int, `:47-55`).
- Resolve: `data.cacheId` → cache → tsserver `CompletionDetails` → `asResolvedCompletionItem` fills
  **`detail`, `documentation`, `additionalTextEdits` + `command`** (from code actions), and may
  rewrite the item into a function-call snippet (`src/completion.ts:355-376`; handler
  `src/lsp-server.ts:676-695`).
- **Cache miss ⇒ `return item` unchanged** (`lsp-server.ts:686-694`) — same fail-open shape as RA.
- Server capability is static `resolveProvider: true`
  ([capability-negotiation](24-completion-capability-negotiation.md):281-283, `lsp-server.ts:261-263`).
- The whole pattern is already cited by this map as the canonical two-phase split
  ([x-lsp-performance-best-practices](../x-lsp-performance-best-practices.md):160;
  [latency](24-completion-latency-architecture.md):123-129, incl. the 1271 ms `completionInfo`
  incident where *auto-import enrichment inside the initial list* was the budget-buster).

### 3.3 zk — strategy C, minimal handler

`handler.CompletionItemResolve`: `if path, ok := params.Data.(string)` → `os.ReadFile(path)` →
`params.Documentation = MarkupContent{Markdown, content}`; any error ⇒ `return params, nil`
(`docs/digests/zk-src-digest.txt:4362-4375`). `data` is the file path — no cache, no recompute, but
it only works for file-backed docs and leaks a path to the client.

### 3.4 gopls / Markdown Oxide / Microsoft markdown LS — strategy D

- **gopls**: `CompletionProvider: &protocol.CompletionOptions{TriggerCharacters: []string{"."}}` —
  **no `resolveProvider` field at all** (`gopls/internal/server/general.go:204-206`; the nearby
  `ResolveProvider: true` at `:107` belongs to `CodeActionOptions`, and
  `gopls/internal/server/resolve.go` is an unrelated interactive-command dialog). gopls ships
  everything eagerly under its 100 ms soft budget
  ([33-lsp-performance-targets](../33-lsp-performance-targets.md):132-136).
- **Markdown Oxide**: `resolve_provider: Some(false)` (digest `:2028-2037`, cited by
  [capability-negotiation](24-completion-capability-negotiation.md):305-307).
- **Microsoft markdown LS**: "No capability gating anywhere in the completion path… works because
  the payload is conservative (no snippets, **no resolve**, no labelDetails)"
  ([capability-negotiation](24-completion-capability-negotiation.md):294-295).
- **rumdl**: `ServerCapabilities` construction absent from the digest — **QUESTION** (owned by unit
  2, `capability-negotiation:308-311`).

### 3.5 Marksman — a registered-but-unadvertised handler

Marksman dispatches `completionItem/resolve` (`digest:3162`) but advertises
`CompletionProvider = { TriggerCharacters = ['[';'#';'(']; ResolveProvider = None; … }`
(`digest:11150-11154`, [capability-negotiation](24-completion-capability-negotiation.md):296-300).
Net effect for capability-checking clients (VS Code, Neovim, Helix): **resolve is never called** —
functionally strategy D with a dead endpoint. The handler body is not in the digest
— **QUESTION** (does it fill `documentation`, or echo the item?).

### 3.6 What the three live designs teach unit 4

1. All three live handlers **fail open**: bad/missing `data` ⇒ return the item unchanged, never an
   error. Adopt verbatim (D4).
2. `data` design trades wire size against server state:
   - RA's stateless payload costs ~100-400 B × every item in a 200-item cap (S8 serialization is
     already the budget's biggest line, `latency:193`).
   - TS's cacheId is ~10 B but adds an invalidation surface (LRU tier exists in Traces already —
     [13-lsp-persistence-caching-patterns](../13-lsp-persistence-caching-patterns.md)) and silently
     degrades on cold miss.
   - zk's path is stateless and self-contained but only addresses *files*.
3. Which one fits Traces follows from what resolve actually does here: **a key lookup into
   refresh-prebuilt artifacts** (§4), not a recompute. A recompute-free resolve does not need a
   cache (nothing to memoise) and does not need RA's position+hash (no analysis to re-run) — D2.

---

## 4. Per-context initial-vs-resolved matrix (decomposition question (a))

Context inventory: [24-completion-dispatch-architecture](24-completion-dispatch-architecture.md):124-148
(C1-C17, detection owners). Cost premise: the initial list must stay inside S5 ≤ 1.5 ms construction
+ S8 ≤ 3 ms serialization for ≤ 200 items (`latency:190,193`), so **per-item payload size matters as
much as per-item compute**.

### 4.1 The four rules the matrix encodes

- **R1 (immutability)** — `label`, `kind`, `sortText`, `filterText`, `insertText`/`textEdit`,
  `preselect`, `labelDetails` are eager always (`[S1]:4`; `item-shape:128`).
- **R2 (`detail` stays eager)** — `detail` is a *live* field (labelDetails fallback), so it must not
  be a resolve placeholder; resolve payloads are `documentation` + `data` **only**
  (`item-shape:529-530`, §9.1, S-15 `:824`). `detail` is also cheap: one short line per item.
- **R3 (`documentation` is the deferred field)** — the only field large enough to be worth deferring
  that is presentation-only (no insert/filter/ordering consequence), and the spec-default deferrable
  field (`[S1]:3`).
- **R4 (`data` only where documentation is deferred)** — items with nothing to resolve carry no
  `data`; zero cost for the no-resolve contexts.

### 4.2 Matrix

"Eager" = present in the `textDocument/completion` response; "resolve" = filled by
`completionItem/resolve`; `data` key sketch per D2 (`c` = context kind, `k` = candidate key).

| # | Context | Eager (initial) | Deferred → `documentation` | `data` key sketch | Cost source of the deferred part | Evidence |
|---|---|---|---|---|---|---|
| C1 | Wikilink target | label, kind, sortText, filterText, textEdit(range), detail = display path, labelDetails | target note's first-line preview / resolution status | `{c:"link", k:<note key>}` | preview is refresh-built (`issues/19:76`); deferral saves **payload**, not compute | `latency:188,209`; `issues/15` |
| C2 | Wikilink heading | …, detail = `note › heading` | section preview around the heading | `{c:"link-h", k:<note#heading>}` | heading universe refresh-built (`latency:188`); section slice = O(1) via memoised content (`latency:186`) | `issues/15:19,85`; `latency:188` |
| C3 | Markdown-link anchor/target | same as C1 | same as C1 | `{c:"mlink", k:<note key>}` | same as C1 | `issues/16:37` |
| C4 | Body tag | label `#tag`, kind, sortText/filterText, textEdit, detail = count | **usage statistics** (16) | `{c:"tag", k:<tag>}` | counts prebuilt (`unique_tags`, `issues/21:54`, `latency:207`); richer stats defer so 200 items don't each ship a paragraph | `capability-negotiation:69` ("16's usage statistics … unit 4") |
| C5 | Frontmatter tags | same as C4 | same as C4 | `{c:"tag-fm", k:<tag>}` | same as C4 | `issues/16:45-47` |
| C6 | Frontmatter key | label, kind, sortText/filterText, textEdit, detail = field **type**, labelDetails = provenance badge | **full Schema Field Definition** (type/constraints/attribution/description) — the ticket's headline case | `{c:"fm-key", k:<field>}` | schema loaded once (`latency:188,210`), but definition text × every field × every keystroke is pure payload waste | `issues/24:13`; `issues/19:21`; `decomposition:56` |
| C7 | Frontmatter value | label, detail = type/short value; select options eager | schema description of the option set (select fields) | `{c:"fm-val", k:<field>[:<opt>]}` | schema-resident | `issues/19:82` |
| C8 | Inline-field key `[k:: ` | label, detail = type if schema-bound | schema Field Definition when bound; **no `data`** when vault-inferred only | `{c:"if-key", k:<field>}` (only if schema-bound) | `VaultFieldIndex` is prebuilt (`latency:208`); docs only exist for schema fields | `issues/19:81` |
| C9 | Task checkbox | **everything eager** — labels are `[ ]`-variants, no docs | — | — (no `data`) | candidate set is tiny (23's marker set) | `issues/23:24`; `research/23:128`; `marker.rs:70-113` |
| C10 | Task date value | everything eager; `detail` = weekday if precomputed | — | — | date candidates are computed anyway for filtering | `issues/23:27` |
| C11 | Daily-note date in link | everything eager (dates are the label) | — | — | same | `part-decomposition:32` |
| C12 | Footnote ref | everything eager (footnote labels) | — | — | tiny universe | `part-decomposition:40` |
| C13 | Callout type | everything eager; `detail` = one-line hint if 17 defines one | — | — | handful of callout types | `part-decomposition:40` |
| C14 | Query DSL (tag/class/LHS/op/RHS) | label, kind, sortText/filterText, textEdit, detail = field type/operator signature (short) | operator/filter **help text**, field constraints (long, static) | `{c:"query", k:<op-or-field>, p:<enclosing expr position>}` — position needed because the completer re-derives from the *expression*, not a global symbol table | 21 deferred resolve entirely to Phase 2 (`issues/21:63`); help text is static | `issues/21:24,26-46`; `decomposition:56` |
| C15 | Template helper `ui.` | label, detail = short signature | helper description + examples (hand-tabled per 22 → static, possibly long) | `{c:"helper", k:<ns.fn>}` | 22's namespace metadata is initialize/refresh-time (`latency:211`) | `issues/22`; unit 7 |
| C16 | Template include/extends | label, detail = path | target template's preview | `{c:"tpl", k:<path>}` | same shape as C1 | `issues/22:16` |
| C17 | File-field / fileClass value | label, detail = path | **file preview** (20's candidate previews) | `{c:"file-val", k:<path>}` | previews refresh-built (`issues/19:76` → `latency:209` "first-line previews … S5 `detail`, or `resolve`") | `decomposition:56`; `issues/19:20` |

### 4.3 Reading the matrix honestly

- **Five contexts never touch resolve** (C9-C13) and twelve defer only presentation text. The
  *insert/filter/order* surface is 100 % eager in every context — which is exactly the invariant
  unit 3 locked (`item-shape:692-696`).
- Deferral buys two different things, depending on context:
  - **Payload off the keystroke path** (C1-C5, C16-C17): docs are already refresh-prebuilt, so
    resolve is a pure lookup; withholding them shrinks S8 for *every* keystroke, not just the first.
  - **Compute off the keystroke path** (C6-C8, C14-C15): schema/operator prose would otherwise have
    to be *assembled* inside S5's 1.5 ms for every visible item.
- If ticket 33's bench shows S8 has slack at the cap, a switch to eager `documentation` for the
  prebuilt-only group is a **one-line policy change per context** (D7 escape hatch) — that is the
  reason D1 keeps the eager/deferred split a per-context table rather than a global hardcode.

---

## 5. Decisions (decomposition questions (a)-(e))

### D1 — `resolveProvider: Some(true)`; defer `documentation` only (answers (a), (e))

- Advertise resolve (`[S1]:273-275`). Defer **`documentation`** on the contexts marked in §4.2.
  Never defer `detail` (R2), never defer any immutable field (R1), never defer `additionalTextEdits`
  / `command` (Traces v1 emits neither).
- Why `true` rather than the Markdown Oxide/gopls `false` (question (e)):
  - `false` forces one of two bads: compute+ship schema/stat/preview prose in **every** keystroke's
    response (blows S5/S8 — the exact failure TS saw with auto-import enrichment,
    `latency:126-128`), or **drop the docs entirely** (19's headline requirement, `issues/24:13`,
    unmet).
  - `true` costs one extra request *per selection* — non-blocking in VS Code/Neovim, and O(1)
    server-side (D5).
  - The `false` servers are exactly the ones with nothing expensive to say (§3.4): MS markdown LS
    ships only kinds+labels; gopls computes docs cheaply under a 100 ms budget 20× looser than
    Traces' 20 ms. Traces' expensive content is precisely schema/stats/preview prose — the case the
    spec invented resolve for (`[S1]:3`).
- Note the round trip is *display-only* for us: no deferred `additionalTextEdits`/`textEdit` means
  acceptance never *needs* resolve. But Helix (`block_on`) and nvim-cmp (80 ms bound) still call it
  at accept to fill whatever they can, and VS Code calls it async to look for extra edits — so those
  clients do wait on (or spend a round trip on) our handler; D5's budget is what keeps that cheap.

### D2 — `data` = self-contained, per-item, stateless key (answers (a))

Shape (per item, bounded — target ≤ 120 B serialized):

```jsonc
{ "c": "fm-key",          // context kind (string enum, stable wire vocabulary)
  "k": "review_status",   // candidate key: what to look up, not how to recompute it
  "v": 1 }                // data schema version (bump ⇒ old clients' payloads detectably stale)
```

plus **context-specific minimally sufficient state** where lookup alone is ambiguous — C14 carries
the enclosing expression position (`p`) because the query completer has no global symbol table; C2
carries `note#heading`. Nothing else (no position for C1/C6/C17: their keys address refresh-built
vault state directly).

Rationale against the two alternatives (§3.6):
- **vs cacheId (TS)**: resolve here is a *lookup into refresh-built artifacts* (`latency:207-209`),
  not a recompute — there is no expensive intermediate result to cache, so a cache would only add
  invalidation surface (refresh swap! `latency:207-212`) for zero work saved. Cold-miss degradation
  (return unchanged) would then silently eat 19's docs after every refresh.
- **vs position+hash (RA)**: RA must *re-run analysis* to reconstruct deferred fields, hence
  position and identity hash. Traces rebuilds the same artifacts at refresh, so `k` is stable across
  keystrokes and re-derivable; hash machinery would be dead weight (and ~4× the wire bytes × cap).
- **Statelessness is the property that makes D4 trivial**: a stale `data` can only mean "candidate
  gone" → return unchanged → client shows the item it already has.
- **`itemDefaults.data` interplay** (question (b)): shared context (`{c…, v…}`) could move to the
  list level with per-item `k` merged on top — *if* the client supports `completionList.itemDefaults`
  **and** `applyKind` Merge for `data` (`[S1]:560-583`). Rejected for now: `ls-types` has no
  `CompletionList.itemDefaults` field at all (hand-serialization only,
  [part-crates](24-part-crates.md):450, `latency:70`), and Helix advertises no `completionList`
  capabilities whatsoever (`capability-negotiation:412`). Keeping `data` self-contained means
  itemDefaults later is a pure wire-size optimisation with **no protocol-behaviour change** — see D6.

### D3 — Capability gating (answers (c))

1. **Advertise `resolveProvider: Some(true)` unconditionally** (static, at initialize). Every
   surveyed client implements the method and gates on *our* flag (§2.2-3); there is no client
   capability that says "don't offer resolve" — `resolveSupport` only names properties (§1.4).
2. **`resolveSupport.properties` gates nothing in v1**: `documentation` is deferrable by default
   since forever (`[S1]:3,119-121`). If we later defer `additionalTextEdits` (auto-import-like
   enrichment) or `textEdit`, gate **per property**, rust-analyzer model
   (`capability-negotiation:69` row D, C-N4 `:538-540`) — never as one boolean.
3. **Sparse-client rule (record for ticket 29, harmonises with 23)**: absent `resolveSupport` ⇒
   still defer `documentation` (legal), defer nothing else, and *never drop a feature* — the shared
   sentence `capability-negotiation:359-362` already proposes: *"absent resolveSupport ⇒
   eager-resolve, never drop."* For completion the degraded form is: docs may be missing until the
   client calls resolve (it will — §2.2-3), insertion/filtering unaffected (§4.3 R1/R2).
   This is the completion analogue of ticket 23's `workspaceSymbol/resolve` full-Location fallback
   (`issues/23:51`, handed to 29 at `issues/23:64`).
4. **Escape hatch**: an `initializationOptions` switch forcing eager-`documentation` mode
   (`resolveProvider: false` + docs inline, capped) — not client sniffing. Precedent: rust-analyzer
   needed per-client disable (PR #18630) and Marksman sniffs `IsVSCode`
   (`capability-negotiation:244-250,300`); recommendation there was a kill-switch, not sniffing
   (Q8 `:605-608`) — unit 4 agrees and does not re-open it.

### D4 — Degraded modes: every failure path is fail-open (answers (c), (e))

| Situation | Behaviour | Precedent |
| :--- | :--- | :--- |
| Client never calls resolve (checks capabilities, old client, Marksman-style `ResolveProvider: None` world) | Items keep label/kind/sort/filter/edit/detail; `documentation` simply absent. **No error, no broken insert.** | gopls/MS-LS/MO ship *without* docs this way (`§3.4`) |
| `data` missing, unparsable, unknown `v`, or key not found (refresh swapped the candidate) | **Return the item unchanged** — do not error | RA `request.rs:1219-1221,1248-1256`; TS `lsp-server.ts:686-694`; zk `zk-src:4374-4375` |
| Document changed while resolve in flight | Complete the O(1) lookup, then return; ticket 12's stale-discard drops it client-side. Reserve `ContentModified` (`[S4]:711-712`) only for the case where we *cannot* answer (position-dependent C14 after an edit) | `issues/12:36`; `[S4]:712` |
| Client sent resolve for an item *we* never deferred | Return unchanged (no-op) | RA's unconditional `data` check |
| Accept happens before resolve returns | Nothing to do — no deferred insert-time fields (D1) | n/a (contrast TS's `additionalTextEdits`, `completion.ts:366-371`) |
| Resolves pile up behind `concurrency_level(1)` | Each runs to completion in O(1) and is discarded if stale; the cost is gate time, bounded by D5's budget | `issues/12:36`; `latency:74` |

### D5 — Resolve latency budget (answers (d))

- **Ticket 33 has no resolve row** (its p95 table covers completion < 20 ms, stretch < 10 ms,
  `issues/33:45-50`; grep of the ticket finds no `resolve`). Unit 4 therefore *proposes* one:
  **`completionItem/resolve` p95 ≤ 5 ms, stretch ≤ 2 ms**, to be recorded by 33.
- Why a separate row rather than "inside the 20 ms": resolve is a different request with a different
  shape (one item, no scoring, no serialization of a list), and clients budget it separately
  (nvim-cmp's 80 ms accept bound, VS Code's 250 ms spinner threshold — §2.1).
- **Why it still must be tiny**: it shares ticket 12's single gate — "a slow resolve delays the
  *next*" request (`latency:74`, quoting `part-crates:166`). Resolve storms arrive per selection
  change (§2.2-1), each stealing gate time from the keystroke that follows.
- **Shape of the handler** (this is the architectural half of the answer to (d)):
  1. **Sync, no `.await`** — ticket 12: sync handlers run inline and never yield, so a resolve
     cannot interleave mid-handler (`issues/12:22`); O(1) work in a sync handler is the cheapest
     possible citizen.
  2. **Validate `data` first, miss ⇒ early return** (D4 fast path; RA does the same order).
  3. **Lookup only, never a vault scan**: the deferred strings come from refresh-built artifacts
     (`latency:207-209`). If a context's doc *cannot* be served from those (C14 help text), it must
     be built at initialize/refresh like 22's metadata — **a resolve-time scan is a bug**, not a
     budget exception.
  4. **No second request in flight**: clients already serialise resolve per selection and cancel
     their previous one (§2.1); nothing for us to debounce server-side — but see D7 telemetry.
- **Is resolve "a second request the client serialises before showing detail"?** Display: no —
  VS Code/Neovim/Helix show the item immediately and fill docs when the response lands (VS Code's
  250 ms spinner only appears if the details pane is open, `suggestWidget.ts:410-414`). Accept:
  Helix and nvim-cmp *do* serialise (block/80 ms) — harmless for Traces because nothing
  insert-time is deferred (D1), but it is why the ≤ 5 ms target exists rather than "whatever".

### D6 — `itemDefaults`: not adopted for unit-4 purposes (answers (b))

- **No.** Reasons, in order of force:
  1. `ls-types` cannot express `CompletionList.itemDefaults` — no field exists
     ([part-crates](24-part-crates.md):450, `latency:70`); hand-serializing the list envelope
     breaks the typed-`tower-lsp-server` path unit 09 chose. (Closes
     [capability-negotiation](24-completion-capability-negotiation.md) Q7: *not* load-bearing for
     unit 4.)
  2. Helix advertises **no** `completionList` capabilities at all (`capability-negotiation:412`),
     so any correctness relying on it fails on a first-class target client.
  3. The only resolve-relevant default is `itemDefaults.data`, and per D2 `data` must be
     self-contained per item anyway (different `k`); moving the shared `c`/`v` up a level saves
     ~20 B/item — inside S8's noise at a 200-item cap.
  4. `itemDefaults` does **not** change the eager/deferred calculus: it is a payload-sharing device
     for *initial* responses, orthogonal to what resolve fills.
- Revisit trigger: only if `lsp_latency.rs` shows S8 serialization (not scoring) dominating at the
  cap — then `itemDefaults.editRange` (unit 5's concern too) is the first win, `data` the second.

### D7 — Measurement hooks

- Record resolve duration server-side (histogram keyed by context kind) — VS Code already ships
  `resolveDuration` telemetry per item (`suggest.ts:77,136-137,141-161`), Neovim measures RTT and
  feeds its debounce (`completion.lua:904-917`), so client-side lag is observable by users of those
  editors; ours must be observable in `lsp_latency.rs` (ticket 33's harness) to defend the D5 row.
- Count fail-open returns (unknown key / bad data) — a non-zero rate after a refresh means the
  invalidation seam between refresh-swap and `data.v` is wrong, which is the one correctness risk
  D2 carries.

---

## 6. Seams handed to other units/tickets

1. **Unit 6 (latency)**: add an **S-R row** to the stage budget: resolve = validate `data` (O(1)) →
   artifact lookup (O(1)) → clone + serialize one item → ≤ 5 ms p95 (D5). It rides the same
   sequential gate as S0-S9 (`latency:176-200`); no new cancellation semantics needed
   (D4's last row).
2. **Unit 8 / ticket 29** (ready-to-paste inputs):
   - `CompletionOptions.resolveProvider` = `Some(true)` — static, unconditional (D3.1).
   - `resolveSupport.properties` is **read but gates nothing in v1**; reserve per-property gating for
     `additionalTextEdits`/`textEdit` (D3.2, row D of `capability-negotiation:69`).
   - Shared rule sentence for 29: *"absent resolveSupport ⇒ defer `documentation` anyway (spec
     default), defer nothing else, never drop"* (D3.3, extends `capability-negotiation:362`).
   - `initializationOptions.resolve = "lazy" | "eager"` kill-switch (D3.4).
   - Closes capability-negotiation **Q7** (itemDefaults not load-bearing for resolve, D6).
3. **Ticket 33**: proposed new p95 row — `completionItem/resolve ≤ 5 ms (stretch 2 ms)` (D5);
   measurement inputs from D7.
4. **Ticket 12**: no change — resolves are sync O(1) handlers; stale-discard covers cancellation
   (`issues/12:32-36`).
5. **Unit 3 (item shape)**: consumed its seams (S-15: documentation + data only; detail stays live)
   (`item-shape:692-696,824`); contributes one requirement back: the `data` key vocabulary
   (`c` enum) is a **wire-stable contract** — renaming a context kind is a breaking change, so it
   must be pinned where the dispatcher (unit 1) pins `ContextKind`.
6. **Ticket 21**: resolves its Phase-2 deferral note (`issues/21:63`) — the *mechanism* is now
   designed (C14 row + position-bearing `data`); Phase 2 only needs to flip it on.
7. **Ticket 19/16/20**: their content requirements map to C6-C8 (schema Field Definitions), C4-C5
   (usage statistics), C17 (file previews) — all deferred per §4.2, all refresh-prebuilt sources
   named there.

---

## 7. Open questions (carried into the grilling)

1. **[Q] Marksman's `CompletionItemResolve` handler body** — registered (`digest:3162`) but
   unadvertised (`digest:11150-11154`); source not in the digest (fetch of
   `src/Marksman/Server.fs` 404'd today — layout/branch **QUESTION**). Either "dead code" or a
   hedge for clients that call anyway; worth 5 minutes before citing Marksman in ticket 29.
2. **[Q] rumdl's completion resolve capability** — still unverified (digest gap,
   `capability-negotiation:308-311`; owned by unit 2). If rumdl advertises `resolveProvider: true`
   while coexisting with Traces in one editor session, capability collision analysis (unit 2) must
   note it.
3. **[Q] Zed's resolve *trigger timing*** — capability comment proves Zed defers
   `additionalTextEdits/command/detail/documentation` and fears `textEdit` deferral
   (`lsp.rs:979-987`), but *when* it fires resolve (selection? accept? debounced?) is unverified.
   Matters only for the storm model in D5, not for the split.
4. **[Q] Does VS Code resolve even with the details pane closed?** — read as **yes** from
   `suggestWidget.ts:374-445` (the resolve promise is created on focus change; only the *spinner* is
   gated on details visibility, `:410-414`). If wrong, resolve volume drops and D5's storm concern
   softens — but the ≤ 5 ms budget stands either way.
5. **[Q] Is `documentation` ever *needed before selection*?** If a client wants docs in the list
   without selection (some floating previews), deferral degrades them. No surveyed client does this
   (§2.1), but it is the one product assumption D1 makes.
6. **[Q] C14's `p` (expression position) staleness** — query expressions are inside template string
   literals that edits shift constantly; is key-based lookup + `ContentModified` enough, or does
   21's Phase-2 resolve need a version stamp per document? Depends on 21's Phase-2 design.
7. **[Q] D5's 5 ms number** — reasoned (single-item lookup + serialize vs nvim-cmp's 80 ms accept
   bound and VS Code's 250 ms spinner), **not measured**. Must be validated by `lsp_latency.rs`
   before ticket 33 adopts the row.

---

## 8. Coverage check vs decomposition outputs (`:59`)

| Expected output | Where |
| :--- | :--- |
| cheap-vs-resolved field matrix per context | §4.2 (+ rules §4.1) |
| gating + degraded-client policy | D3, D4 |
| resolve latency budget statement | D5 (+ seam 1, seam 3) |
| explicit recommendation on `itemDefaults` adoption | D6 (closes capability-negotiation Q7) |
| `data` payload design (sub-question (a)) | D2 |
| `resolveSupport` absent behaviour ((c)) | D3.3 |
| precedent: `resolveProvider: false` vs load-bearing ((e)) | §3.4 vs §3.1-3.3, decision D1 |
