# Research: completion-path crates for ticket 24 (completion architecture)

Date: 2026-10-06. Supports `.scratch/md-pkm-lsp/issues/24-completion-architecture.md`. Foundational input to the
ticket-24 decomposition (`research/24-part-decomposition.md`) units **1** (dispatch), **2** (trigger chars),
**4** (resolve), **5** (snippets), **8** (capability negotiation) — this file answers "what do the crates and
the protocol actually expose"; the units decide "what Traces does with it". Overlapping seams are cited, not
re-decided.

**Method**: rust-docs-mcp (`list_cached_crates`, `structure`, `search_items_*`, `get_item_*`) plus direct reads of
the rust-docs local cache at `.rust-docs/cache/crates/<name>/<version>/source/` (the repo's single source of truth
per AGENTS.md — no docs.rs fetches were used for API facts). `ls-types 0.0.2` was newly cached for this research
(task `18b26be7`); `ls-types 0.0.6` cached separately (task `e9b523d8`) once crates.io showed 0.0.2 is stale —
see §2.0. Web sources are used **only** in explicitly-marked **Ecosystem practice** subsections and for spec quotes.
Spec quotes come from the **LSP 3.18** primary source file
[`language/completion.md`](https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.18/language/completion.md)
(fetched 2026-10-06), because the local `docs/refs/lsp_spec.md` (which *is* the 3.18 corpus — `fullTitle:
"… - 3.18"`) contains only `{% include_relative language/completion.md %}` (line 659) with no `language/`
directory alongside it: the local file has TOC/changelog/other sections but **zero** `triggerCharacters`/`isIncomplete`
hits, so completion detail must come from the primary source (decomposition's corpus caveat). Unverified items
are marked **[question]**.

---

## 0. Dependency status in this repo (answering the "post-ticket-09 state" question first)

Checked `Cargo.toml` (single manifest — no workspace members) and `Cargo.lock`:

| Crate | In `Cargo.toml`? | In `Cargo.lock`? | Notes |
| :--- | :--- | :--- | :--- |
| `tokio` | **No** | No | Not yet a direct dependency. Cached in `.rust-docs` at **1.53.1**. |
| `tower-lsp-server` | **No** | No | Decision recorded in ticket 09, **not yet added to the manifest**. Cached at **0.23.0**. |
| `lsp-types` | **No** | **No** | Absent from both manifests. Cached at 0.97.0, but it is **not** the crate `tower-lsp-server` uses (see §2). |
| `ls-types` | **No** | No | The types crate `tower-lsp-server 0.23.0` actually depends on (`ls-types = "0.0"`). Newly cached at **0.0.2**. |
| `ropey` | **No** | No | Ticket 11/14 decision, not yet added. Cached at **1.6.1** (and a `master` snapshot). |
| `fuzzy-matcher` | **No** (direct) | **Yes — 0.3.7**, as a *transitive* dep of `inquire` (feature `fuzzy`, Cargo.lock line ~799) | Ticket 15 adopted it for link resolution; direct-dependency add still pending. |
| `strsim` | **Yes — `strsim = "0.11.1"`** (Cargo.toml:157) | yes | Used by `src/strsim.rs`. |

**Conclusion**: none of the LSP crates are pinned yet; the settled tickets are design decisions awaiting
implementation. All versions below are the ones sitting in the local `.rust-docs/` cache, which is what
rust-docs-mcp will serve until the manifest pins something else.

---

## 1. `tower-lsp-server` 0.23.0 (primary transport crate)

Cached: `.rust-docs/cache/crates/tower-lsp-server/0.23.0` (88.6 MB, docs generated).
Edition 2024, `rust-version = "1.85"` (source `Cargo.toml.orig`).

### 1.1 Which crate owns the LSP types

`tower-lsp-server` does **not** depend on `lsp-types`; it depends on **`ls-types = "0.0"`** (resolves to 0.0.2 in
its own `Cargo.lock`), the `tower-lsp-community` fork of `lsp-types`, and re-exports it:

```rust
// src/lib.rs:75-76
/// A re-export of [`lsp-types`](https://docs.rs/lsp-types) for convenience.
pub use ls_types;
```

So in handler code, types are `tower_lsp_server::ls_types::*` (or `use tower_lsp_server::ls_types::{…};`).
The example in `src/lib.rs:9` shows `use tower_lsp_server::ls_types::*;`. Direct dependents should import
through this re-export rather than adding a separate `lsp-types` dep — adding `lsp-types` itself would give
**structurally identical but nominally different** types that will not type-check against the trait.

### 1.2 The completion request/response path (exact signatures)

`LanguageServer` is built by the `rpc!` macro over `src/server.rs` (trait span lines 131–1397); methods are
declared `async fn` with `#[rpc(name = "…")]` and may carry a default body. Bound: `pub trait LanguageServer: (Send + Sync + 'static)`.

```rust
// src/server.rs:891-896
#[rpc(name = "textDocument/completion")]
async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
    let _ = params;
    error!("got a `textDocument/completion` request, but it is not implemented");
    Err(Error::method_not_found())
}

// src/server.rs:902-907
#[rpc(name = "completionItem/resolve")]
async fn completion_resolve(&self, params: CompletionItem) -> Result<CompletionItem> {
    let _ = params;
    error!("got a `completionItem/resolve` request, but it is not implemented");
    Err(Error::method_not_found())
}
```

- `Result` is `crate::jsonrpc::Result` (i.e. `Result<T, jsonrpc::Error>`), imported at `src/server.rs:9`.
- Both methods have **default bodies** → they are *optional*; not implementing them makes the server answer
  JSON-RPC `-32601 MethodNotFound` (which is also what the client sees if `completionProvider` was never
  advertised).
- `initialize` has **no default body** (required): `async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult>` (`src/server.rs` ~line 141).
- Example implementation (`examples/common/lsp.rs:113-118`):

```rust
async fn completion(&self, _: CompletionParams) -> Result<Option<CompletionResponse>> {
    Ok(Some(CompletionResponse::Array(vec![
        CompletionItem::new_simple("Hello".to_string(), "Some detail".to_string()),
        CompletionItem::new_simple("Bye".to_string(), "More detail".to_string()),
    ])))
}
```

Routing is generated once at service build: `LspService::new(Backend::new)` →
`crate::server::generated::register_lsp_methods(router, state, pending, client)` (`src/service.rs:85`).
There is **no per-method subscription API** — you get `completion`/`completion_resolve` routing for free the
moment you implement the trait methods; activation toward the client is controlled solely by what you return
in `InitializeResult.capabilities` (and/or dynamic registration, §1.4).

### 1.3 What the trait doc says about lazy resolve (quoted)

From the `completion` method docs (`src/server.rs`, matches lsp-types docs text):

> Since 3.16.0, the client can signal that it can resolve more properties lazily. This is done using the
> `completion_item.resolve_support` client capability which lists all properties that can be filled in during
> a `completionItem/resolve` request.
> All other properties (usually `sort_text`, `filter_text`, `insert_text`, and `text_edit`) must be provided
> in the `textDocument/completion` response and must not be changed during resolve.

### 1.4 Capability registration & re-registration

- **Static**: return `InitializeResult { capabilities: ServerCapabilities { completion_provider: Some(CompletionOptions {…}), .. }, server_info }` from `initialize` (§2.4).
- **Dynamic**: `Client::register_capability` / `unregister_capability` (`src/service/client.rs:103-135`):

```rust
pub async fn register_capability(&self, registrations: Vec<Registration>) -> jsonrpc::Result<()>
pub async fn unregister_capability(&self, unregisterations: Vec<Unregistration>) -> jsonrpc::Result<()>
```

  `Registration { id: String, method: String, register_options: Option<serde_json::Value> }` — so a
  re-registration of completion with a different trigger-character set is
  `Registration { id: "<unique>", method: "textDocument/completion", register_options: Some(to_value(CompletionRegistrationOptions{..})?) }`.
  Gated by the client capability `textDocument.completion.dynamicRegistration`
  (`CompletionClientCapabilities.dynamic_registration`, §2.5). The `initialized` notification is the
  documented place to do this (its doc: *"The server can use the `initialized` notification, for example, to
  dynamically register capabilities with the client."*). The `Registration.id` must be unique per
  registration; unregister with the same id. **[question]** whether any mainstream client (VS Code)
  actually *replaces* an existing `textDocument/completion` registration under the same id vs requiring
  unregister→register — unverified; behavior of duplicate registrations is client-defined.

### 1.5 Sequential-dispatch model (ticket 12 interplay)

`transport::Server` (`src/transport.rs`):

```rust
const DEFAULT_MAX_CONCURRENCY: usize = 4;              // line 24
pub const fn new(stdin: I, stdout: O, socket: L) -> Self   // line 68; max_concurrency: DEFAULT_MAX_CONCURRENCY
pub const fn concurrency_level(mut self, max: usize) -> Self   // line 99
pub async fn serve<T>(self, mut service: T)            // line 105
```

Doc on `concurrency_level` (lines 79-95, quoted):

> This setting specifies how many incoming requests may be processed concurrently. Setting this value to
> `1` forces all requests to be processed sequentially, thereby implicitly disabling support for the
> `$/cancelRequest` notification. […] If not explicitly specified, `max` defaults to 4.

Implications for completion under the settled `concurrency_level(1)` model:

- Completion handlers run **inline and strictly one-at-a-time** with everything else (didChange, hover, …).
  A completion handler that blocks for its full duration blocks document syncing too — hence the <20 ms
  budget (ticket 33) is a *system-wide* latency requirement here, not just a nicety.
- `$/cancelRequest` is effectively off → a stale completion request still runs to completion; the client
  discards the late result (matches ticket 12's "stale-result discard"). No server-side cancellation hooks
  to design around for completion.
- `completionItem/resolve` is also serialized behind the same gate: a slow resolve delays the *next*
  keystroke's `textDocument/completion`. Keep resolve strictly bounded or accept keystroke lag.

Runtime: `default = ["runtime-tokio"]`, `runtime-tokio = ["tokio", "tokio-util"]`; `tokio` is an optional
dependency (`Cargo.toml.orig`). The stdio example is `#[tokio::main]` + `LspService::new` +
`Server::new(stdin, stdout, socket).serve(service).await` (`examples/stdio.rs`).

### 1.6 Ecosystem practice *(web evidence)*

- `tower-lsp-server` (the `tower-lsp-community` fork) is already recorded in `map.md:37,54` as the
  **de facto standard / most-recommended Rust LSP framework across 2025/2026 tutorials** after an explicit
  2026-09-03 web sweep; the original `tower-lsp` is abandoned. That audit was for ticket 09 and stands for
  ticket 24's transport questions too — no contrary evidence found in this pass.
- The abandoned-`tower-lsp`-vs-fork distinction matters because older tutorials show
  `tower_lsp::lsp_types`; under `tower-lsp-server 0.23` the path is `tower_lsp_server::ls_types`
  (§1.1). Marked as ecosystem/documentation-drift risk, not an API question.

---

## 2. `ls-types` (cached 0.0.2, latest 0.0.6) — the types that actually own the completion surface

Cached: `.rust-docs/cache/crates/ls-types/0.0.2` (newly cached 2026-10-06) and `0.0.6` (task `e9b523d8`,
after crates.io showed 0.0.2 is stale). `repository = "https://github.com/tower-lsp-community/ls-types"`,
description "Types for the Language Server Protocol specification", **authors include the original
`lsp-types` authors** (Markus Westerlind, Bruno Medeiros) + Milo Moisson; edition 2024; features
`default = []`, `proposed = []` **(0.0.2 — see §2.0)**.

For completeness: cached `lsp-types 0.97.0` is the *original* crate (edition 2018, same first authors,
`https://docs.rs/lsp-types`). **`tower-lsp-server 0.23.0` does not depend on it**; it is present in the cache
only as historical/alternative material (likely from ticket 07's `lsp-server`+`lsp-types` exploration).
Do not add `lsp-types` alongside `tower-lsp-server`.

### 2.0 Version status & the `proposed` feature (crates.io + empirical probe)

crates.io API (fetched 2026-10-06): `ls-types` latest **0.0.6** (2026-03-08); history
`0.0.0` (2025-08-08), `0.0.1`/`0.0.2` (2025-12-07), `0.0.3` (2026-02-09), `0.0.5`/`0.0.6` (2026-03-08).
Feature sets per version (crates.io `version.features`): 0.0.2/0.0.3 → `{default: [], proposed: []}`;
0.0.5 → `{default: []}` (**`proposed` removed**); 0.0.6 → `{}` (no features at all).

`tower-lsp-server 0.23.0` declares `ls-types = "0.0"` (`Cargo.toml.orig:29`) and
`proposed = ["ls-types/proposed"]` (`Cargo.toml.orig:21`). Semver caret on `0.0` admits any `0.0.x`, so a
**fresh downstream build resolves `ls-types 0.0.6`** — verified empirically (temp probe crate outside this
repo, cargo 1.98.0, 2026-10-06):

- `tower-lsp-server = "0.23"` (no features) → resolves `ls-types 0.0.6` + `fluent-uri 0.4.1`,
  `cargo check` **clean**.
- `tower-lsp-server = { features = ["proposed"] }` → resolution **fails**:
  *"package `tower-lsp-server` depends on `ls-types` with feature `proposed` but `ls-types` does not have
  that feature"* (only one `ls-types` 0.0.6 in the candidate set). → **tower-lsp-server's `proposed` feature
  is currently unusable** without pinning `ls-types = "=0.0.3"` (the last version with the feature).
- `cargo check` on the default build passed *without* activating `proposed`, so the dangling feature
  reference is validated only when activated, not eagerly.

Practical reading: in ls-types 0.0.6 the formerly-proposed types (e.g. `inline_completion`) are
**unconditionally present** (the `#[cfg(feature = "proposed")]` gates on `mod inline_completion` and 4
`ServerCapabilities`/request sites were removed between 0.0.2 and 0.0.6) — so the `proposed` feature looks
**vestigial, not a capability loss**; default builds are unaffected. Side effect to note: `fluent-uri` moved
`0.3` → `0.4` in the same window (the `Uri` type Traces will carry in every
`TextDocumentIdentifier`/`TextEdit`-adjacent position params), so pinning `ls-types = "=0.0.2"` to get
`proposed` would also drag `fluent-uri 0.3` — avoid unless a genuinely proposed type is ever needed.
`cargo update` behavior for Traces: whatever `ls-types` version is in the lock at first resolution sticks
until an explicit update — decide deliberately (0.0.6 recommended; it is what a fresh build gets anyway).

**Spec-surface diff 0.0.2 vs 0.0.6**: `completion.rs` is byte-identical in structure (628 lines both,
identical field sets) — all §2.1–§2.5 quotes hold for both versions. Files that did change:
`code_action.rs`, `document_diagnostic.rs`, `lib.rs`, `notification.rs`, `request.rs`,
`semantic_tokens.rs`, `uri.rs` — none completion-related.

### 2.1 Request/response types (src/request.rs)

```rust
// request.rs:283-289
pub enum Completion {}
impl Request for Completion {
    type Params = crate::CompletionParams;
    type Result = Option<crate::CompletionResponse>;
    const METHOD: &'static str = "textDocument/completion";
}

// request.rs:293-297
pub enum ResolveCompletionItem {}
impl Request for ResolveCompletionItem {
    type Params = crate::CompletionItem;
    type Result = crate::CompletionItem;
    // METHOD = "completionItem/resolve" (via request! macro, request.rs:46-48)
```

### 2.2 Params & context (src/completion.rs)

```rust
// completion.rs:369-383
pub struct CompletionParams {
    #[serde(flatten)] pub text_document_position: TextDocumentPositionParams, // text_document + position
    #[serde(flatten)] pub work_done_progress_params: WorkDoneProgressParams,
    #[serde(flatten)] pub partial_result_params: PartialResultParams,
    #[serde(skip_serializing_if = "Option::is_none")] pub context: Option<CompletionContext>,
}

// completion.rs:387-395
pub struct CompletionContext {
    pub trigger_kind: CompletionTriggerKind,
    #[serde(skip_serializing_if = "Option::is_none")] pub trigger_character: Option<String>,
}

// completion.rs:400-408
pub struct CompletionTriggerKind(i32);
lsp_enum! {
    impl CompletionTriggerKind {
        const INVOKED = 1;
        const TRIGGER_CHARACTER = 2;
        const TRIGGER_FOR_INCOMPLETE_COMPLETIONS = 3;
    }
}
```

Spec semantics (LSP **3.18** primary source `language/completion.md`, fetched 2026-10-06,
<https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.18/language/completion.md>):

- `Invoked = 1`: "triggered by typing an identifier (24x7 code complete), manual invocation (e.g Ctrl+Space) or via API."
- `TriggerCharacter = 2`: "triggered by a trigger character specified by the `triggerCharacters` properties of the `CompletionRegistrationOptions`."
- `TriggerForIncompleteCompletions = 3`: "re-triggered as the current completion list is incomplete."
- **`context` is only sent if the client declared `completion.contextSupport === true`**
  (spec on `CompletionParams`: "This is only available if the client specifies to send this using the client
  capability `completion.contextSupport === true`"). → **context detection must not depend on
  `CompletionContext`**; it is a hint, availability not guaranteed.

### 2.3 Response types & `CompletionItem` fields (src/completion.rs)

```rust
// completion.rs:350-353
pub enum CompletionResponse {
    Array(Vec<CompletionItem>),
    List(CompletionList),
}
// From<Vec<CompletionItem>> and From<CompletionList> impls exist (lines 355-365)

// completion.rs:414-421
pub struct CompletionList {
    pub is_incomplete: bool,
    pub items: Vec<CompletionItem>,
}
```

Spec note: "If a `CompletionItem[]` is provided it is interpreted to be complete. So it is the same as
`{ isIncomplete: false, items }`"; `isIncomplete: true` means "Further typing should result in recomputing
this list. Recomputed lists have all their items replaced (not appended)".

`CompletionItem` (completion.rs:425-548) — full field inventory, in order:

| Field | Type | Notes (quoted/paraphrased from source docs) |
| :--- | :--- | :--- |
| `label` | `String` | required; default insert text |
| `label_details` | `Option<CompletionItemLabelDetails>` | 3.17; `{ detail, description }` (lines 567-579) |
| `kind` | `Option<CompletionItemKind>` | enum constants TEXT=1 … TYPE_PARAMETER=25 (lines 31-58); SNIPPET=15, FILE=17, REFERENCE=18, FOLDER=19 |
| `detail` | `Option<String>` | cheap-ish; resolvable lazily (pre-3.16 default) |
| `documentation` | `Option<Documentation>` | `enum Documentation { String(String), MarkupContent(MarkupContent) }` (lib.rs:2397); `MarkupContent { kind: MarkupKind::{PlainText,Markdown}, value }` |
| `deprecated` | `Option<bool>` | |
| `preselect` | `Option<bool>` | |
| `sort_text` | `Option<String>` | "used when comparing this item with other items. When `falsy` the label is used" |
| `filter_text` | `Option<String>` | "used when filtering a set of completion items. When `falsy` the label is used" |
| `insert_text` | `Option<String>` | doc explicitly recommends `text_edit` instead (VS Code word-prefix interpretation) |
| `insert_text_format` | `Option<InsertTextFormat>` | "applies to both `insertText` and the `newText` of a provided `textEdit`. If omitted defaults to PlainText" |
| `insert_text_mode` | `Option<InsertTextMode>` | AS_IS=1, ADJUST_INDENTATION=2 |
| `text_edit` | `Option<CompletionTextEdit>` | `enum CompletionTextEdit { Edit(TextEdit), InsertAndReplace(InsertReplaceEdit) }` (lines 266-269); **range must be single-line and contain the request position**; insert range must be a prefix of replace range |
| `additional_text_edits` | `Option<Vec<TextEdit>>` | "must not overlap with the main edit nor with themselves" |
| `command` | `Option<Command>` | `Command { title, command, arguments: Option<Vec<Value>> }` (lib.rs:441) — runs *after* insertion |
| `commit_characters` | `Option<Vec<String>>` | length=1 chars |
| `data` | `Option<serde_json::Value>` | "preserved on a completion item between a completion and a completion resolve request" ← **the lazy-resolve payload channel** |
| `tags` | `Option<Vec<CompletionItemTag>>` | DEPRECATED=1 |

Helper: `CompletionItem::new_simple(label: String, detail: String) -> Self` (lines 553-559).

`TextEdit { range: Range, new_text: String }` (lib.rs:470-477). `Range { start: Position, end: Position }`,
`Position { line: u32, character: u32 }` (lib.rs:175-184) — `character` doc: *"The meaning of this offset is
determined by the negotiated `PositionEncodingKind`."*

### 2.4 Server capability (src/completion.rs + lib.rs)

```rust
// completion.rs:286-325
pub struct CompletionOptions {
    #[serde(skip_serializing_if = "Option::is_none")] pub resolve_provider: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub trigger_characters: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")] pub all_commit_characters: Option<Vec<String>>,
    #[serde(flatten)] pub work_done_progress_options: WorkDoneProgressOptions,
    #[serde(skip_serializing_if = "Option::is_none")] pub completion_item: Option<CompletionOptionsCompletionItem>, // 3.17 label_details_support
}
// completion.rs:340-346
pub struct CompletionRegistrationOptions {
    #[serde(flatten)] pub text_document_registration_options: TextDocumentRegistrationOptions,
    #[serde(flatten)] pub completion_options: CompletionOptions,
}
```

`ServerCapabilities.completion_provider: Option<CompletionOptions>` (lib.rs:1779-1781).
Trigger-character doc on `CompletionOptions.trigger_characters` (lib.rs:291-300): *"Most tools trigger
completion request automatically… Characters that make up identifiers don't need to be listed here. If code
complete should automatically be trigger on characters not being valid inside an identifier (for example `.`)
list them in `triggerCharacters`."* — i., registering `#`, `[`, `:` etc. is about *request* generation, not
about which completions come back.

Spec model quote (why trigger chars are only a latency optimization):

> "to achieve consistency across languages and to honor different clients usually **the client is responsible
> for filtering and sorting**. This has also the advantage that client can experiment with different filter
> and sorting models. However servers can enforce different behavior by setting a `filterText` / `sortText`"
>
> "for speed clients should be able to filter an already received completion list if the user continues
> typing. Servers can opt out of this using a `CompletionList` and mark it as `isIncomplete`."

And the two insertion modes:

> "**Completion item provides an insertText / label without a text edit**: … the client should filter against
> what the user has already typed using the word boundary rules of the language…"
>
> "**Completion Item with text edits**: in this mode the server tells the client that it actually knows what
> it is doing. If you create a completion item with a text edit at the current cursor position **no word
> guessing takes place and no automatic filtering (like with an `insertText`) should happen**. This mode can
> be combined with a sort text and filter text… If the text edit is a replace edit then the range denotes the
> word used for filtering."

### 2.5 Client capabilities read at `initialize` (src/completion.rs, lib.rs)

Path: `InitializeParams.capabilities.text_document.completion`
(`TextDocumentClientCapabilities.completion: Option<CompletionClientCapabilities>`, lib.rs:1293).

```rust
// completion.rs:215-246
pub struct CompletionClientCapabilities {
    pub dynamic_registration: Option<bool>,
    pub completion_item: Option<CompletionItemCapability>,
    pub completion_item_kind: Option<CompletionItemKindCapability>,
    pub context_support: Option<bool>,          // gates CompletionContext being sent at all
    pub insert_text_mode: Option<InsertTextMode>,
    pub completion_list: Option<CompletionListCapability>,   // itemDefaults support (3.17)
}
```

`CompletionItemCapability` (completion.rs:63-130) — the guards ticket 24 must read:

- `snippet_support: Option<bool>` — doc: *"Client supports snippets as insert text. A snippet can define tab
  stops and placeholders with `$1`, `$2` and `${3:foo}`. `$0` defines the final tab stop…"* → gate for
  `InsertTextFormat::SNIPPET`.
- `resolve_support: Option<CompletionItemCapabilityResolveSupport>` where
  `CompletionItemCapabilityResolveSupport { properties: Vec<String> }` (lines 134-137) — doc: *"Indicates
  which properties a client can resolve lazily on a completion item. Before version 3.16.0 only the
  predefined properties `documentation` and `details` could be resolved lazily."*
  → if `properties` contains e.g. `"additionalTextEdits"` (rust-analyzer's auto-import case), ticket 24 can
  defer those too; if `resolve_support` is `None`, only `detail`/`documentation` may be deferred.
- Also present: `commit_characters_support`, `documentation_format: Option<Vec<MarkupKind>>`,
  `deprecated_support`, `preselect_support`, `tag_support`, `insert_replace_support`,
  `insert_text_mode_support`, `label_details_support`.

`InsertTextFormat` (completion.rs:17-24): `PLAIN_TEXT = 1`, `SNIPPET = 2`.
`CompletionListCapability { item_defaults: Option<Vec<String>> }` (lines 200-211) — if the client lists
`"insertTextFormat"` / `"editRange"` etc., defaults can be hoisted onto `CompletionList.itemDefaults`
(spec: "Servers are only allowed to return default values if the client signals support for this").

### 2.6 Position encoding (knot tying ticket 11 ↔ completion ranges)

- `ServerCapabilities.position_encoding: Option<PositionEncodingKind>` (lib.rs:1758); doc: *"If the client
  didn't provide any position encodings the only valid value that a server can return is 'utf-16'. If
  omitted it defaults to 'utf-16'."*
- `ClientCapabilities.general.position_encodings: Option<Vec<PositionEncodingKind>>` (lib.rs:1510).
- `PositionEncodingKind::{UTF8 = "utf-8", UTF16 = "utf-16", UTF32 = "utf-32"}` (lib.rs:252-264); UTF-16
  *"is the default and must always be supported by servers"*.

→ Every `text_edit`/`additional_text_edits` range Traces emits must be in the negotiated encoding.
Default (and only universally safe) choice: UTF-16, i.e. ticket 11's `LineIndex::byte_to_utf16_cu` for file
buffers and ropey's UTF-16 API (§3) for live buffers. Negotiating `utf-8` is allowed but *adds* a second code
path for every client that doesn't offer it.

### 2.7 Ecosystem practice *(web evidence)*

- No separate adoption question here: `ls-types` is the crate `tower-lsp-server` mandates (§1.1, from local
  source). The only ecosystem wrinkle is documentation drift: most third-party material still says
  `lsp_types` — treat any snippet using `tower_lsp::lsp_types` as written for the dead `tower-lsp` line.

### 2.8 Spec-vs-crate gaps (verified against 0.0.2 **and** 0.0.6 — material to units 4/8)

Checked by grepping `completion.rs`/`lib.rs` in both cached versions; **all three gaps persist in 0.0.6**:

| Spec feature (since) | ls-types status | Consequence for Traces |
| :--- | :--- | :--- |
| `CompletionList.itemDefaults` (3.17: `commitCharacters`, `editRange`, `insertTextFormat`, `insertTextMode`, `data`) + `CompletionItemDefaults` struct | **Absent** — `CompletionList` is only `{ is_incomplete, items }` (completion.rs:414-421); no `CompletionItemDefaults` type anywhere | Server **cannot emit** item-defaults through the typed API. The *client capability* `CompletionListCapability.item_defaults: Option<Vec<String>>` **is** present (line 210) → a Traces client could advertise support, but Traces-the-server has nothing to answer with. Unit 4's `itemDefaults` recommendation is **blocked by the crate** unless ls-types gains the field (upstream PR) or items are hand-serialized (e.g. a custom `serde` response enum) |
| `CompletionItem.textEditText` (3.17) | **Absent** (no `text_edit_text` field) | No `textEditText` shorthand even if itemDefaults existed |
| `CompletionList.applyKind` / `ApplyKind` / `completionList.applyKindSupport` (3.18) | **Absent** | 3.18-only merge semantics unavailable; harmless while itemDefaults itself is unavailable |

Not a gap but adjacent: `CompletionItem.label_details` **is** present (line 435), so 16's
`label_details.detail` usage-stats plan is crate-supported; and nothing in ls-types gates completion types
behind `cfg(feature = "proposed")` in either version (the gates that exist cover `inline_completion` and
unrelated capability fields — removed entirely in 0.0.6, §2.0).

---

## 3. `ropey` 1.6.1 (live-buffer text, ticket 14)

Cached: `.rust-docs/cache/crates/ropey/1.6.1` (plus a `master` snapshot). Not pinned in the manifest.

### 3.1 Conversion APIs relevant to completion `TextEdit`s (src/rope.rs, src/slice.rs)

On `Rope` (same set exists on `RopeSlice`, slice.rs:323-452):

```rust
pub fn byte_to_char(&self, byte_idx: usize) -> usize          // rope.rs:634  (O(log N), panics OOB)
pub fn byte_to_line(&self, byte_idx: usize) -> usize          // rope.rs:653
pub fn char_to_byte(&self, char_idx: usize) -> usize          // rope.rs:670
pub fn char_to_line(&self, char_idx: usize) -> usize          // rope.rs:689
pub fn char_to_utf16_cu(&self, char_idx: usize) -> usize      // rope.rs:706
pub fn utf16_cu_to_char(&self, utf16_cu_idx: usize) -> usize  // rope.rs:727
pub fn line_to_byte(&self, line_idx: usize) -> usize          // rope.rs:745
pub fn line_to_char(&self, line_idx: usize) -> usize          // rope.rs:763
pub fn len_utf16_cu(&self) -> usize                           // rope.rs:261
// non-panicking variants:
pub fn try_byte_to_char(&self, byte_idx: usize) -> Result<usize>          // rope.rs:1412
pub fn try_char_to_utf16_cu(&self, char_idx: usize) -> Result<usize>      // rope.rs:1460
pub fn try_utf16_cu_to_char(&self, utf16_cu_idx: usize) -> Result<usize>  // rope.rs:1476
pub fn try_line_to_byte(&self, line_idx: usize) -> Result<usize>          // rope.rs:1495
// …and the rest of the try_* family
```

- Doc on `char_to_utf16_cu` (rope.rs:694-706): *"Ropey stores text internally as utf8, but sometimes it is
  necessary to interact with external APIs that still use utf16. […] Runs in O(log N) time."*
  `utf16_cu_to_char` note: *"if the utf16 code unit is in the middle of a char, returns the index of the char
  that it belongs to."*
- **There is no `byte_to_utf16_cu`** — conversions are char-indexed; compose
  `byte → char → utf16_cu` (two O(log N) hops). Same for the reverse. The internal tree already tracks
  `utf16_surrogates` per node (`src/tree/text_info.rs`), so no O(n) rescans.
- `Rope::slice<R: RangeBounds<usize>>(&self, char_range: R) -> RopeSlice` (rope.rs:948-952; *char*-indexed
  ranges; panics OOB — `get_slice(...).unwrap()`).
- The panicking methods are the ergonomic default; this repo's lints (`panic = "deny"`,
  `unwrap_used = "deny"` — though those bite on *our* code, not ropey's) and `missing_panics_doc` push
  toward using the `try_*` family in library code.

### 3.2 What this enables/constrains for completion

- Converting an LSP `CompletionParams.text_document_position.position` (line + UTF-16 `character`) into a
  buffer range to slice candidates: `line_to_byte(line)` → walk/measure UTF-16 within the line, or
  `line_to_char` + `utf16_cu_to_char` arithmetic. Both are O(log N) — trivially inside the 20 ms budget.
- Emitting `TextEdit { range }` for a candidate span stored as byte offsets (ticket 11's `Range<BytePos>` on
  live buffers): bytes → chars (`byte_to_char`) → UTF-16 (`char_to_utf16_cu`) per endpoint, then
  `Position { line: byte_to_line-derived, character }`.
- Use `try_*` variants + explicit clamping ("If the character value is greater than the line length it
  defaults back to the line length" — spec rule for `Position.character`) rather than letting ropey panic on
  a stale position racing an edit. Under sequential dispatch a completion request cannot interleave with
  `didChange`, so the rope is stable for the handler's duration — stale-position races are a non-issue
  *inside* a handler (they are an issue only for the client's later application of the edit).
- ropey has **no** incremental line/UTF-16 *index* separate from the rope — you don't need one; the rope is
  the index.

### 3.3 Ecosystem practice *(web evidence)*

Not separately researched — `ropey` adoption was settled by ticket 14 (`map.md:74`, "ropey adopted narrowly
for live-buffer text") on API grounds; nothing in this pass contradicts it, and no "which rope crate"
question is live for ticket 24.

---

## 4. `fuzzy-matcher` 0.3.7 (scoring/filtering candidates; ticket 15's pick)

Pinned: **transitively** via `inquire` (Cargo.lock), direct-dep adoption recorded in ticket 15
(`map.md:79`: "New crates: `camino`, `fuzzy-matcher`"). Latest on crates.io is 0.3.7 (see ecosystem
subsection). Cached: `.rust-docs/cache/crates/fuzzy-matcher/0.3.7`.

### 4.1 API (src/lib.rs, src/skim.rs)

```rust
// lib.rs:6-13 (private aliases; they resolve to these types in signatures)
type IndexType = usize;   // u32 with feature "compact"
type ScoreType = i64;     // i32 with feature "compact"

// lib.rs:15-23
pub trait FuzzyMatcher: Send + Sync {
    /// fuzzy match choice with pattern, and return the score & matched indices of characters
    fn fuzzy_indices(&self, choice: &str, pattern: &str) -> Option<(ScoreType, Vec<IndexType>)>;
    /// fuzzy match choice with pattern, and return the score of matching
    fn fuzzy_match(&self, choice: &str, pattern: &str) -> Option<ScoreType> {
        self.fuzzy_indices(choice, pattern).map(|(score, _)| score)
    }
}
```

Implementors: `SkimMatcher` (legacy, skim.rs:54), **`SkimMatcherV2`** (skim.rs:602, impl at 1059), `ClangdMatcher`
(clangd.rs:96). Idiomatic usage (crate README):

```rust
use fuzzy_matcher::skim::SkimMatcherV2;
let matcher = SkimMatcherV2::default();
let (score, indices) = matcher.fuzzy_indices("axbycz", "abc").unwrap(); // score higher = better; None = no match
```

`SkimMatcherV2` builder (skim.rs:629-662): `score_config(SkimScoreConfig)`, `element_limit(usize)`,
`ignore_case()`, `smart_case()` (**default**: `CaseMatching::Smart`), `respect_case()`,
`use_cache(bool)` (**default `true`** — thread-local score-matrix + char caches,
`m_cache`/`c_cache` as `CachedThreadLocal<RefCell<Vec<…>>>`), `debug(bool)`.

Properties that matter for completion:

- `&self` methods, trait requires `Send + Sync` → one shared matcher instance (e.g. `OnceLock<SkimMatcherV2>`)
  can serve handlers; with `concurrency_level(1)` there's no contention anyway.
- `fuzzy_match(choice, pattern)` — filter pass; `fuzzy_indices` also returns matched char indices, useful if
  Traces ever wants to compute its own highlight ranges (LSP completion has **no** per-char highlight field;
  indices are server-side-only information — client re-derives highlighting itself). Don't pay for indices
  in the hot path; use `fuzzy_match`.
- **Empty pattern returns `Some`** (legacy `skim::fuzzy_match`: `if pattern.is_empty() { return Some((0, Vec::new())) }`, skim.rs:83-85; V2 path likewise treats empty as match) — important for "just typed `[`" contexts where the word is empty: everything matches, score ties → ordering falls back to `sort_text`/insertion order.
- Scores are `i64`, only meaningful for relative ordering within one matcher config; do not persist them.

### 4.2 Adequacy for per-keystroke filtering

- Complexity: skim V2 builds a Smith-Waterman-style score matrix bounded by `element_limit` (default 0 =
  unlimited? — **[question]** exact `element_limit == 0` semantics: skim.rs reads `element_limit: 0` in
  `Default`; the matrix path checks it, treat 0 as "no limit" unless verified otherwise) per
  (choice, pattern) pair, O(|choice|·|pattern|) with early-outs for non-matching first chars.
- Scale math for Traces: candidate sets are tags + frontmatter fields + note paths + query symbols —
  realistically 10²–10⁴ strings, each ≤ ~100 chars, pattern ≤ ~20 chars. Even the external skim benchmark
  numbers (§4.3) put a *full 100k-item miss* scan at ~17 ms; at 10⁴ items that's ~1.7 ms worst case,
  well inside the <20 ms budget (ticket 33) **if** the candidate list is built once and filtered once per
  request. This matches the repo's own prior finding: `research/21-query-language-intelligence.md:124`
  "`fuzzy-matcher` (skim) — Baseline — Keep for now (already in codebase)" and ":125" "`nucleo-matcher` —
  6x faster — Consider for Phase 2 if >100 candidates".
- Caveat: per-keystroke filtering happens **on every completion request**, and with `concurrency_level(1)`
  the whole request (context detection + candidate generation + scoring + JSON serialization) shares the
  20 ms. The serialization of thousands of `CompletionItem`s may dominate scoring — flag for ticket 33's
  `lsp_latency.rs` harness rather than assuming the matcher is the bottleneck.

### 4.3 Ecosystem practice *(web evidence)*

Two distinct questions, per the map's standing rule (`map.md:41`):

**(a) What do Rust LSPs actually use for completion filtering?**

- rust-analyzer historically did **no** server-side fuzzy filtering and relied on the client; its tracking
  issue for server-side sorting/filtering (`github.com/rust-lang/rust-analyzer/issues/7935`, 2021, closed
  completed) spells out the LSP constraint: *"LSP actually insists that the client does sorting&filtering, so
  we need to hack around that… set `isIncomplete` to true; set sort text to `format!("{04}", i)`"* — i.,
  the established pattern for "server computed the order" is: pre-filter server-side, force re-request with
  `isIncomplete: true`, and encode final order in zero-padded `sortText`.
- gopls added server-side fuzzy matching (commit `2adf828`, `go.googlesource.com/tools.git`): *"Make use of
  the existing fuzzy matcher to perform server side fuzzy completion matching. Previously the server did
  exact prefix matching… Having the server do fuzzy matching has two main benefits: Deep completions now
  update as you type. The completion candidates returned to the client are marked 'incomplete', causing the
  client to refresh the candidates after every keystroke… All editors get fuzzy matching for free."*
- Neither rust-analyzer nor gopls uses `fuzzy-matcher`; rust-analyzer wrote its own scorer (issue #7935
  "Fuzzy scoring needs to be implemented from scratch I think"), gopls wrote `internal/lsp/fuzzy`. So
  "which matcher crate" has **no** convergent Rust-LSP answer — it is genuinely this project's pick.
- No evidence found of a widely-used Rust LSP using `nucleo-matcher` inside an LSP completion path (helix
  uses nucleo for its *picker UI*, not an LSP). **[question]** — absence-of-evidence, not evidence-of-absence.

**(b) Is `fuzzy-matcher` a reasonable pick, or should `nucleo-matcher` be flagged?**

- `fuzzy-matcher` (crates.io API, fetched 2026-10-06): latest **0.3.7 published 2020-10-04** — no release in
  ~6 years; 33.6M all-time / 8.9M recent downloads; MIT; 1.5k code lines; repo `github.com/lotabout/fuzzy-matcher`.
- `nucleo-matcher` (crates.io API, fetched 2026-10-06): latest **0.3.1 published 2024-02-20**; 4.55M
  all-time / 1.89M recent downloads; MPL-2.0; repo `github.com/helix-editor/nucleo`.
  ⚠️ **Correction to repo research**: `research/21-query-language-intelligence.md:257` cites
  "`nucleo-matcher` 0.3.5" — no such version exists on crates.io (max is 0.3.1). Version claims in that
  file should be re-checked before reuse.
- nucleo README (docs.rs/crate/nucleo-matcher/latest): *"If you are looking for a replacement of the
  fuzzy-matcher crate … you should use the nucleo-matcher crate"*; *"Compared to skim (and the fuzzy-matcher
  crate) nucleo has an even larger performance advantage and is often around six times faster"*; published
  skim-vs-nucleo table over ~100k-item lists: never_matches 17.44 ms (skim) vs 2.30 ms (nucleo),
  `//.h` 35.46 ms vs 9.53 ms. Also: better Unicode (skim's *"bonus system and even case insensitivity only
  work for ASCII"*), matcher is reusable but *"eagerly allocate[s] a fairly large chunk of heap memory
  (around 135KB)"* — cache it in a `OnceLock`, don't build per request. API:
  `Matcher::fuzzy_match(&mut self, haystack: Utf32Str<'_>, needle: Utf32Str<'_>) -> Option<u16>` (note
  `&mut self` and pre-segmented `Utf32Str` inputs — a slightly fiddlier API than fuzzy-matcher's `&self, &str`).
- **Verdict-shaped summary** (recommendation, marked as such): `fuzzy-matcher` is fine for Traces' candidate
  volumes and is already the settled pick (ticket 15); `nucleo-matcher` is the credible upgrade path if
  ticket 33's harness ever shows scoring (rather than I/O or serialization) at fault, and it is what the
  wider ecosystem reaches for today. Not a ticket-24 blocker either way — ticket 24's architecture should
  not hard-code matcher internals (wrap it behind Traces' own `rank_candidates`-style function).

---

## 5. `strsim` 0.11.1 (pinned) — the existing "did you mean" machinery

Direct dependency `strsim = "0.11.1"` (Cargo.toml:157). Cached source:

```rust
// strsim source src/lib.rs:269
pub fn levenshtein(a: &str, b: &str) -> usize
```

Used via the repo's own wrapper (`src/strsim.rs:11-21`):

```rust
pub(crate) fn closest_match<'a, T>(
    candidates: impl Iterator<Item = (T, &'a str)>,
    input: &str,
) -> Option<T> {
    let threshold = input.chars().count().div_ceil(2).max(1);
    candidates
        .map(|(item, name)| (item, levenshtein(input, name)))
        .min_by_key(|&(_, distance)| distance)
        .filter(|&(_, distance)| distance <= threshold)
        .map(|(item, _)| item)
}
```

Consumers: `Schema::suggest_field` (`src/schema/model.rs:129`, tests assert e.g.
`suggest_field("statu") == Some("status")`, and `suggest_field("completely_unrelated") == None`) and query
field errors (`src/query/grammar/field.rs:24`). The template layer forwards it into diagnostics
(`src/template/engine/schema.rs:347-352`).

**`suggest_class()` does not exist in the codebase yet** — it is listed as a *prerequisite* of ticket 21
(`issues/21-query-language-intelligence.md:49`) and `grep -r suggest_class src/` returns nothing.
**[question]**: when implemented, will it share `closest_match` (expected) or diverge?

Role split for ticket 24: `strsim::closest_match` returns **one** best candidate above a threshold — it is a
*diagnostic* helper ("did you mean…?"), not a completion ranker (no scores, no top-N, first-min tie-break
only). Completion ranking/filtering should stay with `fuzzy-matcher` (§4); the two can coexist (fuzzy-matcher
for the completion list, `closest_match` for the zero-match escalation diagnostic, e.g. ticket 15's
"Create note" path or ticket 21's Warning tier).

### 5.1 Ecosystem practice *(web evidence)*

Not applicable — no adoption question is being asked about `strsim` (already pinned, already used for a
diagnostics job). For ranking-vs-Levenshtein generally: Levenshtein cost is O(|a|·|b|) per candidate
without the substring fast-paths of skim/nucleo, but `closest_match` is only invoked on *failure* paths,
not per keystroke.

---

## Implications for ticket 24

**Context detection (dispatcher design)**

1. The handler always receives `CompletionParams { text_document_position, context: Option<…> }`. Because
   `context` is only sent when `contextSupport` is true (§2.2), **the single outward-walking AST/span
   dispatcher must be the source of truth**; `CompletionContext.trigger_kind`/`trigger_character` are
   optional hints (useful for "just typed `[` → wikilink branch, skip scanning back for `[[`"), never a
   dispatch mechanism on their own. A client without `contextSupport` (or a manual Ctrl+Space,
   `INVOKED = 1`) must land in the same place via buffer inspection.
2. Trigger-character *registration* and context detection are independent layers: registering a character
   only controls *when the client sends a request* (spec: "Characters that make up identifiers don't need
   to be listed here"), while the dispatcher decides *what the request means*. The layered option in the
   ticket ("both") is the one the protocol shapes toward: a registered char just makes context detection
   run sooner/cheaper.
3. Context detection cost is now *the* hot path (runs on registered chars, inside 20 ms, blocking the whole
   sequential queue per §1.5). It must operate on the span-aware AST (ticket 11) + rope slice without
   re-parsing — full re-parse per completion would be the budget.

**Trigger-character registration**

4. Static registration is one field: `ServerCapabilities.completion_provider.trigger_characters =
   Some(vec![…])` returned from `initialize` (§2.4). Dynamic replacement is possible via
   `Client::register_capability` gated on `dynamicRegistration` (§1.4), but no evidence it's *needed* —
   trigger sets are knowable at initialize time from config (e.g. rumdl coexistence toggles). Recommend the
   ticket decide "static, config-derived at initialize" unless a runtime-toggle requirement appears.
5. Candidate set to reconcile (inputs from resolved tickets, from the tickets themselves):
   ticket 16 → `#`; ticket 19 → `[`, `:`; ticket 21 → `#`, `@` (with `(`, `"` deferred to its Phase 2);
   rumdl (ticket 03 research) → `(`, `#`, `/`, `.`, `-` — **overlaps on `#`** (and possibly `.`/`-` if Traces
   ever registers them). Since one `triggerCharacters` array serves the whole server, overlap is a *duplication
   concern, not a conflict* — both servers receiving one request is harmless (each no-ops when its own
   context check fails). The real coordination question stays what ticket 24 already frames: recommending
   `enableLinkCompletions = false` (plus `enableLinkNavigation`/`enableSymbols`, per
   `research/tool-rumdl-boundary.md:34-42`) so *rumdl's* completions don't fire inside wikilink/standard-link
   targets Traces owns. Also mind: every registered char fires a completion request **even mid-word outside
   any Traces context** — each miss costs one sequential handler slot (§1.5); keep the miss path near-zero.
6. There is no per-context trigger registration in LSP — `triggerCharacters` is a flat `Vec<String>` on the
   single `completionProvider` (§2.4). "Independent per-context registration" as an alternative in the
   ticket is **not expressible** statically; it would require dynamic register/unregister of
   `textDocument/completion` itself (§1.4) — technically possible, client-dependent, and strictly more
   complex than one flat set + server-side dispatch. Evidence leans to the layered/flat option.

**Lazy resolve (`completionItem/resolve`)**

7. Mechanics are settled: implement `completion_resolve` (§1.2); advertise `resolve_provider: Some(true)`;
   stash whatever context is needed in `CompletionItem.data: Option<serde_json::Value>` (round-trip
   preserved by the client, §2.3); fill `documentation` (and possibly `detail`, `additional_text_edits`)
   on resolve.
8. Hard constraints to design against (quoted in §1.3 and §2.2): `sort_text`, `filter_text`, `insert_text`,
   `text_edit` **must be present in the initial response and must not change during resolve**. And what is
   lazily legal is client-capability-defined: default allow-list is only `detail` + `documentation`; more
   (e.g. `additionalTextEdits`, which rust-analyzer's auto-import feature depends on) requires the client's
   `resolve_support.properties` to say so (§2.5). → Ticket 24 should specify: *initial list = label + kind +
   sort_text + filter_text + text_edit (+ data); resolve = documentation + detail, opportunistically
   additional_text_edits iff advertised.*
9. Under `concurrency_level(1)` resolve is serialized with the next keystroke's completion (§1.5): resolve
   must read already-materialized data (or do bounded work), not walk the vault. Schema Field Definition
   docs (the ticket's example) should be pre-resolved into the data payload at list time *if* cheap enough,
   else fetched from always-resident schema structures (ticket 33: schema definitions always resident).

**Snippets**

10. Gate: `InitializeParams…completion_item.snippet_support` — only emit
    `insert_text_format: Some(InsertTextFormat::SNIPPET)` when `Some(true)` (§2.5). Fallback without
    snippet support: plain-text insertion (e.g. `[[target|]]`-style text and let the user arrow back, or
    a `command`-based cursor placement — `command` executes after insertion but needs a client-side
    command handler, which Traces does not have; effectively **no reliable no-snippet cursor placement** —
    worth an explicit decision line in the ticket).
11. Snippet syntax contract (quoted §2.5): `$1`/`${3:foo}` tab stops, `$0` final stop, linked placeholders
    by identifier. For "cursor left inside `[[|]]`": `${1:target}$0`-shaped snippet with the surrounding
    `[[…]]` inside `text_edit.new_text` (the format applies to `textEdit.newText` too, §2.3).
    Multiline snippet caveat: `text_edit.range` **must be single-line and contain the request position**
    (§2.3) — snippets themselves may insert newlines (InsertTextMode AS_IS vs ADJUST_INDENTATION, §2.3).
12. 3.17 escape hatch: `CompletionList.itemDefaults.insertTextFormat` if the client advertises
    `completionList.itemDefaults` support (§2.5) — avoids repeating the format per item. **Caveat: the
    server-side field does not exist in ls-types (0.0.2–0.0.6) — see §2.8**; this escape hatch is
    spec-legal but crate-blocked today.

**Filtering/sorting ownership (a decision the ticket hasn't listed but the architecture implies)**

13. Spec default: **client filters and sorts**; server influences via `filter_text`/`sort_text` (§2.4).
    But `text_edit` mode changes the rules: "no word guessing takes place and no automatic filtering …
    should happen", with replace-ranges still supplying the filter word (§2.4). Traces's natural shape is
    text-edit mode (replace a partial `[[tar` / `#pro` / `field: val` span precisely) — so the ticket must
    consciously choose: (a) return the (pre-filtered) candidate set and let the client re-filter via
    `filter_text` against the replace range, or (b) server-filter each keystroke with `fuzzy-matcher` and
    return `CompletionList { is_incomplete: true, .. }` so every keystroke re-queries (rust-analyzer/gopls
    pattern, §4.3a). Option (b) costs a full server round per keystroke (sequential! §1.5) but guarantees
    consistent ranking across clients; option (a) is cheaper per keystroke but delegates quality to the
    client's fuzzer. Zero-padded `sort_text` is the documented way to enforce server order either way.
14. Empty-word contexts (cursor right after `[`/`#`/`:`): fuzzy-matcher returns `Some(0)` for empty patterns
    (§4.1) → no filtering happens, everything ranks equal; ordering then depends entirely on `sort_text`
    (recency/frequency/prefix heuristics must live there).

**Open questions for the grilling session**

- **[q1]** Dynamic re-registration semantics (replace vs unregister+register) across VS Code/Neovim clients —
  unverified (§1.4).
- **[q2]** `SkimMatcherV2::element_limit(0)` semantics (unlimited vs zero-capacity) — not fully traced in
  skim.rs (§4.2).
- **[q3]** `suggest_class()` shape — doesn't exist yet; assumed `closest_match`-based (§5).
- **[q4]** Whether any mainstream Rust LSP completion path uses `nucleo-matcher` — no evidence found either
  way (§4.3a).
- **[q5]** ~~local spec can't serve as citation~~ — **resolved**: local `docs/refs/lsp_spec.md` is the 3.18
  corpus (TOC + changelog + non-include sections only; 0 hits for `triggerCharacters`/`isIncomplete`), and
  all spec quotes here are from the primary 3.18 source file `language/completion.md` (URL in §2.2 /
  method header). Remaining sub-question: `research/24-part-decomposition.md`'s capability list mentions
  `documentationMarkdown`, which does **not** appear anywhere in the 3.18 completion source or in ls-types —
  **[question]** where that name came from (unit 8 should not cite it without a source).
- **[q6]** `tower-lsp-server`'s `proposed` feature is unbuildable against current `ls-types` (§2.0 probe);
  confirmed inert-but-broken, no Traces impact unless a pin to `ls-types =0.0.3` is ever chosen — flag to
  ticket 09/29 only if `--all-features` CI builds are planned.
