# Ticket 24: Zed completion client reality check (Q1–Q7)

Unit: cross-cutting verification pass for ticket 24 parts (trigger/rumdl unit, item-shape unit,
resolve unit, capability unit). Purpose: replace "how Zed probably behaves" with verified
`file:line` facts from Zed's own source, and mark where ticket-24 decisions are confirmed,
qualified, or weakened.

- **Source**: <https://github.com/zed-industries/zed>, branch `main`, all files fetched
  **2026-10-07** (raw.githubusercontent.com) into a local read-only snapshot. No commits, no clone.
- **Citations**: upstream-relative paths + line numbers against that 2026-10-07 snapshot, e.g.
  `crates/editor/src/completions.rs:506-516`.
- **Legend**: **V** = verified against the fetched source during this pass; **R** = carried from an
  earlier ticket-24 unit, not re-read this pass; **[question]** = still open.

---

## Q1 — When several servers can complete at one position, does Zed query each for every registered trigger character, or only the matching provider?

**Answer: it queries every capable server for every gated keystroke. Zed has no per-character
provider filter (no VS Code-style `providerFilter`). The character gate only decides *whether any
request happens at all*; once it passes, the request fans out to all servers attached to the buffer
that pass capability + language-scope checks. [V]**

Mechanism, in order:

1. **Typed-char entry (only path for typing).** `Editor::handle_input`
   (`crates/editor/src/input.rs:72`) suppresses selection side-effects during the edit
   (`change_selections(... .completions(false))`, `input.rs:486` — this stops the menu logic in
   `selection.rs` from firing mid-typing) and then calls `trigger_completion_on_input`
   (`input.rs:530`), which forwards to `open_or_update_completions_menu`
   (`crates/editor/src/completions.rs:101-146`, `:321`).
2. **Character gate.** `load_provider_completions`
   (`crates/editor/src/completions.rs:506-516`) = provider exists **and**
   `is_completion_trigger(...)` (`completions.rs:507-515`). The LSP provider's implementation
   (`completions.rs:1513-1541`):
   - empty text → false (`:1521-1526`); **more than one char (paste) → false** (`:1527-1529`);
   - `trigger_in_words && classifier.is_word(char)` → true (`:1536-1538`);
   - else `buffer.completion_triggers().contains(text)` (`:1540`).
