# Completion latency architecture — stage-by-stage budget for the per-request pipeline

Companion to [ticket 24 (completion architecture)](../../issues/24-completion-architecture.md): this unit does
*not* decide context-detection strategy, trigger characters, resolve, or snippets (24 owns those) — it
supplies the **latency architecture** those decisions must fit inside: where milliseconds may be spent per
completion request, what must be precomputed instead, and how the pipeline stays responsive under
sequential dispatch.

Sources: local repo (`src/`, `benches/`, this effort's tickets/research) marked with `file:line`;
ecosystem prior art marked with URL. Claims I could not verify are marked **QUESTION**.

---

## 1. Substrate: the four constraints that bound the budget

| Constraint | Source | Consequence for completion |
|---|---|---|
| **p95 < 20 ms** per completion request (stretch < 10 ms) | ticket 33:50 | One wall-clock budget for *everything* the handler does — context detection, candidate generation, scoring, serialization. |
| `concurrency_level(1)` — one handler future polled at a time; sync handlers block the pipeline; `$/cancelRequest` implicitly disabled, stale results discarded on completion | ticket 12:20-36 | No parallel request handling; a slow completion directly delays the next `didChange`/hover. Superseded requests still run to completion (wasted work is a *budget* concern, not a correctness one). |
| Re-parse per edit: overlay `note_of()` parses buffer content on demand, full re-parse, no incremental parsing | ticket 14:23-27 | Reading the document at completion time is not free: typically ~0.02 ms/10 KB, but see the long-line anomaly below. |
| Refresh + queries never overlap; `Arc<WorkspaceIndex>` swap happens between handlers; index is immutable | ticket 12:40-44 | Completion always sees a consistent snapshot; anything expensive can be built *during* refresh, never mid-request. |

**Long-line anomaly (measured):** `parse_markdown__line_density/4000` = **18.6 ms on a 51.2 KB fixture**
(4,000-*character* lines) vs the <10 ms single-file reparse target; realistic 4,000-line notes ≈ 2–6 ms
(ticket 33:118, bench self-flags it at `benches/note_parsing.rs:470-476`). A single pathological open note
can eat nearly the whole 20 ms budget *if* it is re-parsed inside the completion handler.

---

## 2. Measured local facts (file:line / ticket)

**Cost hot spots already identified** (from `research/24-part-codebase.md:310-316`):

- **Per-request vault-wide scan** — `matched_file_rows` is `(0..index.entries().len())` + filter
  (`src/query/service.rs:305-316`; also cited at `:132-139` by ticket 12:15 and `map.md:33`). redb
  multimap enumeration (`src/index/store.rs:1219`) has the same shape. *Must not* appear in any completion
  path — completion needs ticket 21's `unique_*` primitives / ticket 33's inverted indexes instead.
- **Per-request overlay re-parse** of the edited note (ticket 14:23) — the one unavoidable per-keystroke
  cost unless memoized per `(uri, version)` (see §7-Q4).
- **Everything else resident** — schema, basename, status-map, template-list are hash-map lookups
  comfortably inside 20 ms (`24-part-codebase.md:314-316`).

**Structures that already exist and are cheap:**

- `WorkspaceIndex` holds every file's parsed `Note` in memory — `FileEntry { file, note: Option<Box<Note>>, inlinks }`
  (`src/index/entry.rs:134-138`); `Note` carries `headings: Box<[Heading]>`, `tags`, `inline_fields`
  (`src/note/model.rs:23-32`). Candidate data is *present*; the risk is **deriving a universe from it
  per request** (O(vault)), not finding it.
- `LineIndex`: O(log n) `line_at` via `partition_point` (`src/position.rs:31-56`), precomputed once per
  document (`:29-30`).
- Task-marker context: `scan_marker_prefix` is a buffer-local, O(line) classifier
  (`src/note/parser/marker.rs:86-113`) — the model for ticket 23's `- [` completion predicate.
- redb already persists `PATHS_BY_TAG`, `PATHS_BY_FILE_CLASS`, `TAGS_BY_PATH` multimaps
  (`src/index/tables.rs:40-68`) — tag/class *lookups* are index-backed; a unique-tag **list** does not
  exist yet (`IndexStore::unique_tags` is a ~30 LOC pre-condition of ticket 21:51, unbuilt).
- Schema: `Schema::fields()` is an `IndexMap` accessor (`src/schema/model.rs:71`); `SchemaService` is
  load-once/in-memory — "Layers 1+2 are O(1)… completion budget <20ms per ticket 33" (ticket 19:50).

**Fuzzy scoring cost** (from `research/24-part-crates.md` §4):

- `fuzzy-matcher` skim V2 builds a Smith-Waterman-style matrix per (choice, pattern) with early-outs
  (`24-part-crates.md:575-577`).
- External skim benchmark: full **100k-item miss ≈ 17 ms**; at **10⁴ items ≈ 1.7 ms** worst case — inside
  20 ms **only if the candidate list is built once and filtered once per request**
  (`24-part-crates.md:578-581`).
- Empty pattern returns `Some` — "just typed `[`" matches everything, all scores tie, ordering falls back
  to `sort_text`/insertion order (`24-part-crates.md:570`). Design consequence: the *first* keystroke of a
  context must return a **pre-truncated, pre-ordered** universe, not a scored-everything list.
- Serialization may dominate scoring: "the whole request … shares the 20 ms. The serialization of
  thousands of `CompletionItem`s may dominate scoring" (`24-part-crates.md:586-588`).
- `CompletionList.itemDefaults` is **absent** from `ls-types`' `CompletionList` (only `{is_incomplete,
  items}`) even though the client capability exists — cannot shave per-item repetition through the typed
  API without hand-serialization (`24-part-crates.md:450`).
- `completionItem/resolve` serialization sits behind the same sequential gate: "a slow resolve delays the
  *next*" request (`24-part-crates.md:166`).

**Schema/first-line previews:** computed during `IndexerService::refresh`, stored in an in-memory
`HashMap` on the analysis host, not redb (ticket 19:76) — precedent for refresh-time derived artifacts.

---

## 3. Ecosystem prior art — how other servers hold a budget (URLs)

### gopls — the only server with an explicit, documented completion *budget*

- Setting doc: *"CompletionBudget is the soft latency goal for completion requests. Most requests finish
  in a couple milliseconds, but in some cases deep completions can take much longer. **As we use up our
  budget we dynamically reduce the search scope** to ensure we return timely results. Zero means
  unlimited."* — `gopls/internal/settings/settings.go:394-399`
  (https://github.com/golang/tools/blob/master/gopls/internal/settings/settings.go)
- Default: **`CompletionBudget: 100 * time.Millisecond`** — `gopls/internal/settings/default.go:110`
  (https://github.com/golang/tools/blob/master/gopls/internal/settings/default.go)
- Mechanism (`gopls/internal/golang/completion/completion.go`,
  https://github.com/golang/tools/blob/master/gopls/internal/golang/completion/completion.go):
  - `budget` field on the completion object (`:126`), populated from `opts.CompletionBudget` (`:645`).
  - **Deadline is relative to the search start, not the whole RPC** — explicitly *excluded* type-checking
    time from the budget because including it "leads to inconsistent results" (`:662-675`).
  - Deadline is **not** stuffed into the context, deliberately separating "user cancelled" (= fail) from
    "our time limit" (= stop searching, succeed with partial results) (`:669-674`).
  - `deepSearch(ctx, …, deadline)` runs candidate-expansion passes gated by the deadline (`:692`, `:715`).
  - Expensive callbacks (goimports etc.) only run `if deadline == nil || time.Now().Before(*deadline)`
    (`:704`).
  - After a minimal valid candidate set exists, it wraps remaining work in `context.WithTimeout(…,
    time.Until(startTime.Add(budget)))` so it returns "**as close to the completion budget as possible**"
    (`:695-701`) — i.e. it intentionally *pads* toward the budget for latency consistency rather than
    returning as fast as it can.

### rust-analyzer — cancellation + two-phase structure

- All `Analysis` APIs return `Cancellable<T>`; a long-running computation is cancelled (returns
  `Err(Cancelled)`), the change applied, the new request run — *"We never use stale data to answer
  requests"*; `apply_change` cancels all outstanding `Analysis`es, blocks until dropped, then applies
  in-place (https://rust-analyzer.github.io/book/contributing/guide.html).
- Completion itself is staged: (1) build `CompletionContext` — including a re-parse with a dummy
  identifier at the cursor ("IntelliJ Trick") plus classification routines; (2) run "a series of
  independent completion routines" (`ide-completion/src/lib.rs`) — i.e. context construction is separated
  from candidate emission, exactly the two stages budgeted separately in §4.
- Cancellation point: the dispatch layer catches `Cancelled` right after completion if the client sent a
  modification meanwhile (guide, "Tying it all together: completion").
- Goal context: sub-100 ms autocomplete, typical 50–200 ms (ticket 33's rust-analyzer section, #17491) —
  Traces' 20 ms is an order of magnitude stricter than RA's own goal.

### tsserver — resolve-phase work and the auto-import lesson

- `completionInfo` (fast list) → `completionEntryDetails` (expensive detail) is the canonical two-phase
  split; VS Code's completion path fills details/caches behind `cacheId`
  (`typescript-language-server/src/completion.ts`; cited in `research/x-lsp-performance-best-practices.md:160`).
- Real-world failure mode: a single `completionInfo` measured **1271 ms**, with maintainers pointing at
  auto-import as the feature to disable to isolate it — expensive *enrichment* inside the initial list is
  what blows budgets (https://github.com/microsoft/TypeScript/issues/46838).
- tsserver's checker recomputes from scratch as you type but "only requests information about what you're
  typing" — laziness, not precompute, keeps it fast (https://github.com/microsoft/TypeScript/wiki/Performance).

### Pylance — precompute at index time, with a *cap*

- *"Indexing pre-parses workspace files and library packages to enable fast auto-imports, workspace symbol
  search, and completions. The trade-off is upfront CPU and memory usage"* — bounded by
  `python.analysis.userFileIndexingLimit` (default **2000** files) and `packageIndexDepths`
  (https://github.com/microsoft/pylance-release/blob/main/docs/howto/performance-tuning.md).
- The relevant pattern: **completion universes are built by a background indexer with an explicit budget,
  never on the request path.**

### Microsoft markdown-language-service — scope-gating and line-local classification

(local digest `docs/digests/lsp_microsoft-markdown-language-service*.txt`, corroborated by
`research/tool-ms-markdown-language-service.md`)

- Cursor context is classified by **line-local regexes** — no document re-parse per completion.
- Directory path suggestions are scoped to `#workspace.readDirectory(parentDir)` — O(directory), not O(vault).
- Workspace header suggestions are **gated behind `##`** (`IncludeWorkspaceHeaderCompletions`) — the
  expensive universe is only offered when the user's syntax proves intent.