3. **The trigger set is a union.** `buffer.completion_triggers()` returns one `BTreeSet<String>`
   per buffer (`crates/language/src/buffer.rs:132`, union maintained by
   `set_completion_triggers` at `:3492-3514`, getter `:3525-3526`) — i.e. the union across all
   language servers attached to that buffer (**R**, unit 2's `trigger-set-union` rule).
4. **Fan-out has no character test.** Local path `LspStore::completions`
   (`crates/project/src/lsp_store.rs:7910-7941`) builds `server_ids` from
   `language_servers_for_buffer` (`lsp_store.rs:1477`) filtered by:
   - `completion_settings.lsp` gate (`lsp_store.rs:7915-7919`);
   - `lsp_command_allowed_for_buffer` (`lsp_store.rs:15049`) → `GetCompletions::check_capabilities`
     = `server_capabilities.completion_provider.is_some()` (`crates/project/src/lsp_command.rs:3173-3179`)
     — **capability only, no char filter**;
   - language scope: `scope.language_allowed(&adapter.name)` (`lsp_store.rs:7933-7937`).

   It then issues one `textDocument/completion` per server (`lsp_store.rs:7953-7995`). The
   upstream/collab path is identical in shape: `all_capable_for_proto_request` with the same
   capability + scope predicate (`lsp_store.rs:7830-7848`).
5. **Context per request.** `trigger_character` is set only if the typed char is in
   `buffer.completion_triggers()` (`completions.rs:521-528`); `trigger_kind` =
   `TRIGGER_CHARACTER` else `INVOKED` (`:531-536`); `to_lsp` always sends
   `context: Some(...)` (`lsp_command.rs:3181-3196`, `:3191`).

**Divergence from the VS Code model the ticket relies on**: VS Code char-gates *per provider*
(`providerFilter`); Zed char-gates the union once, then queries **all** capable servers. A
Traces-only char such as `[` therefore also reaches rumdl whenever rumdl is attached to that
buffer, scope-allowed, and completion-capable. See Impact #1/#2.

---

## Q2 — Do completions from multiple servers at one position produce one merged popup or two?

**Answer: one popup, always. All server responses merge into a single `CompletionsMenu`; two
completion popups are structurally impossible. There is, however, no LSP-vs-LSP deduplication, and
`is_incomplete` is OR-ed across servers. [V]**

- **Single menu slot.** The editor holds one `context_menu`; a finished query installs
  `Some(CodeContextMenu::Completions(menu))` (`crates/editor/src/completions.rs:787-788`), and
  `has_visible_completions_menu` is singular (`completions.rs:94-99`). Code-actions menus are
  evicted when completions open (`crates/editor/src/selection.rs:1619-1627`).
- **Merge order.** LSP responses from every server are concatenated first
  (`completions.rs:648-657`), then buffer words (`:664-687`), then snippets (`:689-694`); the menu
  is built only from the combined vector (`:696-731`).
- **`is_incomplete` = OR across all responses** (`completions.rs:650-651`) with an explicit TODO
  that any one incomplete source forces re-querying *all* sources
  (`completions.rs:640-641`). If rumdl ever returns `isIncomplete: true`, Traces gets re-queried on
  every subsequent keystroke too (and vice versa).
- **Dedup only at the seams**: buffer-word vs LSP by `new_text` (`completions.rs:668-670`) and
  snippet-prefix dedup by `snippet_deduplication_key`
  (`crates/editor/src/code_context_menus.rs:1465-1476`). **No dedup between two LSP servers' items
  — duplicates from different servers would both render.**
- **Fan-in**: the local path `join_all`s every server task before building the menu
  (`lsp_store.rs:8009-8011`); `lsp_fetch_timeout_ms` defaults to `0` = wait indefinitely
  (`assets/settings/default.json:2384`; timeout wiring `lsp_store.rs:7944-7951`). One slow server
  delays the whole popup (unless a positive timeout is configured).

---

## Q3 — Session mechanics: what opens, updates, re-queries, and closes the menu per keystroke?

**Answer: sessions open only on (a) manual invocation, (b) a union trigger char, (c) a word char —
never on deletion or bare cursor movement. A complete session filters locally without any server
round-trip; an incomplete (`isIncomplete: true`) session re-queries every gated keystroke, always
with `INVOKED` — Zed never sends trigger-kind 3. Typing a non-word, unregistered char (e.g. `|`)
closes the menu and sends nothing. Deletion closes the menu and sends nothing. [V]**

Entry points (complete list, verified by grep):

| Event | Path | Server request? |
| :--- | :--- | :--- |
| Typed char | `input.rs:72` → `trigger_completion_on_input` (`input.rs:530`, fn `completions.rs:101-146`) → `open_or_update_completions_menu` (`:321`) | Only if `is_completion_trigger` passes (`:506-516`) |
| Manual `ctrl-space` / `editor::ShowCompletions` | `completions.rs:36-42` (`open_or_update(None, None, false)`), documented `docs/src/completions.md:14-20` | **Always** — `trigger: None` passes the gate unconditionally (`:507`, `is_none_or`) |
| `show_word_completions` (words only) | `completions.rs:23-34` | No (provider replaced by `Words` source) |
| Cursor/selection change with menu open | `selection.rs:1631-1661` | Only in the narrow keep-menu branch (below) |
| Backspace / delete | `editor.rs:5236-5281` | **No** |

Behavior per scenario:

- **(a) Registered trigger char typed** (e.g. `[`, from the union): query →
  `TRIGGER_CHARACTER` context (`completions.rs:521-536`); menu opens/updates. Gated by the
  `show_completions_on_input` setting for typed triggers (`completions.rs:384-388`; default `true`
  `assets/settings/default.json:378`), **not** for manual invocation (no `trigger` → gate skipped).
- **(b) Word char typed**: gated by `trigger_in_words` (`input.rs:507-508` =
  `show_edit_predictions_in_menu() || !had_active_edit_prediction`) and the classifier; request
  carries `INVOKED` (`completions.rs:531-536`). No minimum-length gate for LSP items —
  `words_min_length` (3, `default.json:2375`) gates only buffer-word completions
  (`completions.rs:554-559`); Markdown buffer words are disabled by default
  (`default.json:2533-2541`).
- **(c) Unregistered non-word char typed (the `[[target|` case)**: two independent stops:
  1. `is_completion_trigger` = false (`completions.rs:1527-1541`) → no provider query
     (`:506-516`);
  2. if the cursor leaves a word, `completion_query` returns `None` (`completions.rs:833-845`,
     word logic `crates/language/src/buffer.rs:4478+`) → an open menu is **hidden**
     (`completions.rs:399-401`, the #32774 fix).
  Net: typing `|` after `[[target` sends **no request to any server** (Traces included) and closes
  the menu. This is the ticket's claimed behavior, confirmed.
- **(d) Keystroke while a session is open, `isIncomplete: false`**: the existing menu is first
  refiltered (`completions.rs:430-441` → `code_context_menus.rs:1335-1361`), then the early return
  applies when the new query extends the initial query (`:447-454`, `(None, _) => true` for
  trigger-opened sessions) **and** the cursor is still at the menu origin (`:455-462`) → **local
  filter only, zero server traffic**.
- **(e) Keystroke while a session is open, `isIncomplete: true`**: `was_complete` is false
  (`completions.rs:443`) → early return skipped → refilter **and re-query all servers**
  (`:506+`), context `INVOKED` for word chars / `TRIGGER_CHARACTER` for union chars. **Zed never
  sends `TriggerForIncompleteCompletions` (kind 3)**: the only `CompletionContext` construction
  site in the codebase is `completions.rs:531-536` (grep for the kind-3 constant across the
  snapshot: zero hits). Servers must treat an `INVOKED` request that follows an earlier completion
  as a possible follow-up.
- **(f) Cursor movement with menu open** (`selection.rs:1631-1661`, gated by
  `SelectionEffects.completions`, default `true` `editor.rs:1382-1390`, documented
  `editor.rs:1361-1372`): keep-menu only if the selection start equals the menu's
  `initial_position` **and** the char before the cursor is a word char
  (`selection.rs:1636-1654`) → then `open_or_update(None, ...)` refilters (`:1656`, early return
  usually applies → no request for complete sessions). Otherwise → `hide_context_menu` (`:1658`).
  Cursor movement **never opens** a session (no menu → `completion_position` is `None` → block
  skipped).
- **(g) Backspace / delete**: `Editor::backspace` (`editor.rs:5236-5281`) first calls
  `change_selections(Default::default())` (`editor.rs:5280`, completions side-effect **on**) with
  the cursor already moved left → cursor ≠ `initial_position` → menu hidden
  (`selection.rs:1658`). Then `insert("")` (`editor.rs:5281`) → `replace_selections`
  (`input.rs:1987-2036`, its own `change_selections` at `:2031`) finds no menu. **Net: deletion
  closes the menu; no LSP request is ever issued on deletion** (typed-char queries come only from
  `handle_input` → `input.rs:530`).
- **(h) Accept**: `do_completion` hides the menu first (`completions.rs:848-856`), applies the main
  edit synchronously (`process_completion_for_edit`, `editor.rs:11640+`), and spawns
  additional-edits work (below).

**[question]** (minor): range/multi-cursor selections can match `initial_position` via
`selection.start` while the head differs; the keep-menu branch's behavior there was not traced.

---

## Q4 — Does Zed filter and sort on the client, or trust the server's order?

**Answer: Zed fully owns filtering and sorting. Server item order is discarded (even for an empty
query). Matching runs against the LSP `filter_text` (fallback `label`); ranking is a derived-Ord
tier chain in which, inside the normal tier, fuzzy score sits strictly above `sortText`, and
`sortText` sits strictly above kind and label — but an entire "other" tier exists where `sortText`
authority is dropped. [V]**

Filtering (`crates/editor/src/code_context_menus.rs`):

- Candidates are built from `completion.filter_text()` at menu construction
  (`code_context_menus.rs:370-375`); `filter_text` = LSP `filterText` else `label`
  (`crates/project/src/project.rs:647-656`, `:7073-7077`).
- `fuzzy::match_strings` over those candidates with **smart case**
  (`code_context_menus.rs:1394-1404`), cap **1000** results (`:1400`). Empty query → all items
  pass with score 0 (`crates/fuzzy/src/strings.rs:133-143`).
- A second label-only pass runs when `filter_text ≠ label` so highlights land on the displayed
  text (`code_context_menus.rs:1409-1441`).

Sorting (`code_context_menus.rs:1455-1463` → `sort_string_matches` `:1516-1611`):

- Always on for LSP (`sort_completions` default `true`, `completions.rs:1186-1188`; flag wired at
  `:417-419`; no provider override exists in the snapshot).
- `MatchTier` (`code_context_menus.rs:1524-1539`), derived `Ord` ⇒ field order = precedence:

  | Rung | Field | Source | Line |
  | :--- | :--- | :--- | :--- |
  | 1 | `sort_exact` | `filter_text() == query` exact equality | `:1592-1597` |
  | 2 | `sort_snippet` | `snippet_sort_order` (default `"inline"` = no-op, `default.json:343`) | `:1580-1585` |
  | 3 | `sort_score` | fuzzy score (descending) | `:1561-1563` |
  | 4 | `sort_positions` | match positions | `:1586` |
  | 5 | `sort_exact_case_matches` | exact-case count | `:1587-1591` |
  | 6 | `sort_text` | **LSP `sortText`** (`lsp_completion.sort_text`) | `:1554-1559` |
  | 7 | `sort_kind` | kind rank: KEYWORD 0, VARIABLE 1, CONSTANT 2, PROPERTY 3, else 4 (`project.rs:7081-7095`) | `:1560-1562` |
  | 8 | `sort_label` | label text | `:1560-1562` |

- **The tier split (Zed-specific)**: if the query's first character starts **no** split word of an
  item's `filter_text` (`:1567-1575`; `split_words` = camelCase + non-alnum→alnum boundaries,
  `completions.rs:1548-1566`), the item falls to `OtherMatch { sort_score }` (`:1577-1579`) where
  **`sortText`, kind, and label are ignored entirely** — it is demoted below every word-start-matching
  item regardless of server ranking. With an **empty** query there is no first char → all items stay
  in `WordStartMatch` (`:1575` `.unwrap_or(false)`) → all scores 0 → **`sortText` → kind → label
  decides the whole menu** (ticket unit 3's empty-menu assumption, confirmed).
- `snippet_sort_order: "none"` removes snippets outright (`:1545-1549`).
- Missing `sortText` yields `None` in the key (`:1554-1559`), and `Option`'s `Ord` sorts `None`
  before `Some` — an item *without* `sortText` floats *above* items with one on otherwise-equal
  keys. Our 100%-`sortText` contract is the right posture.

Alignment with ticket unit 3 (S-2 / S-4 / S-5):

- **S-2 (score strictly above `sortText`) HOLDS** — rung 3 vs rung 6.
- **S-4 (`sortText` above kind above label) HOLDS** — rung 6/7/8, verified end-to-end including the
  empty-query case.
- **S-5 (filterText co-designed with the replace range) HOLDS with a refinement**: Zed's query is
  the **surrounding-word segment under the cursor** (`completions.rs:833-845` +
  `buffer.rs:4478+`), *not* `textEdit.range.start..cursor`. Consequences: (i) `filter_text` must
  allow that word segment as a subsequence (it does for C1–C8 shapes: the typed text is a suffix of
  our filterTexts); (ii) to keep `sortText` authority, a split word of `filterText` must start with
  the query's first character — `#parent/child` survives a `#`-led query because the first split
  chunk is `"#"`; (iii) `filterText` is optional (label fallback) but should always be set.

---

## Q5 — When does Zed call `completionItem/resolve`? What does it block?

**Answer: resolve fires in visible-window batches whenever the menu's selection changes — on menu
build, on every refilter (i.e. every keystroke while a menu is open), and on every selection change
within the menu. Batches are serial per item, run detached with no cross-batch cancellation, and
dedupe only via a per-item `resolved` flag. Accepting a completion never blocks on resolve: the
main edit lands first, additional edits + resolve are spawned. [V]**

- **Triggers** (all funnel through `handle_selection_changed`,
  `code_context_menus.rs:640-659` → `resolve_visible_completions` `:662`):
  - menu construction → `set_filter_results` (`completions.rs:774` →
    `code_context_menus.rs:1480-1514`, selection set at `:1512`);
  - **every refilter** — `menu.filter` spawns → `set_filter_results` (`code_context_menus.rs:1344-1359`);
  - every in-menu selection change (`update_selection_index` `:564-569`; nav fns
    `:499-556`).
  - Not hover- or tooltip-driven: `provider.selection_changed` is a no-op
    (`completions.rs:1184`); markdown parsing of already-resolved docs is local
    (`code_context_menus.rs:660`, `:767-776`).
- **Window**: visible count (≈12 fallback, `:693`; else last rendered range) expanded **±4**
  (`RESOLVE_BEFORE_ITEMS` `:68`, `RESOLVE_AFTER_ITEMS` `:69`, applied `:710-716`) ⇒ **up to ~20
  items per batch**. Items that already have documentation are skipped (`:719-723`); the selected
  item is always included (`:726-736`) "for out-of-spec servers".
- **Gating**: `resolve_completions` flag — `true` for every LSP-built menu
  (`code_context_menus.rs:396`), `false` only for snippet-choice menus (`:476`); checked at
  `:667-669`. Independent of `show_completion_documentation` (default `true`,
  `default.json:381`) — docs-off does **not** reduce resolve traffic.
- **Backend**: `LspStore::resolve_completions` (`lsp_store.rs:8018+`) selects servers with
  `resolveProvider` (`:8039` + `GetCompletions::can_resolve_completions`,
  `lsp_command.rs:3150-3157`) and runs a **serial** per-item loop; `resolve_completion_local`
  (`lsp_store.rs:8142-8300`) skips items whose `resolved` flag is set (`:8164-8166`, request at
  `:8170`), then fills documentation/detail/labels.
- **No cancellation**: the batch task is spawned detached (`code_context_menus.rs:743-768`);
  rapid navigation starts overlapping batches. The per-item `resolved` flag is the only
  dedupe — there is no request token/cancel against the server. **R** (confirming unit 4's model).
- **Accept path**: `do_completion` hides the menu first (`completions.rs:848-856`), applies the
  main edit immediately (`process_completion_for_edit`, `editor.rs:11640+`), then additional
  text edits run in a spawned task which resolves the item first if needed
  (`lsp_store.rs:8406-8486`, resolve at `:8479-8486`; spawn site `completions.rs` accept path,
  **R**). **Accept never waits on resolve.**

**Budget reading (D5)**: the worst-case burst is ~20 serial resolves at menu open, plus a few per
keystroke-refilter (only newly-visible, doc-less items), plus ~4–8 per navigation step. Server-side
resolve latency directly bounds each burst: 20 × 5 ms = 100 ms of serialized server work per menu
event (2 ms stretch ⇒ 40 ms). Because every keystroke while a menu is open triggers
`set_filter_results` → resolve, the ≤5 ms (stretch 2 ms) budget is load-bearing for *typing*, not
just navigation.

---

## Q6 — How tolerant is Zed of servers that decline (null) or return empty completions?

**Answer: fully tolerant. `null` → empty result for that server; empty results → no menu; per-server
errors/timeouts are dropped without affecting other servers; items with unclippable ranges are
dropped and force `isIncomplete: true` so the list is re-queried. [V]**

- `response_from_lsp(completions: Option<lsp::CompletionResponse>)`: `None` → `(Vec::new(), false)`
  (`lsp_command.rs:3197-3214`, the else arm at `:3213-3214`). Both `Array` and `List` variants
  accepted (`:3200-3211`).
- Per-server timeout (when `lsp_fetch_timeout_ms > 0`) → `Ok(None)` → dropped with a warning
  (`lsp_store.rs:7961-7996`); task error → `task.await.ok()??` → dropped (`:7997-8002`); responses
  flattened (`:8010-8011`) — **one server declining never affects another's items**.
- All servers empty → `completions.is_empty()` → no menu installed (`completions.rs:696-697`);
  empty match set → `menu.visible()` false → no menu installed (`completions.rs:779`,
  `:787-788`); the empty-result path hides an existing menu (`:802-807`).
- Range hygiene: `parse_completion_text_edit` clips both endpoints to the document; out-of-document
  ranges drop the whole item (`lsp_command.rs:3456-3499` → drop at `:3268-3273`); any such drop
  forces `is_incomplete = true` (`:3317-3321`) so a transiently-bad response is retried.
- **Range-less items are not dropped**: Zed infers a replace range from the surrounding word (or an
  empty range in whitespace) with insert range = start..cursor (`lsp_command.rs:3277-3303`) —
  a client-side fallback mirroring VS Code.

**Conclusion for the Traces design**: responding `null` for contexts you decline is safe in Zed (and
in VS Code); it degrades to "no items from that server" and never hides another server's popup.

---

## Q7 — Do Zed's advertised completion capabilities still match unit 8's table?

**Answer: yes — unit 8 §6's Zed column re-verifies row-for-row against the 2026-10-07 snapshot.
Two cite drifts, plus three advertised-but-unconsumed fields worth recording. (Spec demands live in
[24-completion-capability-negotiation](24-completion-capability-negotiation.md); this section is
Zed behavior only.) [V]**

Verified `initialize` capabilities (`crates/lsp/src/lsp.rs`):

| Row | Zed advertises | Line(s) | Consumed? |
| :--- | :--- | :--- | :--- |
| position encodings | **UTF-16 only** | `:889-891` | yes |
| `snippetSupport` | `true` | `:978` | yes — `insert_text_format == SNIPPET` (`project.rs:7106-7111`) |
| `resolveSupport.properties` | `additionalTextEdits`, `command`, `detail`, `documentation` | `:979-989` (**cite drift**: was `979-987` in unit 8; the "NB: Do not have this resolved…" comment is `:985-986`, `textEdit` deliberately commented out `:987`) | yes |
| `deprecatedSupport` | `true` | `:990` | (not re-checked) |
| `tagSupport` | `[DEPRECATED]` | `:991-994` | (not re-checked) |
| `insertReplaceSupport` | `true` | `:995` | yes — `InsertReplaceEdit` parsed (`lsp_command.rs:3250-3263`, `:3460-3466`); insert-vs-replace applied at accept via `lsp_insert_mode` (`editor.rs:11697-11760`) |
| `labelDetailsSupport` | `true` | `:996` | **NO — never read** (grep: only the snippet path `completions.rs:1411` and a sanitize doc-comment `lsp_store.rs:16785-16787`; no LSP `label_details` consumption anywhere) |
| `insertTextModeSupport` | `[AS_IS, ADJUST_INDENTATION]` | `:997-1002` | yes (item-level `insert_text_mode`, `lsp_store.rs:15753`) |
| `documentationFormat` | `[Markdown, PlainText]` | `:1003-1006` | yes |
| item-level `insertTextMode` | `ADJUST_INDENTATION` | `:1007` | yes |
| `completionList.itemDefaults` | `commitCharacters`, `editRange`, `insertTextMode`, `insertTextFormat`, `data` | `:1008-1015` | **partially** — only `editRange` (`lsp_command.rs:3236-3264`) and `data` (`:3332-3340`) are consumed; `insertTextFormat`/`insertTextMode` from `itemDefaults` are not hoisted (only item-level fields are read: `project.rs:7111`, `lsp_store.rs:15753`); `commitCharacters` **never read anywhere** |
| `contextSupport` | `true` | `:1017` | yes — context always sent (`lsp_command.rs:3191`) |
| `dynamicRegistration` | `true` | `:1018` | (not re-checked) |
| `preselectSupport` | **absent** (`.default()` at `:1005`, `:1019`) | — | **never read** (grep across completion files: zero hits) |
| `commitCharactersSupport` | **absent** | — | never read |
| `completionItemKind.valueSet` | **absent** | — | kind used only for icon + sort rank (`project.rs:7081-7095`) |

Two deltas to unit 8 as written: `resolveSupport` now spans `:979-989` (comment/textEdit lines),
and `itemDefaults` is advertised as five keys but the client honors two. Both **widen** our
sparse-client floor: nothing in Zed's behavior requires us to serve `labelDetails`, `commitCharacters`,
`preselect`, or `itemDefaults` keys beyond `editRange` + `data`.

---

## Impact on ticket-24 decisions

1. **Unit 2 — trigger set + session-opening rule (union gate, per-server trigger sets)**
   → **HOLDS-WITH-CAVEAT**. Opening paths verified exactly as modeled: manual, union trigger char,
   word char; deletion/cursor movement never open a session. Caveats: (a) the union gate is
   *global to the buffer*, not per-provider — Zed then fans out to **every** capable server
   (`lsp_store.rs:7910-7941`), so our session-opening rule must be enforced by *response* (what we
   choose to answer), not by expecting the client to route triggers selectively; (b) `|` /
   unregistered non-word chars verifiably produce no request and close the menu
   (`completions.rs:399-401`, `:506-516`); (c) multi-char input (paste) never queries
   (`completions.rs:1527-1529`).
2. **Unit 2 — benign rumdl-collision claim**
   → **HOLDS**, with the mechanism now precise: rumdl receives Traces-triggered queries (and
   Traces receives rumdl-triggered ones) purely because of union-gate + capability fan-out. It is
   benign for correctness (one menu, disjoint-ish shapes, `null`-tolerant client per Q6) but it is
   real *extra request volume* on rumdl for every gated keystroke whenever both servers are attached
   — rumdl must answer positions it doesn't own with `null`/empty, never errors. Additional
   coupling found: `isIncomplete` is OR-ed across servers (`completions.rs:650-651`, TODO `:640-641`),
   so **one** server returning `isIncomplete: true` forces full re-query of *both* servers every
   keystroke — recommend an explicit `isIncomplete: false` discipline for both servers.
3. **Unit 3 — sortText/filterText contract (S-2 score>sortText, S-4 sortText>kind>label, S-5
   filterText↔range co-design)**
   → **HOLDS-WITH-CAVEAT**. S-2 and S-4 verified exactly (tier rungs 3/6/7/8), including the
   empty-query case where `sortText`→kind→label fully orders the menu (`strings.rs:133-143` +
   `code_context_menus.rs:1516-1611`). New clauses to document: (a) Zed adds two rungs *above*
   score (exact `filter_text==query`, snippet-setting) and two *between* score and `sortText`
   (positions, exact-case) — these only promote correct items, so the contract survives; (b) the
   `OtherMatch` demotion (`:1567-1579`) voids `sortText` authority entirely when no split word of
   `filterText` starts with the query's first char — filterTexts must keep a split word aligned with
   the typed prefix (true for C1–C8; re-check for any future shape where the typed segment starts
   mid-word-of-filterText); (c) Zed's query is the cursor's surrounding-word segment, not
   `range.start..cursor` — suffix-style filterTexts are required (they already are); (d) a missing
   `sortText` sorts *above* provided ones — keep the 100%-set guarantee.
4. **Unit 4 — resolve split (detail deferred) + D5 ≤5 ms (stretch 2 ms) budget**
   → **HOLDS-WITH-CAVEAT**. The split matches Zed's consumption (`detail` + `documentation` in
   `resolveSupport`, accept never blocks). Storm model sharpened: batches of up to ~20 serial
   resolves fire at menu open **and at every keystroke-refilter and selection change** (only
   doc-less items + always-selected), detached, uncancellable across batches, deduped only per item
   (`code_context_menus.rs:662-745`, `lsp_store.rs:8164-8166`); docs-off does not reduce them.
   The D5 budget is therefore on the typing path, not just navigation: 20×5 ms = 100 ms serialized
   server work per menu event at budget. Also: Zed never sends trigger-kind 3 — servers will see
   `INVOKED` follow-ups, so unit-1's "ignore triggerKind" stance is vindicated, with the note that
   a follow-up `INVOKED` at the same position may be a re-query.
5. **Unit 8 — sparse-client floor (capability table)**
   → **HOLDS**, row-for-row (Q7 table). Corrections: `resolveSupport` cite → `lsp.rs:979-989`;
   add three "advertised but not consumed" facts (`labelDetails`, `itemDefaults` beyond
   `editRange`/`data`, and `commitCharacters` advertised inside `itemDefaults` yet never read) and
   two "absent and never read" facts (`preselect`, `commitCharactersSupport`) — all of which let us
   serve a *smaller* surface than Zed advertises without behavior loss.

---

## Questions: closed / newly opened

| Source | Question | Status |
| :--- | :--- | :--- |
| Unit 2 Q1 | Who gets queried on each trigger char — per-provider or fan-out? | **CLOSED** — union gate, then fan-out to all capability+scope-passing servers (`lsp_store.rs:7910-7941`); no per-char provider filter exists |
| Unit 2 Q2 | One popup or two with two servers? | **CLOSED** — always one merged `CompletionsMenu`; no LSP-vs-LSP dedup; `isIncomplete` OR-ed |
| Unit 2 Q7 | Per-keystroke session mechanics (open/update/close, backspace, `\|`) | **CLOSED** — Q3 table; incl. no kind-3, deletion closes silently, `\|` sends nothing |
| Unit 4 Q3 | When does resolve fire, and what does it block? | **CLOSED** — selection-window batches (open/refilter/selection change), never blocks accept; budget note above |
| Unit 3 S-5 | Does Zed's filter word equal `range.start..cursor`? | **CLOSED, refined** — it equals the cursor's surrounding-word segment (`completions.rs:833-845`); suffix filterTexts survive |
| Unit 8 §6 | Does the Zed capability column still hold? | **CLOSED** — row-for-row confirmed; cite drift `resolveSupport:979-989`; `itemDefaults` consumed-subset noted |
| This pass | Keep-menu branch under multi-cursor / range selections | **OPEN** (minor; no protocol impact) |
| This pass | Mouse-hover → selection mapping inside the menu (element code not traced) — affects resolve-trigger completeness only | **OPEN** (minor) |
| This pass | Edit-prediction menu interplay (`show_edit_predictions_in_menu`) when both features are active | **OPEN** (out of scope for ticket 24) |