- Per-document `TableOfContents` cached in `MdWorkspaceInfoCache`; `token.isCancellationRequested` checked
  between stages; directory expansion deferred to `editor.action.triggerSuggest` (progressive disclosure).

### Markdown Oxide — the anti-pattern (and the useful precedent)

(digest `docs/digests/lsp_feel-ix-343-markdown-oxide-src-digest.txt`)

- `link_completer.rs` / `tag_completer.rs` call `vault.select_referenceable_nodes(None)` — an **O(vault)
  collect on every completion request** (digest lines ~3836, ~5289), then rayon-parallel fuzzy filtering.
  It works for small vaults; it is exactly the shape ticket 33:50 says not to ship.
- Useful precedent: it logs `"Completions Done took {}ms"` (digest line ~299) — per-request timing in
  production — and it parallelizes *inside* the handler with rayon (see §6-4).

### zk / Marksman — precomputed collections; per-candidate item construction

- zk builds `buildTagCompletionList` from `notebook.FindCollections(CollectionKindTag, nil)` — a
  **precomputed collection with note counts** (digest, `research/tool-zk.md`) — and keeps separate
  *invoked* vs *trigger-character* lists (do less when the user didn't explicitly ask).
- Marksman registers both `textDocument/completion` and `completionItem/resolve`
  (digest line ~3161-3162); `Completions.wikiDoc` maps **one `CompletionItem` per candidate document** —
  item construction cost is O(vault docs) unless capped upstream (digest ~4751-4790).

---

## 4. Stage-by-stage latency budget

Per request, in execution order under `concurrency_level(1)`. "Target" is p95 inside ticket 33's 20 ms;
typical-case sum ≈ **11 ms**, leaving ~9 ms slack for allocation/GC noise, contention with the client, and
the degenerate cases in §7. The stretch goal (<10 ms) is reachable only with the serialization caps in
S7/S8 (i.e. ≤ ~200 items in the initial list) — flagged in §7-Q2.

| # | Stage | Precompute vs per-request | Target | Mechanism / evidence |
|---|---|---|---|---|
| S0 | Dispatch/queue wait | Per-request (queue) | not in handler budget; drains at ≤ 20 ms → ≥ 50 req/s | ticket 12:20-36; superseded requests still execute, then discard (ticket 12:36) |
| S1 | Content access (`ContentResolver::note_of`/`text_of`) | **Per-request, must memoize per `(uri, version)`** | ≤ 1 ms typical | overlay full re-parse (ticket 14:23-27); ~0.02 ms/10 KB typical, **18.6 ms/51.2 KB** long-line worst case (ticket 33:118) → memoization is the mitigation, see §7-Q4 |
| S2 | Context detection: cursor → which completer | Per-request, **line-local / O(log n)** | ≤ 1 ms (degenerate "not a context" path ≤ 0.3 ms) | `LineIndex::line_at` O(log n) (`src/position.rs:56`); `scan_marker_prefix` O(line) (`src/note/parser/marker.rs:86`); frontmatter/inline-field regions need ticket 11 spans; **flag: ticket 21's `{{`/`}}` depth counter is O(document)** (ticket 21:24) — see §7-Q3 |
| S3 | Candidate-universe selection | **Precomputed — O(1) map/vec handoff** | ≤ 0.5 ms | schema fields (`Schema::fields()` `src/schema/model.rs:71`); tag/class universes from refresh-built `unique_*` (ticket 21:51) / redb `PATHS_BY_TAG` (`src/index/tables.rs:40`); note-path universe prebuilt from `WorkspaceIndex::entries()`; vault heading universe refresh-built (ticket 15); `VaultFieldIndex` pre-built in refresh (ticket 19:50); template/helper namespaces from ticket 22's memo; first-line previews refresh-built (ticket 19:76). **Never** `matched_file_rows`-style scans (`src/query/service.rs:305-316`) |
| S4 | Prefix filter + fuzzy score | Per-request, bounded | ≤ 3 ms | skim V2 ≈ 1.7 ms @10⁴ worst case (`24-part-crates.md:578-581`); empty-pattern tie behavior means ordering must come from precomputed `sort_text` on first keystroke (`:570`); cap evaluated candidates (§6-5) |
| S5 | `CompletionItem` construction | Per-request over **top-K only** | ≤ 1.5 ms (≤ 200 items) | one item per survivor; details/docs pushed to `completionItem/resolve` (ticket 24:13; tsserver `completionEntryDetails` precedent §3); Marksman's per-doc mapping (§3) shows the uncapped O(vault) shape to avoid |
| S6 | Range mapping byte → UTF-16 | Per-request, O(log n) per position | ≤ 0.5 ms | `LineIndex` (`src/position.rs:31-56`); **`byte_to_utf16_cu` does not exist yet** — `rg utf16 src/` is empty (`24-part-codebase.md:88`); cost is 2×O(log n) per item's range |
| S7 | Sort + cap (`maxItems`) | Per-request | ≤ 0.5 ms | O(K log K); the cap is also S8's throttle; zero-padded `sortText` + `isIncomplete` pattern if server-side ordering is wanted (`24-part-crates.md`, RA issue #7935) |
| S8 | Serialize response (serde_json → JSON-RPC) | Per-request | ≤ 3 ms | "may dominate scoring" (`24-part-crates.md:586-588`); `itemDefaults` unavailable through typed `ls-types` (`:450`); thousands of items ≈ out of budget → cap, don't optimize |
| S9 | Write back / stale discard | Per-request | ~0 (cheap) | ticket 12:36 stale-result discard; wasted S1–S8 work for superseded requests is the HOL tax measured in §7-Q5 |

**Degenerate path (most keystrokes):** cursor not in any completion context → S2 early-exit → empty list.
This must be O(line) and never touch S3–S8; it is the common case and the reason scope-gating (§6-4) matters
more than raw scoring speed.

---

## 5. Precomputation inventory — what is built when

| Artifact | Built at | Invalidated at | Feeds |
|---|---|---|---|
| `WorkspaceIndex` (all `Note`s, `inlinks`) | refresh (rayon parse + redb persist + `Arc` swap) | `didChangeWatchedFiles` refresh cycle (ticket 12:40-44) | S3 note/heading/tag candidate data |
| Unique tag list + counts, unique file-class list (`IndexStore::unique_tags`, `unique_file_classes`) | refresh (ticket 21 pre-conditions, ~50 LOC total) | refresh swap | S3 tag/class universes; hover counts (ticket 21:66-70) |
| `VaultFieldIndex` (vault-wide field-key index) | `IndexerService::refresh` (ticket 19:50) | refresh swap | S3 inline-field value universes |
| First-line previews for hover/completion details | refresh (ticket 19:76) | refresh swap | S5 `detail`, or `resolve` |
| Schema registry (`SchemaService`, `extends` topo-sorted) | load-once at initialize | config/schema reload (ticket 22's caller-side `clear_templates()` pattern) | S3 O(1) field/class universes |
| Template/helper-namespace metadata (ticket 22 static analysis) | initialize eager attempt (ticket 22 Q23(a′)); memo in ticket 33's LRU tier | `clear_templates()` caller-side invalidation | S3 query/helper symbol universes |
| Note-path display universe (`Vec<Arc<str>>` sorted, pre-slugified) | refresh | refresh swap | S3/S5 wikilink targets (avoids per-item `Doc.pathFromRoot`-style formatting — Marksman §3) |
| Per-document `LineIndex` + span table | parse of that document (S1) | document version bump | S2/S6 |
| Query-context detection state (ticket 21 depth counter) | **currently per-request O(N)** — candidate for per-`(uri, version)` memo | version bump | S2 |

Rules implied by the table: (a) anything O(vault) is built during refresh or initialize — never inside the
handler; (b) every artifact has a named invalidation point (refresh swap / version bump / `clear_`),
because `WorkspaceIndex` is immutable and swapped between handlers (ticket 12:44) — no locks needed;
(c) LRU-tiered memos (ticket 33) may hold *recomputable* derivations, never the only copy of a universe.

---

## 6. Head-of-line-blocking mitigations under `concurrency_level(1)`

With one handler at a time, completion latency *is* the pipeline latency: a 50 ms completion delays the
next `didChange`, hover, and diagnostics. Mitigations, in order of leverage:

1. **Hard per-stage wall-clock deadline inside the handler (gopls pattern).** Check `Instant` between
   S2→S3, S4→S5, S7→S8; on overrun, *degrade gracefully* (return the partial, pre-ordered top-K; skip
   enrichment callbacks) rather than fail. Keep the deadline out of the cancellation context — "user
   cancelled" and "budget expired" are different outcomes (gopls `completion.go:669-674`, §3). Do **not**
   adopt gopls's sleep-to-budget latency padding (§3) — it spends time to smooth p50, which is backwards
   for a 20 ms budget.
2. **Build during idle, serve from memory.** All §5 artifacts are produced in refresh/initialize, which
   already run exclusively between handlers (ticket 12:40-44). The completion handler's only
   vault-wide-shaped work is *filtering a prebuilt vec* (S4), never *building one*.
3. **Scope-gate by user intent (MS-MLS `##` pattern, zk invoked-vs-trigger).** Only run an expensive
   completer when trigger characters/`triggerKind: Invoked` prove intent; otherwise S2 early-exits in
   ≤ 0.3 ms. This makes the *majority* of keystrokes near-free and reserves S3–S8 for real completion
   contexts — the single biggest HOL lever.
4. **rayon *inside* the handler? Measure first, probably don't.** Markdown Oxide parallelizes fuzzy
   filtering with rayon (§3) and `rayon` is already a dependency (`map.md` ground facts; parallel
   refresh/parse precedent `src/index/service.rs`). Under sequential dispatch, in-handler rayon still
   overlaps cores — but at 10⁴ candidates ≈ 1.7 ms single-threaded (§2), scoring is not the bottleneck;
   serialization (S8) is. Rayon adds contention with nothing to win. **QUESTION → §7-Q6.**
5. **Bounded candidate evaluation (gopls deep-search deadline, pylance index cap).** Cap S4's evaluated
   set (e.g. ≤ 10⁴) and S5/S7's emitted set (`maxItems`, default ≤ 200 for the initial list). The cap is
   also what keeps S8 inside 3 ms and makes `isIncomplete: true` re-requests the client's problem, not
   ours.
6. **Keep per-request cost well under the drain rate.** p95 20 ms ⇒ ≥ 50 requests/s drained sequentially;
   a fast typist issues ~5–10 completions/s. The margin only holds if *no* stage has an unbounded tail —
   which is why S1's pathological 18.6 ms parse (§1) and S2's O(N) counter (§7-Q3) are the two named
   tail-risks, not the median path.
7. **Stale-result waste accounting.** Superseded requests run to completion and are discarded (ticket
   12:36) — there is no mid-execution cancellation to lean on (ticket 12:15). Cheap requests are the only
   mitigation; measure the wasted-work fraction (§7-Q5) and, if it's material, consider version-stamping
   checks at the S2/S3 boundary as a cheap early-out (still no mid-loop cancellation machinery).
8. **`ContentModified` on mid-handler document change** (ticket 12:36) so a stale S1 snapshot never ships
   as a valid result.

---

## 7. Open questions & measurements the implementation spec must add

Budget work the spec owes — none of these can be closed from the planning corpus alone:

- **Q1 (required): per-stage instrumentation in the completion handler.** `Instant` spans for S1–S8,
  reported p50/p95 at 1K/5K/10K/20K-file synthetic vaults, wired into ticket 33's `lsp_latency` harness
  (ticket 33:100 defines the groups; completion currently has no code path to measure — `src/` contains
  none). Without this the 20 ms is an assertion, not a budget.
- **Q2: serialization vs scoring split.** Isolate S8 from S4 (serialize the same item list with scoring
  stubbed) — `24-part-crates.md:586-588` flags serialization as a *possible* dominator but nobody has
  measured it. Directly determines whether the <10 ms stretch target is reachable and whether hand-serializing
  `itemDefaults` (blocked in typed API, `:450`) is worth it.
- **Q3: ticket 21's O(document) `{{`/`}}` depth counter at completion frequency.** Cost on a 500 KB note?
  If > ~1 ms, memo per `(uri, version)` (it can't change without a version bump). Ticket 21:24 states
  O(N) but no measurement.
- **Q4: overlay `note_of()` memoization per `(uri, version)`.** Today: parse-on-demand per call
  (ticket 14:23-27). Worst case 18.6 ms/51.2 KB (§1) would blow S1 alone. Measure: completion frequency ×
  parse cost on long-line fixtures; if the tail is real, add a version-keyed memo (bounded, tiny — open
  files <100, ticket 14:23).
- **Q5: queue depth + discarded-request rate under fast typing.** What does tower-lsp-server's buffer do
  when requests arrive faster than 20 ms drain — unbounded queue, drop, or backpressure? **QUESTION**
  (not verified against tower-lsp-server source this session; ticket 09/12 territory). Measure wasted
   S1–S8 work on superseded requests.
- **Q6: in-handler rayon — worth it?** Benchmark single-thread vs rayon S4 at 10⁴ candidates before
   adopting Markdown Oxide's pattern (§6-4). Default: don't (YAGNI).
- **Q7: universe sizes at 20K files.** Tags/paths/headings/field-keys counts feed S4's O(K·P) math and
  memory residency; get real distributions (the §5 precompute trades memory for latency — record both).
- **Q8: trigger-character / rumdl overlap** is ticket 24's question, but it has a latency dimension:
  every extra trigger character fires S1–S8 on keystrokes that previously early-exited — audit the final
  trigger set (ticket 24:12) against the §6-3 gating design before locking it.
- **Q9: which contexts are worth the budget.** Cross-doc heading completion (S3 vault heading universe)
  is the most expensive universe to build and refresh; decide residency vs lazy redb enumeration with a
  cap once Q7 gives real numbers.

---

## 8. One-paragraph summary

The 20 ms budget is achievable **only if every O(vault)-shaped step is precomputed at refresh/initialize
(§5) and the handler is reduced to: line-local context classification (≤1 ms), O(1) universe handoff
(≤0.5 ms), bounded filter+score (≤3 ms), top-K item build (≤1.5 ms), O(log n) UTF-16 mapping (≤0.5 ms),
sort+cap (≤0.5 ms), capped serialization (≤3 ms)** — ~11 ms typical with slack, and a near-free early exit
for the majority of keystrokes that aren't in a completion context. Under `concurrency_level(1)` there is
no cancellation to hide behind, so the tail risks are named explicitly: the 18.6 ms long-line overlay
re-parse (S1), ticket 21's O(document) depth counter (S2), and unbounded response serialization (S8).
gopls supplies the deadline/degrade pattern, Pylance and MS-MLS supply the scope-gating and
index-time-precompute pattern, rust-analyzer and tsserver supply the two-phase (fast list → resolve)
split, and Markdown Oxide supplies the anti-pattern (O(vault) collect per request) to design against.

---

## Sources

**Local:** `src/query/service.rs:132-139,305-316`; `src/index/entry.rs:134-138`;
`src/index/tables.rs:40-68`; `src/note/model.rs:23-32`; `src/note/parser/marker.rs:86-113`;
`src/position.rs:29-56`; `src/schema/model.rs:71`; `benches/note_parsing.rs:460-480`;
tickets 12:15-56, 14:23-27, 19:50,76, 21:24,51, 22 (Q23, analysis memo), 23:24, 24:11-14, 33:39,50,100,118;
`research/24-part-codebase.md:74-90,310-316`; `research/24-part-crates.md:450,570-588`; `research/33-lsp-performance-targets.md`;
`research/x-lsp-performance-best-practices.md:160`; `map.md` ground facts; digests
`lsp_feel-ix-343-markdown-oxide-src-digest.txt`, `lsp_artempyanykh-marksman-digest.txt`, `tool-zk.md`,
`tool-ms-markdown-language-service.md`.

**Ecosystem:** gopls `settings.go:394-399` + `default.go:110` + `completion.go:126,645,662-715`
(github.com/golang/tools); rust-analyzer book guide (rust-analyzer.github.io/book/contributing/guide.html);
TypeScript #46838 + wiki/Performance (github.com/microsoft/TypeScript); Pylance performance-tuning doc
(github.com/microsoft/pylance-release/blob/main/docs/howto/performance-tuning.md); MS markdown-language-service
(microsoft/vscode-markdown-language-server — via local digest); server-side-sorting precedent RA #7935
(quoted in `24-part-crates.md` §4.3).
